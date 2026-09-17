//! The Key mappings card: header, capture banner, D-pad stage, and the
//! unique/duplicate footer.
//!
//! The Iced original is the mappings block in `SettingsApp::settings_view`
//! plus `mapping_pad`, `rebind_banner`, `assignment_status`, and
//! `resolve_dpad` in `iced-ui/app.rs`; `docs/architecture/ui.md` owns the
//! contract they must keep. The card paints and pushes [`Message`]s only: it
//! never saves settings, sends IPC, or mutates application state, and it
//! returns the messages so the caller can feed them to `state::update`.
//!
//! Colours come from [`super::theme`] so the palette keeps one owner; the
//! card's pixel metrics stay local and name the Iced `theme::` constant they
//! replace.

use std::f32::consts::PI;
use std::time::Duration;

use egui::{
    Align, Color32, CornerRadius, FontId, Id, Layout, Margin, Painter, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

use crate::core::PhysicalKey;
use crate::protocol::{KeySlot, UiSnapshot};

use super::{
    keycap::{self, DOWN, Direction, Keycap, KeycapMode, LEFT, RIGHT, UP},
    message::Message,
    state::{State, key_slot_index},
    theme,
    timeline::Timeline,
};

/// `rounded-2xl`: the card and the stage tile.
const CARD_RADIUS: u8 = 16;
/// `theme::BUTTON_PADDING`: `py-1.5 px-3` plus the 1px border iced lays inside
/// the padding. The reference draws a 30px control.
const BUTTON_PAD_X: f32 = 13.0;
const BUTTON_HEIGHT: f32 = 30.0;
const BUTTON_RADIUS: u8 = 12;
const BUTTON_TEXT_SIZE: f32 = 12.0;
const BUTTON_ICON: f32 = 14.0;
const BUTTON_ICON_GAP: f32 = 6.0;
/// The banner's ESC chip: `theme::banner_cancel_button` with `px-2.5 py-1`.
const ESC_PAD_X: f32 = 10.0;
const ESC_PAD_Y: f32 = 4.0;
const ESC_RADIUS: u8 = 8;
const ESC_TEXT_SIZE: f32 = 11.0;
/// The banner (reference `px-4 py-2.5`) and its `rounded-xl`.
const BANNER_PAD_X: f32 = 16.0;
const BANNER_PAD_Y: f32 = 10.0;
const BANNER_RADIUS: u8 = 12;
/// The D-pad stage's `p-5` and the tile's square (`icons::DpadTile`).
const STAGE_PADDING: f32 = 20.0;
const DPAD_TILE_SIZE: f32 = 80.0;
/// The middle row's own box: three keycap squares plus two section gaps. The
/// row has to declare its size because a centered vertical layout can only
/// center a child whose size it can measure before layout; a bare
/// `ui.horizontal` scope takes the whole stage width and lays its children
/// from the left edge, which is the F01 misalignment.
const DPAD_ROW_SIZE: Vec2 = Vec2::new(
    3.0 * keycap::KEYCAP_SIZE + 2.0 * theme::SECTION_GAP,
    DPAD_TILE_SIZE,
);

/// Draws the whole Key mappings card and returns the messages the frame
/// produced. The card needs a snapshot to draw: with no connection the page
/// shows the waiting body instead, so this yields no messages and paints
/// nothing.
pub fn key_mappings_card(ui: &mut Ui, state: &State) -> Vec<Message> {
    let Some(snapshot) = &state.snapshot else {
        return Vec::new();
    };
    let mut messages = Vec::new();

    egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(Stroke::new(2.0, theme::CARD_BORDER))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(20))
        .shadow(theme::SHADOW_CARD)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme::SECTION_GAP;
            header(ui, state, &mut messages);
            if snapshot.capture_slot.is_some() && rebind_banner(ui, state) {
                messages.push(Message::CancelCapture);
            }
            mapping_pad(ui, state, snapshot, &mut messages);
            footer(ui, state, snapshot);
        });

    messages
}

fn header(ui: &mut Ui, state: &State, messages: &mut Vec<Message>) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            section_title(ui, Icon::Keyboard, state.language.text("Key mappings"));
            text(
                ui,
                state
                    .language
                    .text("Hardware scan codes the SOCD filter uses"),
                12.0,
                theme::MUTED_TEXT,
                false,
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let restore = secondary_button(
                ui,
                Icon::Restore,
                state.language.text("Restore mapping defaults"),
            );
            if restore.clicked() {
                messages.push(Message::RestoreMappingDefaults);
            }
        });
    });
}

/// The capture banner: a solid indigo bar naming the prompt with an explicit
/// ESC cancel. Returns whether the cancel chip was pressed.
fn rebind_banner(ui: &mut Ui, state: &State) -> bool {
    let mut cancelled = false;
    egui::Frame::NONE
        .fill(theme::INDIGO_600)
        .corner_radius(CornerRadius::same(BANNER_RADIUS))
        .inner_margin(Margin {
            left: BANNER_PAD_X as i8,
            right: BANNER_PAD_X as i8,
            top: BANNER_PAD_Y as i8,
            bottom: BANNER_PAD_Y as i8,
        })
        .shadow(theme::SHADOW_BANNER)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                dot(ui, Color32::WHITE);
                text(
                    ui,
                    state.language.text("Press a new key on your keyboard..."),
                    12.0,
                    Color32::WHITE,
                    true,
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if esc_button(ui, state.language.text("ESC Cancel")).clicked() {
                        cancelled = true;
                    }
                });
            });
        });
    cancelled
}

/// The D-pad stage: the rebind hint, the four directional keycaps, and the
/// centre joystick tile that resolves the same Last-Input-Priority rule the
/// engine applies to the two opposing pairs.
fn mapping_pad(ui: &mut Ui, state: &State, snapshot: &UiSnapshot, messages: &mut Vec<Message>) {
    let duplicates = duplicate_slots(&snapshot.draft.bindings);
    let timeline = state.monitor.timeline();
    egui::Frame::NONE
        .fill(theme::INSET)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(STAGE_PADDING as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = theme::SECTION_GAP;
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    icon(ui, Icon::Edit, 12.0, theme::MUTED_TEXT);
                    text(
                        ui,
                        state.language.text("Click keycap to rebind"),
                        11.0,
                        theme::MUTED_TEXT,
                        true,
                    );
                });
                directional_keycap(ui, state, snapshot, &duplicates, &UP, timeline, messages);
                ui.allocate_ui_with_layout(
                    DPAD_ROW_SIZE,
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = theme::SECTION_GAP;
                        directional_keycap(
                            ui,
                            state,
                            snapshot,
                            &duplicates,
                            &LEFT,
                            timeline,
                            messages,
                        );
                        let (shift_x, shift_y, active) =
                            resolve_dpad(&state.pressed_keys, &state.press_timestamps, timeline);
                        dpad_center_tile(ui, shift_x, shift_y, active);
                        directional_keycap(
                            ui,
                            state,
                            snapshot,
                            &duplicates,
                            &RIGHT,
                            timeline,
                            messages,
                        );
                    },
                );
                directional_keycap(ui, state, snapshot, &duplicates, &DOWN, timeline, messages);
            });
        });
}

/// One directional keycap. The mode is the Iced `keycap` decision unchanged:
/// the capture slot wins, then a live press -- from the window's own key
/// events or, while recording, the timeline's held state -- then rest.
fn directional_keycap(
    ui: &mut Ui,
    state: &State,
    snapshot: &UiSnapshot,
    duplicates: &[bool; 4],
    direction: &Direction,
    timeline: Option<&Timeline>,
    messages: &mut Vec<Message>,
) {
    let index = key_slot_index(direction.slot);
    let held =
        state.pressed_keys[index] || timeline.is_some_and(|timeline| timeline.held(direction.slot));
    let mode = if snapshot.capture_slot == Some(direction.slot) {
        KeycapMode::Rebinding
    } else if held {
        KeycapMode::Pressed
    } else {
        KeycapMode::Normal
    };
    let keycap = Keycap {
        name: &snapshot.keys[index].name,
        mode,
        duplicate: duplicates[index],
    };
    if let Some(message) = keycap::keycap(ui, direction, keycap) {
        messages.push(message);
    }
}

/// The footer row below the stage: the direction count on the left, the
/// unique/duplicate assignment status on the right.
fn footer(ui: &mut Ui, state: &State, snapshot: &UiSnapshot) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::BORDER);
    ui.horizontal(|ui| {
        // `text-slate-400`: [`theme::ICON_MUTED`] owns that exact byte
        // (`#94a3b8`); [`theme::MUTED_TEXT`] would darken the caption to
        // slate-500.
        text(
            ui,
            state.language.text("4 directions mapped"),
            11.0,
            theme::ICON_MUTED,
            false,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            assignment_status(ui, &duplicate_slots(&snapshot.draft.bindings), state);
        });
    });
}

fn assignment_status(ui: &mut Ui, duplicates: &[bool; 4], state: &State) {
    let duplicate = duplicates.contains(&true);
    text(
        ui,
        state.language.text(if duplicate {
            "Duplicate key bindings detected."
        } else {
            "All keys uniquely assigned."
        }),
        11.0,
        if duplicate {
            theme::RED_600
        } else {
            theme::EMERALD_700
        },
        true,
    );
    icon(
        ui,
        if duplicate {
            Icon::Warning
        } else {
            Icon::Check
        },
        14.0,
        if duplicate {
            theme::ERROR_TEXT
        } else {
            theme::GREEN_CHECK
        },
    );
}

/// Last-input-priority resolution for the centre tile. The timeline resolves
/// the winner while it is showing filtered output (`!physical`), and the
/// window's own press set with its timestamps resolves it otherwise: the later
/// press takes a contested pair. The `!physical` condition is unchanged from
/// the Iced `resolve_dpad`.
fn resolve_dpad(
    pressed_keys: &[bool; 4],
    press_timestamps: &[Option<std::time::Instant>; 4],
    timeline: Option<&Timeline>,
) -> (f32, f32, bool) {
    if let Some(timeline) = timeline.filter(|timeline| !timeline.physical) {
        let x = match timeline.winner(KeySlot::HorizontalFirst, KeySlot::HorizontalSecond) {
            Some(KeySlot::HorizontalFirst) => -1,
            Some(_) => 1,
            None => 0,
        };
        let y = match timeline.winner(KeySlot::VerticalFirst, KeySlot::VerticalSecond) {
            Some(KeySlot::VerticalFirst) => -1,
            Some(_) => 1,
            None => 0,
        };
        let diagonal = x != 0 && y != 0;
        let reach = if diagonal { 13.0 } else { 18.0 };
        return (x as f32 * reach, y as f32 * reach, x != 0 || y != 0);
    }

    let is_up = pressed_keys[0] || timeline.is_some_and(|t| t.held(KeySlot::VerticalFirst));
    let is_down = pressed_keys[1] || timeline.is_some_and(|t| t.held(KeySlot::VerticalSecond));
    let is_left = pressed_keys[2] || timeline.is_some_and(|t| t.held(KeySlot::HorizontalFirst));
    let is_right = pressed_keys[3] || timeline.is_some_and(|t| t.held(KeySlot::HorizontalSecond));

    let mut x = 0;
    if is_left && is_right {
        x = match (press_timestamps[2], press_timestamps[3]) {
            (Some(l), Some(r)) if r >= l => 1,
            (Some(_), Some(_)) => -1,
            _ => 1,
        };
    } else if is_left {
        x = -1;
    } else if is_right {
        x = 1;
    }

    let mut y = 0;
    if is_up && is_down {
        y = match (press_timestamps[0], press_timestamps[1]) {
            (Some(u), Some(d)) if d >= u => 1,
            (Some(_), Some(_)) => -1,
            _ => 1,
        };
    } else if is_up {
        y = -1;
    } else if is_down {
        y = 1;
    }

    let diagonal = x != 0 && y != 0;
    let reach = if diagonal { 13.0 } else { 18.0 };
    (x as f32 * reach, y as f32 * reach, x != 0 || y != 0)
}

/// Flags every binding slot that shares its key with another slot. Ported
/// unchanged from the Iced `duplicate_slots`.
fn duplicate_slots(bindings: &[PhysicalKey; 4]) -> [bool; 4] {
    std::array::from_fn(|index| {
        bindings
            .iter()
            .enumerate()
            .any(|(other, binding)| other != index && *binding == bindings[index])
    })
}

/// The 80x80 joystick tile from `icons::DpadTile`: slate inset, a dashed
/// guide ring, the resting centre dot, and the moving dot that lights up with
/// the Immediate accent while a direction wins.
///
/// The dot travels to its target offset with the reference's 75 ms ease-out
/// segment instead of teleporting (F05). The accent state still switches on
/// the frame it changes; only the displacement is interpolated.
fn dpad_center_tile(ui: &mut Ui, shift_x: f32, shift_y: f32, active: bool) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(DPAD_TILE_SIZE), Sense::hover());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Other,
            ui.is_enabled(),
            "D-pad direction preview",
        )
    });

    // egui is immediate-mode, so the in-flight segment lives in the frame's
    // temp memory keyed by the tile, the way `preview.rs` keeps its clock. The
    // tile drives its own repaints while the dot travels and asks for none
    // once it settles.
    let target = Vec2::new(shift_x, shift_y);
    let now = ui.input(|input| input.time);
    let motion_id = Id::new("ui-mapping-dpad-dot");
    let (offset, repaint_in) = ui.data_mut(|data| {
        let motion = data.get_temp_mut_or_default::<DotMotion>(motion_id);
        (motion.poll(now, target), motion.remaining(now))
    });
    if let Some(repaint_in) = repaint_in {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(repaint_in));
    }

    let painter = ui.painter_at(rect);
    let radius = CornerRadius::same(16);
    painter.rect_filled(rect, radius, theme::SLATE_100);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let center = rect.center();
    let ring: Vec<Pos2> = (0..=72)
        .map(|step| {
            let angle = step as f32 / 72.0 * 2.0 * PI;
            Pos2::new(center.x + angle.cos() * 24.0, center.y + angle.sin() * 24.0)
        })
        .collect();
    painter.extend(Shape::dashed_line(
        &ring,
        Stroke::new(1.0, theme::BORDER),
        3.0,
        3.0,
    ));
    painter.circle_filled(center, 4.0, theme::DPAD_GUIDE_DOT);

    let dot = Pos2::new(center.x + offset.x, center.y + offset.y);
    if active {
        painter.circle_filled(dot, 12.0, theme::DPAD_ACTIVE_GLOW);
        painter.circle_filled(
            Pos2::new(dot.x, dot.y + 1.0),
            10.0,
            theme::DPAD_ACTIVE_DOT_SHADOW,
        );
        painter.circle_filled(dot, 10.0, theme::IMMEDIATE_ACCENT);
    } else {
        painter.circle_filled(
            Pos2::new(dot.x, dot.y + 1.0),
            9.0,
            theme::DPAD_IDLE_DOT_SHADOW,
        );
        painter.circle_filled(dot, 9.0, theme::DPAD_IDLE_DOT);
    }
}

/// The reference's `duration-75`: one dot travel takes 75 ms.
const DOT_TRAVEL_SECS: f64 = 0.075;
/// The cadence of the intermediate frames the tile asks for while the dot
/// travels, the same 16 ms the timeline playhead uses; the final request of a
/// segment is shortened to its remaining time.
const DOT_FRAME_STEP_SECS: f64 = 0.016;

/// The centre dot's travel state. egui is immediate-mode, so the card keeps
/// the in-flight segment in the frame's temp memory instead of a widget tree;
/// the reference's CSS `transition-all duration-75 ease-out` is the
/// counterpart.
#[derive(Clone, Copy, Default)]
struct DotMotion {
    /// Whether a target has been observed before. The first observation
    /// adopts its target without a segment, the way a CSS transition does not
    /// run on mount.
    seen: bool,
    in_flight: bool,
    /// The offsets the segment interpolates between. On the frame a segment
    /// starts the dot still shows `from`.
    from: Vec2,
    to: Vec2,
    start: f64,
}

impl DotMotion {
    /// Advances to `now` (egui's monotonic input time, seconds) and returns
    /// the offset to draw for `target`. A target change starts a 75 ms
    /// ease-out segment; a change mid-flight restarts from the displayed
    /// offset, which is what a CSS transition does. Pure by design: tests
    /// drive it with explicit times, never a sleep.
    fn poll(&mut self, now: f64, target: Vec2) -> Vec2 {
        if !self.seen {
            self.seen = true;
            self.to = target;
            return target;
        }
        if !self.in_flight {
            if target == self.to {
                return self.to;
            }
            self.start_segment(now, self.to, target);
            return self.from;
        }
        let progress = (now - self.start) / DOT_TRAVEL_SECS;
        if progress >= 1.0 {
            self.in_flight = false;
            self.from = self.to;
            return self.to;
        }
        let displayed = self.from + (self.to - self.from) * ease_out(progress as f32);
        if target != self.to {
            self.start_segment(now, displayed, target);
            return displayed;
        }
        displayed
    }

    fn start_segment(&mut self, now: f64, from: Vec2, to: Vec2) {
        self.from = from;
        self.to = to;
        self.start = now;
        self.in_flight = true;
    }

    /// Seconds until the next repaint is useful, or `None` once settled.
    fn remaining(&self, now: f64) -> Option<f64> {
        if !self.in_flight {
            return None;
        }
        let left = (DOT_TRAVEL_SECS - (now - self.start)).max(0.0);
        Some(left.min(DOT_FRAME_STEP_SECS))
    }
}

/// CSS `ease-out` (`cubic-bezier(0, 0, 0.58, 1)`), the curve the reference's
/// `duration-75 ease-out` names, evaluated for a linear progress in `0..=1`.
/// The curve's x is monotonic, so bisecting the bezier parameter converges to
/// its inverse; 24 halvings resolve it well past display precision.
fn ease_out(progress: f32) -> f32 {
    let x = progress.clamp(0.0, 1.0);
    if x <= 0.0 || x >= 1.0 {
        return x;
    }
    // x(s) = s^2 * (3 * (1 - s) * 0.58 + s), y(s) = s^2 * (3 * (1 - s) + s)
    // for control points (0, 0) and (0.58, 1).
    let curve = |s: f32| s * s * (3.0 * (1.0 - s) * 0.58 + s);
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        if curve(mid) < x {
            low = mid;
        } else {
            high = mid;
        }
    }
    let s = (low + high) / 2.0;
    s * s * (3.0 - 2.0 * s)
}

/// Section title: the icon plus a 15px heavy label, the pair the timing and
/// measurement cards share.
fn section_title(ui: &mut Ui, kind: Icon, label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        icon(ui, kind, 16.0, theme::PRIMARY_TEXT);
        text(ui, label, theme::HEADING_SIZE, theme::BODY_TEXT, true);
    });
}

/// Outlined action control: white shell, slate edge, slate-600 label, and the
/// Iced `theme::secondary_button` hover (indigo ink over an indigo-50 wash).
///
/// The label galley is laid out with [`Color32::PLACEHOLDER`] on purpose: a
/// galley carries the colour it was laid out with, and `Painter::galley`'s
/// fallback colour only reaches `PLACEHOLDER` glyphs. Laying this text out in
/// a real colour would pin it there and ignore the per-state `ink` below.
fn secondary_button(ui: &mut Ui, kind: Icon, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(BUTTON_TEXT_SIZE),
        Color32::PLACEHOLDER,
    );
    let content = BUTTON_ICON + BUTTON_ICON_GAP + galley.size().x;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(content + 2.0 * BUTTON_PAD_X, BUTTON_HEIGHT),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let hovered = response.hovered();
    let (fill, edge, label_ink) = if hovered {
        (
            theme::HOVER_WASH,
            theme::NAME_HOVER_BORDER,
            theme::INDIGO_600,
        )
    } else {
        (theme::SURFACE, theme::BORDER, theme::ICON_SECONDARY)
    };
    let painter = ui.painter_at(rect);
    let radius = CornerRadius::same(BUTTON_RADIUS);
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
    let start = rect.center().x - content / 2.0;
    paint_icon(
        &painter,
        Rect::from_min_size(
            Pos2::new(start, rect.center().y - BUTTON_ICON / 2.0),
            Vec2::splat(BUTTON_ICON),
        ),
        kind,
        action_icon_ink(kind, label_ink),
    );
    keycap::stamp_galley(
        &painter,
        Pos2::new(
            start + BUTTON_ICON + BUTTON_ICON_GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        &galley,
        label_ink,
        BUTTON_TEXT_SIZE,
    );
    response
}

/// The icon ink rule from the Iced `icon_label`: a Restore glyph keeps the
/// reference's slate-600 ink in every state, while other action icons inherit
/// the button's text colour so their hover states keep working. The label
/// itself follows the hover ink either way.
fn action_icon_ink(kind: Icon, label_ink: Color32) -> Color32 {
    if kind == Icon::Restore {
        theme::ICON_SECONDARY
    } else {
        label_ink
    }
}

/// The banner's ESC chip (reference `bg-indigo-700 hover:bg-indigo-800`).
fn esc_button(ui: &mut Ui, label: &str) -> Response {
    // `PLACEHOLDER` for the same reason as `secondary_button`: the chip's ink
    // is decided at paint time.
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(ESC_TEXT_SIZE),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(
            galley.size().x + 2.0 * ESC_PAD_X,
            galley.size().y + 2.0 * ESC_PAD_Y,
        ),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let fill = if response.hovered() {
        theme::INDIGO_800
    } else {
        theme::INDIGO_700
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(ESC_RADIUS), fill);
    keycap::stamp_galley(
        &painter,
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        &galley,
        Color32::WHITE,
        ESC_TEXT_SIZE,
    );
    response
}

/// One measured line of text, painted at its natural size. `bold` picks the
/// local weight approximation in [`keycap::stamp_galley`]. The galley is laid
/// out with [`Color32::PLACEHOLDER`] so the `color` handed to the painter is
/// the one that renders.
///
/// Painted text still publishes a [`WidgetType::Label`] node: the footer's
/// duplicate/uniqueness status is the card's only correctness signal, and
/// without a node neither `egui_kittest` nor a screen reader could see it.
/// Decorative glyphs (`icon`, `dot`) stay unlabelled; the D-pad tile keeps the
/// label it already had.
fn text(ui: &mut Ui, content: &str, size: f32, color: Color32, bold: bool) -> Rect {
    let galley = ui.painter().layout_no_wrap(
        content.to_owned(),
        FontId::proportional(size),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), content));
    if bold {
        keycap::stamp_galley(ui.painter(), rect.min, &galley, color, size);
    } else {
        ui.painter().galley(rect.min, galley, color);
    }
    rect
}

/// A small filled circle for the banner's white dot, as `dot(color)` drew it.
fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// A fixed-size icon node. The card needs five glyphs; each is traced from
/// the matching `icons::draw_icon` path so the ports stay comparable.
fn icon(ui: &mut Ui, kind: Icon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_icon(ui.painter(), rect, kind, color);
}

/// The card's icon set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Icon {
    Keyboard,
    Restore,
    Edit,
    Check,
    Warning,
}

fn paint_icon(painter: &Painter, rect: Rect, kind: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    let line = |coordinates: &[(f32, f32)]| {
        painter.add(Shape::line(
            coordinates.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match kind {
        Icon::Keyboard => {
            painter.rect_stroke(
                Rect::from_min_max(point(0.15, 0.23), point(0.85, 0.78)),
                CornerRadius::same(1),
                stroke,
                StrokeKind::Middle,
            );
            line(&[(0.32, 0.6), (0.68, 0.6)]);
            for x in [0.28, 0.5, 0.72] {
                painter.circle_filled(point(x, 0.4), size * 0.04, color);
            }
        }
        Icon::Restore => {
            // The Iced arc runs PI * 0.2 -> PI * 1.8 around the centre at r
            // 0.32; sampled because epaint shapes are polylines.
            let arc: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let angle = PI * 0.2 + PI * 1.6 * (step as f32 / 24.0);
                    point(0.5 + angle.cos() * 0.32, 0.5 + angle.sin() * 0.32)
                })
                .collect();
            painter.add(Shape::line(arc, stroke));
            let x = 0.5 + (PI * 1.8).cos() * 0.32;
            let y = 0.5 + (PI * 1.8).sin() * 0.32;
            line(&[(x - 0.15, y), (x, y), (x, y + 0.15)]);
        }
        Icon::Edit => line(&[
            (0.2, 0.8),
            (0.2, 0.65),
            (0.65, 0.2),
            (0.8, 0.35),
            (0.35, 0.8),
            (0.2, 0.8),
        ]),
        Icon::Check => line(&[(0.15, 0.52), (0.4, 0.78), (0.85, 0.25)]),
        Icon::Warning => {
            line(&[(0.5, 0.15), (0.85, 0.85), (0.15, 0.85), (0.5, 0.15)]);
            line(&[(0.5, 0.38), (0.5, 0.62)]);
            painter.circle_filled(point(0.5, 0.75), size * 0.05, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::{DisplayKey, UiCommand, UiSnapshot},
        settings::Settings,
    };

    fn baseline_snapshot() -> UiSnapshot {
        let keys = std::array::from_fn(|index| DisplayKey {
            physical: PhysicalKey::new(0x11 + index as u16, false),
            name: ["W", "S", "A", "D"][index].into(),
        });
        UiSnapshot {
            filter_enabled: true,
            saved: Settings::default(),
            draft: Settings::default(),
            keys,
            capture_slot: None,
            measurement_active: false,
            measurement: None,
        }
    }

    fn baseline_state() -> State {
        State {
            connected: true,
            draft: Some(Settings::default()),
            snapshot: Some(baseline_snapshot()),
            ..State::default()
        }
    }

    /// The harness's stand-in for the runtime: it feeds the card's messages
    /// through `state::update` and answers the two capture commands the way
    /// the server would, so a click is observable as a state change.
    struct FakeRuntime {
        state: State,
        sent: Vec<UiCommand>,
    }

    impl FakeRuntime {
        fn frame(&mut self, ui: &mut Ui) {
            let messages = key_mappings_card(ui, &self.state);
            for message in messages {
                for effect in super::super::state::update(&mut self.state, message) {
                    let super::super::state::Effect::Send(command) = effect else {
                        continue;
                    };
                    match &command {
                        UiCommand::BeginKeyCapture(slot) => {
                            if let Some(snapshot) = self.state.snapshot.as_mut() {
                                snapshot.capture_slot = Some(*slot);
                            }
                        }
                        UiCommand::CancelKeyCapture => {
                            if let Some(snapshot) = self.state.snapshot.as_mut() {
                                snapshot.capture_slot = None;
                            }
                        }
                        _ => {}
                    }
                    self.sent.push(command);
                }
            }
        }
    }

    #[test]
    fn duplicate_slots_flag_every_sharer() {
        let distinct = [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1F, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x20, false),
        ];
        assert_eq!(duplicate_slots(&distinct), [false; 4]);

        let pair = [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x20, false),
        ];
        assert_eq!(duplicate_slots(&pair), [true, true, false, false]);

        let triple = [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x11, false),
        ];
        assert_eq!(duplicate_slots(&triple), [true, false, true, true]);
    }

    /// The footer's status is painted text, so this is also the regression
    /// guard for `text()` publishing a labelled node: if the painted status
    /// stops reaching AccessKit, no label can be found either way.
    #[test]
    fn the_footer_status_follows_the_duplicate_bindings() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                    sent: Vec::new(),
                },
            );

        // Distinct bindings publish only the unique status.
        harness.get_by_label("All keys uniquely assigned.");
        assert!(
            harness
                .query_by_label("Duplicate key bindings detected.")
                .is_none()
        );

        // Sharing one physical key between two slots must flip the footer, and
        // the flipped status must be reachable by label.
        let shared = harness
            .state()
            .state
            .snapshot
            .as_ref()
            .unwrap()
            .draft
            .bindings[0];
        harness
            .state_mut()
            .state
            .snapshot
            .as_mut()
            .unwrap()
            .draft
            .bindings[1] = shared;
        harness.run();
        harness.get_by_label("Duplicate key bindings detected.");
        assert!(
            harness
                .query_by_label("All keys uniquely assigned.")
                .is_none()
        );
    }

    #[test]
    fn resolve_dpad_handles_opposing_presses_and_diagonals() {
        let timestamps = [None; 4];
        let (x, y, active) = resolve_dpad(&[false; 4], &timestamps, None);
        assert_eq!((x, y, active), (0.0, 0.0, false));

        let (x, y, active) = resolve_dpad(&[true, false, false, false], &timestamps, None);
        assert_eq!((x, y, active), (0.0, -18.0, true));

        let (x, y, active) = resolve_dpad(&[true, false, false, true], &timestamps, None);
        assert_eq!((x, y, active), (13.0, -13.0, true));

        // Opposing horizontal keys: Left pressed first, Right pressed second,
        // so the later press wins.
        let now = std::time::Instant::now();
        let timestamps = [
            None,
            None,
            Some(now),
            Some(now + std::time::Duration::from_millis(10)),
        ];
        let (x, _, active) = resolve_dpad(&[false, false, true, true], &timestamps, None);
        assert_eq!(x, 18.0);
        assert!(active);
    }

    /// The colour a painted label actually renders in.
    ///
    /// A galley carries the colour it was laid out with, and the colour handed
    /// to `Painter::galley` only reaches sections laid out with
    /// [`Color32::PLACEHOLDER`]. Reading both is what makes the
    /// layout-colour defect observable: a label laid out in a real colour
    /// renders in that colour no matter what the paint call passes.
    fn painted_text_color(
        harness: &egui_kittest::Harness<'_, FakeRuntime>,
        needle: &str,
    ) -> Option<Color32> {
        harness.output().shapes.iter().find_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            if !text.galley.text().contains(needle) {
                return None;
            }
            let layout_color = text
                .galley
                .job
                .sections
                .first()
                .map(|section| section.format.color)
                .unwrap_or(Color32::PLACEHOLDER);
            Some(if layout_color == Color32::PLACEHOLDER {
                text.fallback_color
            } else {
                layout_color
            })
        })
    }

    #[test]
    fn the_restore_button_paints_its_label_in_the_state_ink() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                    sent: Vec::new(),
                },
            );

        assert_eq!(
            painted_text_color(&harness, "Restore mapping defaults"),
            Some(theme::ICON_SECONDARY),
            "at rest the label must render in the secondary ink, not in the colour it was laid out with"
        );

        harness.get_by_label("Restore mapping defaults").hover();
        harness.run();
        assert_eq!(
            painted_text_color(&harness, "Restore mapping defaults"),
            Some(theme::INDIGO_600),
            "hover must move the label to the indigo ink"
        );
    }

    #[test]
    fn other_action_icons_follow_the_button_ink() {
        assert_eq!(
            action_icon_ink(Icon::Restore, theme::INDIGO_600),
            theme::ICON_SECONDARY
        );
        assert_eq!(
            action_icon_ink(Icon::Keyboard, theme::INDIGO_600),
            theme::INDIGO_600
        );
    }

    /// Every solid stroke colour painted by a widget's own painter, i.e. the
    /// shapes clipped to that widget's rect.
    fn painted_stroke_colors(
        harness: &egui_kittest::Harness<'_, FakeRuntime>,
        clip: Rect,
    ) -> Vec<Color32> {
        use egui::epaint::ColorMode;

        fn stroke_colors(shape: &egui::Shape) -> Vec<Color32> {
            match shape {
                egui::Shape::Path(path) => match path.stroke.color {
                    ColorMode::Solid(color) => vec![color],
                    ColorMode::UV(_) => Vec::new(),
                },
                egui::Shape::LineSegment { stroke, .. } => vec![stroke.color],
                egui::Shape::Rect(rect) => vec![rect.stroke.color],
                egui::Shape::Circle(circle) => vec![circle.stroke.color],
                _ => Vec::new(),
            }
        }

        harness
            .output()
            .shapes
            .iter()
            .filter(|clipped| clipped.clip_rect == clip)
            .flat_map(|clipped| stroke_colors(&clipped.shape))
            .collect()
    }

    #[test]
    fn the_restore_icon_keeps_the_iced_slate_ink_on_hover() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                    sent: Vec::new(),
                },
            );
        let button = harness.get_by_label("Restore mapping defaults").rect();

        // Migration constraint 5 preserves the Iced `icon_label` rule: the
        // Restore glyph keeps slate-600 in every state while the label moves to
        // the hover ink. R1 asked for one shared ink variable, which would have
        // recoloured the icon; this test records the evidence-backed deviation.
        for hovered in [false, true] {
            if hovered {
                harness.get_by_label("Restore mapping defaults").hover();
                harness.run();
            }
            let strokes = painted_stroke_colors(&harness, button);
            assert!(
                strokes.contains(&theme::ICON_SECONDARY),
                "the Restore glyph must paint in the secondary slate (hovered: {hovered})"
            );
            assert!(
                !strokes.contains(&theme::INDIGO_600),
                "the Restore glyph must not follow the label's hover ink (hovered: {hovered})"
            );
        }
    }

    #[test]
    fn the_restore_button_emits_the_mapping_command() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                    sent: Vec::new(),
                },
            );
        harness.get_by_label("Restore mapping defaults").click();
        harness.run();
        assert!(
            harness
                .state()
                .sent
                .contains(&UiCommand::RestoreMappingDefaults),
            "the restore button must reach the state layer"
        );
    }

    #[test]
    fn a_physically_held_key_renders_the_pressed_state() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                    sent: Vec::new(),
                },
            );
        // The window's own key-event path sets this flag; a held bound key
        // must stay observable in the accessible tree.
        harness.state_mut().state.pressed_keys[0] = true;
        harness.run();
        harness.get_by_label("UP keycap: W (pressed)");
    }

    #[test]
    fn clicking_a_keycap_starts_and_cancels_rebinding() {
        use egui_kittest::{Harness, kittest::Queryable};

        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                    sent: Vec::new(),
                },
            );

        // All four keycaps are reachable by label, which is what keeps a
        // keycap that loses its accessible name failing the build.
        for label in [
            "UP keycap: W",
            "DOWN keycap: S",
            "LEFT keycap: A",
            "RIGHT keycap: D",
        ] {
            harness.get_by_label(label);
        }

        harness.get_by_label("UP keycap: W").click();
        harness.run();
        harness.get_by_label("UP keycap: rebinding");
        assert_eq!(
            harness
                .state()
                .state
                .snapshot
                .as_ref()
                .unwrap()
                .capture_slot,
            Some(KeySlot::VerticalFirst)
        );

        // Pressing the capturing keycap cancels the capture and returns it to
        // the bound key's name.
        harness.get_by_label("UP keycap: rebinding").click();
        harness.run();
        harness.get_by_label("UP keycap: W");
        assert_eq!(
            harness
                .state()
                .state
                .snapshot
                .as_ref()
                .unwrap()
                .capture_slot,
            None
        );
    }

    /// A card harness. `step_dt` is the frame clock the animation tests
    /// control; the harness default is 250 ms, which would overrun a 75 ms
    /// transition in a single step.
    fn harness_with(state: State, step_dt: f32) -> egui_kittest::Harness<'static, FakeRuntime> {
        egui_kittest::Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .with_step_dt(step_dt)
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state,
                    sent: Vec::new(),
                },
            )
    }

    /// The mapping card at the 16 ms frame clock the animation contract
    /// assumes.
    fn card_harness() -> egui_kittest::Harness<'static, FakeRuntime> {
        harness_with(baseline_state(), 1.0 / 60.0)
    }

    /// The shortest repaint request this frame made; `Duration::MAX` when the
    /// frame requested none.
    fn repaint_delay(harness: &egui_kittest::Harness<'_, FakeRuntime>) -> std::time::Duration {
        harness
            .output()
            .viewport_output
            .values()
            .map(|output| output.repaint_delay)
            .min()
            .unwrap_or(std::time::Duration::MAX)
    }

    /// F01: UP, the centre tile, and DOWN share one vertical centre axis, and
    /// the middle row is symmetric about that axis. The middle row declares
    /// its size so the centered stage can center it; a bare `ui.horizontal`
    /// scope took the full stage width and started at its left edge, leaving
    /// the tile about 70 px left of the UP/DOWN axis.
    #[test]
    fn the_dpad_cluster_rows_share_one_center_axis() {
        use egui_kittest::kittest::Queryable;

        let harness = card_harness();

        let up = harness.get_by_label("UP keycap: W").rect().center();
        let down = harness.get_by_label("DOWN keycap: S").rect().center();
        let left = harness.get_by_label("LEFT keycap: A").rect().center();
        let right = harness.get_by_label("RIGHT keycap: D").rect().center();
        let tile = harness
            .get_by_label("D-pad direction preview")
            .rect()
            .center();

        for (name, point) in [("UP", up), ("DOWN", down)] {
            assert!(
                (point.x - tile.x).abs() <= 1.0,
                "{name} and the centre tile must share the vertical axis: {point:?} vs {tile:?}"
            );
        }
        for (name, point) in [("LEFT", left), ("RIGHT", right)] {
            assert!(
                (point.y - tile.y).abs() <= 1.0,
                "{name} and the centre tile must share the horizontal axis: {point:?} vs {tile:?}"
            );
        }
        assert!(
            ((tile.x - left.x) - (right.x - tile.x)).abs() <= 1.0,
            "the middle row must be symmetric about the tile: {left:?} {tile:?} {right:?}"
        );
    }

    /// F03: the footer carries the direction count on the left and the
    /// assignment status on the right, on one row below the divider.
    #[test]
    fn the_footer_pairs_the_direction_count_with_the_status() {
        use egui_kittest::kittest::Queryable;

        let harness = card_harness();

        let count = harness.get_by_label("4 directions mapped").rect();
        let status = harness.get_by_label("All keys uniquely assigned.").rect();
        assert!(
            (count.center().y - status.center().y).abs() <= 1.0,
            "the count and the status must share the footer row: {count:?} vs {status:?}"
        );
        assert!(
            count.right() <= status.left(),
            "the count is the left label and the status the right one: {count:?} vs {status:?}"
        );
    }

    /// The moving dot's centre from the tile's own painted circles: the
    /// topmost circle of the moving set (`9..=12` px: dot, its shadow, its
    /// glow). The 4 px resting guide dot cannot join that set.
    fn painted_dot_center(harness: &egui_kittest::Harness<'_, FakeRuntime>, tile: Rect) -> Pos2 {
        harness
            .output()
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle)
                    if (9.0..=12.0).contains(&circle.radius) && tile.contains(circle.center) =>
                {
                    Some(circle.center)
                }
                _ => None,
            })
            .min_by(|a, b| a.y.partial_cmp(&b.y).expect("finite dot centre"))
            .expect("the tile paints its moving dot")
    }

    /// F05: a target change travels with a 75 ms ease-out segment instead of
    /// teleporting. The toggle frame keeps the previous position, the next
    /// frame is partway, and the dot settles on the target within 75 ms while
    /// the tile drives its own repaints only during the travel.
    #[test]
    fn the_dpad_dot_glides_to_its_target() {
        use egui_kittest::kittest::Queryable;

        let mut harness = card_harness();
        let tile = harness.get_by_label("D-pad direction preview").rect();
        let center = tile.center();
        assert!(
            (painted_dot_center(&harness, tile) - center).length() <= 0.5,
            "the dot rests at the tile centre"
        );

        // A held UP key is a target change. The toggle frame still shows the
        // pre-change position, and the tile starts requesting frames.
        harness.state_mut().state.pressed_keys[0] = true;
        harness.step();
        assert!(
            (painted_dot_center(&harness, tile) - center).length() <= 0.5,
            "the toggle frame must keep the previous position"
        );
        assert!(
            repaint_delay(&harness) <= std::time::Duration::from_millis(17),
            "the travelling dot must request its intermediate frame"
        );

        harness.step();
        let moving = painted_dot_center(&harness, tile);
        assert!(
            moving.y < center.y && moving.y > center.y - 18.0,
            "one 16 ms frame must be strictly inside the travel: {moving:?} vs {center:?}"
        );

        // 75 ms at 16 ms per frame: the sixth frame after the toggle settles.
        harness.run_steps(6);
        let settled = painted_dot_center(&harness, tile);
        assert!(
            (settled.y - (center.y - 18.0)).abs() <= 0.5,
            "the dot must come to rest at the 18 px cardinal target: {settled:?}"
        );
        assert_eq!(
            repaint_delay(&harness),
            std::time::Duration::MAX,
            "a settled dot must stop requesting frames"
        );
    }

    #[test]
    fn ease_out_matches_the_css_curve_shape() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        // CSS ease-out front-loads the travel: past halfway by half time.
        assert!(ease_out(0.5) > 0.6, "got {}", ease_out(0.5));
        let mut previous = 0.0;
        for step in 0..=20 {
            let value = ease_out(step as f32 / 20.0);
            assert!(value >= previous, "the curve must not go backwards");
            previous = value;
        }
    }

    #[test]
    fn the_dot_motion_settles_without_a_segment_on_first_sight() {
        let mut motion = DotMotion::default();
        assert_eq!(
            motion.poll(0.0, Vec2::new(0.0, -18.0)),
            Vec2::new(0.0, -18.0)
        );
        assert_eq!(motion.remaining(0.0), None);
    }

    #[test]
    fn the_dot_motion_glides_over_the_reference_duration() {
        let mut motion = DotMotion::default();
        motion.poll(0.0, Vec2::ZERO);

        // The toggle frame keeps the old position and schedules the segment.
        assert_eq!(motion.poll(1.0, Vec2::new(18.0, 0.0)), Vec2::ZERO);
        assert!(
            motion
                .remaining(1.0)
                .is_some_and(|left| left <= DOT_FRAME_STEP_SECS)
        );

        // Strictly between the ends until the 75 ms elapse, monotone, and
        // front-loaded by the ease-out curve.
        let quarter = motion.poll(1.0 + DOT_TRAVEL_SECS / 4.0, Vec2::new(18.0, 0.0));
        let half = motion.poll(1.0 + DOT_TRAVEL_SECS / 2.0, Vec2::new(18.0, 0.0));
        assert!(quarter.x > 0.0 && quarter.x < 18.0);
        assert!(half.x > quarter.x && half.x < 18.0);
        assert!(
            half.x > 10.0,
            "ease-out must be past halfway at half time: {half:?}"
        );

        // The nominal deadline is a float, so it can land a hair short: the
        // dot shows the target with one last (effectively immediate) repaint
        // request, and the next frame settles and stops requesting frames.
        let at_deadline = motion.poll(1.0 + DOT_TRAVEL_SECS, Vec2::new(18.0, 0.0));
        assert!((at_deadline.x - 18.0).abs() < 1e-3, "got {at_deadline:?}");
        assert_eq!(
            motion.poll(1.0 + 2.0 * DOT_TRAVEL_SECS, Vec2::new(18.0, 0.0)),
            Vec2::new(18.0, 0.0)
        );
        assert_eq!(motion.remaining(1.0 + 2.0 * DOT_TRAVEL_SECS), None);
    }

    #[test]
    fn a_mid_flight_retarget_continues_from_the_displayed_offset() {
        let mut motion = DotMotion::default();
        motion.poll(0.0, Vec2::ZERO);
        motion.poll(0.0, Vec2::new(18.0, 0.0));

        let halfway = motion.poll(DOT_TRAVEL_SECS / 2.0, Vec2::new(18.0, 0.0));
        assert!(halfway.x > 0.0 && halfway.x < 18.0);

        // The retarget keeps the displayed offset on its frame and travels
        // from there to the new target.
        let retargeted = motion.poll(DOT_TRAVEL_SECS / 2.0, Vec2::new(-18.0, 0.0));
        assert_eq!(retargeted, halfway);
        assert_eq!(
            motion.poll(
                DOT_TRAVEL_SECS / 2.0 + DOT_TRAVEL_SECS,
                Vec2::new(-18.0, 0.0)
            ),
            Vec2::new(-18.0, 0.0)
        );
    }
}
