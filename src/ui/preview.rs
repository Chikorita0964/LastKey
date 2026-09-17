//! Illustrative examples only. This module never reads or predicts monitor output.
//!
//! T5 owns the mode preview: the card mounted below the mode selector in
//! Immediate mode, its three examples, the 850 ms four-phase clock, and the
//! transport controls. The Iced sources are `iced-ui/app.rs::timing_preview`
//! and the `preview::clock` widget in `iced-ui/preview.rs`; the transport
//! glyphs are procedural paths ported from `iced-ui/icons.rs`, so the card
//! depends on no bundled font glyph coverage (R2 round 1 issue 7).

use std::time::Duration;

use egui::{
    Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, Painter, Pos2, Rect, Response,
    Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType,
};

use super::{
    language::Language,
    message::{Message, PreviewAction},
    theme, timing,
};
use crate::settings::{SocdMode, TimingSettings};

#[derive(Default)]
pub struct Preview {
    pub example: usize,
    pub phase: usize,
    pub playing: bool,
}

impl Preview {
    pub fn update(&mut self, action: PreviewAction) {
        match action {
            PreviewAction::Previous => {
                self.example = (self.example + 2) % 3;
                self.phase = 0;
            }
            PreviewAction::Next => {
                self.example = (self.example + 1) % 3;
                self.phase = 0;
            }
            PreviewAction::Toggle => self.playing = !self.playing,
            PreviewAction::Tick if self.playing => self.phase = (self.phase + 1) % 4,
            PreviewAction::Tick => {}
        }
    }

    pub fn held(&self) -> (bool, bool) {
        (
            self.phase == 0 || (self.phase == 1 && self.example == 2),
            self.phase >= 2 || (self.phase == 1 && self.example != 1),
        )
    }
}

pub fn delay_label(min: u32, max: u32) -> String {
    let number = |micros| {
        let value = format!("{:.1}", micros as f32 / 1_000.0);
        value.strip_suffix(".0").unwrap_or(&value).to_owned()
    };
    if min == max {
        format!("{} ms", number(min))
    } else {
        format!("{}~{} ms", number(min), number(max))
    }
}

/// The three examples the preview illustrates, in order.
const EXAMPLES: [SocdMode; 3] = [
    SocdMode::Immediate,
    SocdMode::PressDelay,
    SocdMode::ReleaseDelay,
];

/// Four phases advance every 850 ms while playing (ui.md:139-143).
const TICK_SECONDS: f64 = 0.85;
/// The nav button's box: the Iced 18px glyph plus its 9px padding.
const NAV_BUTTON: f32 = 36.0;
/// The play pill's corner radius (the Iced `preview_pill` `radius: 16`).
const PILL_RADIUS: CornerRadius = CornerRadius::same(16);
/// The unselected example dot: the Iced `app.rs:1351` literal
/// `Color::from_rgb8(0xcb, 0xd5, 0xe1)`, one byte off [`theme::SLATE_300`].
const DOT_IDLE: Color32 = Color32::from_rgb(0xcb, 0xd5, 0xe1);
/// The neutral gap dot (`w-2.5 h-2.5`). The reference lets the Press Delay
/// gap -- the only phase the neutral dot renders in -- highlight it at
/// `scale-110` with its `shadow-2xs` lift.
const NEUTRAL_DOT: f32 = 10.0;
const NEUTRAL_DOT_HIGHLIGHT: f32 = 1.1;
/// The highlight ink, the reference's `bg-indigo-500`. Tailwind v4's
/// `oklch(58.5% 0.233 277.117)` resolves to `#615fff`; the same conversion
/// reproduces the theme's verified indigo-400/600/700 and slate-300
/// read-backs exactly. The theme publishes no indigo-500 token, so the value
/// stays beside its only consumer.
const NEUTRAL_DOT_INDIGO: Color32 = Color32::from_rgb(0x61, 0x5f, 0xff);
/// The highlight's `shadow-2xs` lift (`0 1px rgb(0 0 0 / 0.05)`).
const NEUTRAL_DOT_SHADOW: egui::epaint::Shadow = egui::epaint::Shadow {
    offset: [0, 1],
    blur: 0,
    spread: 0,
    color: Color32::from_black_alpha(13),
};

/// The 850 ms four-phase clock's timing state. egui is immediate-mode, so the
/// card keeps this in the frame's temp memory keyed by the card instead of a
/// widget tree; the Iced `Clock` widget's own state is the counterpart.
#[derive(Clone, Default)]
pub struct PreviewClock {
    deadline: Option<f64>,
    example: usize,
}

impl PreviewClock {
    /// Advance to `now` (egui's monotonic input time, seconds) and report
    /// whether a tick is due. Not playing or not visible clears the deadline,
    /// matching the Iced clock unmounting and restarting in those states.
    /// Pure by design: tests drive it with explicit times, never a sleep.
    pub fn poll(&mut self, now: f64, playing: bool, visible: bool, example: usize) -> bool {
        if !playing || !visible {
            self.deadline = None;
            self.example = example;
            return false;
        }
        if self.example != example {
            self.deadline = None;
            self.example = example;
        }
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.deadline = Some(now + TICK_SECONDS);
                true
            }
            None => {
                self.deadline = Some(now + TICK_SECONDS);
                false
            }
            Some(_) => false,
        }
    }

    /// Seconds until the next tick, for the repaint request.
    pub fn remaining(&self, now: f64) -> Option<f64> {
        self.deadline.map(|deadline| (deadline - now).max(0.0))
    }
}

/// The mode preview card: Previous / Play-Pause pill / Next, the A-D example
/// illustration, the output-state caption, the example-position indicator, and
/// the animation clock.
///
/// Display-only apart from the three transport controls; the caller maps a
/// click to a `Message` (migration constraint 2). `awake` mirrors window
/// focus; `clock_mounted` mirrors the Iced view's clock mount, which was
/// `matches!(self.profiles, ProfileDialog::Closed)` (`iced-ui/app.rs:1357-1363`)
/// -- pass `false` while a profile dialog is open and the clock deadline
/// resets, no tick is emitted, and no frame is requested, which is the
/// unmount-and-restart behaviour of the Iced widget. The caller decides
/// whether to mount the card at all the same way the Iced view did.
pub fn preview_card(
    ui: &mut Ui,
    timing: &TimingSettings,
    preview: &Preview,
    language: Language,
    awake: bool,
    clock_mounted: bool,
    messages: &mut Vec<Message>,
) -> Response {
    let mode = EXAMPLES[preview.example % 3];
    let (min, max) = match mode {
        SocdMode::PressDelay => (
            timing.socd_transition_min_micros,
            timing.socd_transition_max_micros,
        ),
        SocdMode::ReleaseDelay => (
            timing.preserved_overlap_min_micros,
            timing.preserved_overlap_max_micros,
        ),
        _ => (0, 0),
    };
    let delay = delay_label(min, max);
    let (old, new) = preview.held();

    let frame = theme::slot_style()
        .inner_margin(Margin::same(12))
        .show(ui, |ui| -> Rect {
            let mut clock_rect = Rect::NOTHING;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                if nav_button(ui, true, language.text("Previous example")) {
                    messages.push(Message::Preview(PreviewAction::Previous));
                }
                let content_width = (ui.available_width() - NAV_BUTTON - 8.0).max(0.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(content_width, 0.0),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui.set_width(content_width);
                        ui.spacing_mut().item_spacing = Vec2::new(0.0, 10.0);
                        if play_pill(ui, mode, preview, language) {
                            messages.push(Message::Preview(PreviewAction::Toggle));
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(16.0, 0.0);
                            example_key(ui, "A", true, old, theme::PRIMARY_TEXT);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
                                // The reference highlights the neutral dot only
                                // in the Press Delay gap (`phase === 1 &&
                                // example === 1`), which is the one phase the
                                // neutral state renders in.
                                key_indicator(
                                    ui,
                                    old,
                                    new,
                                    mode == SocdMode::PressDelay && preview.phase == 1,
                                );
                                delay_badge(ui, &delay, mode, preview.phase);
                            });
                            example_key(ui, "D", false, new, theme::PURPLE_600);
                        });
                        // The caption names the game's resolved output, not a
                        // raw input pair: during the Release Delay overlap the
                        // old direction stays held, so the pair state is
                        // described as an overlap rather than two outputs.
                        let state_key = match (old, new) {
                            (true, true) => "Opposite-direction overlap",
                            (false, false) => "Neutral gap (neither key active)",
                            (true, false) => "A output active",
                            (false, true) => "D output active",
                        };
                        caption(ui, language.text(state_key));
                        example_dots(ui, preview.example);
                        // The Iced view mounted its `Clock` state widget here,
                        // as the example column's last child. It paints
                        // nothing; its own rect is what the viewport gate
                        // reads, so scrolling the card out stops the frames
                        // (ui.md:144-145).
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 0.0),
                            Sense::hover(),
                        );
                        clock_rect = rect;
                    },
                );
                if nav_button(ui, false, language.text("Next example")) {
                    messages.push(Message::Preview(PreviewAction::Next));
                }
            });
            clock_rect
        });
    let clock_rect = frame.inner;
    let response = frame.response;

    drive_clock(ui, preview, clock_rect, awake, clock_mounted, messages);
    response
}

/// The card's clock: pushes `PreviewAction::Tick` when a phase is due and
/// requests a repaint for the next one, but only while playing, awake, and
/// mounted, and only while its own widget rect is on screen -- the rules the
/// Iced `Clock` widget enforced with its own redraw requests (ui.md:144-145:
/// the preview starts paused and its widget stops requesting redraws outside
/// the scroll viewport; the widget unmounted while a dialog was open).
fn drive_clock(
    ui: &Ui,
    preview: &Preview,
    rect: Rect,
    awake: bool,
    clock_mounted: bool,
    messages: &mut Vec<Message>,
) {
    let clock_id = egui::Id::new("ui-preview-clock");
    let now = ui.input(|input| input.time);
    let playing = preview.playing && awake && clock_mounted;
    let visible = ui.is_rect_visible(rect);
    let tick = ui.data_mut(|data| {
        data.get_temp_mut_or_default::<PreviewClock>(clock_id).poll(
            now,
            playing,
            visible,
            preview.example,
        )
    });
    if tick {
        messages.push(Message::Preview(PreviewAction::Tick));
    }
    let remaining = ui.data_mut(|data| {
        data.get_temp_mut_or_default::<PreviewClock>(clock_id)
            .remaining(now)
    });
    if let Some(remaining) = remaining {
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(remaining));
    }
}

/// The pill: "Preview" + the example's mode + a play/stop glyph. The Iced
/// `theme::preview_pill` fills it with the example's accent while playing and
/// keeps the outlined neutral shell otherwise; the glyphs are procedural, not
/// the mount point's font characters.
fn play_pill(ui: &mut Ui, mode: SocdMode, preview: &Preview, language: Language) -> bool {
    let glyph = 12.0;
    let label_galley = ui.painter().layout_no_wrap(
        language.text("Preview").to_owned(),
        FontId::new(10.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let value_galley = ui.painter().layout_no_wrap(
        timing::mode_label(mode, language).to_owned(),
        FontId::new(11.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let height = 2.0 * 5.0 + label_galley.size().y.max(value_galley.size().y).max(glyph);
    let width = 2.0 * 12.0 + label_galley.size().x + 6.0 + value_galley.size().x + 6.0 + glyph;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            language.text(if preview.playing {
                "Pause preview"
            } else {
                "Play preview"
            }),
        )
    });

    let hovered = response.hovered();
    let accent = timing::mode_color(mode);
    let (fill, edge, label_ink, value_ink, glyph_ink) = if preview.playing {
        (
            if hovered {
                theme::shade(accent, 0.88)
            } else {
                accent
            },
            theme::shade(accent, 0.8),
            Color32::from_rgba_unmultiplied(255, 255, 255, 191),
            theme::SURFACE,
            theme::SURFACE,
        )
    } else {
        (
            if hovered {
                theme::HOVER_WASH
            } else {
                theme::SURFACE
            },
            theme::BORDER,
            theme::MUTED_TEXT,
            theme::BODY_TEXT,
            theme::MUTED_TEXT,
        )
    };

    let painter = ui.painter();
    painter.rect(
        rect,
        PILL_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Middle,
    );
    let center_y = rect.center().y;
    let mut x = rect.left() + 12.0;
    painter.galley(
        Pos2::new(x, center_y - label_galley.size().y / 2.0),
        label_galley.clone(),
        label_ink,
    );
    x += label_galley.size().x + 6.0;
    theme::stamp_galley(
        painter,
        Pos2::new(x, center_y - value_galley.size().y / 2.0),
        &value_galley,
        value_ink,
        11.0,
    );
    x += value_galley.size().x + 6.0;
    let glyph_rect = Rect::from_min_size(Pos2::new(x, center_y - glyph / 2.0), Vec2::splat(glyph));
    if preview.playing {
        paint_stop(painter, glyph_rect, glyph_ink);
    } else {
        paint_play(painter, glyph_rect, glyph_ink);
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// One example key: the Iced `key` closure -- a 52x52 keycap that fills with
/// the key's accent while held, carrying the key letter over a direction
/// glyph. Normal carries the keycap style's black-5% drop shadow; Pressed
/// takes the accent ring at the theme's pressed glow blur.
fn example_key(ui: &mut Ui, name: &str, left: bool, held: bool, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
    let (fill, edge, ink, arrow_ink) = if held {
        (accent, accent, Color32::WHITE, Color32::WHITE)
    } else {
        (theme::SURFACE, theme::BORDER, theme::BODY_TEXT, accent)
    };
    let radius = CornerRadius::same(12);
    let painter = ui.painter();
    if held {
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 0],
                blur: theme::KEYCAP_PRESSED_GLOW_BLUR,
                spread: 0,
                color: accent,
            }
            .as_shape(rect, radius),
        );
    } else {
        painter.add(theme::SHADOW_KEYCAP.as_shape(rect, radius));
    }
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(2.0, edge),
        egui::StrokeKind::Inside,
    );
    let letter = painter.layout_no_wrap(
        name.to_owned(),
        FontId::new(18.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    theme::stamp_galley(
        painter,
        Pos2::new(
            rect.center().x - letter.size().x / 2.0,
            rect.center().y - 8.0 - letter.size().y / 2.0,
        ),
        &letter,
        ink,
        18.0,
    );
    paint_arrow(
        painter,
        Rect::from_center_size(
            Pos2::new(rect.center().x, rect.center().y + 11.0),
            Vec2::splat(12.0),
        ),
        left,
        arrow_ink,
    );
}

/// The game's view between the two example keys: an arrow for one live key,
/// the reference's neutral gap dot for none, and its short bar when both are
/// live. `gap_highlight` is the Press Delay gap phase, where the reference
/// raises the neutral dot in indigo.
fn key_indicator(ui: &mut Ui, old: bool, new: bool, gap_highlight: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(60.0, 26.0), Sense::hover());
    let painter = ui.painter();
    match (old, new) {
        (true, false) => {
            paint_arrow(
                painter,
                Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
                true,
                theme::PRIMARY_TEXT,
            );
        }
        (false, true) => {
            paint_arrow(
                painter,
                Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
                false,
                theme::PURPLE_600,
            );
        }
        (false, false) => {
            // The reference's neutral dot: `w-2.5 h-2.5 rounded-full
            // bg-slate-300`, highlighted `bg-indigo-500 scale-110 shadow-2xs`
            // during the Press Delay gap.
            let (color, diameter) = if gap_highlight {
                (NEUTRAL_DOT_INDIGO, NEUTRAL_DOT * NEUTRAL_DOT_HIGHLIGHT)
            } else {
                (theme::SLATE_300, NEUTRAL_DOT)
            };
            if gap_highlight {
                painter.add(NEUTRAL_DOT_SHADOW.as_shape(
                    Rect::from_center_size(rect.center(), Vec2::splat(diameter)),
                    theme::CIRCLE_RADIUS,
                ));
            }
            painter.circle_filled(rect.center(), diameter / 2.0, color);
        }
        (true, true) => {
            painter.rect_filled(
                Rect::from_center_size(rect.center(), Vec2::new(18.0, 3.0)),
                CornerRadius::same(2),
                theme::VIOLET_500,
            );
        }
    }
}

/// The delay badge under the indicator; phase 1 highlights the configured
/// range with the example's accent (ui.md:143), including 0 ms for Immediate.
fn delay_badge(ui: &mut Ui, delay: &str, mode: SocdMode, phase: usize) {
    let highlighted = phase == 1;
    let fill = if highlighted {
        timing::mode_color(mode)
    } else {
        theme::SLATE_100
    };
    let ink = if highlighted {
        Color32::WHITE
    } else {
        theme::MUTED_TEXT
    };
    Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.colored_label(
                ink,
                egui::RichText::new(delay).font(FontId::new(10.0, theme::UI_FONT)),
            );
        });
}

/// The example caption. Painted text still publishes its node, so a label
/// query observes which state the A-D pair resolves to.
fn caption(ui: &mut Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(12.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), text));
    theme::stamp_galley(ui.painter(), rect.min, &galley, theme::BODY_TEXT, 12.0);
}

/// Narrow example-position indicator: one slot per example, the selected one
/// elongated in its own accent (reference dots).
fn example_dots(ui: &mut Ui, selected_example: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
        for (example, mode) in EXAMPLES.iter().enumerate() {
            let selected = example == selected_example;
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(if selected { 20.0 } else { 6.0 }, 6.0),
                Sense::hover(),
            );
            let color = if selected {
                timing::mode_color(*mode)
            } else {
                DOT_IDLE
            };
            ui.painter().rect_filled(rect, CornerRadius::same(3), color);
        }
    });
}

/// A transport control: the Iced `nav_button` (18px glyph, 9px padding,
/// circular hover wash). The glyph is a path, not a font character.
fn nav_button(ui: &mut Ui, left: bool, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(NAV_BUTTON), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter()
            .circle_filled(rect.center(), NAV_BUTTON / 2.0, theme::HOVER_WASH);
    }
    let ink = if response.hovered() {
        theme::INDIGO_600
    } else {
        theme::ICON_MUTED
    };
    paint_chevron(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        left,
        ink,
    );
    response.clicked()
}

/// The Iced `ChevronLeft`/`ChevronRight` glyph: a tall stroke, half-width
/// 0.15 and half-height 0.38 around the center, stroked heavier than body
/// icons (`iced-ui/icons.rs:432-446`).
fn paint_chevron(painter: &Painter, rect: Rect, left: bool, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.11).max(1.8), color);
    let (tip, base) = if left { (0.35, 0.65) } else { (0.65, 0.35) };
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    painter.add(Shape::line(
        vec![point(base, 0.12), point(tip, 0.5), point(base, 0.88)],
        stroke,
    ));
}

/// The Iced `Play` glyph: the filled triangle inset by the icon's padding
/// (`iced-ui/icons.rs:394-406`).
fn paint_play(painter: &Painter, rect: Rect, color: Color32) {
    let size = rect.width().min(rect.height());
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    painter.add(Shape::convex_polygon(
        vec![point(0.23, 0.15), point(0.85, 0.5), point(0.23, 0.85)],
        color,
        Stroke::NONE,
    ));
}

/// The Iced `Stop` glyph: the filled square at `0.2..0.8` of the box
/// (`iced-ui/icons.rs:88-91`).
fn paint_stop(painter: &Painter, rect: Rect, color: Color32) {
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(
                rect.left() + rect.width() * 0.2,
                rect.top() + rect.height() * 0.2,
            ),
            Vec2::new(rect.width() * 0.6, rect.height() * 0.6),
        ),
        CornerRadius::ZERO,
        color,
    );
}

/// The Iced `ArrowLeft`/`ArrowRight` glyph (`iced-ui/icons.rs:407-423`), used
/// by the example keys and the game-view indicator.
fn paint_arrow(painter: &Painter, rect: Rect, left: bool, color: Color32) {
    let size = rect.width().min(rect.height());
    let stroke = Stroke::new((size * 0.1).max(1.2), color);
    let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
    if left {
        painter.add(Shape::line(
            vec![point(0.85, 0.5), point(0.15, 0.5)],
            stroke,
        ));
        painter.add(Shape::line(
            vec![point(0.43, 0.22), point(0.15, 0.5), point(0.43, 0.78)],
            stroke,
        ));
    } else {
        painter.add(Shape::line(
            vec![point(0.15, 0.5), point(0.85, 0.5)],
            stroke,
        ));
        painter.add(Shape::line(
            vec![point(0.57, 0.22), point(0.85, 0.5), point(0.57, 0.78)],
            stroke,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    struct CardState {
        preview: Preview,
        timing: TimingSettings,
        clock_mounted: bool,
        clip: Option<Rect>,
        messages: Vec<Message>,
    }

    /// A card harness. `clip` narrows the viewport the card is drawn into, to
    /// exercise the scroll-viewport gate; the preview starts paused unless the
    /// caller passes a playing preview.
    fn card_harness_with(
        preview: Preview,
        timing: TimingSettings,
        clock_mounted: bool,
        clip: Option<Rect>,
    ) -> Harness<'static, CardState> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .build_ui_state(
                |ui, state: &mut CardState| {
                    if let Some(clip) = state.clip {
                        ui.set_clip_rect(clip);
                    }
                    let CardState {
                        preview,
                        timing,
                        clock_mounted,
                        messages,
                        ..
                    } = state;
                    let _ = preview_card(
                        ui,
                        timing,
                        preview,
                        Language::English,
                        true,
                        *clock_mounted,
                        messages,
                    );
                },
                CardState {
                    preview,
                    timing,
                    clock_mounted,
                    clip,
                    messages: Vec::new(),
                },
            );
        harness.run_steps(2);
        harness
    }

    fn card_harness(preview: Preview, timing: TimingSettings) -> Harness<'static, CardState> {
        card_harness_with(preview, timing, true, None)
    }

    fn playing_preview() -> Preview {
        Preview {
            playing: true,
            ..Default::default()
        }
    }

    /// The shortest repaint request this frame made; `Duration::MAX` when the
    /// frame requested none.
    fn repaint_delay(harness: &Harness<'_, CardState>) -> Duration {
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

    /// Every path this frame painted, flattened out of `Shape::Vec`.
    fn painted_paths<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::PathShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::PathShape>) {
            match shape {
                egui::Shape::Path(path) => out.push(path.clone()),
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

    /// Every circle this frame painted, flattened out of `Shape::Vec`.
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

    /// Every text drawn this frame, in paint order.
    fn painted_texts<State>(harness: &Harness<'_, State>) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
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

    #[test]
    fn transport_controls_paint_paths_not_font_glyphs() {
        let harness = card_harness(Preview::default(), TimingSettings::default());
        let paths = painted_paths(&harness);
        assert!(
            paths
                .iter()
                .any(|path| path.closed && path.fill == theme::MUTED_TEXT),
            "the paused pill draws the Play triangle as a filled path"
        );
        assert!(
            paths
                .iter()
                .filter(|path| {
                    path.stroke.color == egui::epaint::ColorMode::Solid(theme::ICON_MUTED)
                })
                .count()
                >= 2,
            "both nav chevrons draw as stroked paths"
        );
        let texts = painted_texts(&harness);
        for glyph in ['\u{23F4}', '\u{23F5}', '\u{23F8}', '\u{25B6}'] {
            assert!(
                !texts.iter().any(|text| text.contains(glyph)),
                "the transport controls must not depend on bundled glyphs ({glyph})"
            );
        }
        harness.get_by_label("Play preview");
        harness.get_by_label("Previous example");
        harness.get_by_label("Next example");

        let harness = card_harness(
            Preview {
                playing: true,
                ..Default::default()
            },
            TimingSettings::default(),
        );
        let stop = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.fill == theme::SURFACE
                    && (rect.rect.width() - 7.2).abs() < 0.01
                    && (rect.rect.height() - 7.2).abs() < 0.01
            })
            .expect("the playing pill draws the Stop square");
        assert_eq!(stop.corner_radius, CornerRadius::ZERO);
    }

    #[test]
    fn transport_clicks_emit_their_messages() {
        let mut harness = card_harness(Preview::default(), TimingSettings::default());
        harness.get_by_label("Next example").click();
        harness.run();
        assert!(matches!(
            harness.state().messages.last(),
            Some(Message::Preview(PreviewAction::Next))
        ));
        harness.get_by_label("Previous example").click();
        harness.run();
        assert!(matches!(
            harness.state().messages.last(),
            Some(Message::Preview(PreviewAction::Previous))
        ));
        harness.get_by_label("Play preview").click();
        harness.run();
        assert!(matches!(
            harness.state().messages.last(),
            Some(Message::Preview(PreviewAction::Toggle))
        ));
    }

    #[test]
    fn phase_one_highlights_the_delay_badge() {
        let timing = TimingSettings {
            socd_transition_min_micros: 2_000,
            socd_transition_max_micros: 4_000,
            ..Default::default()
        };
        let badge_fill = |preview: Preview| {
            let harness = card_harness(preview, timing.clone());
            let label = harness.get_by_label("2~4 ms");
            painted_rects(&harness)
                .into_iter()
                .find(|rect| {
                    rect.corner_radius == CornerRadius::same(8)
                        && rect.rect.contains_rect(label.rect())
                })
                .expect("the delay badge paints its rounded rect")
                .fill
        };
        let mode = EXAMPLES[1];
        assert_eq!(
            badge_fill(Preview {
                example: 1,
                phase: 1,
                playing: false,
            }),
            timing::mode_color(mode)
        );
        assert_eq!(
            badge_fill(Preview {
                example: 1,
                phase: 0,
                playing: false,
            }),
            theme::SLATE_100
        );
    }

    #[test]
    fn the_caption_names_every_resolved_state() {
        // Release Delay phase 1 is the physical overlap; Press Delay phase 1
        // is the neutral gap; the two single-key states come from the
        // Immediate example's first two phases.
        let cases = [
            (0, 0, "A output active"),
            (0, 1, "D output active"),
            (1, 1, "Neutral gap (neither key active)"),
            (2, 1, "Opposite-direction overlap"),
        ];
        for (example, phase, label) in cases {
            let harness = card_harness(
                Preview {
                    example,
                    phase,
                    playing: false,
                },
                TimingSettings::default(),
            );
            harness.get_by_label(label);
        }
    }

    #[test]
    fn the_neutral_gap_dot_keeps_the_reference_styling() {
        // The neutral dot renders only in the Press Delay gap phase, where
        // the reference highlights it `bg-indigo-500 scale-110 shadow-2xs`.
        let harness = card_harness(
            Preview {
                example: 1,
                phase: 1,
                playing: false,
            },
            TimingSettings::default(),
        );
        let dot = painted_circles(&harness)
            .into_iter()
            .find(|circle| circle.fill == NEUTRAL_DOT_INDIGO)
            .expect("the gap dot paints the indigo highlight");
        assert!(
            (dot.radius - NEUTRAL_DOT * NEUTRAL_DOT_HIGHLIGHT / 2.0).abs() < 0.01,
            "the highlighted dot keeps the reference's scale-110 diameter"
        );
        assert!(
            painted_rects(&harness).iter().any(|rect| {
                rect.fill == Color32::from_black_alpha(13)
                    && rect.blur_width == 0.0
                    && rect.rect.contains(dot.center)
            }),
            "the highlight keeps the reference's shadow-2xs lift"
        );
    }

    #[test]
    fn the_neutral_dot_rests_on_slate_without_the_gap_highlight() {
        // The reference's resting branch (`bg-slate-300`) cannot be reached
        // through the four-phase card -- the neutral state only exists in the
        // highlighted gap -- so the rule is pinned directly.
        let mut harness = Harness::builder()
            .with_size(egui::vec2(120.0, 60.0))
            .build_ui(|ui| {
                key_indicator(ui, false, false, false);
            });
        harness.run();
        let dot = painted_circles(&harness)
            .into_iter()
            .find(|circle| circle.fill == theme::SLATE_300)
            .expect("the resting neutral dot is slate-300");
        assert!((dot.radius - NEUTRAL_DOT / 2.0).abs() < 0.01);
    }

    #[test]
    fn the_clock_advances_every_850_ms_while_playing() {
        let mut clock = PreviewClock::default();
        assert!(
            !clock.poll(0.0, true, true, 0),
            "mounting schedules the first phase, it does not tick"
        );
        assert!(!clock.poll(0.5, true, true, 0));
        assert!(
            clock.poll(0.9, true, true, 0),
            "the first tick lands at 850 ms"
        );
        assert!(!clock.poll(1.0, true, true, 0));
        assert!(
            clock.poll(1.8, true, true, 0),
            "the next phase follows 850 ms later"
        );

        // Pausing or leaving the viewport clears the deadline; the Iced clock
        // unmounts in those states and restarts on remount.
        assert!(!clock.poll(2.0, false, true, 0));
        assert!(clock.remaining(2.0).is_none());
        assert!(
            !clock.poll(2.0, true, true, 0),
            "remounting schedules afresh"
        );
        assert!(clock.poll(2.9, true, true, 0));

        // A different example restarts the cycle.
        assert!(!clock.poll(3.0, true, true, 1));
        assert!(clock.poll(3.9, true, true, 1));
    }

    #[test]
    fn a_paused_card_emits_no_tick() {
        let harness = card_harness(Preview::default(), TimingSettings::default());
        assert!(
            harness.state().messages.is_empty(),
            "the preview starts paused, so no tick may be emitted"
        );
    }

    /// R2 round-2 finding 1(a): the viewport gate reads the clock widget's own
    /// rect, not the card's, so scrolling the widget out stops the frames
    /// (ui.md:144-145).
    #[test]
    fn the_clock_stops_where_its_widget_leaves_the_viewport() {
        // On screen: the clock schedules its next phase.
        let harness = card_harness_with(playing_preview(), TimingSettings::default(), true, None);
        let visible = repaint_delay(&harness);
        assert!(
            visible <= Duration::from_millis(850),
            "a visible playing clock must schedule the next phase, got {visible:?}"
        );

        // Clipped to the first 16px of the card: the widget rect is outside
        // the viewport, so no tick and no frame request.
        let clip = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 16.0));
        let harness = card_harness_with(
            playing_preview(),
            TimingSettings::default(),
            true,
            Some(clip),
        );
        assert!(
            harness.state().messages.is_empty(),
            "an off-screen clock emits no tick"
        );
        let hidden = repaint_delay(&harness);
        assert!(
            hidden > Duration::from_millis(850),
            "an off-screen clock must stop requesting redraws, got {hidden:?}"
        );
    }

    /// R2 round-2 finding 1(b): the Iced view did not mount its `Clock`
    /// widget while a profile dialog was open (`iced-ui/app.rs:1357-1363`);
    /// `clock_mounted = false` reproduces that unmount, and the clock rule
    /// test pins the fresh schedule after a remount.
    #[test]
    fn an_unmounted_clock_emits_nothing() {
        let harness = card_harness_with(playing_preview(), TimingSettings::default(), false, None);
        assert!(
            harness.state().messages.is_empty(),
            "an unmounted clock emits no tick"
        );
        let delay = repaint_delay(&harness);
        assert!(
            delay > Duration::from_millis(850),
            "an unmounted clock requests no frames, got {delay:?}"
        );
    }

    // The data-model tests ported from `iced-ui/preview.rs`.

    #[test]
    fn badges_keep_integers_and_fractional_ranges_readable() {
        assert_eq!(delay_label(0, 0), "0 ms");
        assert_eq!(delay_label(2000, 2000), "2 ms");
        assert_eq!(delay_label(2100, 4000), "2.1~4 ms");
    }

    #[test]
    fn examples_show_the_resolution_then_the_new_key_without_running_the_filter() {
        let mut preview = Preview::default();
        preview.update(PreviewAction::Tick);
        assert_eq!(preview.phase, 0);
        preview.update(PreviewAction::Toggle);
        for expected in [(false, true), (false, false), (true, true)] {
            assert_eq!(preview.held(), (true, false));
            preview.update(PreviewAction::Tick);
            assert_eq!(preview.held(), expected);
            preview.update(PreviewAction::Tick);
            assert_eq!(preview.held(), (false, true));
            preview.update(PreviewAction::Next);
        }
        assert_eq!(preview.example, 0);
    }
}
