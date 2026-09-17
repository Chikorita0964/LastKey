//! Page header: branding, connection status, the icon-only profile /
//! language / engine controls, and the amber dirty badge.
//!
//! Ports the header row of `SettingsApp::view` (iced-ui/app.rs:1071-1146) and
//! the dirty badge of `settings_actions` (iced-ui/app.rs:2162-2170). The Iced
//! app mounts the badge in the pinned action bar; T6 owns it because the
//! dispatch assigns it, and T7 mounts it there. The module paints and pushes
//! [`Message`]s only: it never saves settings, sends IPC, or mutates state.
//!
//! # Ownership boundary for the glyphs
//!
//! [`theme::Icon`] (T2's shared action set) carries five glyphs: `Keyboard`,
//! `Restore`, `Edit`, `Check`, `Warning`. The page glyphs the header and the
//! panel controls need -- `Layers`, `Languages`, `Power`, `Close` -- are
//! painted here because this task owns no theme file. Each traces the same
//! `iced-ui/icons.rs` path with the same paint-only shape as the shared set,
//! so folding them into it later is a move, not a rewrite; `mapping.rs`
//! carries its own five-glyph copy for the same boundary reason.
//!
//! # Accessible names
//!
//! The three header controls have no visible label (the reference renders
//! icons with `aria-label`s), so each publishes an invented English name:
//! `Profile slots`, `Language`, `Engine on/off`. Localizing accessible names
//! is the open policy T2's report raised; these are the port's names until
//! that decision lands.

use std::f32::consts::PI;
use std::time::Duration;

use egui::{
    Align, Color32, CornerRadius, Id, Layout, Margin, Painter, Pos2, Rect, Response, RichText,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

use super::{message::Message, state::State, theme};

/// The drawn height of the header bar. The Iced `HEADER_HEIGHT`
/// (iced-ui/app.rs:78) is the same 60, and `profiles::PROFILE_PANEL_TOP`
/// anchors the overlay one `PAGE_PADDING + HEADER_HEIGHT + 8` below the
/// window top.
pub const HEADER_HEIGHT: f32 = 60.0;

/// Iced `Padding { left: 20.0, ..Padding::from(12) }` (iced-ui/app.rs:1140).
const HEADER_PADDING: Margin = Margin {
    left: 20,
    right: 12,
    top: 12,
    bottom: 12,
};

/// The content height that keeps the drawn bar at [`HEADER_HEIGHT`] once the
/// vertical insets are added around it.
const HEADER_CONTENT_HEIGHT: f32 =
    HEADER_HEIGHT - HEADER_PADDING.top as f32 - HEADER_PADDING.bottom as f32;

/// The 14px glyphs the Iced header buttons drew (`icons::icon(name, 14.0, ..)`).
const HEADER_ICON: f32 = 14.0;
/// The status dot's 8px box (`fn dot`, iced-ui/app.rs:2462).
const STATUS_DOT: f32 = 8.0;
/// The dot's `ring-4` outer band: 4px outside the dot box (Header.tsx:143-148).
const STATUS_RING: f32 = 4.0;
/// Tailwind's `animate-pulse` period: the dot's opacity falls to half and
/// returns over 2 s.
const PULSE_SECONDS: f32 = 2.0;
/// Tailwind's `animate-ping` period and the 75% keyframe where it is fully
/// expanded and transparent.
const PING_SECONDS: f32 = 1.0;
const PING_RISE: f32 = 0.75;
/// The fallback frame time when the backend predicts none.
const ANIMATION_FRAME_FALLBACK: Duration = Duration::from_millis(16);
/// The dot-to-status gap (Iced `.spacing(14)`).
const STATUS_GAP: f32 = 14.0;
/// The Iced `widgets::logo` is a fixed 32x32.
const LOGO_SIZE: f32 = 32.0;
/// The dot's ring steps the theme does not publish (`ring-amber-100`,
/// `ring-emerald-100`), plus the reference's `bg-indigo-500` rebinding ink.
/// All three are derived from the reference's Tailwind v4 oklch declarations;
/// the same conversion reproduces the theme's verified indigo-100, slate-100,
/// slate-200, amber-500, and emerald-500 read-backs exactly. They stay beside
/// their only consumer because this task owns no theme file; `preview.rs`
/// carries the same indigo-500 value for its neutral-dot highlight, and both
/// should fold into the theme when that file is next owned.
const AMBER_100: Color32 = Color32::from_rgb(0xfe, 0xf3, 0xc6);
const EMERALD_100: Color32 = Color32::from_rgb(0xd0, 0xfa, 0xe5);
const INDIGO_500: Color32 = Color32::from_rgb(0x61, 0x5f, 0xff);

/// Draws the header bar and returns the messages this frame produced.
pub fn header(ui: &mut Ui, state: &State) -> Vec<Message> {
    let mut messages = Vec::new();
    let connected = state.connected;
    let has_snapshot = state.snapshot.is_some();
    let filter_enabled = state
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.filter_enabled);
    // Iced gates each control with `on_press_maybe`: a control whose condition
    // fails is inert, not an error path, so a press simply produces no message.
    let profiles_enabled = connected && has_snapshot;
    let languages_enabled = has_snapshot;
    let power_enabled = profiles_enabled && state.pending_filter.is_none();
    // The power glyph keeps the Iced ink pair: indigo while the filter is on,
    // muted while it is off (`Some(if filter_enabled { .. } else { .. })`).
    let power_ink = if filter_enabled {
        theme::PRIMARY_TEXT
    } else {
        theme::ICON_MUTED
    };

    theme::card_style()
        .inner_margin(HEADER_PADDING)
        .show(ui, |ui| {
            ui.set_height(HEADER_CONTENT_HEIGHT);
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::SECTION_GAP;
                logo(ui);
                ui.label(RichText::new("LastKey").size(theme::HEADING_SIZE).strong());
                title_divider(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = STATUS_GAP;
                    status_dot(ui, state);
                    ui.label(
                        RichText::new(state.language.text(&state.status))
                            .size(theme::BODY_TEXT_SIZE)
                            .strong()
                            .color(theme::SLATE_600),
                    );
                });
                // The three controls sit against the right edge. `right_to_left`
                // lays them out from the right, so they are added in visual
                // reverse: power, language, profiles.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, power_enabled, Icon::Power, "Engine on/off", power_ink)
                        .clicked()
                    {
                        messages.push(Message::ToggleFilter);
                    }
                    if icon_button(
                        ui,
                        languages_enabled,
                        Icon::Languages,
                        "Language",
                        theme::PRIMARY_TEXT,
                    )
                    .clicked()
                    {
                        messages.push(Message::OpenLanguages);
                    }
                    if icon_button(
                        ui,
                        profiles_enabled,
                        Icon::Layers,
                        "Profile slots",
                        theme::PRIMARY_TEXT,
                    )
                    .clicked()
                    {
                        messages.push(Message::OpenProfiles);
                    }
                });
            });
        });

    messages
}

/// The amber "Unsaved Draft Changes" badge. Draws nothing while the draft is
/// clean; the caller mounts it unconditionally and the badge appears with the
/// first uncommitted edit (the Iced action bar did the same with its
/// conditional element slot).
pub fn dirty_badge(ui: &mut Ui, state: &State) {
    if !state.is_dirty() {
        return;
    }
    let ink = theme::AMBER_DARK;
    theme::dirty_badge()
        .inner_margin(Margin {
            left: theme::BUTTON_PADDING.left as i8,
            right: theme::BUTTON_PADDING.right as i8,
            top: theme::BUTTON_PADDING.top as i8,
            bottom: theme::BUTTON_PADDING.bottom as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::BUTTON_ICON_GAP;
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::splat(theme::BUTTON_ICON), Sense::hover());
                theme::paint_icon(ui.painter(), rect, theme::Icon::Edit, ink);
                ui.label(
                    RichText::new(state.language.text("Unsaved Draft Changes"))
                        .size(theme::BUTTON_TEXT_SIZE)
                        .strong()
                        .color(ink),
                );
            });
        });
}

/// One icon-only header control: the outlined shell the Iced `secondary_button`
/// style draws (white fill, `BORDER` edge, `HOVER_WASH` fill and
/// `NAME_HOVER_BORDER` edge under the pointer) with the 29px height and
/// `HEADER_ICON_PADDING` measured from the reference. The glyph takes an
/// explicit ink, as the Iced icons did, so the power-off state keeps its
/// muted colour rather than following a button text colour.
fn icon_button(ui: &mut Ui, enabled: bool, kind: Icon, label: &str, ink: Color32) -> Response {
    let size = Vec2::new(
        HEADER_ICON + theme::HEADER_ICON_PADDING.left + theme::HEADER_ICON_PADDING.right,
        theme::HEADER_ICON_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let hovered = enabled && response.hovered();
    let (fill, edge) = if hovered {
        (theme::HOVER_WASH, theme::NAME_HOVER_BORDER)
    } else {
        (theme::SURFACE, theme::BORDER)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    paint_icon(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(HEADER_ICON)),
        kind,
        ink,
    );
    response
}

/// The header logo: the 32x32 RGBA icon build.rs unpacked, drawn as a texture.
/// The texture is created once per context and cached in temp memory.
fn logo(ui: &mut Ui) {
    let texture = logo_texture(ui.ctx());
    ui.add(
        egui::Image::new((texture.id(), Vec2::splat(LOGO_SIZE)))
            .fit_to_exact_size(Vec2::splat(LOGO_SIZE)),
    );
}

fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = Id::new("lastkey-logo");
    if let Some(handle) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [
            super::icon::WINDOW_ICON_WIDTH as usize,
            super::icon::WINDOW_ICON_HEIGHT as usize,
        ],
        super::icon::WINDOW_ICON_RGBA,
    );
    let handle = ctx.load_texture("lastkey-logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, handle.clone()));
    handle
}

/// The title/status divider (`h-4 w-px bg-slate-200`, Header.tsx:136).
fn title_divider(ui: &mut Ui) {
    /// `h-4`.
    const DIVIDER_HEIGHT: f32 = 16.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, DIVIDER_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, theme::BORDER);
}

/// The connection/status mark: the reference's 8px circle inside its 4px state
/// ring (`w-2 h-2 ... ring-4`, Header.tsx:140-151), with the `animate-pulse`
/// and `animate-ping` states driven while they are shown (ui.md:24-26). A
/// deactivated window still paints the current frame but asks for no new
/// ones: it animates nothing and requests no frames at all (ui.md:27-31).
fn status_dot(ui: &mut Ui, state: &State) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(STATUS_DOT), Sense::hover());
    let now = ui.input(|input| input.time) as f32;
    let paint = DotState::of(state).paint(now);
    let painter = ui.painter();
    painter.circle_filled(rect.center(), paint.ring_radius, paint.ring);
    painter.circle_filled(rect.center(), paint.dot_radius, paint.dot);
    if paint.animating && state.focused {
        request_animation_frame(ui);
    }
}

/// Asks for one more frame just after the predicted next one. egui subtracts
/// the predicted frame time from a repaint request, so a fixed small delay
/// collapses into an immediate repaint loop; one frame time plus a millisecond
/// keeps the animation on the backend's cadence with a non-zero delay. The dot
/// drives its own repaint and only while it animates (ui.md:24-26).
fn request_animation_frame(ui: &Ui) {
    let frame = Duration::try_from_secs_f32(ui.input(|input| input.predicted_dt))
        .unwrap_or(ANIMATION_FRAME_FALLBACK);
    ui.ctx()
        .request_repaint_after(frame + Duration::from_millis(1));
}

/// The status dot's four reference states (Header.tsx:142-148): the engine-off
/// or disconnected grey, the measuring amber pulse, the rebinding indigo ping,
/// and the synchronized emerald.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DotState {
    Idle,
    Measuring,
    Rebinding,
    Synced,
}

/// One frame of the dot's paint programme: the dot and ring circles, and
/// whether the state keeps animating.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DotPaint {
    dot_radius: f32,
    dot: Color32,
    ring_radius: f32,
    ring: Color32,
    animating: bool,
}

impl DotState {
    /// The reference's precedence: the inactive state wins, then measuring,
    /// then rebinding. The port counts the disconnected runtime as inactive
    /// because the reference has no separate connection state, and no engine
    /// status can be trusted without a connection.
    fn of(state: &State) -> Self {
        let snapshot = state.snapshot.as_ref();
        let engine_on = state.connected && snapshot.is_some_and(|snapshot| snapshot.filter_enabled);
        if !engine_on {
            Self::Idle
        } else if snapshot.is_some_and(|snapshot| snapshot.measurement_active) {
            Self::Measuring
        } else if snapshot.is_some_and(|snapshot| snapshot.capture_slot.is_some()) {
            Self::Rebinding
        } else {
            Self::Synced
        }
    }

    /// The dot and ring inks at rest (`bg-*-500 ring-*-100`).
    fn ink(self) -> (Color32, Color32) {
        match self {
            Self::Idle => (theme::SLATE_300, theme::SLATE_100),
            Self::Measuring => (theme::AMBER_500, AMBER_100),
            Self::Rebinding => (INDIGO_500, theme::INDIGO_100),
            Self::Synced => (theme::EMERALD_500, EMERALD_100),
        }
    }

    /// The frame's circles at `now` (seconds): `animate-pulse`'s opacity wave,
    /// `animate-ping`'s expansion, or the static pair. The ping is the whole
    /// circle expanding to double size and fading out, holding the invisible
    /// tail of its cycle exactly as the reference's keyframes do.
    fn paint(self, now: f32) -> DotPaint {
        let (dot, ring) = self.ink();
        let (scale, alpha, animating) = match self {
            Self::Measuring => {
                let phase = now.rem_euclid(PULSE_SECONDS) / PULSE_SECONDS;
                let wave = 0.5 + 0.5 * (phase * std::f32::consts::TAU).cos();
                (1.0, 0.5 + 0.5 * wave, true)
            }
            Self::Rebinding => {
                let phase = (now.rem_euclid(PING_SECONDS) / PING_SECONDS / PING_RISE).min(1.0);
                let eased = 1.0 - (1.0 - phase).powi(3);
                (1.0 + eased, 1.0 - eased, true)
            }
            Self::Idle | Self::Synced => (1.0, 1.0, false),
        };
        DotPaint {
            dot_radius: STATUS_DOT / 2.0 * scale,
            dot: dot.gamma_multiply(alpha),
            ring_radius: (STATUS_DOT / 2.0 + STATUS_RING) * scale,
            ring: ring.gamma_multiply(alpha),
            animating,
        }
    }
}

/// The four page glyphs this task paints; see the module-level boundary note.
/// `theme::Icon` keeps the shared action set and is used for `Check`/`Edit`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Icon {
    Layers,
    Languages,
    Power,
    Close,
}

/// Trace one [`Icon`] into `rect` at `color`. The geometry is
/// `iced-ui/icons.rs::draw_icon`'s, in the same normalized coordinates the
/// shared [`theme::paint_icon`] uses, so the two painters stay comparable.
pub(super) fn paint_icon(painter: &Painter, rect: Rect, kind: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let fine = Stroke::new((size * 0.09).max(1.2), color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    let polyline = |stroke: Stroke, coordinates: &[(f32, f32)]| {
        painter.add(Shape::line(
            coordinates.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match kind {
        Icon::Layers => {
            // Three stacked plates: the top one closed, the lower two open
            // chevrons (iced-ui/icons.rs:453-471).
            polyline(
                fine,
                &[
                    (0.5, 0.12),
                    (0.88, 0.31),
                    (0.5, 0.5),
                    (0.12, 0.31),
                    (0.5, 0.12),
                ],
            );
            polyline(fine, &[(0.12, 0.5), (0.5, 0.69), (0.88, 0.5)]);
            polyline(fine, &[(0.12, 0.69), (0.5, 0.88), (0.88, 0.69)]);
        }
        Icon::Languages => {
            // The reference's translate glyph: six strokes in a 24-unit box,
            // drawn at the reference's 0.09 stroke (iced-ui/icons.rs:335-353).
            let parts: &[&[(f32, f32)]] = &[
                &[(8.0, 2.0), (8.0, 5.0)],
                &[(2.0, 5.0), (14.0, 5.0)],
                &[(4.0, 14.0), (10.0, 8.0), (12.0, 5.0)],
                &[(5.0, 8.0), (11.0, 14.0)],
                &[(12.0, 22.0), (17.0, 11.0), (22.0, 22.0)],
                &[(14.0, 18.0), (20.0, 18.0)],
            ];
            for part in parts {
                let coordinates: Vec<(f32, f32)> =
                    part.iter().map(|&(x, y)| (x / 24.0, y / 24.0)).collect();
                polyline(fine, &coordinates);
            }
        }
        Icon::Power => {
            // An arc open at the top, with the stem down its centre
            // (iced-ui/icons.rs:483-493). The arc is sampled because epaint
            // shapes are polylines.
            let arc: Vec<Pos2> = (0..=24)
                .map(|step| {
                    let start = -PI / 2.0 + 0.6;
                    let end = -PI / 2.0 - 0.6 + 2.0 * PI;
                    let angle = start + (end - start) * (step as f32 / 24.0);
                    point(0.5 + angle.cos() * 0.3, 0.56 + angle.sin() * 0.3)
                })
                .collect();
            painter.add(Shape::line(arc, stroke));
            polyline(stroke, &[(0.5, 0.14), (0.5, 0.56)]);
        }
        Icon::Close => {
            polyline(stroke, &[(0.15, 0.15), (0.85, 0.85)]);
            polyline(stroke, &[(0.85, 0.15), (0.15, 0.85)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use egui_kittest::{Harness, kittest::Queryable};

    use crate::{
        core::PhysicalKey,
        protocol::{DisplayKey, KeySlot, UiSnapshot},
        settings::Settings,
    };

    fn baseline_state() -> State {
        let draft = Settings::default();
        State {
            connected: true,
            inputs: super::super::state::TimingInputs::from_timing(&draft.timing),
            draft: Some(draft),
            snapshot: Some(UiSnapshot {
                filter_enabled: true,
                saved: Settings::default(),
                draft: Settings::default(),
                keys: std::array::from_fn(|index| DisplayKey {
                    physical: PhysicalKey::new(0x11 + index as u16, false),
                    name: ["W", "S", "A", "D"][index].into(),
                }),
                capture_slot: None,
                measurement_active: false,
                measurement: None,
            }),
            ..State::default()
        }
    }

    /// The header's own frame routes through `state::update`, so a click is
    /// observable as a state change rather than a returned message.
    struct FakeRuntime {
        state: State,
    }

    impl FakeRuntime {
        fn frame(&mut self, ui: &mut Ui) {
            let messages = header(ui, &self.state);
            for message in messages {
                let _ = super::super::state::update(&mut self.state, message);
            }
        }
    }

    #[test]
    fn the_header_controls_open_the_panels() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                },
            );

        harness.get_by_label("Profile slots").click();
        harness.run();
        assert!(matches!(
            harness.state().state.profiles,
            super::super::state::ProfileDialog::List
        ));

        harness.get_by_label("Language").click();
        harness.run();
        assert!(matches!(
            harness.state().state.profiles,
            super::super::state::ProfileDialog::Languages
        ));
    }

    #[test]
    fn the_engine_control_asks_the_engine_to_toggle() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                },
            );

        harness.get_by_label("Engine on/off").click();
        harness.run();
        assert_eq!(harness.state().state.pending_filter, Some(false));
    }

    #[test]
    fn the_dirty_badge_follows_the_draft() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, state: &mut State| dirty_badge(ui, state),
                baseline_state(),
            );

        assert!(
            harness.query_by_label("Unsaved Draft Changes").is_none(),
            "a clean draft must not show the badge"
        );

        // One timing value is enough to move the draft away from the snapshot.
        harness
            .state_mut()
            .draft
            .as_mut()
            .unwrap()
            .timing
            .socd_transition_max_micros += 1;
        harness.run();
        harness.get_by_label("Unsaved Draft Changes");
    }

    /// Renders one status dot in `state` and runs one frame.
    fn dot_harness(state: State) -> Harness<'static, State> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(160.0, 40.0))
            .build_ui_state(|ui, state: &mut State| status_dot(ui, state), state);
        harness.run();
        harness
    }

    /// The shortest repaint request the last frame made; `Duration::MAX` when
    /// the frame requested none.
    fn repaint_delay<State>(harness: &Harness<'_, State>) -> Duration {
        harness
            .output()
            .viewport_output
            .values()
            .map(|output| output.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX)
    }

    /// Every `Shape::Rect` this frame painted, flattened out of `Shape::Vec`.
    fn painted_rects<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::RectShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(rect) => out.push(rect.clone()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &harness.output().shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    /// Every `Shape::Circle` this frame painted, flattened out of `Shape::Vec`.
    fn painted_circles<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::CircleShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::CircleShape>) {
            match shape {
                egui::Shape::Circle(circle) => out.push(*circle),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &harness.output().shapes {
            collect(&clipped.shape, &mut out);
        }
        out
    }

    /// Compares two colours channel-wise, tolerating the sub-byte rounding of
    /// a fade factor at an arbitrary animation instant.
    fn close(left: Color32, right: Color32, tolerance: u8) -> bool {
        left.to_srgba_unmultiplied()
            .iter()
            .zip(right.to_srgba_unmultiplied().iter())
            .all(|(left, right)| left.abs_diff(*right) <= tolerance)
    }

    /// Whether the frame painted the 8px dot and its 4px ring in the given
    /// inks (each within a two-byte tolerance for fades).
    fn ring_and_dot<State>(harness: &Harness<'_, State>, ring: Color32, dot: Color32) -> bool {
        let circles = painted_circles(harness);
        let ring_painted = circles.iter().any(|circle| {
            (circle.radius - (STATUS_DOT / 2.0 + STATUS_RING)).abs() < 0.01
                && close(circle.fill, ring, 2)
        });
        let dot_painted = circles.iter().any(|circle| {
            (circle.radius - STATUS_DOT / 2.0).abs() < 0.01 && close(circle.fill, dot, 2)
        });
        ring_painted && dot_painted
    }

    #[test]
    fn the_title_and_status_are_separated_by_the_reference_divider() {
        let harness = Harness::builder()
            .with_size(egui::vec2(1040.0, 800.0))
            .build_ui_state(
                |ui, runtime: &mut FakeRuntime| runtime.frame(ui),
                FakeRuntime {
                    state: baseline_state(),
                },
            );

        let dividers: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| {
                rect.fill == theme::BORDER
                    && (rect.rect.width() - 1.0).abs() < 0.01
                    && (rect.rect.height() - 16.0).abs() < 0.01
            })
            .collect();
        assert_eq!(dividers.len(), 1, "the header draws one 1px x 16px divider");
        assert!(
            dividers[0].rect.center().x > harness.get_by_label("LastKey").rect().right(),
            "the divider must follow the title"
        );
    }

    #[test]
    fn the_dot_state_follows_the_reference_precedence() {
        let state = baseline_state();
        assert_eq!(DotState::of(&state), DotState::Synced);

        // Measuring and rebinding are the middle states.
        let mut measuring = baseline_state();
        measuring.snapshot.as_mut().unwrap().measurement_active = true;
        assert_eq!(DotState::of(&measuring), DotState::Measuring);
        let mut rebinding = baseline_state();
        rebinding.snapshot.as_mut().unwrap().capture_slot = Some(KeySlot::VerticalFirst);
        assert_eq!(DotState::of(&rebinding), DotState::Rebinding);
        let mut both = baseline_state();
        both.snapshot.as_mut().unwrap().measurement_active = true;
        both.snapshot.as_mut().unwrap().capture_slot = Some(KeySlot::VerticalFirst);
        assert_eq!(
            DotState::of(&both),
            DotState::Measuring,
            "measuring wins over rebinding"
        );

        // The inactive state wins over both, for an engine off or a
        // disconnected runtime.
        let mut engine_off = both;
        engine_off.snapshot.as_mut().unwrap().filter_enabled = false;
        assert_eq!(DotState::of(&engine_off), DotState::Idle);
        let mut disconnected = baseline_state();
        disconnected.connected = false;
        assert_eq!(DotState::of(&disconnected), DotState::Idle);
    }

    #[test]
    fn the_synced_dot_wears_the_emerald_ring() {
        let harness = dot_harness(baseline_state());
        assert!(
            ring_and_dot(&harness, EMERALD_100, theme::EMERALD_500),
            "the synchronized dot must paint the emerald ring and dot"
        );
        assert!(
            repaint_delay(&harness) > Duration::from_millis(100),
            "a static dot must not request animation frames"
        );
    }

    #[test]
    fn the_measuring_dot_pulses_amber() {
        let mut harness = dot_harness(baseline_state());
        harness
            .state_mut()
            .snapshot
            .as_mut()
            .unwrap()
            .measurement_active = true;
        // A pulsing dot keeps requesting frames, so the frame is stepped with
        // its cycle pinned to zero rather than run to completion.
        harness.input_mut().time = Some(0.0);
        harness.step();
        assert!(
            ring_and_dot(&harness, AMBER_100, theme::AMBER_500),
            "the measuring dot must paint the amber ring and dot"
        );
        assert!(
            repaint_delay(&harness) <= Duration::from_millis(50),
            "the pulse must schedule its own frames"
        );
    }

    #[test]
    fn the_rebinding_dot_pings_indigo() {
        let mut harness = dot_harness(baseline_state());
        harness.state_mut().snapshot.as_mut().unwrap().capture_slot = Some(KeySlot::VerticalFirst);
        // A pinging dot keeps requesting frames, so the frame is stepped with
        // its cycle pinned to zero rather than run to completion.
        harness.input_mut().time = Some(0.0);
        harness.step();
        assert!(
            ring_and_dot(&harness, theme::INDIGO_100, INDIGO_500),
            "the rebinding dot must paint the indigo ring and dot"
        );
        assert!(
            repaint_delay(&harness) <= Duration::from_millis(50),
            "the ping must schedule its own frames"
        );
    }

    #[test]
    fn the_unfocused_dot_paints_the_frame_but_requests_no_repaint() {
        // Both animations stop when the window is deactivated: the dot still
        // paints its current frame, but it must request no frames at all
        // (ui.md:27-31), like the preview clock and the timeline playhead.
        let mut measuring = dot_harness(baseline_state());
        measuring
            .state_mut()
            .snapshot
            .as_mut()
            .unwrap()
            .measurement_active = true;
        measuring.state_mut().focused = false;
        measuring.input_mut().time = Some(0.0);
        measuring.step();
        assert!(
            repaint_delay(&measuring) > Duration::from_millis(100),
            "an unfocused pulse must request no frames"
        );
        assert!(
            ring_and_dot(&measuring, AMBER_100, theme::AMBER_500),
            "the unfocused pulse still paints its current frame"
        );

        let mut rebinding = dot_harness(baseline_state());
        rebinding
            .state_mut()
            .snapshot
            .as_mut()
            .unwrap()
            .capture_slot = Some(KeySlot::VerticalFirst);
        rebinding.state_mut().focused = false;
        rebinding.input_mut().time = Some(0.0);
        rebinding.step();
        assert!(
            repaint_delay(&rebinding) > Duration::from_millis(100),
            "an unfocused ping must request no frames"
        );
        assert!(
            ring_and_dot(&rebinding, theme::INDIGO_100, INDIGO_500),
            "the unfocused ping still paints its current frame"
        );
    }

    #[test]
    fn the_idle_dot_stays_slate_without_an_animation() {
        let mut harness = dot_harness(baseline_state());
        harness.state_mut().connected = false;
        harness.run();
        assert!(
            ring_and_dot(&harness, theme::SLATE_100, theme::SLATE_300),
            "the inactive dot must paint the slate ring and dot"
        );
        assert!(
            repaint_delay(&harness) > Duration::from_millis(100),
            "the inactive dot must not request animation frames"
        );
    }

    #[test]
    fn the_pulse_and_ping_waves_match_the_reference_cycles() {
        // The static states paint their exact inks and request nothing.
        let synced = DotState::Synced.paint(123.0);
        assert_eq!(synced.dot, theme::EMERALD_500);
        assert_eq!(synced.ring, EMERALD_100);
        assert!(!synced.animating);

        // `animate-pulse`: full opacity at the cycle start, half at its end.
        let start = DotState::Measuring.paint(0.0);
        assert_eq!(start.dot, theme::AMBER_500);
        assert!(start.animating);
        let halfway = DotState::Measuring.paint(PULSE_SECONDS / 2.0);
        assert!(close(halfway.dot, theme::AMBER_500.gamma_multiply(0.5), 1));

        // `animate-ping`: the circle doubles and vanishes by the 75% keyframe.
        let start = DotState::Rebinding.paint(0.0);
        assert_eq!(start.dot, INDIGO_500);
        assert_eq!(start.dot_radius, STATUS_DOT / 2.0);
        assert!(start.animating);
        let gone = DotState::Rebinding.paint(PING_SECONDS * PING_RISE);
        assert_eq!(gone.dot_radius, STATUS_DOT);
        assert_eq!(gone.ring_radius, STATUS_DOT + 2.0 * STATUS_RING);
        assert_eq!(gone.dot.to_srgba_unmultiplied()[3], 0);
        assert!(gone.animating);
    }
}
