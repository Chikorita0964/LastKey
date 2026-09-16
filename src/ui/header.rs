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

use egui::{
    Align, Color32, Id, Layout, Margin, Painter, Pos2, Rect, Response, RichText, Sense, Shape,
    Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
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
/// The dot-to-status gap (Iced `.spacing(14)`).
const STATUS_GAP: f32 = 14.0;
/// The Iced `widgets::logo` is a fixed 32x32.
const LOGO_SIZE: f32 = 32.0;

/// Draws the header bar and returns the messages this frame produced.
pub fn header(ui: &mut Ui, state: &State) -> Vec<Message> {
    let mut messages = Vec::new();
    let connected = state.connected;
    let has_snapshot = state.snapshot.is_some();
    let filter_enabled = state
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.filter_enabled);
    let state_color = if connected {
        theme::EMERALD_500
    } else {
        theme::SLATE_300
    };
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
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = STATUS_GAP;
                    status_dot(ui, state_color);
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

/// The connection/status mark (`fn dot`): an 8px filled circle.
fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(STATUS_DOT), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), STATUS_DOT / 2.0, color);
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
    use crate::{
        core::PhysicalKey,
        protocol::{DisplayKey, UiSnapshot},
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
        use egui_kittest::{Harness, kittest::Queryable};

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
        use egui_kittest::{Harness, kittest::Queryable};

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
        use egui_kittest::{Harness, kittest::Queryable};

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
}
