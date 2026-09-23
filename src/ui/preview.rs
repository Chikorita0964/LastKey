//! Illustrative examples only. This module never reads or predicts monitor output.
//!
//! The mode preview: the card mounted below the mode selector in Immediate
//! mode, its three examples, the 850 ms four-phase A -> D -> A loop, and the
//! transport controls. The transport glyphs come from [`theme::Icon`], so the
//! card depends on no bundled font glyph coverage (R2 round 1 issue 7).

use std::time::Duration;

use egui::{
    Align, Color32, FontId, Frame, Id, Layout, Margin, Painter, Pos2, Rect, Response, Sense, Shape,
    Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::{
    keycap,
    language::Language,
    message::{Message, PreviewAction},
    motion, theme, timing,
};
use crate::settings::SocdMode;

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

    /// Whether the loop is on one of its two legs: the phases where the
    /// example's rule, not a resting key, is what the card is showing. The
    /// rests and the legs alternate, so the phase's parity is the test.
    pub fn transitioning(&self) -> bool {
        self.phase % 2 == 1
    }

    /// The pair the card draws: `(A held, D held)`.
    ///
    /// The loop plays A -> D -> A. Phases 0 and 2 rest on a key -- the one the
    /// last leg delivered -- and phases 1 and 3 are the legs between them,
    /// where the example's rule is the whole picture. A gap is a gap whichever
    /// way the loop is moving, and an overlap is too, so only `Immediate`'s
    /// swap has a side: it shows the new key alone, which is D on the way out
    /// and A on the way back.
    pub fn held(&self) -> (bool, bool) {
        // Phase 1 is the leg that lands on D; phase 3 is the one back to A.
        let arriving_at_a = self.phase == 3;
        match self.phase {
            0 => (true, false),
            2 => (false, true),
            _ => match EXAMPLES[self.example % 3] {
                SocdMode::PressDelay => (false, false),
                SocdMode::ReleaseDelay => (true, true),
                // The swap shows the key the leg lands on, so the two legs are
                // mirror images. `RandomMix` is not one of the three examples
                // the card illustrates, so the preview never asks for it; the
                // arm keeps the match total without claiming a picture the card
                // cannot show.
                SocdMode::Immediate | SocdMode::RandomMix => (arriving_at_a, !arriving_at_a),
            },
        }
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

/// The delay range each delay example illustrates, in microseconds.
///
/// The preview is an illustration of how a mode resolves, not a readout of the
/// draft. It is mounted in Immediate mode, where the press- and release-delay
/// fields belong to other modes and are not on screen at all; showing whatever
/// those fields happened to hold would make the same example read differently
/// from one session to the next, and the badge would change width mid-loop. The
/// illustrated ranges are therefore the product's own defaults -- the values
/// `DEFAULT_TIMING_SETTINGS` ships and every profile starts from -- so the
/// preview shows the same two examples on every launch.
///
/// `the_illustrated_ranges_are_the_product_defaults` pins these to
/// [`TimingSettings::default`] so the pair cannot drift apart.
const DEMO_TRANSITION_MICROS: (u32, u32) = (2_000, 4_000);
const DEMO_PRESERVED_MICROS: (u32, u32) = (2_000, 6_000);

/// Four phases advance every 850 ms while playing (ui.md, "Illustrative
/// Preview"). The loop plays A -> D -> A: phases 0 and 2 rest on a key and
/// phases 1 and 3 are the legs between them, so the card shows both directions
/// of the overlap rather than restarting at A.
const TICK_SECONDS: f64 = 0.85;
/// The nav button's box: an 18px glyph plus its 9px padding.
const NAV_BUTTON: f32 = 36.0;
/// The example key's box (the reference's `w-12 h-12 sm:w-13 sm:h-13`, 52px
/// at the settings window's width).
const KEYCAP_BOX: f32 = 52.0;
/// The keycap row's gap (the reference's `gap-5 sm:gap-6`, 24px at the
/// settings window's width).
const KEYCAP_GAP: f32 = 24.0;
/// The indicator box between the keys (the reference's `relative w-12 h-7`).
const INDICATOR_WIDTH: f32 = 48.0;
const INDICATOR_HEIGHT: f32 = 28.0;
/// The keycap row's content width: the two keys and the indicator with a gap
/// between each, 200px. The reference centers that row on the column, so the
/// width is what [`centered_row`] places.
const KEYCAP_ROW_WIDTH: f32 = 2.0 * KEYCAP_BOX + INDICATOR_WIDTH + 2.0 * KEYCAP_GAP;
/// The delay badge's `absolute -bottom-4` hang: the badge is out of the
/// reference's flex flow, so its bottom edge sits 16px past the indicator
/// box's bottom without growing the row.
const BADGE_HANG: f32 = 16.0;
/// The example-position dots (the reference's `h-1.5 rounded-full` slots with
/// a `gap-1.5` rhythm): the selected slot is 20px, the idle ones 6px.
const DOT_HEIGHT: f32 = 6.0;
const DOT_SELECTED: f32 = 20.0;
const DOT_IDLE_WIDTH: f32 = 6.0;
const DOT_GAP: f32 = 6.0;
/// The dots row's content width, centered on the column like the keycap row.
const DOTS_ROW_WIDTH: f32 = DOT_SELECTED + 2.0 * DOT_IDLE_WIDTH + 2.0 * DOT_GAP;
/// The delay badge's own scale states: `scale-95` at rest and `scale-105`
/// while phase 1 highlights it. Paint-time only, like
/// [`keycap::PRESS_SCALE`].
///
/// A CSS `transform: scale()` does not move the layout box, so the reference's
/// absolutely-positioned badge keeps its unscaled box for the `-bottom-4`
/// anchor and only the painted pill grows or shrinks. This port folds the scale
/// into the drawn size and leaves the caller's anchor alone, which reproduces
/// that split.
const BADGE_SCALE_RESTING: f32 = 0.95;
const BADGE_SCALE_ACTIVE: f32 = 1.05;
/// The badge's `px-1.5 py-0.5` padding plus its 1px edge, the
/// `box-sizing: border-box` conversion [`delay_badge_size`] documents.
const BADGE_PAD_X: f32 = 7.0;
const BADGE_PAD_Y: f32 = 3.0;
/// The neutral gap dot (`w-2.5 h-2.5`). The reference lets the Press Delay
/// gap -- the only phase the neutral dot renders in -- highlight it at
/// `scale-110` with its `shadow-2xs` lift.
const NEUTRAL_DOT: f32 = 10.0;
const NEUTRAL_DOT_HIGHLIGHT: f32 = 1.1;
/// The highlight's `shadow-2xs` lift (`0 1px rgb(0 0 0 / 0.05)`).
const NEUTRAL_DOT_SHADOW: egui::epaint::Shadow = egui::epaint::Shadow {
    offset: [0, 1],
    blur: 0,
    spread: 0,
    color: Color32::from_black_alpha(13),
};

/// The 850 ms A -> D -> A clock's timing state. egui is immediate-mode, so
/// the card keeps this in the frame's temp memory keyed by the card instead
/// of a widget tree.
#[derive(Clone, Default)]
pub struct PreviewClock {
    /// When the phase currently on screen began, or `None` while idle.
    anchor: Option<f64>,
    example: usize,
}

impl PreviewClock {
    /// Advance to `now` (egui's monotonic input time, seconds) and report
    /// whether a tick is due. Not playing or not visible clears the anchor,
    /// which restarts the dwell when the card remounts.
    ///
    /// Every phase holds the screen for a full `TICK_SECONDS`: a tick moves
    /// the anchor to `now`, not to the boundary it passed, so two ticks can
    /// never land closer together than one period whatever the frame times or
    /// the leftover state were. This card is an illustration, not a clock --
    /// a phase the user cannot read is worse than a loop that runs late, so
    /// lateness delays the picture and never compresses it.
    ///
    /// Anchoring on an absolute grid instead is what produced the bug this
    /// replaces: a card that was paused, scrolled away, or left behind by a
    /// mode switch came back with a backlog of boundaries already behind it
    /// and cashed them in at frame rate, flashing the whole loop past in a
    /// few frames. Re-anchoring on the wake makes that unreachable rather
    /// than unlikely.
    ///
    /// Pure by design: tests drive it with explicit times, never a sleep.
    pub fn poll(&mut self, now: f64, playing: bool, visible: bool, example: usize) -> bool {
        if !playing || !visible {
            self.anchor = None;
            self.example = example;
            return false;
        }
        if self.example != example {
            self.anchor = None;
            self.example = example;
        }
        match self.anchor {
            Some(anchor) if now - anchor >= TICK_SECONDS => {
                self.anchor = Some(now);
                true
            }
            Some(_) => false,
            // A fresh mount spends a full period on the phase already drawn
            // before advancing, so playing never jump-cuts the first picture.
            None => {
                self.anchor = Some(now);
                false
            }
        }
    }

    /// Seconds until the next tick, for the repaint request.
    pub fn remaining(&self, now: f64) -> Option<f64> {
        self.anchor
            .map(|anchor| (anchor + TICK_SECONDS - now).max(0.0))
    }
}

/// The mode preview card: Previous / Play-Pause pill / Next, the A-D example
/// illustration, the output-state caption, the example-position indicator, and
/// the animation clock.
///
/// Display-only apart from the three transport controls; the caller maps a
/// click to a `Message` (migration constraint 2). `awake` mirrors window
/// focus; `clock_mounted` mirrors whether the clock is mounted at all, which is
/// `matches!(self.profiles, ProfileDialog::Closed)`
/// -- pass `false` while a profile dialog is open and the clock deadline
/// resets, no tick is emitted, and no frame is requested, which is the
/// unmount-and-restart behaviour of the clock. The caller decides
/// whether to mount the card at all the same way.
///
/// The card takes no [`TimingSettings`]: every value it illustrates is a fixed
/// demo range ([`DEMO_TRANSITION_MICROS`]), so nothing the user types into the
/// draft can reach it.
pub fn preview_card(
    ui: &mut Ui,
    preview: &Preview,
    language: Language,
    awake: bool,
    clock_mounted: bool,
    messages: &mut Vec<Message>,
) -> Response {
    let mode = EXAMPLES[preview.example % 3];
    let (min, max) = match mode {
        SocdMode::PressDelay => DEMO_TRANSITION_MICROS,
        SocdMode::ReleaseDelay => DEMO_PRESERVED_MICROS,
        _ => (0, 0),
    };
    let delay = delay_label(min, max);
    let (old, new) = preview.held();

    // The reference (`InputTimingsCard.tsx` `TimingPreview`) sits directly on
    // the timing group's transparent grid container
    // (`grid grid-cols-[36px_minmax(0,1fr)_36px] ... pt-0 pb-1`) with no
    // enclosing grey slot frame and no horizontal inset: the group's own
    // padding is the only margin, and the preview adds just its 4px `pb-1`.
    // The nav buttons flank a centered column of pill, A-D illustration,
    // caption, and dots.
    let frame = Frame::NONE
        .stroke(Stroke::new(1.0, Color32::TRANSPARENT))
        .inner_margin(Margin {
            left: 0,
            right: 0,
            top: 0,
            bottom: 4,
        })
        .show(ui, |ui| -> Rect {
            // The reference grid is `grid-cols-[36px_minmax(0,1fr)_36px]
            // items-center`: the 36px nav buttons center on the whole center
            // column. egui lays horizontal children out top-aligned in order,
            // so the row is composed manually: the center column renders at
            // its final x offset, then the nav buttons are overlaid centered
            // on the measured column height.
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
            let avail = ui.available_rect_before_wrap();
            let content_width = (avail.width() - 2.0 * NAV_BUTTON - 16.0).max(0.0);
            let center_origin = Pos2::new(avail.min.x + NAV_BUTTON + 8.0, avail.min.y);
            let mut center_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        center_origin,
                        Vec2::new(content_width, avail.height()),
                    ))
                    .layout(Layout::top_down(Align::Center)),
            );
            center_ui.set_width(content_width);
            // The reference center column is `flex flex-col items-center
            // gap-2.5 sm:gap-3`: 12px at the card's rendered width. Every
            // child is fixed-size or text, so the layout's interactive floor
            // would inflate each row past its content box.
            center_ui.spacing_mut().item_spacing = Vec2::new(0.0, 12.0);
            center_ui.spacing_mut().interact_size.y = 0.0;
            if play_pill(&mut center_ui, mode, preview, language) {
                messages.push(Message::Preview(PreviewAction::Toggle));
            }
            // The reference's keycap row is `flex items-center justify-center
            // gap-5 sm:gap-6`, and the column is `items-center`: the row
            // shrinks to its 200px content and is then centered. egui's
            // `ui.horizontal` gives its child the column's full width and
            // packs from the left, so the row is placed explicitly instead.
            centered_row(
                &mut center_ui,
                KEYCAP_ROW_WIDTH,
                KEYCAP_BOX,
                KEYCAP_GAP,
                |ui| {
                    example_key(ui, "A", old, &keycap::LEFT);
                    // The reference centers the arrow/dot/overlap bar in a
                    // `relative w-12 h-7` box and overlaps the delay badge at
                    // `-bottom-4`; one combined layout keeps that footprint
                    // instead of stacking two rows. The neutral dot highlights
                    // only in the Press Delay gap, the leg whose picture is
                    // the neutral state itself.
                    center_indicator(
                        ui,
                        old,
                        new,
                        mode == SocdMode::PressDelay && preview.transitioning(),
                        &delay,
                        mode,
                        preview.transitioning(),
                    );
                    example_key(ui, "D", new, &keycap::RIGHT);
                },
            );
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
            caption(&mut center_ui, language.text(state_key));
            // The dots row carries the reference's `pt-0.5` over
            // the column's `gap-2.5` rhythm, and centers like the keycap row.
            center_ui.add_space(2.0);
            centered_row(&mut center_ui, DOTS_ROW_WIDTH, DOT_HEIGHT, DOT_GAP, |ui| {
                example_dots(ui, preview.example);
            });
            // The clock's state widget mounts here, as the
            // example column's last child. It paints nothing; its own rect
            // is what the viewport gate reads, so scrolling the card out
            // stops the frames (ui.md, "Repaint Policy").
            //
            // The nav buttons center on the column's *visible* box, so the
            // column rect is measured before this zero-height allocation:
            // `min_rect` would otherwise grow by the trailing 12px item
            // spacing and drop the nav center 6px below the reference's.
            let column_rect = center_ui.min_rect();
            let clock_rect = center_ui
                .allocate_exact_size(Vec2::new(center_ui.available_width(), 0.0), Sense::hover())
                .0;
            let center_rect = column_rect;
            let nav_y = center_rect.center().y - NAV_BUTTON / 2.0;
            if nav_button_at(
                ui,
                Rect::from_min_size(Pos2::new(avail.min.x, nav_y), Vec2::splat(NAV_BUTTON)),
                true,
                language.text("Previous example"),
            ) {
                messages.push(Message::Preview(PreviewAction::Previous));
            }
            if nav_button_at(
                ui,
                Rect::from_min_size(
                    Pos2::new(avail.min.x + NAV_BUTTON + 8.0 + content_width + 8.0, nav_y),
                    Vec2::splat(NAV_BUTTON),
                ),
                false,
                language.text("Next example"),
            ) {
                messages.push(Message::Preview(PreviewAction::Next));
            }
            // Advance the parent cursor past the composed row.
            ui.allocate_exact_size(
                Vec2::new(avail.width(), center_rect.height()),
                Sense::hover(),
            );
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
/// the clock enforces with its own redraw requests (ui.md, "Repaint Policy":
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
        // The phase advances in `dispatch_all`, after the page has painted, so
        // this frame still shows the outgoing picture. Without a frame to draw
        // the incoming one the card would hold it until the *next* tick, and
        // every phase would reach the screen a whole period late.
        ui.ctx().request_repaint();
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

/// The pill: "PREVIEW" + the example's mode + a play/stop glyph. The pill
/// `theme::preview_pill` fills it with the example's accent while playing and
/// keeps the outlined neutral shell otherwise; the glyphs are the theme's, not
/// the mount point's font characters.
fn play_pill(ui: &mut Ui, mode: SocdMode, preview: &Preview, language: Language) -> bool {
    let glyph = 12.0;
    // The reference renders the label `text-[11px] leading-4`: the same 11px
    // as the mode value beside it, not the 10px the reference file used.
    let label_galley = ui.painter().layout_no_wrap(
        language.text("PREVIEW").to_owned(),
        FontId::new(11.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let value_galley = ui.painter().layout_no_wrap(
        timing::mode_label(mode, language).to_owned(),
        FontId::new(11.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    // The reference pill is `px-3 py-1` around an 18px line box: 12px
    // horizontal, 4px vertical, so it measures 26px tall. The line box is the
    // `leading-4` literal (16px) plus the two 1px borders the reference counts
    // inside its `box-sizing: border-box` height.
    let line_box = 16.0 + 2.0;
    let height = 2.0 * 4.0
        + label_galley
            .size()
            .y
            .max(value_galley.size().y)
            .max(line_box);
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
    // The Preview label is tinted per example while playing: each example
    // takes the `-200` step of its own mode ramp, so the three arms read as
    // the three ramps the mode table lists.
    let active_label = match mode {
        SocdMode::Immediate => theme::BLUE_200,
        SocdMode::ReleaseDelay => theme::VIOLET_200,
        _ => theme::INDIGO_200,
    };
    let (fill, edge, label_ink, value_ink, glyph_ink) = if preview.playing {
        (
            if hovered {
                theme::shade(accent, 0.88)
            } else {
                accent
            },
            theme::shade(accent, 0.8),
            active_label,
            theme::WHITE,
            if hovered {
                theme::INDIGO_100
            } else {
                theme::WHITE
            },
        )
    } else {
        (
            if hovered {
                theme::INDIGO_50_60
            } else {
                theme::WHITE
            },
            theme::SLATE_200,
            if hovered {
                theme::INDIGO_500
            } else {
                theme::SLATE_500
            },
            if hovered {
                theme::INDIGO_700
            } else {
                theme::SLATE_900
            },
            if hovered { accent } else { theme::SLATE_400 },
        )
    };

    let painter = ui.painter();
    // The play pill's corner radius.
    painter.rect(
        rect,
        theme::PILL_RADIUS,
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
        theme::paint_icon(painter, glyph_rect, theme::Icon::Stop, glyph_ink);
    } else {
        theme::paint_icon(painter, glyph_rect, theme::Icon::Play, glyph_ink);
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// One example key: a 52x52 keycap that fills with the key's accent while
/// held, carrying the key letter over a direction glyph. The resting key is
/// `bg-white border border-slate-200/90 shadow-xs`; the held key is
/// `bg-<accent> border border-<accent>-700 shadow-inner scale-95 ring-2
/// ring-<accent>-300`, so it takes a one-step-darker edge, an inset shadow, a
/// tactile compression about its centre, and the soft outer ring.
///
/// `direction` is the key's row in [`keycap`]'s direction table -- A takes
/// [`keycap::LEFT`] and D takes [`keycap::RIGHT`] -- so the example keys share
/// the D-pad's accents, edges, and ring steps rather than carrying a private
/// copy. The letter stays a parameter because the reference prints the key the
/// player presses, not the direction it maps to.
///
/// The chrome -- box, compression, glow, ring, inset shadow, edge -- is
/// [`keycap::paint_surface`]'s, shared with the D-pad and timeline keycaps;
/// only the letter-over-arrow content below belongs to this card.
fn example_key(ui: &mut Ui, name: &str, held: bool, direction: &keycap::Direction) {
    let (accent, edge, arrow_ink, ring) = (
        direction.accent,
        direction.edge,
        direction.accent,
        direction.ring,
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(KEYCAP_BOX), Sense::hover());
    let (fill, border, text_ink, arrow) = if held {
        (accent, edge, theme::WHITE, theme::WHITE)
    } else {
        (theme::WHITE, theme::SLATE_200, theme::SLATE_900, arrow_ink)
    };
    // One scale per key, keyed by its own letter so the two animate
    // independently.
    let scale = keycap::press_scale(ui, Id::new("ui-preview-key-scale").with(name), held);
    keycap::paint_surface(
        ui,
        &keycap::Surface {
            rect,
            radius: theme::CONTROL_RADIUS,
            fill,
            // The reference draws this edge with a `border` utility, which is
            // 1px, not the 2px of the D-pad keycap's own stroke.
            edge: Stroke::new(1.0, border),
            shadow: if held {
                keycap::glow(accent, theme::KEYCAP_PRESSED_GLOW_BLUR)
            } else {
                theme::SHADOW_XS
            },
            inner_shadow: held,
            // The reference's `ring-2 ring-*-300`: a 2px outside stroke on the
            // keycap's own box, outside its 1px edge -- the same class the
            // D-pad's pressed state takes. The ring belongs to the same
            // element, so the `scale-95` compresses it too: 2px becomes 1.9px
            // and the ring hugs the compressed box, which is why the measured
            // ring spans 49.4 + 2 * 1.9 = 53.2px.
            ring: held.then_some((theme::KEYCAP_RING_PRESSED * scale, ring)),
            scale,
        },
        |painter, box_rect| {
            let letter = painter.layout_no_wrap(
                name.to_owned(),
                FontId::new(18.0, theme::UI_FONT),
                Color32::PLACEHOLDER,
            );
            theme::stamp_galley(
                painter,
                Pos2::new(
                    box_rect.center().x - letter.size().x / 2.0,
                    box_rect.center().y - 8.0 - letter.size().y / 2.0,
                ),
                &letter,
                text_ink,
                18.0,
            );
            // The example key's own row picks the direction; the glyph is
            // the shared keycap arrow, so all four directions stay one shape
            // wherever they are drawn.
            keycap::paint_arrow(
                painter,
                Rect::from_center_size(
                    Pos2::new(box_rect.center().x, box_rect.center().y + 11.0),
                    Vec2::splat(12.0),
                ),
                direction.arrow,
                arrow,
            );
        },
    );
}

/// The game's view between the two example keys: an arrow for one live key,
/// the neutral gap dot for none, and a short bar when both are live.
/// `gap_highlight` is the Press Delay gap phase, where the neutral dot is
/// raised in indigo. The two single-key arrows take the same step the example
/// keys' resting arrows take. Paints centered on `center` so both the legacy
/// row and the unified [`center_indicator`] share one glyph rule.
fn indicator_glyph(painter: &Painter, center: Pos2, old: bool, new: bool, gap_highlight: bool) {
    match (old, new) {
        (true, false) => {
            keycap::paint_arrow(
                painter,
                Rect::from_center_size(center, Vec2::splat(24.0)),
                keycap::Arrow::Left,
                theme::INDIGO_600,
            );
        }
        (false, true) => {
            keycap::paint_arrow(
                painter,
                Rect::from_center_size(center, Vec2::splat(24.0)),
                keycap::Arrow::Right,
                theme::PURPLE_600,
            );
        }
        (false, false) => {
            // The reference's neutral dot: `w-2.5 h-2.5 rounded-full
            // bg-slate-300`, highlighted `bg-indigo-500 scale-110 shadow-2xs`
            // during the Press Delay gap.
            let (color, diameter) = if gap_highlight {
                (theme::INDIGO_500, NEUTRAL_DOT * NEUTRAL_DOT_HIGHLIGHT)
            } else {
                (theme::SLATE_300, NEUTRAL_DOT)
            };
            if gap_highlight {
                painter.add(NEUTRAL_DOT_SHADOW.as_shape(
                    Rect::from_center_size(center, Vec2::splat(diameter)),
                    theme::CIRCLE_RADIUS,
                ));
            }
            painter.circle_filled(center, diameter / 2.0, color);
        }
        (true, true) => {
            painter.rect_filled(
                Rect::from_center_size(center, Vec2::new(18.0, 3.0)),
                theme::CIRCLE_RADIUS,
                theme::VIOLET_500,
            );
        }
    }
}

/// The delay badge's own size: the 9px label plus its `px-1.5 py-0.5`
/// padding and its 1px edge. The badge is absolutely positioned in the
/// reference, so callers need this to place it without giving it a layout box.
///
/// The padding is the reference's 6/2 **plus the 1px edge**, the same
/// `box-sizing: border-box` correction the theme documents for every control:
/// a CSS border is drawn inside its box, so the drawn pill is
/// `content + 2 * padding + 2 * border` and reproducing it needs the edge added
/// to the padding. Measured against the reference's own `0 ms` badge: 15.75px
/// tall against this port's 9px line plus 2*(2+1) = 16px, and 31.62px wide
/// against its ~17.7px line plus 2*(6+1) = 31.7px.
fn delay_badge_size(ui: &Ui, delay: &str) -> Vec2 {
    ui.painter()
        .layout_no_wrap(
            delay.to_owned(),
            FontId::new(9.0, theme::UI_FONT),
            Color32::PLACEHOLDER,
        )
        .size()
        + Vec2::new(2.0 * BADGE_PAD_X, 2.0 * BADGE_PAD_Y)
}

/// The highlighted badge's edge and ring, per example.
///
/// The reference writes three near-parallel class strings whose only
/// differences are the example's accent family and how the edge is derived:
///
/// - Immediate: `border-[#2f47c4]/50 ring-2 ring-[#3a55e8]/25` -- both the
///   edge and the ring are literals, and the ring is the accent at a quarter
///   alpha rather than a 200 step.
/// - Press Delay: `border-indigo-700/50 ring-2 ring-indigo-200/90`.
/// - Release Delay: `border-violet-700/50 ring-2 ring-violet-200/90`.
///
/// Both edges are a `-700` step at half alpha. A CSS border paints over its
/// own element's background, so the half-alpha edge composites against the
/// badge's fill rather than the card behind it -- the same `border-box` rule
/// [`accent_tint`](super::theme) models. The two rings are `-200` steps at
/// 90%, which stay translucent because they sit on the group's inset surface
/// rather than on a colour the port can name here.
fn badge_highlight_ink(mode: SocdMode) -> (Color32, Color32) {
    match mode {
        SocdMode::Immediate => (
            theme::over(theme::BLUE_700, 0.5, timing::mode_color(mode)),
            theme::with_alpha(timing::mode_color(mode), 64),
        ),
        SocdMode::ReleaseDelay => (
            theme::over(theme::VIOLET_700, 0.5, timing::mode_color(mode)),
            theme::with_alpha(theme::VIOLET_200, 230),
        ),
        _ => (
            theme::over(theme::INDIGO_700, 0.5, timing::mode_color(mode)),
            theme::with_alpha(theme::INDIGO_200, 230),
        ),
    }
}

/// The delay badge painted at `top` (its top edge), centered on `center_x`.
///
/// The reference badge is a `rounded-full` pill (`px-1.5 py-0.5
/// text-[9px] leading-tight`), and its two states are:
///
/// - at rest, `bg-slate-100/90 text-slate-400 font-medium border
///   border-slate-200/60 opacity-60 scale-95`;
/// - highlighted while the loop is transitioning, the example's accent fill
///   with white bold text, `shadow-xs ring-2` and `scale-105 opacity-100`.
///
/// Both `opacity` and `scale` are folded into the drawn geometry: the reference
/// blends the whole pill toward the card behind it and shrinks the painted box
/// about its centre, neither of which moves its layout box. Publishing the
/// delay text as a label node via `interact` (which claims the rect without
/// touching the layout cursor) keeps the caption-style label query observing
/// the value while the badge stays out of the row's flow, the way
/// `absolute -bottom-4` does in the reference.
fn paint_delay_badge(
    ui: &mut Ui,
    center_x: f32,
    top: f32,
    delay: &str,
    mode: SocdMode,
    transitioning: bool,
) -> Rect {
    let highlighted = transitioning;
    let accent = timing::mode_color(mode);
    let galley = ui.painter().layout_no_wrap(
        delay.to_owned(),
        FontId::new(9.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    // The reference badge is `text-[9px] px-1.5 py-0.5 border`: 6px and 2px
    // of padding around the 9px text, with the 1px edge counted inside the
    // box. See [`delay_badge_size`].
    let unscaled = galley.size() + Vec2::new(2.0 * BADGE_PAD_X, 2.0 * BADGE_PAD_Y);
    // `scale-105` / `scale-95`, about the pill's centre. The badge's `top` is
    // the caller's unscaled anchor, so the scale is applied about that box's
    // centre and the painted pill grows or shrinks into it.
    //
    // The reference reaches this through `transition-all duration-150`, so the
    // scale glides rather than snapping. Only the transform is a continuous
    // quantity here -- the fills and inks are theme tokens with no meaningful
    // midpoint, the same split the example keys keep. One scale, keyed by the
    // badge itself so the two states animate independently of the keys.
    let scale = motion::animate(
        ui,
        Id::new("ui-preview-badge-scale"),
        if highlighted {
            BADGE_SCALE_ACTIVE
        } else {
            BADGE_SCALE_RESTING
        },
        motion::DURATION_150_SECS,
        motion::Easing::Standard,
    );
    let size = unscaled * scale;
    let anchor_center_y = top + unscaled.y / 2.0;
    let rect = Rect::from_min_size(
        Pos2::new(center_x - size.x / 2.0, anchor_center_y - size.y / 2.0),
        size,
    );
    // `rounded-full`: epaint clamps a radius to half the box, so the maximum
    // byte draws the reference's pill exactly.
    let radius = theme::CIRCLE_RADIUS;
    let (fill, ink, edge) = if highlighted {
        (accent, theme::WHITE, badge_highlight_ink(mode).0)
    } else {
        // `opacity-60` on the whole pill: the fill and its hairline are
        // composited toward the card the badge sits over, and the ink is
        // blended the same way the reference's browser blends it. The ink is
        // already slate-400 ([`theme::SLATE_400`]).
        (
            theme::over_card(theme::SLATE_100),
            theme::over_card(theme::SLATE_400),
            theme::over_card(theme::SLATE_200),
        )
    };
    let painter = ui.painter().clone();
    if highlighted {
        // The reference's `shadow-xs` lift, which only the active badge
        // carries. Added before the fill, the way the example keys stack
        // [`theme::SHADOW_XS`].
        painter.add(theme::SHADOW_XS.as_shape(rect, radius));
    }
    painter.rect_filled(rect, radius, fill);
    if highlighted {
        // The reference's active `ring-2`: a 2px band starting at the badge's
        // own edge and reaching 2px past it. `StrokeKind::Outside` already
        // measures outward from `rect`, so the pill must *not* be expanded
        // first -- doing both draws the ring 4px out, twice the reference's
        // width.
        let (_, ring) = badge_highlight_ink(mode);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, ring),
            egui::StrokeKind::Outside,
        );
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    // `leading-tight` (11.25px at 9px) is 2.25px taller than this port's shaped
    // line, so the text centres in the pill rather than sitting on its padding
    // edge, which is what the reference's line box does.
    let text_pos = Pos2::new(
        rect.center().x - galley.size().x / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    // The two states carry different weights in the reference: the resting
    // badge is `font-medium` and the highlighted one `font-bold`. Only the bold
    // one takes the double-stamp approximation; stamping the resting pill would
    // draw it heavier than the reference's medium.
    if highlighted {
        theme::stamp_galley(&painter, text_pos, &galley, ink, 9.0);
    } else {
        painter.galley(text_pos, galley.clone(), ink);
    }
    // Publish the text line (not the padded badge) as the label node, the
    // way the old `Frame` + `colored_label` did: the badge rect contains the
    // label rect, which is what the phase-one highlight test reads. `interact`
    // is deliberate -- `allocate_rect` would advance the row cursor to the
    // absolutely-positioned badge and shove the D keycap left.
    let text_rect = Rect::from_min_size(text_pos, galley.size());
    let id = egui::Id::new("ui-preview-delay-badge").with(delay);
    ui.interact(text_rect, id, Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), delay));
    rect
}

/// The unified center column between the two example keys: the reference's
/// `relative w-12 h-7` indicator box with the delay badge overlapping its
/// bottom boundary (`absolute -bottom-4`). The indicator takes exactly the
/// reference's 48x28 box and the badge hangs below it without entering the
/// row's flow, so the keycap row keeps the reference's 52px height and its
/// 200px content width. Returns the indicator's allocated rect.
fn center_indicator(
    ui: &mut Ui,
    old: bool,
    new: bool,
    gap_highlight: bool,
    delay: &str,
    mode: SocdMode,
    transitioning: bool,
) -> Rect {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(INDICATOR_WIDTH, INDICATOR_HEIGHT), Sense::hover());
    let painter = ui.painter().clone();
    indicator_glyph(&painter, rect.center(), old, new, gap_highlight);
    // `absolute -bottom-4`: the badge's bottom edge sits 16px past the
    // indicator box's bottom, so its top is that anchor minus its own height.
    let badge_height = delay_badge_size(ui, delay).y;
    paint_delay_badge(
        ui,
        rect.center().x,
        rect.max.y + BADGE_HANG - badge_height,
        delay,
        mode,
        transitioning,
    );
    rect
}

/// The pre-unification indicator row, kept for the glyph parity tests: a
/// 60x26 box painting [`indicator_glyph`] at its center.
#[cfg(test)]
fn key_indicator(ui: &mut Ui, old: bool, new: bool, gap_highlight: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(60.0, 26.0), Sense::hover());
    indicator_glyph(ui.painter(), rect.center(), old, new, gap_highlight);
}

/// The example caption: the reference's `text-[11px] leading-4
/// text-slate-600 font-medium text-center min-h-4 tracking-tight`. Painted
/// text still publishes its node, so a label query observes which state the
/// A-D pair resolves to.
fn caption(ui: &mut Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(11.0, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), text));
    theme::stamp_galley(ui.painter(), rect.min, &galley, theme::SLATE_600, 11.0);
}

/// A row of fixed-width children, centered on the column.
///
/// The reference's center column is `flex flex-col items-center`, so every
/// row shrinks to its own content and is then centered. egui's `ui.horizontal`
/// instead hands its child the column's *full* width and packs from the left
/// (`Layout::left_to_right` aligns on `Align2([LEFT, ..])`), which pins a
/// shrink-to-fit row to the left edge. This helper allocates the row's known
/// content width at the column's center and lays the children out inside it,
/// so the row lands where the reference puts it regardless of the column's
/// width. `gap` is the reference's row gap; the column's own 12px rhythm must
/// not leak between the row's children.
fn centered_row(
    ui: &mut Ui,
    content_width: f32,
    height: f32,
    gap: f32,
    add_contents: impl FnOnce(&mut Ui),
) {
    let available = ui.available_rect_before_wrap();
    let left = available.center().x - content_width / 2.0;
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                Pos2::new(left, available.min.y),
                Vec2::new(content_width, height),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.set_width(content_width);
    row.spacing_mut().item_spacing = Vec2::new(gap, 0.0);
    row.spacing_mut().interact_size.y = 0.0;
    add_contents(&mut row);
    // Advance the column cursor past the row's box. The row's painted badge
    // hangs past its own bottom (the reference's `-bottom-4`), so the
    // allocation is the layout box, not the paint bounds.
    ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
}

/// Narrow example-position indicator: one slot per example, the selected one
/// elongated in its own accent. The reference dots are `h-1.5 rounded-full`
/// with a `gap-1.5` rhythm: 20px active, 6px idle, 6px apart. `rounded-full`
/// is a *pill*, not a fixed radius: a 20x6 slot and a 6x6 slot both clamp to
/// half their height, so the elongated one keeps square ends.
fn example_dots(ui: &mut Ui, selected_example: usize) {
    ui.spacing_mut().item_spacing = Vec2::new(DOT_GAP, 0.0);
    for (example, mode) in EXAMPLES.iter().enumerate() {
        let selected = example == selected_example;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(
                if selected {
                    DOT_SELECTED
                } else {
                    DOT_IDLE_WIDTH
                },
                DOT_HEIGHT,
            ),
            Sense::hover(),
        );
        let color = if selected {
            timing::mode_color(*mode)
        } else {
            theme::SLATE_300
        };
        ui.painter().rect_filled(rect, theme::CIRCLE_RADIUS, color);
    }
}

/// A transport control: an 18px glyph with 9px padding,
/// circular hover wash). The glyph is a path, not a font character.
/// Paints at `rect` so the preview row can center the buttons on the
/// measured center column instead of top-aligning them in layout order.
///
/// The button is an **overlay**, so it interacts without allocating: it is
/// placed against the center column's measured height, not laid out in it.
/// `allocate_rect` would also advance the parent's cursor past the whole
/// button box, and the frame that already reserved the column's height would
/// reserve a second copy of it below -- the empty tail the card carried.
fn nav_button_at(ui: &mut Ui, rect: Rect, left: bool, label: &str) -> bool {
    let response = ui.interact(rect, ui.make_persistent_id(label), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter()
            .circle_filled(rect.center(), NAV_BUTTON / 2.0, theme::INDIGO_50_60);
    }
    let ink = if response.hovered() {
        theme::INDIGO_600
    } else {
        theme::SLATE_400
    };
    paint_chevron(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        left,
        ink,
    );
    response.clicked()
}

/// The previous/next glyph: a tall stroke, half-width
/// 0.15 and half-height 0.38 around the center, stroked heavier than body
/// chevrons.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::TimingSettings;
    use egui_kittest::{Harness, kittest::Queryable};

    struct CardState {
        preview: Preview,
        clock_mounted: bool,
        clip: Option<Rect>,
        messages: Vec<Message>,
        /// The card's own frame rect, as the last frame laid it out.
        card_rect: Rect,
    }

    /// A card harness. `clip` narrows the viewport the card is drawn into, to
    /// exercise the scroll-viewport gate; the preview starts paused unless the
    /// caller passes a playing preview.
    fn card_harness_with(
        preview: Preview,
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
                        clock_mounted,
                        messages,
                        card_rect,
                        ..
                    } = state;
                    *card_rect = preview_card(
                        ui,
                        preview,
                        Language::English,
                        true,
                        *clock_mounted,
                        messages,
                    )
                    .rect;
                },
                CardState {
                    preview,
                    clock_mounted,
                    clip,
                    messages: Vec::new(),
                    card_rect: Rect::NOTHING,
                },
            );
        harness.run_steps(2);
        harness
    }

    fn card_harness(preview: Preview) -> Harness<'static, CardState> {
        card_harness_with(preview, true, None)
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

    /// The delay badge's pill: the `rounded-full` filled shape containing the
    /// badge's label.
    ///
    /// The badge is not the only rounded shape at that spot -- it also paints
    /// its `shadow-xs` (a blurred, one-pixel-offset copy of the same rect) and,
    /// while highlighted, its `ring-2` (an unfilled stroke) -- so identifying
    /// the pill by radius and containment alone would match whichever of the
    /// three happens to come first in paint order. The pill is the one that is
    /// both unblurred and actually filled.
    fn badge_pill<State>(harness: &Harness<'_, State>, label: Rect) -> egui::epaint::RectShape {
        painted_rects(harness)
            .into_iter()
            .find(|rect| {
                rect.corner_radius == theme::CIRCLE_RADIUS
                    && rect.blur_width == 0.0
                    && rect.fill != Color32::TRANSPARENT
                    && rect.rect.contains_rect(label)
            })
            .expect("the delay badge paints its rounded-full pill")
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
        let harness = card_harness(Preview::default());
        let paths = painted_paths(&harness);
        assert!(
            paths
                .iter()
                .any(|path| path.closed && path.fill == theme::SLATE_400),
            "the paused pill draws the Play triangle as a filled path"
        );
        assert!(
            paths
                .iter()
                .filter(|path| {
                    path.stroke.color == egui::epaint::ColorMode::Solid(theme::SLATE_400)
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

        let harness = card_harness(Preview {
            playing: true,
            ..Default::default()
        });
        let stop = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.fill == theme::WHITE
                    && (rect.rect.width() - 7.2).abs() < 0.01
                    && (rect.rect.height() - 7.2).abs() < 0.01
            })
            .expect("the playing pill draws the Stop square");
        assert_eq!(stop.corner_radius, egui::CornerRadius::ZERO);
    }

    /// The card reserves its content's height and nothing more.
    ///
    /// The nav buttons are an **overlay**: they are placed against the center
    /// column's measured height, not laid out inside it. An allocating overlay
    /// (`Ui::allocate_rect`) would advance the parent's cursor past the whole
    /// 36px button box, and the frame -- which has already reserved the
    /// column's height -- would reserve a second copy of it below, leaving an
    /// empty band under the dots. This pins the card's own height to its
    /// painted content: the last thing the card draws is the dots row.
    #[test]
    fn the_card_reserves_no_space_below_its_content() {
        let harness = card_harness(Preview::default());
        let response = harness.state().card_rect;
        let dots = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.rect.height() == DOT_HEIGHT)
            .expect("the card paints its dots row")
            .rect;
        // The card's own bottom pad is its `pb-1` (4px); nothing else may
        // follow the dots. A phantom row would show up as a taller box.
        let tail = response.max.y - dots.max.y;
        assert!(
            tail < 8.0,
            "the preview card must not reserve space below its dots: \
             card ends {}, dots end {}, tail {tail}",
            response.max.y,
            dots.max.y
        );
    }

    #[test]
    fn the_nav_buttons_center_against_the_content_column() {
        // The reference grid is `items-center`: the 36px nav buttons center on
        // the visible center column (pill through dots). The column's own
        // centre is the pill's top through the dots' bottom, which the
        // trailing zero-height clock allocation must not shift.
        let harness = card_harness(Preview::default());
        let prev = harness.get_by_label("Previous example").rect();
        let next = harness.get_by_label("Next example").rect();
        let pill = harness.get_by_label("Play preview").rect();
        let caption = harness.get_by_label("A output active").rect();
        let dots = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.rect.height() == DOT_HEIGHT)
            .expect("the card paints its dots row")
            .rect;
        assert_eq!((prev.width(), prev.height()), (NAV_BUTTON, NAV_BUTTON));
        assert_eq!((next.width(), next.height()), (NAV_BUTTON, NAV_BUTTON));
        // The visible column runs from the pill's top to the dots' bottom,
        // which is what the reference's `items-center` measures.
        let column_center = (pill.top() + dots.max.y) / 2.0;
        for nav in [prev, next] {
            assert!(
                (nav.center().y - column_center).abs() < 0.5,
                "the nav button centers on the visible column: nav {} vs column {column_center}",
                nav.center().y
            );
        }
        // The caption is centered in the column too, so the nav centre sits
        // between the keycap row and the dots rather than below both.
        assert!(prev.center().y < caption.center().y);
        assert!(prev.center().y < dots.center().y);
    }

    #[test]
    fn the_held_keys_carry_the_reference_outer_ring() {
        // The reference's `ring-2 ring-indigo-300` / `ring-purple-300`
        // paints a 2px outer ring on the held keycap, outside the keycap's
        // own 1px edge. The ring follows the `scale-95` compression, so it
        // measures 2px past the 49.4px painted box.
        let mut held = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                example_key(ui, "A", true, &keycap::LEFT);
                example_key(ui, "D", true, &keycap::RIGHT);
            });
        held.run();
        // The ring is the keycap's own `ring-2` on the compressed box, drawn
        // as an outside stroke: the ring's outer edge is the 49.4px painted
        // square plus the 1.9px stroke on each side.
        let ring_width = 2.0 * keycap::PRESS_SCALE;
        let ring_outer = 52.0 * keycap::PRESS_SCALE + 2.0 * ring_width;
        for ring in [theme::INDIGO_300, theme::PURPLE_300] {
            assert!(
                painted_rects(&held)
                    .iter()
                    .any(|rect| rect.stroke.color == ring
                        && (rect.stroke.width - ring_width).abs() < 0.01
                        && (rect.rect.width() + 2.0 * rect.stroke.width - ring_outer).abs() < 0.5),
                "the held key paints the 2px outer ring in {ring:?}"
            );
        }
        let mut resting = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                example_key(ui, "A", false, &keycap::LEFT);
            });
        resting.run();
        assert!(
            !painted_rects(&resting)
                .iter()
                .any(|rect| rect.stroke.color == theme::INDIGO_300),
            "the resting key paints no outer ring"
        );
    }

    #[test]
    fn the_center_column_overlaps_the_badge_on_one_allocation() {
        // The reference's `relative w-12 h-7` box centers the glyph in 28px
        // and overlaps the badge at `-bottom-4`: one allocation of 48px
        // width holding both, not two stacked rows.
        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                center_indicator(ui, true, false, false, "0 ms", EXAMPLES[0], false);
            });
        harness.run();
        let shapes = harness.output().shapes.len();
        assert!(shapes > 0, "the unified indicator paints");
        for (old, new, gap) in [
            (true, false, false),
            (false, false, true),
            (true, true, false),
        ] {
            let mut glyph = Harness::builder()
                .with_size(egui::vec2(200.0, 100.0))
                .build_ui(|ui| {
                    let painter = ui.painter().clone();
                    indicator_glyph(&painter, Pos2::new(100.0, 50.0), old, new, gap);
                });
            glyph.run();
            assert!(
                !glyph.output().shapes.is_empty(),
                "({old}, {new}): the shared glyph paints"
            );
        }
    }

    #[test]
    fn the_play_pill_keeps_the_accent_hierarchy() {
        // Example 0 while playing: the Preview label takes the Immediate
        // ramp's `-200` step on the `BLUE_600` fill; the glyph stays white at
        // rest.
        let harness = card_harness(Preview {
            example: 0,
            playing: true,
            ..Default::default()
        });
        assert!(
            painted_rects(&harness)
                .iter()
                .any(|rect| rect.fill == timing::mode_color(EXAMPLES[0])
                    && rect.corner_radius == theme::PILL_RADIUS)
        );
        assert!(
            painted_rects(&harness)
                .iter()
                .any(|rect| rect.fill == theme::WHITE
                    && (rect.rect.width() - 7.2).abs() < 0.01
                    && (rect.rect.height() - 7.2).abs() < 0.01),
            "the playing glyph paints the white Stop square"
        );
        let label = theme::BLUE_200;
        assert_eq!((label.r(), label.g(), label.b()), (0xbe, 0xdb, 0xff));
    }

    #[test]
    fn the_caption_and_dots_keep_the_reference_rhythm() {
        // Caption `text-[11px]` in slate-600; dots `h-1.5` with a 6px gap:
        // 20px active, 6px idle.
        let mut caption_harness = Harness::builder()
            .with_size(egui::vec2(300.0, 60.0))
            .build_ui(|ui| {
                caption(ui, "A output active");
            });
        caption_harness.run();
        let label = caption_harness.get_by_label("A output active");
        assert!((label.rect().height() - 11.0).abs() < 4.0);
        let mut dots = Harness::builder()
            .with_size(egui::vec2(200.0, 60.0))
            .build_ui(|ui| {
                // The card centers the dots row on the column, exactly as it
                // does the keycap row.
                centered_row(ui, DOTS_ROW_WIDTH, DOT_HEIGHT, DOT_GAP, |ui| {
                    example_dots(ui, 0);
                });
            });
        dots.run();
        let dots: Vec<_> = painted_rects(&dots)
            .into_iter()
            .filter(|rect| rect.rect.height() == DOT_HEIGHT && rect.rect.width() <= DOT_SELECTED)
            .collect();
        assert_eq!(dots.len(), 3, "one dot per example");
        assert!((dots[0].rect.width() - DOT_SELECTED).abs() < 0.01);
        assert!((dots[1].rect.width() - DOT_IDLE_WIDTH).abs() < 0.01);
        assert!((dots[1].rect.min.x - dots[0].rect.max.x - DOT_GAP).abs() < 1.0);
        assert!((dots[2].rect.min.x - dots[1].rect.max.x - DOT_GAP).abs() < 1.0);
        // The reference centers the row on the column: the 44px row inside
        // the 200px harness must sit at x = 78, not against the left edge.
        let row_left = dots[0].rect.min.x;
        let row_right = dots[2].rect.max.x;
        assert!(
            ((row_left + row_right) / 2.0 - 100.0).abs() < 1.0,
            "the dots row centers on the column, got {}",
            (row_left + row_right) / 2.0
        );
    }

    /// The reference's held keycap is `scale-95`: the painted square
    /// compresses about its centre while the allocated box stays put, and the
    /// ring is part of the same element so it compresses too. A held key that
    /// draws at the full 52px reads 2.6px larger than the reference.
    #[test]
    fn the_held_key_compresses_its_painted_box() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                example_key(ui, "A", true, &keycap::LEFT);
            });
        harness.run();
        let painted = 52.0 * keycap::PRESS_SCALE;
        assert!(
            painted_rects(&harness).iter().any(|rect| {
                rect.fill == theme::INDIGO_600 && (rect.rect.width() - painted).abs() < 0.01
            }),
            "the held fill must compress to {painted}px"
        );

        let mut resting = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                example_key(ui, "A", false, &keycap::LEFT);
            });
        resting.run();
        assert!(
            painted_rects(&resting).iter().any(|rect| {
                rect.fill == theme::WHITE && (rect.rect.width() - 52.0).abs() < 0.01
            }),
            "the resting key keeps the full 52px box"
        );
    }

    /// The reference's held key takes `border-<accent>-700`, a one-step
    /// darker edge than its `bg-<accent>` fill, and draws it 1px wide (a
    /// `border` utility, not the D-pad keycap's own 2px stroke).
    #[test]
    fn the_held_key_draws_the_darker_one_pixel_edge() {
        for (key, fill, edge) in [
            (&keycap::LEFT, theme::INDIGO_600, theme::INDIGO_700),
            (&keycap::RIGHT, theme::PURPLE_600, theme::PURPLE_700),
        ] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(200.0, 100.0))
                .build_ui(|ui| {
                    example_key(ui, "A", true, key);
                });
            harness.run();
            assert!(
                painted_rects(&harness).iter().any(|rect| {
                    rect.stroke.color == edge && (rect.stroke.width - 1.0).abs() < 0.01
                }),
                "the held key must draw a 1px {edge:?} edge, not its own {fill:?} fill"
            );
        }
    }

    /// The pill is the reference's `px-3 py-1` around an 18px line box, so it
    /// measures 26px tall -- not the 23px the 10px label and 12px glyph
    /// produced.
    #[test]
    fn the_play_pill_takes_the_reference_line_box() {
        let harness = card_harness(Preview::default());
        let pill = harness.get_by_label("Play preview").rect();
        assert!(
            (pill.height() - 26.0).abs() < 0.5,
            "the pill is the reference's 26px, got {}",
            pill.height()
        );
    }

    #[test]
    fn transport_clicks_emit_their_messages() {
        let mut harness = card_harness(Preview::default());
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
    fn the_preview_sits_directly_on_the_timing_group_without_a_slot_frame() {
        // The reference's `TimingPreview` grid has no grey slot fill: the
        // preview's own frame is transparent, so the group's surface shows
        // through. A SURFACE frame spanning the preview's width (the old
        // `slot_style` wrapper) must be gone.
        let harness = card_harness(Preview::default());
        let wide_surfaces: Vec<_> = painted_rects(&harness)
            .into_iter()
            .filter(|rect| rect.fill == theme::WHITE && rect.rect.width() > 200.0)
            .collect();
        assert!(
            wide_surfaces.is_empty(),
            "no slot-size SURFACE frame may wrap the preview: {wide_surfaces:?}"
        );
    }

    #[test]
    fn the_example_keys_normalise_the_arrow_onto_the_fill_step() {
        // A resting arrow and the held fill are the same mode base, so the two
        // must agree and neither may carry a value that belongs to no ramp.
        let mut resting = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                example_key(ui, "A", false, &keycap::LEFT);
                example_key(ui, "D", false, &keycap::RIGHT);
            });
        resting.run();
        let paths: Vec<_> = painted_paths(&resting)
            .into_iter()
            .filter(|path| !path.points.is_empty())
            .collect();
        assert!(
            paths
                .iter()
                .any(|path| path.stroke.color == egui::epaint::ColorMode::Solid(theme::INDIGO_600)),
            "Key A's resting arrow takes the indigo-600 step the fill uses"
        );
        assert!(
            paths
                .iter()
                .any(|path| path.stroke.color == egui::epaint::ColorMode::Solid(theme::PURPLE_600)),
            "Key D's resting arrow takes the purple-600 step the fill uses"
        );
        let mut held = Harness::builder()
            .with_size(egui::vec2(200.0, 100.0))
            .build_ui(|ui| {
                example_key(ui, "A", true, &keycap::LEFT);
            });
        held.run();
        assert!(
            painted_rects(&held)
                .iter()
                .any(|rect| rect.fill == theme::INDIGO_600
                    && (rect.rect.width() - 52.0 * keycap::PRESS_SCALE).abs() < 0.01),
            "Key A's held fill keeps the bg-indigo-600 class colour, compressed by scale-95"
        );
        // The held key takes the `border-<accent>-700` edge, not its own
        // fill colour.
        assert!(
            painted_rects(&held).iter().any(|rect| {
                rect.stroke.color == theme::INDIGO_700 && (rect.stroke.width - 1.0).abs() < 0.01
            }),
            "the held key's edge is the one-step-darker indigo-700"
        );
    }

    #[test]
    fn the_transition_arrows_round_their_caps() {
        // Round line caps; epaint has neither, so each arrow vertex carries a
        // fill disc at half the stroke width. Both directions must keep that
        // emulation.
        for arrow in [keycap::Arrow::Left, keycap::Arrow::Right] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(120.0, 60.0))
                .build_ui(|ui| {
                    let painter = ui.painter().clone();
                    keycap::paint_arrow(
                        &painter,
                        Rect::from_min_size(Pos2::ZERO, Vec2::splat(24.0)),
                        arrow,
                        theme::INDIGO_600,
                    );
                });
            harness.run();
            let width = painted_paths(&harness)
                .iter()
                .find(|path| path.stroke.color == egui::epaint::ColorMode::Solid(theme::INDIGO_600))
                .map(|path| path.stroke.width)
                .expect("the arrow strokes its shaft and chevron");
            let caps = painted_circles(&harness)
                .iter()
                .filter(|circle| {
                    circle.fill == theme::INDIGO_600 && (circle.radius - width / 2.0).abs() < 0.01
                })
                .count();
            assert!(
                caps >= 5,
                "{arrow:?}: the shaft and chevron vertices each keep a round cap disc, got {caps}"
            );
        }
    }

    #[test]
    fn the_game_view_arrows_take_the_normalised_step() {
        // The center indicator's single-key arrows take the same step the
        // example keys use.
        for (old, new, ink) in [
            (true, false, theme::INDIGO_600),
            (false, true, theme::PURPLE_600),
        ] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(120.0, 60.0))
                .build_ui(|ui| {
                    key_indicator(ui, old, new, false);
                });
            harness.run();
            assert!(
                painted_paths(&harness)
                    .iter()
                    .any(|path| path.stroke.color == egui::epaint::ColorMode::Solid(ink)),
                "({old}, {new}): the game-view arrow strokes {ink:?}"
            );
        }
    }

    /// The preview is an illustration, so the two delay examples must read the
    /// same on every launch: their ranges are the product defaults rather than
    /// whatever the draft holds. This pins the constants to
    /// [`TimingSettings::default`] so a default change cannot leave the
    /// preview advertising a range the product no longer ships.
    #[test]
    fn the_illustrated_ranges_are_the_product_defaults() {
        let defaults = TimingSettings::default();
        assert_eq!(
            DEMO_TRANSITION_MICROS,
            (
                defaults.socd_transition_min_micros,
                defaults.socd_transition_max_micros
            )
        );
        assert_eq!(
            DEMO_PRESERVED_MICROS,
            (
                defaults.preserved_overlap_min_micros,
                defaults.preserved_overlap_max_micros
            )
        );
        // And they are what the card actually paints, in the reference's own
        // `min~max ms` notation.
        assert_eq!(
            delay_label(DEMO_TRANSITION_MICROS.0, DEMO_TRANSITION_MICROS.1),
            "2~4 ms"
        );
        assert_eq!(
            delay_label(DEMO_PRESERVED_MICROS.0, DEMO_PRESERVED_MICROS.1),
            "2~6 ms"
        );
    }

    /// The badge is a fixed illustration: walking the three examples must paint
    /// `0 ms`, `2~4 ms`, `2~6 ms` regardless of what the draft says, so the
    /// badge never changes width with the user's edits.
    #[test]
    fn the_badge_ignores_the_draft_it_is_drawn_beside() {
        let mut texts = Vec::new();
        for example in 0..3 {
            let harness = card_harness(Preview {
                example,
                phase: 0,
                playing: false,
            });
            texts.push(
                painted_texts(&harness)
                    .into_iter()
                    .find(|text| text.ends_with(" ms"))
                    .expect("the badge paints its delay text"),
            );
        }
        assert_eq!(texts, vec!["0 ms", "2~4 ms", "2~6 ms"]);
    }

    /// The reference's badge carries `transition-all duration-150`, so the
    /// highlight must glide: the frame the phase turns over still paints the
    /// resting pill, and only later frames reach `scale-105`. A version that
    /// snapped would jump straight to the active size on that frame.
    ///
    /// The harness's default `step_dt` is 250 ms -- larger than the whole
    /// transition -- so this drives it at 10 ms to resolve the curve.
    #[test]
    fn the_badge_scale_glides_over_the_reference_duration() {
        /// The phase the closure reads, so the highlight can start mid-test.
        struct Phase(usize);
        let mut harness = Harness::builder()
            .with_size(egui::vec2(600.0, 400.0))
            .with_step_dt(0.01)
            .build_ui_state(
                |ui, phase: &mut Phase| {
                    let mut messages = Vec::new();
                    let _ = preview_card(
                        ui,
                        &Preview {
                            example: 1,
                            phase: phase.0,
                            playing: false,
                        },
                        Language::English,
                        true,
                        true,
                        &mut messages,
                    );
                },
                Phase(0),
            );

        // The badge's drawn width, recovered through its own label node.
        let badge_width = |harness: &Harness<'_, Phase>| {
            let label = harness.get_by_label("2~4 ms").rect();
            badge_pill(harness, label).rect.width()
        };

        harness.run();
        let resting = badge_width(&harness);

        // Turn the phase over. The frame the segment starts still shows the
        // resting pill, so the width must not jump.
        harness.state_mut().0 = 1;
        harness.step();
        let first = badge_width(&harness);
        assert!(
            (first - resting).abs() < 0.01,
            "the transition's first frame keeps the resting scale: {first} vs {resting}"
        );

        // It then grows through intermediate sizes before settling. The
        // reference's 150 ms at this harness's 10 ms cadence is 15 frames, so
        // the loop covers the whole segment.
        let settled = resting * BADGE_SCALE_ACTIVE / BADGE_SCALE_RESTING;
        let mut widths = Vec::new();
        for _ in 0..18 {
            harness.step();
            widths.push(badge_width(&harness));
        }
        assert!(
            widths
                .iter()
                .any(|width| *width > resting && *width < settled),
            "the badge passes through an intermediate size: {widths:?}"
        );
        assert!(
            widths.iter().any(|width| (width - settled).abs() < 0.01),
            "the badge settles on scale-105: {widths:?}"
        );
        // And it must not overshoot the target on the way: the curve is
        // monotonic, so no frame may exceed the settled size.
        assert!(
            widths.iter().all(|width| *width <= settled + 0.01),
            "the badge never overshoots scale-105: {widths:?}"
        );
    }

    /// The badge highlights while the loop is moving, not on one phase of
    /// four: both legs of the round trip carry the example's rule, and the
    /// resting phases are the two keys the loop pauses on.
    #[test]
    fn the_legs_highlight_the_delay_badge() {
        let badge = |preview: Preview| {
            let harness = card_harness(preview);
            let label = harness.get_by_label("2~4 ms").rect();
            let pill = badge_pill(&harness, label);
            (harness, pill)
        };
        let mode = EXAMPLES[1];
        // Phase 1 is the leg to D and phase 3 the leg back to A: both show the
        // example's rule, so both highlight the badge.
        for phase in [1, 3] {
            let (harness, active) = badge(Preview {
                example: 1,
                phase,
                playing: false,
            });
            assert_eq!(
                active.fill,
                timing::mode_color(mode),
                "phase {phase} highlights the badge"
            );
            // The reference's `border-indigo-700/50`: a half-alpha `-700` step
            // composited over the badge's own fill, because a CSS border paints
            // over its element's background. The edge is its own `rect_stroke`
            // shape, not the fill's outline.
            let edge = theme::over(theme::INDIGO_700, 0.5, timing::mode_color(mode));
            assert!(
                painted_rects(&harness).iter().any(|rect| {
                    rect.stroke.color == edge && (rect.stroke.width - 1.0).abs() < 0.01
                }),
                "phase {phase} paints the reference's half-alpha indigo-700 edge"
            );
            // The reference's `ring-2 ring-indigo-200/90`, measured outward
            // from the badge's own edge -- pinned by
            // `the_highlighted_ring_hugs_the_pill_without_a_gap`.
            assert!(
                painted_rects(&harness).iter().any(|rect| {
                    rect.stroke.color == theme::with_alpha(theme::INDIGO_200, 230)
                        && (rect.stroke.width - 2.0).abs() < 0.01
                }),
                "phase {phase} paints the reference's 2px ring"
            );
            // The reference's `shadow-xs` lift, which only the active badge
            // carries. Scoped to the pill's own rect: the example keycaps paint
            // a byte-identical lift of their own, so a frame-wide search would
            // find theirs instead.
            let lift = |rect: &egui::epaint::RectShape| {
                rect.blur_width == theme::SHADOW_XS.blur as f32
                    && rect.fill == theme::SHADOW_XS.color
                    && rect.rect.intersects(active.rect)
            };
            assert!(
                painted_rects(&harness).iter().any(lift),
                "phase {phase} paints its shadow-xs lift"
            );
        }
        // The resting phases are the loop's two keys, and neither carries the
        // highlight.
        for phase in [0, 2] {
            let (resting_harness, resting) = badge(Preview {
                example: 1,
                phase,
                playing: false,
            });
            assert_eq!(
                resting.fill,
                theme::over_card(theme::SLATE_100),
                "phase {phase} keeps the reference's opacity-60, composited"
            );
            let hairline = theme::over_card(theme::SLATE_200);
            assert!(
                painted_rects(&resting_harness).iter().any(|rect| {
                    rect.stroke.color == hairline && (rect.stroke.width - 1.0).abs() < 0.01
                }),
                "phase {phase} keeps the slate-200/60 hairline, composited"
            );
            // The resting badge carries no ring and no lift: both belong to the
            // highlighted branch alone.
            assert!(
                !painted_rects(&resting_harness).iter().any(|rect| {
                    rect.stroke.color == theme::with_alpha(theme::INDIGO_200, 230)
                        && (rect.stroke.width - 2.0).abs() < 0.01
                }),
                "phase {phase} paints no ring"
            );
            assert!(
                !painted_rects(&resting_harness)
                    .iter()
                    .any(|rect| rect.fill == theme::SHADOW_XS.color
                        && rect.blur_width == theme::SHADOW_XS.blur as f32
                        && rect.rect.intersects(resting.rect)),
                "phase {phase} paints no shadow-xs lift"
            );
        }
    }

    /// The highlighted badge's `ring-2` is a 2px band from the pill's own edge
    /// outward. An earlier version expanded the pill by the ring width *and*
    /// drew an outside stroke, which put the band 2px too far out and made it
    /// read 4px wide -- the reference's `ring-2` is a box-shadow spread, which
    /// starts at the border box and grows outward by exactly 2px.
    #[test]
    fn the_highlighted_ring_hugs_the_pill_without_a_gap() {
        let harness = card_harness(Preview {
            example: 1,
            phase: 1,
            playing: false,
        });
        let label = harness.get_by_label("2~4 ms").rect();
        let pill = badge_pill(&harness, label);
        let ring = painted_rects(&harness)
            .into_iter()
            .find(|rect| {
                rect.stroke.color == theme::with_alpha(theme::INDIGO_200, 230)
                    && (rect.stroke.width - 2.0).abs() < 0.01
            })
            .expect("the highlighted badge paints its 2px ring");
        // The ring's rect is the pill's own box: `StrokeKind::Outside` grows
        // the band outward from there.
        assert!(
            (ring.rect.width() - pill.rect.width()).abs() < 0.01
                && (ring.rect.height() - pill.rect.height()).abs() < 0.01,
            "the ring starts at the pill's edge: pill {:?} vs ring {:?}",
            pill.rect,
            ring.rect
        );
    }

    /// The resting badge is the reference's pill, not the old rounded rect: a
    /// `rounded-full` shell at `scale-95`, with the reference's own `w-4 h-4`
    /// proportions for a `0 ms` label.
    #[test]
    fn the_resting_badge_is_the_reference_pill() {
        let harness = card_harness(Preview::default());
        let label = harness.get_by_label("0 ms").rect();
        let pill = badge_pill(&harness, label);

        // The reference's `0 ms` badge measures 31.62 x 15.75 CSS px before
        // its `scale-95`, so the drawn pill is that times 0.95.
        let unscaled = pill.rect.size() / BADGE_SCALE_RESTING;
        assert!(
            (unscaled.y - 15.75).abs() < 1.0,
            "the badge keeps the reference's py-0.5 pill height: {unscaled:?}"
        );
        assert!(
            (unscaled.x - 31.62).abs() < 1.5,
            "the badge keeps the reference's px-1.5 pill width: {unscaled:?}"
        );
        assert!(
            (pill.rect.height() - unscaled.y * BADGE_SCALE_RESTING).abs() < 0.01,
            "the resting pill is drawn at scale-95"
        );
        // A pill: the radius is the maximum, which epaint clamps to half the
        // box, so the corners are semicircular.
        assert_eq!(pill.corner_radius, theme::CIRCLE_RADIUS);
        assert!(
            pill.rect.width() > pill.rect.height(),
            "the pill is wider than tall: {:?}",
            pill.rect
        );
    }

    /// Every pair the loop can draw is one the caption names. The round trip
    /// passes through the gap and the overlap on the way out and again on the
    /// way back, so a caption that missed a leg would leave the card
    /// unlabelled for 850 ms.
    #[test]
    fn the_caption_names_every_resolved_state() {
        let caption = |example, phase| {
            let harness = card_harness(Preview {
                example,
                phase,
                playing: false,
            });
            let texts = painted_texts(&harness);
            texts
                .into_iter()
                .find(|text| {
                    matches!(
                        text.as_str(),
                        "A output active"
                            | "D output active"
                            | "Neutral gap (neither key active)"
                            | "Opposite-direction overlap"
                    )
                })
                .unwrap_or_else(|| panic!("example {example} phase {phase} paints no caption"))
        };
        // The rests are the same for every example: the loop leaves A and
        // comes back to it.
        for example in 0..3 {
            assert_eq!(caption(example, 0), "A output active");
            assert_eq!(caption(example, 2), "D output active");
        }
        // The legs carry the example's rule, in both directions.
        assert_eq!(caption(0, 1), "D output active");
        assert_eq!(caption(0, 3), "A output active");
        assert_eq!(caption(1, 1), "Neutral gap (neither key active)");
        assert_eq!(caption(1, 3), "Neutral gap (neither key active)");
        assert_eq!(caption(2, 1), "Opposite-direction overlap");
        assert_eq!(caption(2, 3), "Opposite-direction overlap");
    }

    #[test]
    fn the_neutral_gap_dot_keeps_the_reference_styling() {
        // The neutral dot renders only in the Press Delay gap phase, where
        // the reference highlights it `bg-indigo-500 scale-110 shadow-2xs`.
        let harness = card_harness(Preview {
            example: 1,
            phase: 1,
            playing: false,
        });
        let dot = painted_circles(&harness)
            .into_iter()
            .find(|circle| circle.fill == theme::INDIGO_500)
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
        // through the looping card -- the gap is always a leg, and every leg
        // highlights the badge and the dot -- so the rule is pinned directly.
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

        // Pausing or leaving the viewport clears the anchor; the clock
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

    /// A late wake delays the loop; it never compresses it. The card is an
    /// illustration, so the invariant that matters is that every phase gets
    /// its full dwell on screen -- a period that runs long is legible, one
    /// that runs short is not.
    #[test]
    fn a_late_frame_delays_the_loop_instead_of_compressing_it() {
        let mut clock = PreviewClock::default();
        let lag = 0.15; // A frame's worth of work, past the 850 ms dwell.
        let mut now = 0.0;

        assert!(!clock.poll(now, true, true, 0), "mounting starts the dwell");

        // Drive the clock the way the event loop does: wait out the dwell,
        // wake `lag` late, tick, and ask again.
        let mut ticks = Vec::new();
        for _ in 0..4 {
            let remaining = clock.remaining(now).expect("the clock stays armed");
            now += remaining + lag;
            assert!(clock.poll(now, true, true, 0), "the wake ticks");
            ticks.push(now);
        }

        // Consecutive phases stay at least a full period apart. They drift
        // later by `lag` each cycle, which is the trade: the loop runs slow
        // rather than showing a phase the user cannot read.
        let mut previous = 0.0;
        for (index, at) in ticks.iter().enumerate() {
            let held = at - previous;
            assert!(
                held >= TICK_SECONDS - 1e-9,
                "phase {index} held for {held}s, under the {TICK_SECONDS}s floor"
            );
            previous = *at;
        }
    }

    /// The bug this pins: a clock that woke long after its period -- a
    /// backgrounded window, a scrolled-away card, a slow frame -- must spend a
    /// full period on the phase it just entered. Cashing in the boundaries it
    /// slept through would flash the loop past at frame rate.
    #[test]
    fn a_long_sleep_does_not_flush_the_phases_it_missed() {
        let mut clock = PreviewClock::default();
        assert!(!clock.poll(0.0, true, true, 0), "mounting starts the dwell");
        assert!(clock.poll(0.9, true, true, 0));

        // A 3 s gap crosses three period boundaries, and exactly one tick
        // comes out of it.
        assert!(clock.poll(3.9, true, true, 0));
        assert!(
            !clock.poll(3.92, true, true, 0),
            "the next frame must not tick again"
        );
        let remaining = clock.remaining(3.92).expect("the clock stays armed");
        assert!(
            (remaining - (TICK_SECONDS - 0.02)).abs() < 1e-9,
            "the new phase gets a full dwell, got {remaining}s away"
        );
    }

    /// Four ticks, four pictures: the loop rests on A, plays the example's leg
    /// to D, rests on D, and plays the leg back to A. Every phase is a state of
    /// its own, so nothing dwells and the loop never cuts from D back to A.
    #[test]
    fn the_cycle_plays_both_legs_of_the_round_trip() {
        for example in 0..3 {
            let mut preview = Preview {
                example,
                phase: 0,
                playing: true,
            };
            let mut seen = Vec::new();
            let mut legs = Vec::new();
            for _ in 0..4 {
                seen.push(preview.held());
                legs.push(preview.transitioning());
                preview.update(PreviewAction::Tick);
            }
            assert_eq!(
                preview.phase, 0,
                "example {example}: four ticks wrap to phase 0"
            );
            // The loop's shape is the same whatever the example: it leaves A
            // and comes back to it.
            assert_eq!(
                seen[0],
                (true, false),
                "example {example} opens resting on A"
            );
            assert_eq!(
                seen[2],
                (false, true),
                "example {example} rests on D before the return leg"
            );
            // The rests are the even phases and the legs the odd ones, which is
            // what `transitioning` reports and what the badge highlight reads.
            assert_eq!(
                legs,
                vec![false, true, false, true],
                "example {example}: the rests and legs alternate"
            );
        }
    }

    /// The loop is a round trip, so the swap's two legs are mirror images:
    /// without the return leg the loop cut from D straight back to A, which is
    /// the one thing the example exists to show.
    #[test]
    fn the_swap_shows_both_directions() {
        let mut preview = Preview {
            example: 0,
            phase: 0,
            playing: true,
        };
        let mut pairs = Vec::new();
        for _ in 0..4 {
            pairs.push(preview.held());
            preview.update(PreviewAction::Tick);
        }
        assert_eq!(
            pairs,
            vec![(true, false), (false, true), (false, true), (true, false)],
            "the loop rests on A, lands on D, rests on D, and lands on A"
        );
        // The two delay examples show the same picture on both legs -- a gap is
        // a gap and an overlap an overlap, whichever way the loop is moving.
        for example in [1, 2] {
            let mut preview = Preview {
                example,
                phase: 1,
                playing: true,
            };
            let out = preview.held();
            preview.update(PreviewAction::Tick);
            preview.update(PreviewAction::Tick);
            assert_eq!(
                preview.held(),
                out,
                "example {example}: the two legs of a delay example are one picture"
            );
        }
    }

    #[test]
    fn a_paused_card_emits_no_tick() {
        let harness = card_harness(Preview::default());
        assert!(
            harness.state().messages.is_empty(),
            "the preview starts paused, so no tick may be emitted"
        );
    }

    /// R2 round-2 finding 1(a): the viewport gate reads the clock widget's own
    /// rect, not the card's, so scrolling the widget out stops the frames
    /// (ui.md, "Repaint Policy").
    #[test]
    fn the_clock_stops_where_its_widget_leaves_the_viewport() {
        // On screen: the clock schedules its next phase.
        let harness = card_harness_with(playing_preview(), true, None);
        let visible = repaint_delay(&harness);
        assert!(
            visible <= Duration::from_millis(850),
            "a visible playing clock must schedule the next phase, got {visible:?}"
        );

        // Clipped to the first 16px of the card: the widget rect is outside
        // the viewport, so no tick and no frame request.
        let clip = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 16.0));
        let harness = card_harness_with(playing_preview(), true, Some(clip));
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

    /// R2 round-2 finding 1(b): the clock is not mounted
    /// while a profile dialog is open;
    /// `clock_mounted = false` reproduces that unmount, and the clock rule
    /// test pins the fresh schedule after a remount.
    #[test]
    fn an_unmounted_clock_emits_nothing() {
        let harness = card_harness_with(playing_preview(), false, None);
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

    /// The resting keycap paints its allocated box, so it is the row's
    /// reference frame: the held key compresses to `scale-95` about the same
    /// centre, which would skew an edge-based measurement.
    fn resting_keycap(harness: &Harness<'_, CardState>) -> Rect {
        painted_rects(harness)
            .into_iter()
            .find(|rect| {
                rect.fill == theme::WHITE
                    && (rect.rect.width() - KEYCAP_BOX).abs() < 0.01
                    && (rect.rect.height() - KEYCAP_BOX).abs() < 0.01
            })
            .expect("the resting example key paints its allocated box")
            .rect
    }

    #[test]
    fn the_keycap_row_centers_on_the_column() {
        // The reference's center column is `flex flex-col items-center`, so
        // the keycap row shrinks to its 200px content and centers. egui's
        // `ui.horizontal` would instead pin it to the column's left edge,
        // which is the bug this pins.
        let harness = card_harness(Preview::default());
        let resting = resting_keycap(&harness);
        let held = painted_rects(&harness)
            .into_iter()
            .find(|rect| rect.fill == theme::INDIGO_600)
            .expect("the held example key paints its fill")
            .rect;
        // The keycap centres are 148px apart in the reference: the 200px row
        // less a half-keycap on each side (200 - 52), which the held key's
        // compression about its own centre cannot disturb.
        let row_center = (held.center().x + resting.center().x) / 2.0;
        let center_to_center = (held.center().x - resting.center().x).abs();
        assert!(
            (center_to_center - (KEYCAP_ROW_WIDTH - KEYCAP_BOX)).abs() < 0.5,
            "the keys sit {center_to_center} apart, not the reference's {}",
            KEYCAP_ROW_WIDTH - KEYCAP_BOX
        );
        // The column's own center is what the reference's `items-center`
        // aligns every child to; the row must match the pill and the caption,
        // not the column's left edge.
        let pill = harness.get_by_label("Play preview").rect();
        let caption = harness.get_by_label("A output active").rect();
        assert!(
            (row_center - pill.center().x).abs() < 1.0,
            "the keycap row centers on the column: row {row_center} vs pill {}",
            pill.center().x
        );
        assert!(
            (row_center - caption.center().x).abs() < 1.0,
            "the keycap row shares the column's center with the caption"
        );
        // The reference's grid centers the column between the two nav
        // buttons, so the row center is the nav midpoint as well.
        let prev = harness.get_by_label("Previous example").rect();
        let next = harness.get_by_label("Next example").rect();
        assert!(
            (row_center - (prev.center().x + next.center().x) / 2.0).abs() < 1.0,
            "the keycap row centers between the nav buttons"
        );
    }

    #[test]
    fn the_indicator_keeps_the_reference_box_and_badge_hang() {
        // The reference indicator is `relative w-12 h-7` (48x28) with the
        // badge absolutely positioned at `-bottom-4`: the badge's *unscaled*
        // box bottom sits 16px past the indicator's bottom without growing the
        // keycap row past 52px. A CSS `scale-95` shrinks only the painted pill,
        // so the anchor is recovered by undoing the scale about the pill's
        // centre.
        let harness = card_harness(Preview::default());
        let label = harness.get_by_label("0 ms").rect();
        let pill = badge_pill(&harness, label).rect;
        let resting = resting_keycap(&harness);
        // The 28px indicator is centered in the 52px row, so its bottom sits
        // (52 + 28) / 2 = 40px below the row's top.
        let indicator_bottom = resting.min.y + (KEYCAP_BOX + INDICATOR_HEIGHT) / 2.0;
        // Undo the paint-time scale to recover the layout box the `-bottom-4`
        // anchor measures.
        let anchor_center = pill.center().y;
        let badge_box_bottom = anchor_center + (pill.height() / BADGE_SCALE_RESTING) / 2.0;
        assert!(
            (badge_box_bottom - (indicator_bottom + BADGE_HANG)).abs() < 0.5,
            "the badge hangs {BADGE_HANG}px past the indicator's bottom: {} vs {}",
            badge_box_bottom,
            indicator_bottom + BADGE_HANG
        );
        // The badge hangs outside the row's own 52px box, which is exactly
        // what the reference's absolute positioning does.
        assert!(
            badge_box_bottom > resting.max.y,
            "the badge must hang past the keycap row's bottom"
        );
    }

    // The data-model tests for the preview clock.

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
            assert_eq!(preview.held(), (true, false), "resting on A");
            preview.update(PreviewAction::Tick);
            assert_eq!(preview.held(), expected);
            preview.update(PreviewAction::Tick);
            assert_eq!(preview.held(), (false, true), "resting on D");
            preview.update(PreviewAction::Next);
        }
        assert_eq!(preview.example, 0);
    }
}
