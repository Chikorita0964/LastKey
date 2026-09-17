//! The settings window: page composition and IPC wiring.
//!
//! The cards push [`Message`]s into one list; [`state::update`] is the only
//! transition function; this module performs the returned [`Effect`]s (send,
//! focus, scroll, raise, close). The view never sends a command itself.
//! Port of `iced-ui/app.rs`, minus Iced: the eframe [`App`] splits the loop
//! into `logic` (IPC pump, focus, keys, effects) and `ui` (the page).

use std::sync::mpsc as std_mpsc;

use eframe::{App, CreationContext, Frame, NativeOptions};
use egui::{
    Align, Color32, CornerRadius, FontId, IconData, Layout, Margin, Painter, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, ViewportBuilder, ViewportCommand,
    WidgetInfo, WidgetType,
};

use super::{
    header, ipc_client,
    language::Language,
    mapping,
    message::{IpcEvent, KeyPress, Message, TimingField},
    profiles,
    state::{self, Effect, ProfileDialog, State},
    theme, timeline, timing,
};
use crate::{
    core::MIN_RECOMMENDATION_SAMPLES,
    protocol::{MeasurementSnapshot, TimingRange, UiCommand, UiView},
};

const WINDOW_TITLE: &str = "LastKey Settings";
const WINDOW_WIDTH: f32 = 1040.0;
const WINDOW_HEIGHT: f32 = 800.0;
/// The Iced action bar's container padding.
const ACTION_BAR_PADDING: f32 = 16.0;
/// The card frame stroke `theme::card_style()` draws, which counts toward the
/// frame's layout size. The bar's reserve has to include it, or the bar sits
/// in the page's bottom margin instead of keeping it.
const CARD_STROKE: f32 = 2.0;
/// The action bar's height, derived from the same constants it is built from
/// (padding top+bottom plus the frame's two strokes and one button row). The
/// body reserves it before the bar is drawn so the scroll owner never jumps
/// and the bar keeps its page-edge margin.
const ACTION_BAR_HEIGHT: f32 = 2.0 * ACTION_BAR_PADDING + 2.0 * CARD_STROKE + theme::BUTTON_HEIGHT;
/// The body never collapses below this; a tiny window scrolls instead.
const BODY_MIN_HEIGHT: f32 = 160.0;
/// Table geometry from the reference (`#card-axis-latencies`): fixed pattern
/// and samples columns, the five figures share the rest.
const PATTERN_COLUMN: f32 = 240.0;
const SAMPLES_COLUMN: f32 = 130.0;
const TABLE_GAP: f32 = 6.0;

pub fn run() -> eframe::Result {
    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            // The Iced shell's minimum window size (iced-ui/app.rs:43-47).
            .with_min_inner_size([960.0, 600.0])
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))),
    )
}

fn window_icon() -> std::sync::Arc<IconData> {
    std::sync::Arc::new(IconData {
        rgba: super::icon::WINDOW_ICON_RGBA.to_vec(),
        width: super::icon::WINDOW_ICON_WIDTH,
        height: super::icon::WINDOW_ICON_HEIGHT,
    })
}

struct SettingsApp {
    state: State,
    connection: Option<ipc_client::Connection>,
    events: std_mpsc::Receiver<ipc_client::Event>,
    /// Deferred effects: they need a live widget tree, so `logic` parks them
    /// and the next frame's `ui` executes them.
    focus_profile_name: bool,
    focus_value_box: Option<TimingField>,
    pending_section: Option<(UiView, bool)>,
}

impl SettingsApp {
    fn new(cc: &CreationContext<'_>) -> Self {
        // The window is light-only like the Iced shell: the port's style is
        // installed for every system theme so the references' pixels hold.
        cc.egui_ctx.all_styles_mut(|style| *style = theme::style());
        let ctx = cc.egui_ctx.clone();
        // The reader thread wakes the window after every queued event; eframe
        // has no async runtime to drive an Iced-style stream.
        let events = ipc_client::connect(move || ctx.request_repaint());
        Self {
            state: State::default(),
            connection: None,
            events,
            focus_profile_name: false,
            focus_value_box: None,
            pending_section: None,
        }
    }

    /// Feeds one message through `update` and performs its effects, in order.
    fn dispatch(&mut self, message: Message, ctx: &egui::Context) {
        for effect in state::update(&mut self.state, message) {
            self.perform(effect, ctx);
        }
    }

    fn dispatch_all(&mut self, messages: Vec<Message>, ctx: &egui::Context) {
        for message in messages {
            self.dispatch(message, ctx);
        }
    }

    fn perform(&mut self, effect: Effect, ctx: &egui::Context) {
        match effect {
            Effect::Send(command) => self.send(command, ctx),
            Effect::FocusProfileName => self.focus_profile_name = true,
            Effect::FocusValueBox(field) => self.focus_value_box = Some(field),
            Effect::ShowSection { view, focus } => self.pending_section = Some((view, focus)),
            Effect::PumpAwake(awake) => {
                if let Some(connection) = &self.connection {
                    connection.set_awake(awake);
                }
            }
            Effect::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// A failed send means the reader thread is gone; route it through the
    /// disconnect path so the state stays the only writer of `State`.
    fn send(&mut self, command: UiCommand, ctx: &egui::Context) {
        let result = self
            .connection
            .as_ref()
            .ok_or_else(|| "The LastKey runtime is disconnected.".to_owned())
            .and_then(|connection| connection.send(command));
        if let Err(error) = result {
            self.dispatch(Message::Ipc(IpcEvent::Disconnected(error)), ctx);
        }
    }

    /// The body: one scroll owner around the settings cards. The action bar
    /// stays outside it, including when results grow (Iced layout).
    fn body(&mut self, ui: &mut Ui, messages: &mut Vec<Message>) {
        let pending = self.pending_section.take();
        let Some(snapshot) = self.state.snapshot.as_ref() else {
            return;
        };
        let names = [
            snapshot.keys[0].name.as_str(),
            snapshot.keys[1].name.as_str(),
            snapshot.keys[2].name.as_str(),
            snapshot.keys[3].name.as_str(),
        ];
        let reserve = if self.state.draft.is_some() {
            theme::SECTION_GAP + ACTION_BAR_HEIGHT
        } else {
            0.0
        };
        let body_height = (ui.available_height() - reserve).max(BODY_MIN_HEIGHT);

        let mut settings_rect = Rect::NOTHING;
        let mut measurement_rect = Rect::NOTHING;
        egui::ScrollArea::vertical()
            .id_salt("settings-body")
            .max_height(body_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::SECTION_GAP);

                settings_rect = ui
                    .scope(|ui| settings_cards(ui, &self.state, messages))
                    .response
                    .rect;

                let _ = ui.scope(|ui| {
                    let _ = timeline::timeline_section(
                        ui,
                        &self.state.monitor,
                        names,
                        self.state.language,
                        self.state.focused,
                        messages,
                    );
                });

                measurement_rect = ui
                    .scope(|ui| measurement_section(ui, &self.state, messages))
                    .response
                    .rect;

                if let Some((view, _)) = pending {
                    let target = match view {
                        UiView::Measurement => measurement_rect,
                        UiView::Settings => settings_rect,
                    };
                    ui.scroll_to_rect(target, Some(Align::TOP));
                }
            });

        if let Some((_, true)) = pending {
            ui.ctx().send_viewport_cmd(ViewportCommand::Focus);
        }
    }
    /// The page as it ships: the canvas, the PAGE_PADDING frame, the floating
    /// header bar, one scrolling body, the floating action bar, and the
    /// overlays. The `App` impl delegates here so the tests can draw the same
    /// page without an eframe frame.
    fn page(&mut self, ui: &mut Ui) {
        let mut messages = Vec::new();

        // The Iced canvas: page background behind everything, the page itself
        // inset by PAGE_PADDING.
        ui.painter()
            .rect_filled(ui.max_rect(), CornerRadius::ZERO, theme::CANVAS);
        egui::Frame::NONE
            .inner_margin(Margin::same(theme::PAGE_PADDING as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::SECTION_GAP);
                messages.extend(header::header(ui, &self.state));
                if self.state.snapshot.is_some() && self.state.draft.is_some() {
                    self.body(ui, &mut messages);
                    actions_bar(ui, &self.state, &mut messages);
                } else {
                    disconnected_body(ui, &self.state, &mut messages);
                }
            });

        // The rename focus request belongs to the frame the box mounts on.
        if self.focus_profile_name {
            profiles::focus_profile_name(ui);
            self.focus_profile_name = false;
        }
        messages.extend(profiles::profile_overlay(ui, &self.state));

        if let Some(field) = self.focus_value_box.take() {
            ui.memory_mut(|memory| memory.request_focus(timing::value_box_id(field)));
        }

        self.dispatch_all(messages, ui.ctx());
    }
}

impl App for SettingsApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        // The window's focus is the pump's awake source and the animation gate.
        let focused = ctx.input(|input| input.focused);
        if focused != self.state.focused {
            let message = if focused {
                Message::WindowFocused
            } else {
                Message::WindowUnfocused
            };
            self.dispatch(message, ctx);
        }

        while let Ok(event) = self.events.try_recv() {
            let message = match event {
                ipc_client::Event::Connected(connection) => {
                    self.connection = Some(connection);
                    Message::Ipc(IpcEvent::Connected)
                }
                ipc_client::Event::Message(event) => Message::Ipc(IpcEvent::Message(event)),
                ipc_client::Event::Disconnected(error) => {
                    self.connection = None;
                    Message::Ipc(IpcEvent::Disconnected(error))
                }
            };
            self.dispatch(message, ctx);
        }

        // The Iced runtime subscription: Escape cancels a capture, every
        // other non-repeat press and every release feeds the press tracker.
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                ..
            } = event
            {
                let message = if key == egui::Key::Escape && pressed {
                    Some(Message::CancelCapture)
                } else if pressed && !repeat {
                    Some(Message::KeyboardPressed(key_press(key, physical_key)))
                } else if !pressed {
                    Some(Message::KeyboardReleased(key_press(key, physical_key)))
                } else {
                    None
                };
                if let Some(message) = message {
                    self.dispatch(message, ctx);
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        self.page(ui);
    }
}

impl Drop for SettingsApp {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            let _ = connection.send(UiCommand::CloseUiSession);
        }
    }
}

/// Reduces an egui key event to the press tracker's shape. The physical name
/// falls back to the logical key when the backend reports no scancode; the
/// state layer matches either spelling (`"W"`, `"KeyW"`, `"ArrowUp"`).
fn key_press(key: egui::Key, physical_key: Option<egui::Key>) -> KeyPress {
    KeyPress {
        character: Some(format!("{key:?}")),
        physical: Some(format!("{:?}", physical_key.unwrap_or(key))),
    }
}

/// The context-memory key where the row publishes the shared content height
/// both cards stretch to, and where each card publishes the natural height it
/// measured first. The memory is the channel so the card entry points keep
/// their signatures: the semantic tests compose the cards directly.
fn card_height_id() -> egui::Id {
    egui::Id::new("settings-cards-height")
}

fn card_natural_id(card: &'static str) -> egui::Id {
    egui::Id::new(("settings-card-natural-height", card))
}

/// How far the row's measured height may drift before the sizing pass is
/// re-run; the frames round to whole pixels, so an exact comparison would
/// discard on every frame.
const CARD_HEIGHT_TOLERANCE: f32 = 0.5;

/// Grows one card's frame to the row's shared height and publishes the
/// natural content height it measured first. The reference stretches both
/// cards to the taller one (`h-full` inside its grid); `expand_to_include_rect`
/// grows the frame's region without the item spacing a trailing
/// `set_min_height` or `add_space` would add, and measuring before the
/// expansion keeps the row from reading its own minimum back.
pub(crate) fn stretch_card(ui: &mut Ui, card: &'static str) {
    let target = ui
        .data(|data| data.get_temp::<f32>(card_height_id()))
        .unwrap_or(0.0);
    let natural = ui.min_rect();
    ui.expand_to_include_rect(Rect::from_min_max(
        natural.min,
        Pos2::new(natural.max.x, natural.top() + target.max(natural.height())),
    ));
    ui.data_mut(|data| data.insert_temp(card_natural_id(card), natural.height()));
}

/// The two settings cards, side by side, each in its own fixed-width column
/// (the Iced `row![mappings, timing_card]`). Shared by the page and its tests
/// so the composition is exercised exactly as it ships. Returns each column's
/// rect so the height contract can be asserted directly.
///
/// The reference stretches both cards to the taller one (`h-full` inside its
/// grid), and egui measures bottom-up, so the row records the taller natural
/// height and re-renders both cards at it. The sizing pass is hidden with a
/// discard, the way `Grid` covers its own first pass; once the height is
/// stable the row runs one pass per frame.
fn settings_cards(ui: &mut Ui, state: &State, messages: &mut Vec<Message>) -> (Rect, Rect) {
    let gap = theme::SECTION_GAP;
    let width = ((ui.available_width() - gap) / 2.0).max(0.0);
    let target = ui
        .data(|data| data.get_temp::<f32>(card_height_id()))
        .unwrap_or(0.0);
    let mut cards = (Rect::NOTHING, Rect::NOTHING);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        cards.0 = ui
            .allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                messages.extend(mapping::key_mappings_card(ui, state));
            })
            .response
            .rect;
        cards.1 = ui
            .allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
                if let Some(draft) = &state.draft {
                    let _ = timing::timing_card(
                        ui,
                        &draft.timing,
                        &state.inputs,
                        &state.editing,
                        state.language,
                        Some(timing::PreviewMount {
                            preview: &state.preview,
                            awake: state.focused,
                            clock_mounted: matches!(state.profiles, ProfileDialog::Closed),
                        }),
                        messages,
                    );
                }
            })
            .response
            .rect;
    });

    let mapping_height = ui
        .data(|data| data.get_temp::<f32>(card_natural_id("mapping")))
        .unwrap_or(0.0);
    let timing_height = ui
        .data(|data| data.get_temp::<f32>(card_natural_id("timing")))
        .unwrap_or(0.0);
    let measured = mapping_height.max(timing_height);
    let changed = (target - measured).abs() > CARD_HEIGHT_TOLERANCE;
    ui.data_mut(|data| data.insert_temp(card_height_id(), measured));
    if changed {
        ui.ctx().request_discard("settings cards: matching heights");
    }
    cards
}

/// The action bar: restore, dirty badge, feedback, revert, apply. It is the
/// one place errors and notices are shown, so their appearance never moves a
/// widget inside the scroll owner. Shared with the tests.
fn actions_bar(ui: &mut Ui, state: &State, messages: &mut Vec<Message>) {
    if state.snapshot.is_none() || state.draft.is_none() {
        return;
    }
    let dirty = state.is_dirty();
    let language = state.language;
    theme::card_style()
        .inner_margin(Margin::same(ACTION_BAR_PADDING as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::ROW_GAP;
                if action_button(
                    ui,
                    Glyph::Restore,
                    language.text("Restore all defaults"),
                    ButtonKind::Secondary,
                    true,
                )
                .clicked()
                {
                    messages.push(Message::RestoreAllDefaults);
                }
                header::dirty_badge(ui, state);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if action_button(
                        ui,
                        Glyph::Check,
                        language.text("Apply"),
                        ButtonKind::Primary,
                        dirty,
                    )
                    .clicked()
                    {
                        messages.push(Message::Apply);
                    }
                    if action_button(
                        ui,
                        Glyph::Revert,
                        language.text("Revert"),
                        ButtonKind::Secondary,
                        dirty,
                    )
                    .clicked()
                    {
                        messages.push(Message::Revert);
                    }
                    // The feedback fills what is left of the bar; its own row
                    // is icon-first with the text hugging the right edge
                    // (Iced: Fill row, right-aligned text).
                    let remaining = ui.available_width();
                    ui.allocate_ui_with_layout(
                        Vec2::new(remaining, theme::BUTTON_HEIGHT),
                        Layout::left_to_right(Align::Center),
                        |ui| feedback(ui, state, dirty),
                    );
                });
            });
        });
}

/// The page body while no snapshot has mounted: the waiting note, a manual
/// snapshot request, and the connection error the action bar cannot show yet.
fn disconnected_body(ui: &mut Ui, state: &State, messages: &mut Vec<Message>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme::ROW_GAP;
        label(
            ui,
            state
                .language
                .text("The settings UI is waiting for LastKey.exe."),
            theme::BODY_TEXT_SIZE,
            theme::BODY_TEXT,
            false,
        );
        if plain_button(
            ui,
            state.language.text("Request snapshot"),
            ButtonKind::Secondary,
            true,
        )
        .clicked()
        {
            messages.push(Message::RequestSnapshot);
        }
        if let Some(error) = &state.error {
            label(ui, error, 12.0, theme::ERROR_TEXT, false);
        }
    });
}

/// Error and notice feedback in the action bar. Rendered as plain text with a
/// blank placeholder when empty, so its presence never moves any widget.
fn feedback(ui: &mut Ui, state: &State, dirty: bool) {
    match (&state.error, &state.notice) {
        (Some(error), _) => {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            paint_glyph(ui.painter(), rect, Glyph::Warning, theme::ERROR_TEXT);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(state.language.text(error))
                            .size(12.0)
                            .strong()
                            .color(theme::RED_600),
                    )
                    .truncate(),
                );
            });
        }
        (None, Some(notice)) => {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            paint_glyph(ui.painter(), rect, Glyph::Check, theme::OK_TEXT);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(state.language.text(notice))
                            .size(12.0)
                            .strong()
                            .color(theme::EMERALD_600),
                    )
                    .truncate(),
                );
            });
        }
        (None, None) if dirty => {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(state.language.text("Click Apply to commit draft edits."))
                            .size(12.0)
                            .color(theme::ICON_MUTED),
                    )
                    .truncate(),
                );
            });
        }
        (None, None) => {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(state.language.text("Synchronized"))
                            .size(12.0)
                            .color(theme::EMERALD_SYNC),
                    )
                    .truncate(),
                );
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                paint_glyph(ui.painter(), rect, Glyph::Check, theme::OK_TEXT);
            });
        }
    }
}

/// The measurement card: header with session controls, the four stat boxes
/// while open, then the latency table and the recommendation tiles.
fn measurement_section(ui: &mut Ui, state: &State, messages: &mut Vec<Message>) {
    let Some(snapshot) = &state.snapshot else {
        return;
    };
    let language = state.language;
    let active = snapshot.measurement_active;
    let is_open = state.session_details_open || active;
    let measurement = snapshot.measurement.unwrap_or_default();

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme::SECTION_GAP;

        theme::card_style()
            .inner_margin(Margin::same(theme::CARD_PADDING as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::SECTION_GAP);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        section_title(ui, Glyph::Chart, language.text("Input timing measurement"));
                        label(
                            ui,
                            language.text("Records your mapped key-pair timing for this session."),
                            12.0,
                            theme::MUTED_TEXT,
                            false,
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = theme::ROW_GAP;
                        let (kind, glyph, label_text) = if active {
                            (ButtonKind::Warning, Glyph::Stop, "Stop measurement")
                        } else {
                            (ButtonKind::Primary, Glyph::Play, "Start measurement")
                        };
                        if action_button(ui, glyph, language.text(label_text), kind, true).clicked()
                        {
                            messages.push(Message::ToggleMeasurement);
                        }
                        if action_button(
                            ui,
                            Glyph::Restore,
                            language.text("Reset session"),
                            ButtonKind::Secondary,
                            true,
                        )
                        .clicked()
                        {
                            messages.push(Message::ResetMeasurement);
                        }
                    });
                });

                if is_open {
                    let stats = [
                        (
                            language.text("Physical key edges"),
                            grouped(measurement.observed_event_count),
                            theme::BODY_TEXT,
                        ),
                        (
                            language.text("Valid paired samples"),
                            grouped(measurement.sample_count),
                            theme::INDIGO_600,
                        ),
                        (
                            language.text("Physical overlap share"),
                            percentage_value(measurement.overlap_count, measurement.sample_count),
                            theme::AMBER_500,
                        ),
                        (
                            language.text("Indistinguishable share"),
                            percentage_value(
                                measurement.near_simultaneous_count,
                                measurement.sample_count,
                            ),
                            theme::RED_600,
                        ),
                    ];
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme::ROW_GAP;
                        let width = ((ui.available_width() - 3.0 * theme::ROW_GAP) / 4.0).max(0.0);
                        for (label_text, value, color) in stats {
                            ui.allocate_ui_with_layout(
                                Vec2::new(width, 76.0),
                                Layout::top_down(Align::Center),
                                |ui| stat_box(ui, label_text, &value, color),
                            );
                        }
                    });
                }
            });

        if is_open {
            latencies_card(ui, &measurement, language);
            recommendations_card(ui, &measurement, language, messages);
        }
    });
}

/// One stat tile: an 11px heavy label at the top, the 19px figure at the
/// bottom, centered in the reference's 76px box.
fn stat_box(ui: &mut Ui, label_text: &str, value: &str, value_color: Color32) {
    theme::group_style()
        .inner_margin(Margin {
            left: 10,
            right: 10,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(76.0 - 24.0);
            ui.vertical_centered(|ui| {
                label(ui, label_text, 11.0, theme::BODY_TEXT, true);
            });
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                label(ui, value, 19.0, value_color, true);
            });
        });
}

/// The latency table card: header, then the table group.
fn latencies_card(ui: &mut Ui, measurement: &MeasurementSnapshot, language: Language) {
    theme::card_style()
        .inner_margin(Margin::same(theme::CARD_PADDING as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::ROW_GAP);
            section_title(
                ui,
                Glyph::Measurement,
                language.text("Measured Input Transitions"),
            );
            label(
                ui,
                language.text("Updates live while measuring"),
                12.0,
                theme::MUTED_TEXT,
                false,
            );
            theme::group_style()
                .inner_margin(Margin::same(theme::GROUP_PADDING as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::ROW_GAP);

                    table_row(
                        ui,
                        [
                            &|ui| heading(ui, language.text("INPUT PATTERN")),
                            &|ui| heading(ui, language.text("SAMPLES")),
                            &|ui| heading(ui, "P10"),
                            &|ui| heading(ui, "P50"),
                            &|ui| heading(ui, "P90"),
                            &|ui| heading(ui, language.text("MIN")),
                            &|ui| heading(ui, language.text("MAX")),
                        ],
                    );
                    hrule(ui);
                    pattern_row(
                        ui,
                        language.text("Neutral transition"),
                        theme::EMERALD_500,
                        measurement.transition_count,
                        [
                            duration_stat(measurement.transition_p10_micros),
                            duration_stat(measurement.transition_median_micros),
                            duration_stat(measurement.transition_p90_micros),
                            duration_stat(measurement.transition_min_micros),
                            duration_stat(measurement.transition_max_micros),
                        ],
                    );
                    hrule(ui);
                    pattern_row(
                        ui,
                        language.text("Physical overlap"),
                        theme::AMBER_500,
                        measurement.overlap_count,
                        [
                            duration_stat(measurement.overlap_p10_micros),
                            duration_stat(measurement.overlap_median_micros),
                            duration_stat(measurement.overlap_p90_micros),
                            duration_stat(measurement.overlap_min_micros),
                            duration_stat(measurement.overlap_max_micros),
                        ],
                    );
                    hrule(ui);
                    // The indistinguishable row carries its explanation in
                    // place of the figures, like the reference's colspan.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = TABLE_GAP;
                        cell(ui, PATTERN_COLUMN, false, |ui| {
                            ui.spacing_mut().item_spacing.x = TABLE_GAP;
                            dot(ui, theme::RED_500);
                            label(
                                ui,
                                language.text("Indistinguishable"),
                                theme::BODY_TEXT_SIZE,
                                theme::BODY_TEXT,
                                true,
                            );
                        });
                        cell(ui, SAMPLES_COLUMN, true, |ui| {
                            figure(
                                ui,
                                &grouped(measurement.near_simultaneous_count),
                                theme::BODY_TEXT,
                            );
                        });
                        ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width(), 0.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                label(
                                    ui,
                                    language.text(
                                        "Unclear input order (<1 ms), excluded from timing ranges.",
                                    ),
                                    11.0,
                                    theme::MUTED_TEXT,
                                    false,
                                );
                            },
                        );
                    });
                });
        });
}

/// The recommendations card: the suggested ranges and the one action that
/// copies them into the draft.
fn recommendations_card(
    ui: &mut Ui,
    measurement: &MeasurementSnapshot,
    language: Language,
    messages: &mut Vec<Message>,
) {
    let has_suggestions =
        measurement.recommended_transition.is_some() || measurement.recommended_overlap.is_some();
    theme::card_style()
        .inner_margin(Margin::same(theme::CARD_PADDING as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::new(0.0, theme::SECTION_GAP);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    section_title(ui, Glyph::Star, language.text("Suggested delays"));
                    label(
                        ui,
                        language.text(
                            "Based on P10-P50 input timings, excluding indistinguishable inputs.",
                        ),
                        12.0,
                        theme::MUTED_TEXT,
                        false,
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if trailing_button(
                        ui,
                        Glyph::ArrowForward,
                        language.text("Apply suggestions"),
                        ButtonKind::Primary,
                        has_suggestions,
                    )
                    .clicked()
                    {
                        messages.push(Message::ApplyRecommendations);
                    }
                });
            });
            let transition = timing_range(measurement.recommended_transition);
            let overlap = timing_range(measurement.recommended_overlap);
            let value_height = suggestion_value_height(
                ui,
                [
                    (&transition, measurement.recommended_transition.is_some()),
                    (&overlap, measurement.recommended_overlap.is_some()),
                ],
            );
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme::ROW_GAP;
                let width = ((ui.available_width() - theme::ROW_GAP) / 2.0).max(0.0);
                suggestion_tile(
                    ui,
                    width,
                    language.text("New Key Press Delay"),
                    language.text("Based on neutral transitions"),
                    &transition,
                    measurement.recommended_transition.is_some(),
                    value_height,
                );
                suggestion_tile(
                    ui,
                    width,
                    language.text("Previous Key Release Delay"),
                    language.text("Based on physical overlaps"),
                    &overlap,
                    measurement.recommended_overlap.is_some(),
                    value_height,
                );
            });
        });
}

/// One recommendation tile: the delay name and its hint stacked on the left,
/// the value hugging the right edge in the shared `value_height` slot, so the
/// tiles in the pair keep one top line and one height (F13).
fn suggestion_tile(
    ui: &mut Ui,
    width: f32,
    label_text: &str,
    hint: &str,
    value: &str,
    available: bool,
    value_height: f32,
) {
    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
        theme::group_style()
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        label(ui, label_text, 12.0, theme::BODY_TEXT, true);
                        label(ui, hint, 11.0, theme::MUTED_TEXT, false);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // The value keeps a right inset so it never
                        // touches the tile edge (Iced: padding right 4).
                        ui.add_space(4.0);
                        let size = if available { 17.0 } else { 13.0 };
                        let color = if available {
                            theme::INDIGO_600
                        } else {
                            theme::ICON_MUTED
                        };
                        value_box(ui, value, size, color, value_height);
                    });
                });
            });
    });
}

/// The recommendation value slot both tiles share. The value is a single line
/// for a range and two lines for the collect prompt, so the pair's height is
/// the taller of the two and the shorter tile keeps the same baseline (F13).
fn suggestion_value_height(ui: &Ui, values: [(&str, bool); 2]) -> f32 {
    values
        .into_iter()
        .map(|(value, available)| {
            let size = if available { 17.0 } else { 13.0 };
            ui.painter()
                .layout_no_wrap(
                    value.to_owned(),
                    FontId::new(size, theme::UI_FONT),
                    Color32::PLACEHOLDER,
                )
                .size()
                .y
        })
        .fold(0.0, f32::max)
}

/// A recommendation value painted on its slot's centre line; the slot keeps the
/// shared height whether the galley is one line or two.
fn value_box(ui: &mut Ui, value: &str, size: f32, color: Color32, height: f32) {
    let galley = ui.painter().layout_no_wrap(
        value.to_owned(),
        FontId::new(size, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(galley.size().x, height), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), value));
    let position = Pos2::new(rect.left(), rect.center().y - galley.size().y / 2.0);
    theme::stamp_galley(ui.painter(), position, &galley, color, size);
}

/// One table row: seven cells sharing the same widths and spacing, so columns
/// line up down the table.
fn table_row(ui: &mut Ui, cells: [&dyn Fn(&mut Ui); 7]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_GAP;
        let widths = column_widths(ui.available_width());
        for (index, cell_content) in cells.into_iter().enumerate() {
            cell(ui, widths[index], index > 0, cell_content);
        }
    });
}

/// Pattern label plus sample count plus the five duration figures. Every figure
/// cell shares one ink and one face: F14 removed the P50 highlight.
fn pattern_row(
    ui: &mut Ui,
    label_text: &str,
    color: Color32,
    count: u32,
    figures: [(String, bool); 5],
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TABLE_GAP;
        let widths = column_widths(ui.available_width());
        cell(ui, widths[0], false, |ui| {
            ui.spacing_mut().item_spacing.x = TABLE_GAP;
            dot(ui, color);
            label(
                ui,
                label_text,
                theme::BODY_TEXT_SIZE,
                theme::BODY_TEXT,
                true,
            );
        });
        cell(ui, widths[1], true, |ui| {
            figure(ui, &grouped(count), theme::BODY_TEXT);
        });
        for ((value, present), width) in figures.into_iter().zip(&widths[2..]) {
            let ink = if present {
                theme::BODY_TEXT
            } else {
                theme::MUTED_TEXT
            };
            cell(ui, *width, true, |ui| figure(ui, &value, ink));
        }
    });
}

/// One fixed-width table cell; `right` right-aligns its content. The cell
/// claims its full width (`set_min_width`), otherwise the row advances by the
/// content's own width and the columns drift apart from row to row.
fn cell(ui: &mut Ui, width: f32, right: bool, content: impl FnOnce(&mut Ui)) {
    let layout = if right {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), layout, |ui| {
        ui.set_min_width(width);
        content(ui);
    });
}

/// The seven column widths for one table row: the reference's fixed pattern and
/// samples columns, then the five figures sharing the rest.
fn column_widths(available: f32) -> [f32; 7] {
    let fill = ((available - PATTERN_COLUMN - SAMPLES_COLUMN - 6.0 * TABLE_GAP) / 5.0).max(0.0);
    [PATTERN_COLUMN, SAMPLES_COLUMN, fill, fill, fill, fill, fill]
}

/// Table header cell: the card title color, 11px heavy.
fn heading(ui: &mut Ui, text: &str) {
    label(ui, text, 11.0, theme::BODY_TEXT, true);
}

/// Numeric table cell: right-aligned in the monospace face so decimal places
/// line up.
fn figure(ui: &mut Ui, value: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(
        value.to_owned(),
        FontId::new(12.0, theme::MONO_FONT),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), value));
    ui.painter().galley(rect.min, galley, color);
}

fn hrule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, theme::BORDER);
}

/// Small status or legend mark, as `dot(color)` drew it.
fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

fn duration_stat(micros: Option<u64>) -> (String, bool) {
    micros.map_or_else(
        || ("-".to_owned(), false),
        |value| (format!("{:.1} ms", value as f64 / 1_000.0), true),
    )
}

fn timing_range(range: Option<TimingRange>) -> String {
    range.map_or_else(
        // Two lines by construction: the tile is too narrow for the full
        // sentence, and an explicit break stays put across DPIs.
        || format!("Collect at least\n{MIN_RECOMMENDATION_SAMPLES} samples"),
        |range| {
            format!(
                "{:.1} - {:.1} ms",
                range.min_micros as f64 / 1_000.0,
                range.max_micros as f64 / 1_000.0
            )
        },
    )
}

fn percentage_value(count: u32, total: u32) -> String {
    if total == 0 {
        "-".to_owned()
    } else {
        format!("{:.1}%", f64::from(count) * 100.0 / f64::from(total))
    }
}

/// Groups an integer count with thousands separators (`1072` -> `1,072`), the
/// reference's `toLocaleString` for every language this window ships.
fn grouped(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A section title: the 16px glyph plus the heavy heading.
fn section_title(ui: &mut Ui, glyph: Glyph, label_text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        paint_glyph(ui.painter(), rect, glyph, theme::PRIMARY_TEXT);
        label(ui, label_text, theme::HEADING_SIZE, theme::BODY_TEXT, true);
    });
}

/// Text with a published node; heavy text uses the port's fake-bold stamp.
fn label(ui: &mut Ui, content: &str, size: f32, color: Color32, bold: bool) {
    let galley = ui.painter().layout_no_wrap(
        content.to_owned(),
        FontId::new(size, theme::UI_FONT),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), content));
    if bold {
        theme::stamp_galley(ui.painter(), rect.min, &galley, color, size);
    } else {
        ui.painter().galley(rect.min, galley, color);
    }
}

/// Which filled or outlined shell a button draws.
#[derive(Clone, Copy, PartialEq)]
enum ButtonKind {
    Secondary,
    Primary,
    Warning,
}

fn action_button(
    ui: &mut Ui,
    glyph: Glyph,
    label_text: &str,
    kind: ButtonKind,
    enabled: bool,
) -> Response {
    button(ui, Some(glyph), label_text, kind, enabled, true)
}

/// Label first, glyph last (the Iced `trailing_icon_label`, used by the
/// suggestions action).
fn trailing_button(
    ui: &mut Ui,
    glyph: Glyph,
    label_text: &str,
    kind: ButtonKind,
    enabled: bool,
) -> Response {
    button(ui, Some(glyph), label_text, kind, enabled, false)
}

fn plain_button(ui: &mut Ui, label_text: &str, kind: ButtonKind, enabled: bool) -> Response {
    button(ui, None, label_text, kind, enabled, true)
}

/// The action buttons. The port's theme owns the outlined variant
/// (`theme::secondary_button`); the filled pair (primary, warning) and the
/// non-action glyphs live here until a shared owner exists.
fn button(
    ui: &mut Ui,
    glyph: Option<Glyph>,
    label_text: &str,
    kind: ButtonKind,
    enabled: bool,
    glyph_first: bool,
) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label_text.to_owned(),
        FontId::proportional(theme::BUTTON_TEXT_SIZE),
        Color32::PLACEHOLDER,
    );
    let icon = glyph.map_or(0.0, |_| theme::BUTTON_ICON + theme::BUTTON_ICON_GAP);
    let padding = match kind {
        ButtonKind::Primary => theme::BUTTON_PADDING_WIDE,
        _ => theme::BUTTON_PADDING,
    };
    let width = galley.size().x + icon + padding.left + padding.right;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, theme::BUTTON_HEIGHT),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, enabled && ui.is_enabled(), label_text)
    });

    let (fill, edge, ink) = button_ink(kind, enabled, response.hovered());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::CONTROL_RADIUS, fill);
    painter.rect_stroke(
        rect,
        theme::CONTROL_RADIUS,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let content = galley.size().x + icon;
    let start = rect.center().x - content / 2.0;
    let icon_at = |x: f32| {
        Rect::from_min_size(
            Pos2::new(x, rect.center().y - theme::BUTTON_ICON / 2.0),
            Vec2::splat(theme::BUTTON_ICON),
        )
    };
    let text_y = rect.center().y - galley.size().y / 2.0;
    match (glyph, glyph_first) {
        (Some(glyph), true) => {
            paint_glyph(&painter, icon_at(start), glyph, glyph_ink(glyph, ink));
            theme::stamp_galley(
                &painter,
                Pos2::new(start + icon, text_y),
                &galley,
                ink,
                theme::BUTTON_TEXT_SIZE,
            );
        }
        (Some(glyph), false) => {
            theme::stamp_galley(
                &painter,
                Pos2::new(start, text_y),
                &galley,
                ink,
                theme::BUTTON_TEXT_SIZE,
            );
            paint_glyph(
                &painter,
                icon_at(start + galley.size().x + theme::BUTTON_ICON_GAP),
                glyph,
                glyph_ink(glyph, ink),
            );
        }
        (None, _) => {
            theme::stamp_galley(
                &painter,
                Pos2::new(start, text_y),
                &galley,
                ink,
                theme::BUTTON_TEXT_SIZE,
            );
        }
    }
    response
}

/// The button shells and inks per kind and state (Iced `theme::primary_button`
/// / `theme::warning_button` / `theme::secondary_button`).
fn button_ink(kind: ButtonKind, enabled: bool, hovered: bool) -> (Color32, Color32, Color32) {
    if !enabled {
        return (theme::SURFACE, theme::BORDER, theme::SLATE_300);
    }
    match kind {
        ButtonKind::Secondary => {
            if hovered {
                (
                    theme::HOVER_WASH,
                    theme::NAME_HOVER_BORDER,
                    theme::INDIGO_600,
                )
            } else {
                (theme::SURFACE, theme::BORDER, theme::ICON_SECONDARY)
            }
        }
        ButtonKind::Primary => {
            if hovered {
                (theme::INDIGO_700, theme::INDIGO_700, Color32::WHITE)
            } else {
                (theme::INDIGO_600, theme::INDIGO_700, Color32::WHITE)
            }
        }
        ButtonKind::Warning => {
            if hovered {
                (theme::AMBER_DARK, theme::AMBER_DARK, Color32::WHITE)
            } else {
                (theme::AMBER_BUTTON, theme::AMBER_DARK, Color32::WHITE)
            }
        }
    }
}

/// Restore affordances keep the reference's slate-600 ink; every other glyph
/// inherits the button text color.
fn glyph_ink(glyph: Glyph, ink: Color32) -> Color32 {
    if glyph == Glyph::Restore {
        theme::ICON_SECONDARY
    } else {
        ink
    }
}

/// The glyphs this page needs. The theme owns Keyboard / Restore / Edit /
/// Check / Warning; the page glyphs from `iced-ui/icons.rs` are traced here
/// until a shared icon owner exists (the same pending wave as the header's
/// and mapping's private sets).
#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Restore,
    Revert,
    Check,
    Warning,
    Chart,
    Measurement,
    Star,
    Play,
    Stop,
    ArrowForward,
}

fn paint_glyph(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    match glyph {
        Glyph::Restore => theme::paint_icon(painter, rect, theme::Icon::Restore, color),
        Glyph::Check => theme::paint_icon(painter, rect, theme::Icon::Check, color),
        Glyph::Warning => theme::paint_icon(painter, rect, theme::Icon::Warning, color),
        Glyph::Revert => {
            let size = rect.width().min(rect.height());
            let stroke = Stroke::new((size * 0.1).max(1.2), color);
            let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
            painter.add(Shape::CubicBezier(
                egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        point(0.85, 0.65),
                        point(0.85, 0.25),
                        point(0.4, 0.25),
                        point(0.25, 0.45),
                    ],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                ),
            ));
            painter.add(Shape::line(
                vec![point(0.25, 0.25), point(0.15, 0.45), point(0.4, 0.55)],
                stroke,
            ));
        }
        Glyph::Chart => {
            let size = rect.width().min(rect.height());
            let quad = |x: f32, y: f32, width: f32, height: f32| {
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left() + x * size, rect.top() + y * size),
                        Vec2::new(width * size, height * size),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            };
            quad(0.15, 0.8, 0.7, 0.1);
            for (x, height) in [(0.2095, 0.266), (0.423, 0.574), (0.6365, 0.406)] {
                quad(x, 0.85 - height, 0.154, height);
            }
        }
        Glyph::Measurement => {
            let size = rect.width().min(rect.height());
            let stroke = Stroke::new((size * 0.1).max(1.2), color);
            let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
            painter.add(Shape::line(
                vec![
                    point(0.15, 0.5),
                    point(0.3, 0.5),
                    point(0.42, 0.18),
                    point(0.6, 0.82),
                    point(0.72, 0.5),
                    point(0.85, 0.5),
                ],
                stroke,
            ));
        }
        Glyph::Star => {
            let size = rect.width().min(rect.height());
            let center = rect.center();
            let star = (0..10)
                .map(|index| {
                    let angle =
                        -std::f32::consts::PI / 2.0 + index as f32 * std::f32::consts::PI / 5.0;
                    let radius = if index % 2 == 0 { 0.45 } else { 0.2 } * size;
                    Pos2::new(
                        center.x + angle.cos() * radius,
                        center.y + angle.sin() * radius,
                    )
                })
                .collect();
            painter.add(Shape::convex_polygon(star, color, Stroke::NONE));
        }
        Glyph::Play => {
            let size = rect.width().min(rect.height());
            let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
            painter.add(Shape::convex_polygon(
                vec![point(0.23, 0.15), point(0.85, 0.5), point(0.23, 0.85)],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Stop => {
            let size = rect.width().min(rect.height());
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 0.2 * size, rect.top() + 0.2 * size),
                    Vec2::splat(0.6 * size),
                ),
                CornerRadius::ZERO,
                color,
            );
        }
        Glyph::ArrowForward => {
            let size = rect.width().min(rect.height());
            let stroke = Stroke::new((size * 0.1).max(1.2), color);
            let point = |x: f32, y: f32| Pos2::new(rect.left() + x * size, rect.top() + y * size);
            painter.add(Shape::line(
                vec![point(0.15, 0.5), point(0.85, 0.5)],
                stroke,
            ));
            painter.add(Shape::line(
                vec![point(0.6, 0.25), point(0.85, 0.5), point(0.6, 0.75)],
                stroke,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::PhysicalKey,
        protocol::{DisplayKey, UiSnapshot},
        settings::{Settings, SocdMode},
        ui::state::TimingInputs,
    };
    use egui_kittest::{Harness, kittest::Queryable};

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
        let draft = Settings::default();
        State {
            connected: true,
            inputs: TimingInputs::from_timing(&draft.timing),
            draft: Some(draft),
            snapshot: Some(baseline_snapshot()),
            ..State::default()
        }
    }

    /// The harness's stand-in for the app loop: it draws the page sections,
    /// runs the messages through `state::update`, and records the commands.
    struct FakeShell {
        state: State,
        sent: Vec<UiCommand>,
        cards: (Rect, Rect),
    }

    impl FakeShell {
        fn frame(&mut self, ui: &mut Ui) {
            let mut messages = Vec::new();
            messages.extend(header::header(ui, &self.state));
            if let (Some(snapshot), Some(_)) = (&self.state.snapshot, &self.state.draft) {
                let names = [
                    snapshot.keys[0].name.as_str(),
                    snapshot.keys[1].name.as_str(),
                    snapshot.keys[2].name.as_str(),
                    snapshot.keys[3].name.as_str(),
                ];
                self.cards = settings_cards(ui, &self.state, &mut messages);
                let _ = timeline::timeline_section(
                    ui,
                    &self.state.monitor,
                    names,
                    self.state.language,
                    self.state.focused,
                    &mut messages,
                );
                measurement_section(ui, &self.state, &mut messages);
                actions_bar(ui, &self.state, &mut messages);
            } else {
                disconnected_body(ui, &self.state, &mut messages);
            }
            messages.extend(profiles::profile_overlay(ui, &self.state));
            for message in messages {
                for effect in state::update(&mut self.state, message) {
                    if let Effect::Send(command) = effect {
                        self.sent.push(command);
                    }
                }
            }
        }
    }

    fn harness(state: State) -> Harness<'static, FakeShell> {
        egui_kittest::Harness::builder()
            .with_size(egui::vec2(1040.0, 1600.0))
            .build_ui_state(
                |ui, shell: &mut FakeShell| shell.frame(ui),
                FakeShell {
                    state,
                    sent: Vec::new(),
                    cards: (Rect::NOTHING, Rect::NOTHING),
                },
            )
    }

    /// The app without a connection: the page tests draw the real page.
    fn app(state: State) -> SettingsApp {
        SettingsApp {
            state,
            connection: None,
            events: std_mpsc::channel().1,
            focus_profile_name: false,
            focus_value_box: None,
            pending_section: None,
        }
    }

    /// A harness around the real page (`SettingsApp::page`) at the shipping
    /// window size, so the floating bars are exercised exactly as they ship.
    fn page_harness(state: State) -> Harness<'static, SettingsApp> {
        egui_kittest::Harness::builder()
            .with_size(egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT))
            .build_ui_state(|ui, app: &mut SettingsApp| app.page(ui), app(state))
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

    /// R2 round-2 finding 1: the shipped page must mount `preview::preview_card`
    /// (procedural paths, the 850 ms clock), not T4's placeholder. The
    /// transport glyphs are paths here, and a tick advances the phase once the
    /// clock's deadline passes.
    #[test]
    fn the_page_mounts_the_real_preview_and_advances_on_its_clock() {
        let mut harness = harness(baseline_state());
        harness.run();

        // The transport controls are procedural paths, not font glyphs.
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
                "the placeholder's font glyphs are gone ({glyph})"
            );
        }

        // Start the clock, then hand it egui time; the phase advances on the
        // 850 ms tick without the test sleeping. The clock needs the window
        // focused, which the real loop feeds from `InputState::focused`.
        harness.state_mut().state.focused = true;
        harness.input_mut().time = Some(10.0);
        harness.step();
        harness.get_by_label("Play preview").click();
        harness.input_mut().time = Some(10.1);
        harness.step();
        assert!(harness.state().state.preview.playing);
        harness.input_mut().time = Some(10.2);
        harness.step();
        assert_eq!(
            harness.state().state.preview.phase,
            0,
            "mounting schedules the first phase, it does not tick"
        );
        harness.input_mut().time = Some(11.1);
        harness.step();
        assert_eq!(
            harness.state().state.preview.phase,
            1,
            "the first tick lands 850 ms after the clock mounted"
        );
    }

    #[test]
    fn key_press_carries_both_key_names() {
        let press = key_press(egui::Key::W, Some(egui::Key::W));
        assert_eq!(press.character.as_deref(), Some("W"));
        assert_eq!(press.physical.as_deref(), Some("W"));
        let press = key_press(egui::Key::ArrowUp, None);
        assert_eq!(press.character.as_deref(), Some("ArrowUp"));
        assert_eq!(press.physical.as_deref(), Some("ArrowUp"));
    }

    #[test]
    fn the_disconnected_body_requests_a_snapshot() {
        let mut harness = harness(State::default());
        harness.run();
        harness.get_by_label("Request snapshot").click();
        harness.run();
        assert!(
            harness.state().sent.contains(&UiCommand::RequestSnapshot),
            "the manual snapshot request is dispatched: {:?}",
            harness.state().sent
        );
    }

    #[test]
    fn the_action_bar_emits_dirty_actions_only_when_dirty() {
        let mut harness = harness(baseline_state());
        harness.run();
        // Clean: Apply and Revert are inert.
        harness.get_by_label("Apply").click();
        harness.get_by_label("Revert").click();
        harness.run();
        assert!(harness.state().sent.is_empty());

        // Dirty: both emit their messages.
        harness
            .state_mut()
            .state
            .draft
            .as_mut()
            .unwrap()
            .timing
            .socd_transition_min_micros += 100;
        harness.run();
        harness.get_by_label("Apply").click();
        harness.run();
        assert!(
            harness
                .state()
                .sent
                .iter()
                .any(|command| matches!(command, UiCommand::UpdateDraft(_)))
        );
        assert!(
            harness
                .state()
                .sent
                .iter()
                .any(|command| matches!(command, UiCommand::Apply))
        );
    }

    #[test]
    fn the_measurement_card_renders_its_table_and_controls() {
        let mut state = baseline_state();
        state.session_details_open = true;
        state.snapshot.as_mut().unwrap().measurement = Some(MeasurementSnapshot {
            observed_event_count: 240,
            sample_count: 40,
            transition_count: 20,
            transition_p10_micros: Some(2_000),
            transition_median_micros: Some(3_000),
            transition_p90_micros: Some(4_000),
            transition_min_micros: Some(1_500),
            transition_max_micros: Some(4_500),
            transition_latest_micros: Some(3_200),
            overlap_count: 8,
            overlap_p10_micros: Some(1_000),
            overlap_median_micros: Some(2_000),
            overlap_p90_micros: Some(3_000),
            overlap_min_micros: Some(500),
            overlap_max_micros: Some(3_500),
            overlap_latest_micros: Some(2_500),
            near_simultaneous_count: 4,
            recommended_transition: Some(TimingRange {
                min_micros: 2_000,
                max_micros: 3_000,
            }),
            recommended_overlap: None,
        });
        let mut harness = harness(state);
        harness.run();
        harness.get_by_label("Input timing measurement");
        harness.get_by_label("Measured Input Transitions");
        harness.get_by_label("Suggested delays");
        harness.get_by_label("20.0%");
        harness.get_by_label("2.0 - 3.0 ms");
        harness.get_by_label("Collect at least\n10 samples");
        // The start control toggles measurement.
        harness.get_by_label("Start measurement").click();
        harness.run();
        assert!(
            harness
                .state()
                .sent
                .iter()
                .any(|command| matches!(command, UiCommand::StartMeasurement))
        );
        // The toggle also opens the session details, which is what keeps the
        // table and the tiles mounted until the runtime reports the stop.
        assert!(harness.state().state.session_details_open);
    }

    /// Distinct durations and counts for the table tests: every figure text is
    /// unique, so a painted text identifies exactly one cell.
    fn table_measurement() -> MeasurementSnapshot {
        MeasurementSnapshot {
            observed_event_count: 1_072,
            sample_count: 1_240,
            transition_count: 1_040,
            transition_p10_micros: Some(2_000),
            transition_median_micros: Some(3_100),
            transition_p90_micros: Some(4_000),
            transition_min_micros: Some(1_500),
            transition_max_micros: Some(4_500),
            transition_latest_micros: Some(3_200),
            overlap_count: 310,
            overlap_p10_micros: Some(900),
            overlap_median_micros: Some(1_800),
            overlap_p90_micros: Some(2_600),
            overlap_min_micros: Some(700),
            overlap_max_micros: Some(3_400),
            overlap_latest_micros: Some(2_500),
            near_simultaneous_count: 62,
            recommended_transition: Some(TimingRange {
                min_micros: 2_400,
                max_micros: 3_700,
            }),
            recommended_overlap: Some(TimingRange {
                min_micros: 1_100,
                max_micros: 2_200,
            }),
        }
    }

    fn open_measurement(state: &mut State) {
        state.session_details_open = true;
        state.snapshot.as_mut().unwrap().measurement = Some(table_measurement());
    }

    /// Every text drawn this frame with the shape that carries its ink and
    /// format.
    fn painted_text_shapes<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::TextShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::TextShape>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.clone()),
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

    fn shapes_with_text<State>(
        harness: &Harness<'_, State>,
        text: &str,
    ) -> Vec<egui::epaint::TextShape> {
        painted_text_shapes(harness)
            .into_iter()
            .filter(|shape| shape.galley.text() == text)
            .collect()
    }

    /// F14 + F22 (APP-TABLE-STYLING-AND-PROPORTIONS): the fixed columns hold the
    /// reference's own widths, the five metric columns share one positive fill,
    /// and every duration figure paints the body ink in the single non-bold
    /// monospace pass -- the P50 highlight is gone.
    #[test]
    fn the_latency_table_uses_the_reference_proportions_and_uniform_figure_ink() {
        let mut state = baseline_state();
        open_measurement(&mut state);
        let mut harness = harness(state);
        harness.run();

        assert_eq!(PATTERN_COLUMN, 240.0, "the reference pattern column width");
        assert_eq!(SAMPLES_COLUMN, 130.0, "the reference samples column width");
        let shapes = painted_text_shapes(&harness);
        let left_edge = |text: &str| {
            shapes
                .iter()
                .find(|shape| shape.galley.text() == text)
                .unwrap_or_else(|| panic!("missing {text}"))
                .pos
                .x
        };
        let right_edge = |text: &str| {
            let shape = shapes
                .iter()
                .find(|shape| shape.galley.text() == text)
                .unwrap_or_else(|| panic!("missing {text}"));
            shape.pos.x + shape.galley.size().x
        };
        let published = right_edge("SAMPLES") - left_edge("INPUT PATTERN");
        assert!(
            (published - (PATTERN_COLUMN + TABLE_GAP + SAMPLES_COLUMN)).abs() <= 1.0,
            "the row lays the fixed columns out with the shared gap: {published}"
        );
        // Each cell claims its width, so every row's figure shares the right
        // edge of its header instead of drifting with the row's own content.
        for (header, figure) in [
            ("SAMPLES", "1,040"),
            ("P10", "2.0 ms"),
            ("P50", "3.1 ms"),
            ("P90", "4.0 ms"),
            ("MIN", "1.5 ms"),
            ("MAX", "4.5 ms"),
        ] {
            let drift = (right_edge(header) - right_edge(figure)).abs();
            assert!(
                drift <= 0.5,
                "{figure} must line up under {header}: {drift}"
            );
        }
        let metrics = ["P10", "P50", "P90", "MIN", "MAX"];
        let steps: Vec<f32> = metrics
            .windows(2)
            .map(|pair| right_edge(pair[1]) - right_edge(pair[0]))
            .collect();
        assert!(
            steps[0] - TABLE_GAP > 0.0,
            "the five metric columns keep a positive fill: {steps:?}"
        );
        for step in &steps {
            assert!(
                (step - steps[0]).abs() <= 0.5,
                "the metric columns share one width: {steps:?}"
            );
        }

        for value in ["3.1 ms", "1.8 ms", "2.0 ms", "0.9 ms"] {
            let drawn = shapes_with_text(&harness, value);
            assert_eq!(
                drawn.len(),
                1,
                "{value} is painted once, without a bold stamp: {drawn:?}"
            );
            assert_eq!(drawn[0].fallback_color, theme::BODY_TEXT, "{value}");
        }
    }

    /// F15 + F21 + F23 (APP-MEASUREMENT-FORMAT-AND-ACCENTS): the integer metrics
    /// group their thousands, the overlap share takes the amber its table dot
    /// uses, and an available suggestion reads in the indigo accent.
    #[test]
    fn the_measurement_card_groups_counts_and_keeps_the_reference_accents() {
        let mut state = baseline_state();
        open_measurement(&mut state);
        let mut harness = harness(state);
        harness.run();

        let texts = painted_texts(&harness);
        for shown in ["1,072", "1,240", "1,040"] {
            assert!(texts.iter().any(|text| text == shown), "{shown}");
        }
        for raw in ["1072", "1240", "1040"] {
            assert!(
                !texts.iter().any(|text| text == raw),
                "{raw} must not paint ungrouped"
            );
        }

        let share = shapes_with_text(&harness, "25.0%");
        assert!(!share.is_empty(), "the overlap share is painted");
        assert!(
            share
                .iter()
                .all(|shape| shape.fallback_color == theme::AMBER_500),
            "the overlap share keeps the amber of its table dot"
        );

        let suggestion = shapes_with_text(&harness, "2.4 - 3.7 ms");
        assert!(
            !suggestion.is_empty(),
            "the transition suggestion is painted"
        );
        assert!(
            suggestion
                .iter()
                .all(|shape| shape.fallback_color == theme::INDIGO_600),
            "an available suggestion takes the indigo accent"
        );
    }

    /// F13 (APP-RECOMMENDATIONS-ALIGNMENT): the two recommendation tiles share
    /// one top line and one height, including the mixed case where one value is
    /// a range and the other is the two-line collect prompt.
    #[test]
    fn the_recommendation_tiles_share_one_top_and_height() {
        let cases: [(&str, Option<TimingRange>, Option<TimingRange>); 3] = [
            (
                "range + prompt",
                Some(TimingRange {
                    min_micros: 2_400,
                    max_micros: 3_700,
                }),
                None,
            ),
            (
                "prompt + range",
                None,
                Some(TimingRange {
                    min_micros: 1_100,
                    max_micros: 2_200,
                }),
            ),
            (
                "range + range",
                Some(TimingRange {
                    min_micros: 2_400,
                    max_micros: 3_700,
                }),
                Some(TimingRange {
                    min_micros: 1_100,
                    max_micros: 2_200,
                }),
            ),
        ];
        for (case, transition, overlap) in cases {
            let mut measurement = table_measurement();
            measurement.recommended_transition = transition;
            measurement.recommended_overlap = overlap;
            let mut harness = Harness::builder()
                .with_size(egui::vec2(1040.0, 1600.0))
                .build_ui_state(
                    |ui, state: &mut State| {
                        let mut messages = Vec::new();
                        recommendations_card(ui, &measurement, state.language, &mut messages);
                    },
                    baseline_state(),
                );
            harness.run();

            let mut frames = painted_rects_filled(&harness, theme::INSET);
            frames.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
            assert_eq!(frames.len(), 2, "{case}: two tile frames: {frames:?}");
            let (first, second) = (frames[0], frames[1]);
            assert!(
                (first.min.y - second.min.y).abs() <= 0.5,
                "{case}: one top line: {first:?} {second:?}"
            );
            assert!(
                (first.height() - second.height()).abs() <= 0.5,
                "{case}: one height: {first:?} {second:?}"
            );
        }
    }

    fn painted_rects_filled<State>(harness: &Harness<'_, State>, fill: Color32) -> Vec<egui::Rect> {
        fn collect(shape: &egui::Shape, fill: Color32, out: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Rect(rect) => {
                    if rect.fill == fill {
                        out.push(rect.rect);
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, fill, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &harness.output().shapes {
            collect(&clipped.shape, fill, &mut out);
        }
        out
    }

    /// F20 (APP-ACTION-BAR-NON-ITALIC): the dirty-draft hint stays upright.
    #[test]
    fn the_dirty_draft_hint_is_not_italic() {
        let mut harness = harness(baseline_state());
        harness.run();
        harness
            .state_mut()
            .state
            .draft
            .as_mut()
            .unwrap()
            .timing
            .socd_transition_min_micros += 100;
        harness.run();

        let hint = shapes_with_text(&harness, "Click Apply to commit draft edits.");
        assert!(!hint.is_empty(), "the dirty hint is painted");
        for shape in &hint {
            let italic = shape
                .galley
                .job
                .sections
                .iter()
                .any(|section| section.format.italics);
            assert!(!italic, "the dirty-draft hint is not italic");
        }
    }

    /// F06: the reference stretches both top-row cards to the taller one
    /// (`h-full` inside its grid), so their bottom edges must agree in every
    /// SOCD mode.
    #[test]
    fn the_two_settings_cards_match_height_in_every_mode() {
        for mode in SocdMode::ALL {
            let mut state = baseline_state();
            state.draft.as_mut().unwrap().timing.mode = mode;
            let mut harness = harness(state);
            harness.run();
            let (mapping, timing) = harness.state().cards;
            println!(
                "{mode:?}: mapping={} timing={}",
                mapping.height(),
                timing.height()
            );
            assert!(
                (mapping.height() - timing.height()).abs() < 0.5,
                "{mode:?}: the cards must share one height, got mappings {} and timing {}",
                mapping.height(),
                timing.height()
            );
        }
    }

    /// The row re-measures instead of keeping a stale height: the taller card
    /// changes with the mode, and switching back restores the first height.
    #[test]
    fn the_card_row_re_measures_when_the_mode_changes() {
        let mut harness = harness(baseline_state());
        harness.run();
        let (immediate_mapping, immediate_timing) = harness.state().cards;
        assert_eq!(immediate_mapping.height(), immediate_timing.height());

        harness
            .state_mut()
            .state
            .draft
            .as_mut()
            .unwrap()
            .timing
            .mode = SocdMode::PressDelay;
        harness.run();
        let (press_mapping, press_timing) = harness.state().cards;
        assert_eq!(press_mapping.height(), press_timing.height());
        assert!(
            press_mapping.height() < immediate_mapping.height(),
            "the row must shrink to the shorter mode, got {} then {}",
            immediate_mapping.height(),
            press_mapping.height()
        );

        harness
            .state_mut()
            .state
            .draft
            .as_mut()
            .unwrap()
            .timing
            .mode = SocdMode::Immediate;
        harness.run();
        let (back_mapping, back_timing) = harness.state().cards;
        assert_eq!(back_mapping.height(), back_timing.height());
        assert_eq!(
            back_mapping.height(),
            immediate_mapping.height(),
            "returning to a mode restores its shared height"
        );
    }

    /// Every card-chrome frame the last frame painted, in paint order: a
    /// `theme::card_style` body with its 2px border.
    fn card_frames<State>(harness: &Harness<'_, State>) -> Vec<egui::epaint::RectShape> {
        fn collect(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::Shape::Rect(rect)
                    if rect.fill == theme::SURFACE
                        && rect.stroke.color == theme::CARD_BORDER
                        && (rect.stroke.width - 2.0).abs() < 0.01 =>
                {
                    out.push(rect.clone());
                }
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

    /// The page's canvas: the PAGE_PADDING frame's outer bounds.
    fn canvas_rect<State>(harness: &Harness<'_, State>) -> Rect {
        fn collect(shape: &egui::Shape, out: &mut Vec<Rect>) {
            match shape {
                egui::Shape::Rect(rect) if rect.fill == theme::CANVAS => out.push(rect.rect),
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
        out.into_iter()
            .max_by(|left, right| left.area().total_cmp(&right.area()))
            .expect("the page canvas is painted")
    }

    /// The header and action bar frames: the page's topmost and bottom-most
    /// full-width card-chrome rectangles. Scrolled cards can sit above the
    /// header, so the bar width is the discriminator.
    fn floating_bars<State>(harness: &Harness<'_, State>, page: Rect) -> (Rect, Rect) {
        let frames: Vec<Rect> = card_frames(harness)
            .into_iter()
            .map(|frame| frame.rect)
            .filter(|rect| rect.width() > page.width() * 0.75)
            .collect();
        let header = frames
            .iter()
            .min_by(|left, right| left.top().total_cmp(&right.top()))
            .expect("the header bar is painted")
            .to_owned();
        let bar = frames
            .iter()
            .max_by(|left, right| left.bottom().total_cmp(&right.bottom()))
            .expect("the action bar is painted")
            .to_owned();
        (header, bar)
    }

    /// The topmost half-width card frame: the mappings/timing row inside the
    /// scroll owner. The row is the only half-width chrome on the page, and it
    /// may scroll up behind the header.
    fn settings_cards_rect<State>(harness: &Harness<'_, State>, page: Rect) -> Rect {
        card_frames(harness)
            .into_iter()
            .map(|frame| frame.rect)
            .filter(|rect| rect.width() < page.width() * 0.75)
            .min_by(|left, right| left.top().total_cmp(&right.top()))
            .expect("the settings cards are painted")
    }

    /// F18: the header and the action bar are floating sticky bars. Each keeps
    /// a page-edge margin on every side of the page canvas.
    #[test]
    fn the_floating_bars_keep_their_window_edge_margins() {
        let mut harness = page_harness(baseline_state());
        harness.run();
        let page = canvas_rect(&harness);
        let (header, bar) = floating_bars(&harness, page);
        assert!(
            (header.top() - page.top() - theme::PAGE_PADDING).abs() < 0.5,
            "the header's top margin is {}, expected PAGE_PADDING",
            header.top() - page.top()
        );
        assert!(
            (header.left() - page.left() - theme::PAGE_PADDING).abs() < 0.5,
            "the header's left margin is {}, expected PAGE_PADDING",
            header.left() - page.left()
        );
        assert!(
            (page.bottom() - bar.bottom() - theme::PAGE_PADDING).abs() < 0.5,
            "the action bar's bottom margin is {}, expected PAGE_PADDING",
            page.bottom() - bar.bottom()
        );
        assert!(
            (page.right() - bar.right() - theme::PAGE_PADDING).abs() < 0.5,
            "the action bar's right margin is {}, expected PAGE_PADDING",
            page.right() - bar.right()
        );
    }

    /// F18: the bars float above the scroll owner: scrolling the body moves the
    /// cards but leaves both bars where they are.
    #[test]
    fn the_floating_bars_stay_pinned_while_the_body_scrolls() {
        let mut harness = page_harness(baseline_state());
        harness.run();
        let page = canvas_rect(&harness);
        let (header, bar) = floating_bars(&harness, page);
        let content = settings_cards_rect(&harness, page);

        harness.get_by_label("Input timings").scroll_down();
        harness.run();
        let (header_after, bar_after) = floating_bars(&harness, page);
        let content_after = settings_cards_rect(&harness, page);

        assert!(
            content_after.top() < content.top() - 10.0,
            "the body must scroll, the cards moved {} to {}",
            content.top(),
            content_after.top()
        );
        assert_eq!(
            header, header_after,
            "the header stays pinned while the body scrolls (was {header:?}, now {header_after:?})"
        );
        assert_eq!(
            bar, bar_after,
            "the action bar stays pinned while the body scrolls (was {bar:?}, now {bar_after:?})"
        );
    }
}
