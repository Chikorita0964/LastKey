//! Page header: branding, connection status, the icon-only profile /
//! language / engine controls, and the amber dirty badge.
//!
//! The header mounts the dirty badge, which the dispatch assigns to the
//! action bar's ownership. The module paints and pushes
//! [`Message`]s only: it never saves settings, sends IPC, or mutates state.
//!
//! # Ownership boundary for the glyphs
//!
//! [`theme::Icon`] is the window's one glyph table. The page controls this
//! module paints -- `Power`, `Languages`, `Layers` -- are variants of it, so a
//! glyph is traced once and a surface supplies only its box and its ink.
//!
//! # Accessible names
//!
//! The three header controls have no visible label (the reference renders
//! icons with `aria-label`s), so each publishes an invented English name:
//! `Profile slots`, `Language`, `Engine on/off`. Localizing accessible names
//! is the open policy T2's report raised; these are the port's names until
//! that decision lands.

use std::time::Duration;

use egui::{
    Align, Color32, CornerRadius, Id, Layout, Margin, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

use super::{message::Message, state::State, theme};

/// The drawn height of the header bar, which
/// `profiles::PROFILE_PANEL_TOP` anchors the overlay one
/// `PAGE_PADDING + HEADER_HEIGHT + 8` below the window top.
pub const HEADER_HEIGHT: f32 = 60.0;

/// The header's container padding: a 20px leading inset and 12px elsewhere.
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

/// The header buttons' glyph box.
const HEADER_ICON: f32 = 14.0;
/// The status dot's 8px box.
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
/// The dot-to-status gap.
const STATUS_GAP: f32 = 14.0;
/// The logo's fixed 32x32 box.
const LOGO_SIZE: f32 = 32.0;

/// Draws the header bar and returns the messages this frame produced.
///
/// The bar is the page's floating top edge (`ui.md` §Layout): card chrome
/// inset by [`theme::PAGE_PADDING`], drawn outside the body's scroll owner so
/// scrolled cards pass behind it.
pub fn header(ui: &mut Ui, state: &State) -> Vec<Message> {
    let mut messages = Vec::new();
    let connected = state.connected;
    let has_snapshot = state.snapshot.is_some();
    let filter_enabled = state
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.filter_enabled);
    // A control whose condition fails is inert, not an error path, so a press
    // simply produces no message.
    let profiles_enabled = connected && has_snapshot;
    let languages_enabled = has_snapshot;
    let power_enabled = profiles_enabled && state.pending_filter.is_none();
    // The power glyph keeps its own ink pair: indigo while the filter is on,
    // muted while it is off.
    let power_ink = if filter_enabled {
        theme::INDIGO_600
    } else {
        theme::SLATE_400
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
                    if icon_button(
                        ui,
                        power_enabled,
                        theme::Icon::Power,
                        "Engine on/off",
                        power_ink,
                    )
                    .clicked()
                    {
                        messages.push(Message::ToggleFilter);
                    }
                    if icon_button(
                        ui,
                        languages_enabled,
                        theme::Icon::Languages,
                        "Language",
                        theme::INDIGO_600,
                    )
                    .clicked()
                    {
                        messages.push(Message::OpenLanguages);
                    }
                    if icon_button(
                        ui,
                        profiles_enabled,
                        theme::Icon::Layers,
                        "Profile slots",
                        theme::INDIGO_600,
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
/// first uncommitted edit (the action bar does the same with its
/// conditional element slot).
pub fn dirty_badge(ui: &mut Ui, state: &State) {
    if !state.is_dirty() {
        return;
    }
    let ink = theme::AMBER_700;
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

/// One icon-only header control: the outlined shell the shared
/// `secondary_button`
/// style draws (white fill, `SLATE_200` edge, `INDIGO_50_60` fill and
/// `INDIGO_300` edge under the pointer) with the 29px height and
/// `HEADER_ICON_PADDING` measured from the reference. The glyph takes an
/// explicit ink, as the header icons do, so the power-off state keeps its
/// muted colour rather than following a button text colour.
fn icon_button(
    ui: &mut Ui,
    enabled: bool,
    kind: theme::Icon,
    label: &str,
    ink: Color32,
) -> Response {
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
        (theme::INDIGO_50_60, theme::INDIGO_300)
    } else {
        (theme::WHITE, theme::SLATE_200)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    theme::paint_icon(
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
        .rect_filled(rect, CornerRadius::ZERO, theme::SLATE_200);
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
            Self::Measuring => (theme::AMBER_500, theme::AMBER_100),
            Self::Rebinding => (theme::INDIGO_500, theme::INDIGO_100),
            Self::Synced => (theme::EMERALD_500, theme::EMERALD_100),
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
                rect.fill == theme::SLATE_200
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
            ring_and_dot(&harness, theme::EMERALD_100, theme::EMERALD_500),
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
            ring_and_dot(&harness, theme::AMBER_100, theme::AMBER_500),
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
            ring_and_dot(&harness, theme::INDIGO_100, theme::INDIGO_500),
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
            ring_and_dot(&measuring, theme::AMBER_100, theme::AMBER_500),
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
            ring_and_dot(&rebinding, theme::INDIGO_100, theme::INDIGO_500),
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
        assert_eq!(synced.ring, theme::EMERALD_100);
        assert!(!synced.animating);

        // `animate-pulse`: full opacity at the cycle start, half at its end.
        let start = DotState::Measuring.paint(0.0);
        assert_eq!(start.dot, theme::AMBER_500);
        assert!(start.animating);
        let halfway = DotState::Measuring.paint(PULSE_SECONDS / 2.0);
        assert!(close(halfway.dot, theme::AMBER_500.gamma_multiply(0.5), 1));

        // `animate-ping`: the circle doubles and vanishes by the 75% keyframe.
        let start = DotState::Rebinding.paint(0.0);
        assert_eq!(start.dot, theme::INDIGO_500);
        assert_eq!(start.dot_radius, STATUS_DOT / 2.0);
        assert!(start.animating);
        let gone = DotState::Rebinding.paint(PING_SECONDS * PING_RISE);
        assert_eq!(gone.dot_radius, STATUS_DOT);
        assert_eq!(gone.ring_radius, STATUS_DOT + 2.0 * STATUS_RING);
        assert_eq!(gone.dot.to_srgba_unmultiplied()[3], 0);
        assert!(gone.animating);
    }
}
