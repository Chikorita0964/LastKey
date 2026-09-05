use iced::{
    Center, Color, Element, Fill, Length, Padding, Size, Subscription, Task, Theme,
    widget::{
        Id, button, column, container, operation, row, rule, scrollable, slider, space, text,
        text::{Alignment, Ellipsis, Wrapping},
        text_input, toggler,
    },
    window,
};

use crate::{
    core::MIN_RECOMMENDATION_SAMPLES,
    protocol::{KeySlot, MeasurementSnapshot, UiCommand, UiEvent, UiSnapshot, UiView},
    settings::{Settings, TimingSettings},
};

use super::{
    ipc_client::{self, Connection, Event},
    theme,
};

pub fn run() -> iced::Result {
    iced::application(SettingsApp::new, SettingsApp::update, SettingsApp::view)
        .title(SettingsApp::title)
        .theme(SettingsApp::theme)
        .subscription(SettingsApp::subscription)
        .settings(iced::Settings {
            default_font: theme::UI_FONT,
            default_text_size: theme::BODY_TEXT_SIZE.into(),
            ..iced::Settings::default()
        })
        .window_size(WINDOW_SIZE)
        .run()
}

/// The one window size shared by both views. View switches used to resize and
/// read as a lag hitch, so the size is deliberately unified; `show_view`
/// therefore never resizes.
const WINDOW_SIZE: Size = Size::new(780.0, 760.0);

/// Stable id of the settings scrollable, giving the recommendations flow a
/// snap target at the bottom of the timing card.
const SETTINGS_BODY_ID: &str = "settings-body";

/// Value-box padding. Iced only aligns line boxes, not glyph ink: with these
/// fonts the ink sits about a pixel above the optical center, and no API moves
/// it. This shifts one pixel from the bottom padding to the top, keeping the
/// total height, as a single named constant — verify visually if the font
/// changes.
const VALUE_BOX_PADDING: Padding = Padding {
    top: 6.0,
    right: 5.0,
    bottom: 4.0,
    left: 5.0,
};

/// Key-badge padding. Same optical correction as `VALUE_BOX_PADDING`: the
/// badge glyphs render a pixel high, so one pixel moves from bottom to top.
const KBD_PADDING: Padding = Padding {
    top: 5.0,
    right: 4.0,
    bottom: 3.0,
    left: 4.0,
};

struct SettingsApp {
    connection: Option<Connection>,
    snapshot: Option<UiSnapshot>,
    draft: Option<Settings>,
    inputs: TimingInputs,
    current_view: UiView,
    /// Whether each value box shows the live input (`true`) or its
    /// press-to-edit facade (`false`). The facade swaps in the real box
    /// already focused and selected, so the first press never flashes a
    /// caret; later presses hit the real box and place the caret natively.
    /// Indexed by field discriminant. Any message that moves focus elsewhere —
    /// pressing another control, or the window losing focus — rearms every
    /// box, so the next press selects all again, Explorer-style.
    editing: [bool; 5],
    status: String,
    /// Success notice shown as a toast until the next server snapshot.
    notice: Option<String>,
    error: Option<String>,
}

/// Editable numeric buffers shadowing the timing draft. Sliders write straight
/// through to the draft; typed text commits on submit so partial input such as
/// an empty field never corrupts the draft mid-keystroke.
#[derive(Clone, Debug, Default, PartialEq)]
struct TimingInputs {
    transition_minimum: String,
    transition_maximum: String,
    preservation_rate: String,
    preserved_minimum: String,
    preserved_maximum: String,
}

impl TimingInputs {
    fn from_timing(timing: &TimingSettings) -> Self {
        Self {
            transition_minimum: format_ms(timing.socd_transition_min_micros),
            transition_maximum: format_ms(timing.socd_transition_max_micros),
            preservation_rate: format_rate(timing.overlap_preservation_rate),
            preserved_minimum: format_ms(timing.preserved_overlap_min_micros),
            preserved_maximum: format_ms(timing.preserved_overlap_max_micros),
        }
    }

    fn set_field(&mut self, field: TimingField, value: String) {
        *self.buffer_mut(field) = value;
    }

    fn buffer_mut(&mut self, field: TimingField) -> &mut String {
        match field {
            TimingField::TransitionMinimum => &mut self.transition_minimum,
            TimingField::TransitionMaximum => &mut self.transition_maximum,
            TimingField::PreservationRate => &mut self.preservation_rate,
            TimingField::PreservedMinimum => &mut self.preserved_minimum,
            TimingField::PreservedMaximum => &mut self.preserved_maximum,
        }
    }

    fn buffer(&self, field: TimingField) -> &str {
        match field {
            TimingField::TransitionMinimum => &self.transition_minimum,
            TimingField::TransitionMaximum => &self.transition_maximum,
            TimingField::PreservationRate => &self.preservation_rate,
            TimingField::PreservedMinimum => &self.preserved_minimum,
            TimingField::PreservedMaximum => &self.preserved_maximum,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum TimingField {
    TransitionMinimum = 0,
    TransitionMaximum = 1,
    PreservationRate = 2,
    PreservedMinimum = 3,
    PreservedMaximum = 4,
}

impl TimingField {
    const ALL: [Self; 5] = [
        Self::TransitionMinimum,
        Self::TransitionMaximum,
        Self::PreservationRate,
        Self::PreservedMinimum,
        Self::PreservedMaximum,
    ];

    const fn index(self) -> usize {
        self as usize
    }

    /// The one definition of "this row is live". The view grays the row with
    /// it and `update` drops slider drags with it, so the two cannot disagree.
    const fn is_editable(self, timing: &TimingSettings) -> bool {
        match self {
            Self::TransitionMinimum | Self::TransitionMaximum => {
                timing.socd_transition_delay_enabled
            }
            Self::PreservationRate | Self::PreservedMinimum | Self::PreservedMaximum => {
                timing.socd_transition_delay_enabled && timing.preserve_overlap
            }
        }
    }

    /// The draft slot this field edits. `PreservationRate` is a percentage,
    /// not a duration, so it has none and keeps its own path.
    fn micros_mut(self, timing: &mut TimingSettings) -> Option<&mut u32> {
        match self {
            Self::TransitionMinimum => Some(&mut timing.socd_transition_min_micros),
            Self::TransitionMaximum => Some(&mut timing.socd_transition_max_micros),
            Self::PreservationRate => None,
            Self::PreservedMinimum => Some(&mut timing.preserved_overlap_min_micros),
            Self::PreservedMaximum => Some(&mut timing.preserved_overlap_max_micros),
        }
    }

    /// The immutable sibling of `micros_mut`, so a display row can derive its
    /// value from `field` instead of restating it beside it.
    fn micros(self, timing: &TimingSettings) -> Option<u32> {
        match self {
            Self::TransitionMinimum => Some(timing.socd_transition_min_micros),
            Self::TransitionMaximum => Some(timing.socd_transition_max_micros),
            Self::PreservationRate => None,
            Self::PreservedMinimum => Some(timing.preserved_overlap_min_micros),
            Self::PreservedMaximum => Some(timing.preserved_overlap_max_micros),
        }
    }

    /// Whether this field's own min/max pair is inverted. The rate field has
    /// no pair; its validity is purely textual and decided by its caller.
    fn pair_invalid(self, timing: &TimingSettings) -> bool {
        match self {
            Self::TransitionMinimum | Self::TransitionMaximum => timing_pair_invalid(
                timing.socd_transition_min_micros,
                timing.socd_transition_max_micros,
            ),
            Self::PreservedMinimum | Self::PreservedMaximum => timing_pair_invalid(
                timing.preserved_overlap_min_micros,
                timing.preserved_overlap_max_micros,
            ),
            Self::PreservationRate => false,
        }
    }
}

/// Parse failure reported when a value box cannot commit its text. Kept as a
/// constant so a later successful submit only clears an error it produced
/// itself, leaving server errors on screen until the next snapshot.
const INVALID_TIMING_TEXT: &str = "Invalid timing value; reverted to the current draft.";

#[derive(Clone, Debug)]
enum Message {
    Ipc(Event),
    RequestSnapshot,
    ShowSettings,
    ShowMeasurement,
    Capture(KeySlot),
    TransitionDelayToggled(bool),
    TimingSliderChanged(TimingField, f32),
    PreserveOverlapToggled(bool),
    TimingTextChanged(TimingField, String),
    TimingTextSubmitted(TimingField),
    ValueBoxActivated(TimingField),
    WindowUnfocused,
    Apply,
    Revert,
    RestoreMappingDefaults,
    RestoreTimingDefaults,
    RestoreAllDefaults,
    ToggleMeasurement,
    ApplyRecommendations,
}

impl SettingsApp {
    fn new() -> Self {
        let current_view = requested_view();
        Self {
            connection: None,
            snapshot: None,
            draft: None,
            inputs: TimingInputs::default(),
            current_view,
            editing: [false; 5],
            status: "Connecting to the LastKey runtime...".into(),
            notice: None,
            error: None,
        }
    }

    fn title(&self) -> String {
        match self.current_view {
            UiView::Settings => "LastKey Settings".into(),
            UiView::Measurement => "LastKey Input Timing Results".into(),
        }
    }

    fn theme(&self) -> Theme {
        Theme::Light
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(ipc_client::connect).map(Message::Ipc),
            iced::event::listen_with(window_unfocused),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.track_box_focus(&message);
        match message {
            Message::Ipc(Event::Connected(connection)) => {
                self.connection = Some(connection);
                self.status = "Connected to the LastKey runtime.".into();
                self.error = None;
                self.send(UiCommand::RequestSnapshot);
            }
            Message::Ipc(Event::Message(event)) => return self.handle_event(*event),
            Message::Ipc(Event::Disconnected(error)) => {
                self.connection = None;
                self.status = "The LastKey runtime is disconnected.".into();
                self.error = Some(error);
            }
            Message::RequestSnapshot => {
                self.send(UiCommand::RequestSnapshot);
            }
            Message::ShowSettings => return self.show_view(UiView::Settings, false),
            Message::ShowMeasurement => return self.show_view(UiView::Measurement, false),
            Message::Capture(slot) => {
                // Timing stays local until Apply; the answering Snapshot
                // merges instead of replacing it (see set_snapshot).
                self.send(UiCommand::BeginKeyCapture(slot));
            }
            Message::TransitionDelayToggled(enabled) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.socd_transition_delay_enabled = enabled;
                }
            }
            Message::TimingSliderChanged(field, milliseconds) => {
                // The muted slider still emits drags while disabled, so the
                // gate lives here as well as in the widget tree.
                if let Some(draft) = self.draft.as_mut()
                    && field.is_editable(&draft.timing)
                    && let Some(slot) = field.micros_mut(&mut draft.timing)
                {
                    let micros = millis_to_micros(milliseconds);
                    *slot = micros;
                    self.inputs.set_field(field, format_ms(micros));
                }
            }
            Message::PreserveOverlapToggled(enabled) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.preserve_overlap = enabled;
                }
            }
            Message::TimingTextChanged(field, value) => {
                self.inputs.set_field(field, value);
            }
            Message::TimingTextSubmitted(field) => {
                if !self.commit_field(field) {
                    self.error = Some(INVALID_TIMING_TEXT.into());
                } else if self.error.as_deref() == Some(INVALID_TIMING_TEXT) {
                    self.error = None;
                }
            }
            Message::ValueBoxActivated(field) => {
                // The facade consumed the press, so the input below never saw
                // it and never showed a caret: focusing and selecting together
                // reveals the box with its whole value selected. Flag bookkeeping
                // already ran in `track_box_focus`.
                return Task::batch([
                    operation::focus(value_box_id(field)),
                    operation::select_all(value_box_id(field)),
                ]);
            }
            Message::WindowUnfocused => {
                // Rearming already ran in `track_box_focus`.
            }
            Message::Apply => {
                // Gate order is explicit: typed text, then local rules, then
                // IPC. The server remains the authoritative gate; this only
                // avoids a round trip for failures we can already name.
                if !self.commit_inputs() {
                    self.error = Some(INVALID_TIMING_TEXT.into());
                    return Task::none();
                }
                let Some(draft) = self.draft.clone() else {
                    return Task::none();
                };
                if let Err(error) = draft.validate() {
                    self.error = Some(error.to_string());
                    return Task::none();
                }
                self.send(UiCommand::UpdateDraft(draft));
                self.send(UiCommand::Apply);
                self.status = "Applying settings...".into();
            }
            Message::Revert => {
                // Timing is locally authoritative: reset now instead of
                // waiting for the reply, which may arrive behind older
                // snapshots that must not undo it.
                if let Some(saved) = self.snapshot.as_ref().map(|s| s.saved.clone()) {
                    self.inputs = TimingInputs::from_timing(&saved.timing);
                    self.draft = Some(saved);
                }
                self.send(UiCommand::Revert);
            }
            Message::RestoreMappingDefaults => {
                self.send(UiCommand::RestoreMappingDefaults);
            }
            Message::RestoreTimingDefaults => {
                // No dedicated server command exists, so the defaults ride
                // the regular draft path: bindings stay, timing resets, and
                // the answering Snapshot keeps the local timing (see
                // set_snapshot) until Apply persists it.
                let defaults = TimingSettings::default();
                self.inputs = TimingInputs::from_timing(&defaults);
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing = defaults;
                }
                if let Some(draft) = self.draft.clone() {
                    self.send(UiCommand::UpdateDraft(draft));
                }
            }
            Message::RestoreAllDefaults => {
                let defaults = Settings::default();
                self.inputs = TimingInputs::from_timing(&defaults.timing);
                self.draft = Some(defaults);
                self.send(UiCommand::RestoreAllDefaults);
            }
            Message::ToggleMeasurement => {
                let active = self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.measurement_active);
                self.send(if active {
                    UiCommand::StopMeasurement
                } else {
                    UiCommand::StartMeasurement
                });
            }
            Message::ApplyRecommendations => return self.apply_recommendations(),
        }
        Task::none()
    }

    /// Rearms value-box facades around focus moves. Typing, scrolling, and IPC
    /// traffic leave focus alone; activating one box arms it and disarms the
    /// rest, while every other message (button presses, slider drags, window
    /// unfocus) disarms all of them. The next press on a disarmed box
    /// therefore selects all again, Explorer-style.
    fn track_box_focus(&mut self, message: &Message) {
        match message {
            Message::TimingTextChanged(..)
            | Message::TimingTextSubmitted(..)
            | Message::Ipc(..) => {}
            Message::ValueBoxActivated(field) => {
                self.editing = [false; 5];
                self.editing[field.index()] = true;
            }
            _ => {
                self.editing = [false; 5];
            }
        }
    }

    /// Parses one typed buffer into the draft, refreshing the buffer from the
    /// stored value. Returns false when the text was invalid and reverted.
    fn commit_field(&mut self, field: TimingField) -> bool {
        let Some(draft) = self.draft.as_mut() else {
            return true;
        };
        if field == TimingField::PreservationRate {
            // Typing 0 means "off": it disables overlap preservation
            // instead of clamping to 1%. The stored rate is kept, so
            // re-enabling restores the previous share.
            let timing = &mut draft.timing;
            let inputs = &mut self.inputs;
            let disables = inputs
                .preservation_rate
                .trim()
                .parse::<f32>()
                .is_ok_and(|value| value.is_finite() && value.round() == 0.0);
            if disables {
                timing.preserve_overlap = false;
                inputs.preservation_rate = format_rate(timing.overlap_preservation_rate);
                return true;
            }
            return commit_text(
                &mut inputs.preservation_rate,
                &mut timing.overlap_preservation_rate,
                parse_rate_text,
                format_rate,
            );
        }
        // All duration fields share one path; the rate above is genuinely
        // special because of the "0 disables" rule.
        let Some(slot) = field.micros_mut(&mut draft.timing) else {
            return true;
        };
        commit_text(
            self.inputs.buffer_mut(field),
            slot,
            parse_ms_text,
            format_ms,
        )
    }

    fn commit_inputs(&mut self) -> bool {
        let mut committed = true;
        for field in TimingField::ALL {
            committed &= self.commit_field(field);
        }
        committed
    }

    /// Copies the measured recommendations into the local draft only. The user
    /// still confirms them with Apply; nothing is committed silently. The view
    /// switches to Settings with the timing card scrolled into view, since the
    /// edited values live at the bottom of that page.
    fn apply_recommendations(&mut self) -> Task<Message> {
        let recommendation = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.measurement)
            .map(|measurement| {
                (
                    measurement.recommended_transition,
                    measurement.recommended_overlap,
                )
            })
            .unwrap_or((None, None));
        if recommendation == (None, None) {
            self.status = format!(
                "Collect at least {MIN_RECOMMENDATION_SAMPLES} samples for recommendations."
            );
            return Task::none();
        }
        if let Some(draft) = self.draft.as_mut() {
            if let Some(range) = recommendation.0 {
                draft.timing.socd_transition_min_micros = range.min_micros;
                draft.timing.socd_transition_max_micros = range.max_micros;
            }
            if let Some(range) = recommendation.1 {
                draft.timing.preserved_overlap_min_micros = range.min_micros;
                draft.timing.preserved_overlap_max_micros = range.max_micros;
            }
        }
        if let Some(draft) = self.draft.as_ref() {
            self.inputs = TimingInputs::from_timing(&draft.timing);
        }
        // No push here: the answering Snapshot would overwrite the guidance
        // below with "synchronized with the runtime", which reads as already
        // active. Apply and the capture/measurement paths push on their own.
        self.notice = Some("Recommendations written to the draft. Select Apply when ready.".into());
        // The state flips to Settings synchronously, so the snap below lands
        // on the freshly built settings scrollable, scrolled to the timing
        // card at the bottom of the page.
        Task::batch([
            self.show_view(UiView::Settings, false),
            operation::snap_to_end(SETTINGS_BODY_ID),
        ])
    }

    fn handle_event(&mut self, event: UiEvent) -> Task<Message> {
        match event {
            UiEvent::Snapshot(snapshot) => {
                self.set_snapshot(snapshot);
                self.status = "Settings are synchronized with the runtime.".into();
                self.error = None;
            }
            UiEvent::ApplySucceeded(snapshot) => {
                self.set_snapshot(snapshot);
                self.notice = Some("Settings applied.".into());
                self.status = "Settings applied.".into();
                self.error = None;
            }
            UiEvent::KeyCaptured { slot, key } => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.bindings[key_slot_index(slot)] = key.physical;
                }
                if let Some(snapshot) = self.snapshot.as_mut() {
                    snapshot.draft.bindings[key_slot_index(slot)] = key.physical;
                    snapshot.keys[key_slot_index(slot)] = key;
                    snapshot.capture_slot = None;
                }
                self.status = "Mapping changed. Select Apply when ready.".into();
            }
            UiEvent::MeasurementUpdated(update) => {
                // A late update must not revive a stopped session; the stop
                // snapshot is authoritative about whether measurement runs.
                if let Some(snapshot) = self.snapshot.as_mut()
                    && snapshot.measurement_active
                {
                    snapshot.measurement = Some(update);
                }
            }
            UiEvent::ValidationFailed(error) | UiEvent::RuntimeError(error) => {
                self.error = Some(error.message);
            }
            UiEvent::FocusRequested(view) => {
                return self.show_view(view, true);
            }
            UiEvent::RuntimeShuttingDown => {
                self.connection = None;
                self.status = "The LastKey runtime is shutting down.".into();
                return window::latest().and_then(window::close);
            }
        }
        Task::none()
    }

    fn set_snapshot(&mut self, snapshot: UiSnapshot) {
        // Timing and its buffers are locally authoritative until Apply, so
        // every Snapshot merges: bindings, capture slot, measurement, and
        // saved come from the server while local timing stays. Only the
        // first snapshot (no local draft yet) replaces wholesale.
        // `ApplySucceeded` flows through the same rule, so mid-apply edits
        // stay dirty instead of vanishing.
        let mut snapshot = snapshot;
        self.notice = None;
        if self.draft.is_none() {
            self.inputs = TimingInputs::from_timing(&snapshot.draft.timing);
        } else if let Some(draft) = self.draft.as_ref() {
            snapshot.draft.timing = draft.timing.clone();
        }
        self.draft = Some(snapshot.draft.clone());
        self.snapshot = Some(snapshot);
    }

    fn show_view(&mut self, view: UiView, focus: bool) -> Task<Message> {
        self.current_view = view;
        if !focus {
            return Task::none();
        }
        window::latest().and_then(move |id| {
            Task::batch([
                window::set_mode(id, window::Mode::Windowed),
                window::gain_focus(id),
            ])
        })
    }

    fn send(&mut self, command: UiCommand) {
        let result = self
            .connection
            .as_ref()
            .ok_or_else(|| "The LastKey runtime is disconnected.".to_string())
            .and_then(|connection| connection.send(command));
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let settings_switch = if matches!(self.current_view, UiView::Settings) {
            button("Settings").style(theme::tab_selected)
        } else {
            button("Settings")
                .style(theme::tab_unselected)
                .on_press(Message::ShowSettings)
        };
        let measurement_switch = if matches!(self.current_view, UiView::Measurement) {
            button("Measurement").style(theme::tab_selected)
        } else {
            button("Measurement")
                .style(theme::tab_unselected)
                .on_press(Message::ShowMeasurement)
        };
        let connected = self.connection.is_some();
        let state_color = if connected {
            theme::OK_TEXT
        } else {
            theme::MUTED_TEXT
        };
        let header = container(
            row![
                row![
                    dot(state_color),
                    text(&self.status)
                        .size(theme::BODY_TEXT_SIZE)
                        .font(theme::UI_FONT_BOLD)
                        .color(theme::MUTED_TEXT)
                        .width(Fill)
                        .wrapping(Wrapping::None)
                        .ellipsis(Ellipsis::End),
                ]
                .spacing(14)
                .align_y(Center)
                .width(Fill),
                container(row![settings_switch, measurement_switch].spacing(6))
                    .padding(4)
                    .style(|_theme| theme::switch_style()),
            ]
            .spacing(theme::SECTION_GAP)
            .align_y(Center),
        )
        .padding(Padding {
            left: 20.0,
            ..Padding::from(12)
        })
        .width(Fill)
        .style(|_theme| theme::card_style());

        let body: Element<_> = match self.current_view {
            UiView::Settings => self.settings_view(),
            UiView::Measurement => self.measurement_view(),
        };
        let actions: Option<Element<'_, Message>> = match self.current_view {
            UiView::Settings => self.settings_actions(),
            UiView::Measurement => None,
        };
        // The connection state lives in the header status line, so no footer
        // strip is needed.
        let mut page = column![header, body]
            .spacing(theme::SECTION_GAP)
            .padding(theme::PAGE_PADDING)
            .height(Fill);
        if let Some(actions) = actions {
            page = page.push(actions);
        }
        container(page)
            .height(Fill)
            .width(Fill)
            .style(|_theme| theme::canvas_style())
            .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let (Some(snapshot), Some(draft)) = (&self.snapshot, &self.draft) else {
            return disconnected_view(self.error.as_ref());
        };
        let timing = &draft.timing;
        let transition_rows_live = TimingField::TransitionMinimum.is_editable(timing);
        let overlap_rows_live = TimingField::PreservedMinimum.is_editable(timing);

        let mappings = container(
            column![
                row![
                    text("Key mappings").size(theme::HEADING_SIZE).width(Fill),
                    button("Restore mapping defaults")
                        .style(theme::secondary_button)
                        .on_press(Message::RestoreMappingDefaults),
                ]
                .align_y(Center),
                text("Hardware scan-code assignment for the SOCD filter. Select Rebind, then press a key.")
                    .size(12)
                    .color(theme::MUTED_TEXT),
                row![
                    axis_panel(
                        "Vertical axis",
                        [
                            (KeySlot::VerticalFirst, "Vertical Primary (Up)"),
                            (KeySlot::VerticalSecond, "Vertical Secondary (Down)"),
                        ],
                        snapshot,
                    ),
                    axis_panel(
                        "Horizontal axis",
                        [
                            (KeySlot::HorizontalFirst, "Horizontal Primary (Left)"),
                            (KeySlot::HorizontalSecond, "Horizontal Secondary (Right)"),
                        ],
                        snapshot,
                    ),
                ]
                .spacing(theme::SECTION_GAP),
                row![
                    space::horizontal(),
                    text("Modifiers like Shift, Ctrl, and Alt are not captured.")
                        .size(12)
                        .color(theme::MUTED_TEXT),
                ],
            ]
            .spacing(theme::SECTION_GAP),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_theme| theme::card_style());

        let timing_card = container(
            column![
                row![
                    text("Input timing").size(theme::HEADING_SIZE).width(Fill),
                    button("Restore timing defaults")
                        .style(theme::secondary_button)
                        .on_press(Message::RestoreTimingDefaults),
                ]
                .align_y(Center),
                text("Edits stay in the local draft until Apply. Sliders cover 0.0-20.0 ms in 0.1 ms steps; every value box is directly editable and stays in sync with its slider.")
                    .size(12)
                    .color(theme::MUTED_TEXT),
                row![
                    text("Enable SOCD transition delay").width(Fill),
                    toggler(timing.socd_transition_delay_enabled)
                        .style(theme::accent_toggler)
                        .on_toggle(Message::TransitionDelayToggled),
                ]
                .align_y(Center),
                transition_group(timing, &self.inputs, &self.editing),
                row![
                    text("Preserve physical overlap").width(Fill),
                    row![
                        text("PROBABILITY").size(11),
                        rate_box(
                            &self.inputs.preservation_rate,
                            overlap_rows_live,
                            self.editing[TimingField::PreservationRate.index()],
                        ),
                        text("%").size(12).color(theme::MUTED_TEXT),
                    ]
                    .spacing(6)
                    .align_y(Center),
                    toggler(timing.preserve_overlap)
                        .style(theme::accent_toggler)
                        .on_toggle_maybe(
                            transition_rows_live.then_some(Message::PreserveOverlapToggled),
                        ),
                ]
                .spacing(theme::ROW_GAP)
                .align_y(Center),
                overlap_group(timing, &self.inputs, &self.editing),
            ]
            .spacing(theme::SECTION_GAP),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_theme| theme::card_style());

        // The snapshot already carries saved settings, so a dirty indicator
        // is free: Apply and Revert rest disabled while there is nothing to
        // do. The restore actions stay enabled; they define new drafts.
        // Typing alone must count as dirty: buffers commit only on submit,
        // so comparing drafts would leave a typed-then-Apply click dead.
        // The bar stays outside the scrollable body so Apply never requires
        // scrolling; it sits directly above the status footer.
        scrollable(column![mappings, timing_card].spacing(theme::SECTION_GAP))
            .id(SETTINGS_BODY_ID)
            .height(Fill)
            .into()
    }

    fn settings_actions(&self) -> Option<Element<'_, Message>> {
        let (Some(snapshot), Some(draft)) = (&self.snapshot, &self.draft) else {
            return None;
        };
        let uncommitted_text = self.inputs != TimingInputs::from_timing(&draft.timing);
        let dirty = self.draft.as_ref() != Some(&snapshot.saved) || uncommitted_text;
        let revert = if dirty {
            button("Revert")
                .style(theme::secondary_button)
                .on_press(Message::Revert)
        } else {
            button("Revert").style(theme::secondary_button)
        };
        let apply = if dirty {
            button("Apply")
                .style(theme::primary_button)
                .on_press(Message::Apply)
        } else {
            button("Apply").style(theme::primary_button)
        };
        // Error and notice feedback lives in this bar as plain text rather
        // than as toggling banners above the scrollable: the page is diffed
        // positionally, so a banner appearing or disappearing above the body
        // hands the scrollable state slot to another widget and resets the
        // scroll offset. Swapping only this text never moves any widget.
        let feedback: Element<'_, Message> = match (&self.error, &self.notice) {
            (Some(error), _) => text(error)
                .size(theme::BODY_TEXT_SIZE)
                .font(theme::UI_FONT_BOLD)
                .color(theme::ERROR_TEXT)
                .width(Fill)
                .align_x(Alignment::Right)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::End)
                .into(),
            (None, Some(notice)) => text(notice)
                .size(theme::BODY_TEXT_SIZE)
                .font(theme::UI_FONT_BOLD)
                .color(theme::OK_TEXT)
                .width(Fill)
                .align_x(Alignment::Right)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::End)
                .into(),
            (None, None) => space::horizontal().into(),
        };
        let actions = container(
            row![
                button("Restore all defaults")
                    .style(theme::secondary_button)
                    .on_press(Message::RestoreAllDefaults),
                feedback,
                // Extra breathing room before Revert, mirroring the widened
                // dot-to-status gap in the header.
                space::horizontal().width(Length::Fixed(8.0)),
                revert,
                apply,
            ]
            .spacing(theme::ROW_GAP)
            .align_y(Center),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_theme| theme::card_style());

        Some(actions.into())
    }

    fn measurement_view(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return disconnected_view(self.error.as_ref());
        };
        let button_label = if snapshot.measurement_active {
            "Stop measurement"
        } else {
            "Start measurement"
        };
        let summary = container(
            column![
                row![
                    column![
                        text("Input timing measurement").size(theme::HEADING_SIZE),
                        text("Measurement observes configured physical key pairs and excludes LastKey output.")
                            .size(12)
                            .color(theme::MUTED_TEXT),
                    ]
                    .width(Fill)
                    .spacing(4),
                    button(button_label)
                        .style(theme::primary_button)
                        .on_press(Message::ToggleMeasurement),
                ]
                .align_y(Center),
                {
                    let stats: Element<_> = match snapshot.measurement {
                        Some(measurement) => row![
                            stat_box(
                                "Physical key edges",
                                measurement.observed_event_count.to_string(),
                                None,
                                None,
                            ),
                            stat_box(
                                "Valid paired samples",
                                measurement.sample_count.to_string(),
                                Some(theme::PRIMARY_TEXT),
                                None,
                            ),
                            stat_box(
                                "Physical overlap share",
                                percentage_value(
                                    measurement.overlap_count,
                                    measurement.sample_count
                                ),
                                Some(theme::WARN_TEXT),
                                (measurement.sample_count != 0).then_some("%"),
                            ),
                            stat_box(
                                "Near-simultaneous share",
                                percentage_value(
                                    measurement.near_simultaneous_count,
                                    measurement.sample_count,
                                ),
                                Some(theme::ERROR_TEXT),
                                (measurement.sample_count != 0).then_some("%"),
                            ),
                        ]
                        .spacing(theme::ROW_GAP)
                        .into(),
                        None => text("No measurement results yet.").into(),
                    };
                    stats
                },
            ]
            .spacing(theme::SECTION_GAP),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_theme| theme::card_style());

        let mut content = column![summary].spacing(theme::SECTION_GAP);
        if let Some(measurement) = snapshot.measurement {
            content = content.push(latencies_card(measurement));
            content = content.push(recommendations_card(measurement));
        }
        scrollable(content).height(Fill).into()
    }
}

impl Drop for SettingsApp {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            let _ = connection.send(UiCommand::CloseUiSession);
        }
    }
}

/// Small status or legend mark.
fn dot(color: Color) -> Element<'static, Message> {
    container(space::horizontal())
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .style(move |_theme| theme::dot_style(color))
        .into()
}

fn axis_panel<'a>(
    title: &'static str,
    slots: [(KeySlot, &'static str); 2],
    snapshot: &'a UiSnapshot,
) -> Element<'a, Message> {
    let duplicates = duplicate_slots(&snapshot.draft.bindings);
    container(
        column![
            text(title.to_uppercase()).size(11).color(theme::MUTED_TEXT),
            key_row(
                slots[0].1,
                slots[0].0,
                snapshot,
                duplicates[key_slot_index(slots[0].0)],
            ),
            key_row(
                slots[1].1,
                slots[1].0,
                snapshot,
                duplicates[key_slot_index(slots[1].0)],
            ),
        ]
        .spacing(theme::ROW_GAP),
    )
    .padding(theme::GROUP_PADDING)
    .width(Fill)
    .style(|_theme| theme::group_style())
    .into()
}

fn key_row<'a>(
    label: &'a str,
    slot: KeySlot,
    snapshot: &'a UiSnapshot,
    duplicate: bool,
) -> Element<'a, Message> {
    let key = &snapshot.keys[key_slot_index(slot)];
    let capture_label = if snapshot.capture_slot == Some(slot) {
        "Press a key…"
    } else {
        "Rebind"
    };
    container(
        row![
            text(label)
                .width(Fill)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::End),
            container(
                text(&key.name)
                    .size(12)
                    .font(theme::MONO_FONT)
                    .width(Fill)
                    .align_x(Alignment::Center),
            )
            .padding(KBD_PADDING)
            .width(Length::Fixed(72.0))
            .style(|_theme| theme::kbd_style()),
            button(capture_label)
                .style(theme::secondary_button)
                .on_press(Message::Capture(slot)),
        ]
        .spacing(theme::ROW_GAP)
        .align_y(Center)
        .width(Fill),
    )
    .padding(6)
    .width(Fill)
    .style(move |_theme| {
        if duplicate {
            theme::slot_error_style()
        } else {
            theme::slot_style()
        }
    })
    .into()
}

/// Millisecond slider with a directly editable value box. Values outside the
/// 0–20 ms slider window stay valid backend values; the slider then pins at
/// the nearest end while the box shows the true draft value. A disabled group
/// keeps its controls on screen in a muted style instead of collapsing, so
/// toggling an option never moves the layout.
///
/// The first press never touches a live input: an untouched box renders as a
/// lookalike button, and pressing it swaps in the real box already focused
/// with its content selected — no caret ever flashes. Later presses hit the
/// real box, so caret placement works natively.
fn transition_group<'a>(
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
) -> Element<'a, Message> {
    container(
        row![
            ms_field(
                "Transition minimum",
                TimingField::TransitionMinimum,
                timing,
                inputs,
                editing,
            ),
            ms_field(
                "Transition maximum",
                TimingField::TransitionMaximum,
                timing,
                inputs,
                editing,
            ),
        ]
        .spacing(theme::SECTION_GAP),
    )
    .padding(theme::GROUP_PADDING)
    .width(Fill)
    .style(|_theme| theme::group_style())
    .into()
}

fn overlap_group<'a>(
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
) -> Element<'a, Message> {
    container(
        row![
            ms_field(
                "Preserved overlap minimum",
                TimingField::PreservedMinimum,
                timing,
                inputs,
                editing,
            ),
            ms_field(
                "Preserved overlap maximum",
                TimingField::PreservedMaximum,
                timing,
                inputs,
                editing,
            ),
        ]
        .spacing(theme::SECTION_GAP),
    )
    .padding(theme::GROUP_PADDING)
    .width(Fill)
    .style(|_theme| theme::group_style())
    .into()
}

/// One millisecond slider row. Every value in the row is derived from `field`,
/// so a row cannot display one field while acting on another.
fn ms_field<'a>(
    label: &'static str,
    field: TimingField,
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
) -> Element<'a, Message> {
    let micros = field
        .micros(timing)
        .expect("ms_field builds duration rows only");
    let buffer = inputs.buffer(field);
    let enabled = field.is_editable(timing);
    let editing = editing[field.index()];
    let pair_invalid = field.pair_invalid(timing);
    // A text box without handlers renders disabled; the slider widget has no
    // disabled state, so it keeps emitting drags that `update` ignores while
    // the group is off (see the `TimingSliderChanged` arm). Red marks an
    // unparseable box or a minimum above its maximum, but only while the
    // group is editable: a disabled group stays gray whatever it holds.
    let invalid = enabled && (pair_invalid || parse_ms_text(buffer).is_none());
    let value_box = value_box(field, buffer, enabled, editing, invalid, 64.0);
    let rail = slider(
        0.0..=20.0,
        (micros as f32 / 1_000.0).min(20.0),
        move |value| Message::TimingSliderChanged(field, value),
    )
    .step(0.1);
    let rail = if enabled {
        rail.style(theme::accent_slider)
    } else {
        rail.style(theme::muted_slider)
    };
    let label_text = if enabled {
        text(label)
    } else {
        text(label).color(theme::MUTED_TEXT)
    };
    column![
        row![
            label_text.width(Fill),
            row![value_box, text("ms").size(12).color(theme::MUTED_TEXT),]
                .spacing(4)
                .align_y(Center),
        ]
        .align_y(Center),
        rail,
        row![
            scale_mark("0.0 ms"),
            space::horizontal(),
            scale_mark("10.0 ms"),
            space::horizontal(),
            scale_mark("20.0 ms"),
        ],
    ]
    .spacing(4)
    .into()
}

fn scale_mark(label: &'static str) -> Element<'static, Message> {
    text(label)
        .size(11)
        .font(theme::MONO_FONT)
        .color(theme::MUTED_TEXT)
        .into()
}

fn rate_box<'a>(buffer: &'a str, enabled: bool, editing: bool) -> Element<'a, Message> {
    // A "0" buffer disables overlap preservation on commit instead of
    // failing, so only genuinely unparseable text counts as invalid here.
    let invalid = enabled && parse_rate_text(buffer).is_none();
    value_box(
        TimingField::PreservationRate,
        buffer,
        enabled,
        editing,
        invalid,
        56.0,
    )
}

/// The press-to-edit value box shared by the millisecond rows and the rate
/// box: a facade until the user activates it, then the live input, already
/// focused and selected. Any fix to the swap (focus ordering, selection,
/// styling) lands here once instead of drifting between two copies.
fn value_box<'a>(
    field: TimingField,
    buffer: &'a str,
    enabled: bool,
    editing: bool,
    invalid: bool,
    width: f32,
) -> Element<'a, Message> {
    if enabled && !editing {
        return value_facade(buffer, field, width, invalid);
    }
    let mut live = text_input("", buffer)
        .font(theme::MONO_FONT)
        .align_x(Alignment::Center)
        .padding(VALUE_BOX_PADDING)
        .style(if invalid {
            theme::value_input_error
        } else {
            theme::value_input
        })
        .id(value_box_id(field))
        .width(Length::Fixed(width));
    if enabled {
        live = live
            .on_input(move |input| Message::TimingTextChanged(field, input))
            .on_submit(Message::TimingTextSubmitted(field));
    }
    live.into()
}

/// Press-to-edit lookalike for an untouched value box. It mirrors the live
/// box visuals so the swap is invisible; buttons report presses on release,
/// after any drag settled, which pairs with focusing and selecting below.
fn value_facade<'a>(
    buffer: &'a str,
    field: TimingField,
    width: f32,
    invalid: bool,
) -> Element<'a, Message> {
    button(
        text(buffer)
            .font(theme::MONO_FONT)
            .width(Fill)
            .align_x(Alignment::Center),
    )
    .style(if invalid {
        theme::facade_button_error
    } else {
        theme::facade_button
    })
    .padding(VALUE_BOX_PADDING)
    .width(Length::Fixed(width))
    .on_press(Message::ValueBoxActivated(field))
    .into()
}

/// Stable widget id per value box, used by the activate-to-select-all flow.
fn value_box_id(field: TimingField) -> Id {
    Id::new(match field {
        TimingField::TransitionMinimum => "timing-transition-minimum",
        TimingField::TransitionMaximum => "timing-transition-maximum",
        TimingField::PreservationRate => "timing-preservation-rate",
        TimingField::PreservedMinimum => "timing-preserved-minimum",
        TimingField::PreservedMaximum => "timing-preserved-maximum",
    })
}

/// Whether a timing minimum exceeds its maximum. Mirrors the range half of
/// [`Settings::validate`] so the offending boxes can blush live.
fn timing_pair_invalid(min_micros: u32, max_micros: u32) -> bool {
    min_micros > max_micros
}

/// Flags every binding slot that shares its key with another slot.
fn duplicate_slots(bindings: &[crate::core::PhysicalKey; 4]) -> [bool; 4] {
    std::array::from_fn(|index| {
        bindings
            .iter()
            .enumerate()
            .any(|(other, binding)| other != index && *binding == bindings[index])
    })
}

fn stat_box(
    label: &'static str,
    value: String,
    color: Option<Color>,
    unit: Option<&'static str>,
) -> Element<'static, Message> {
    let value_color = color.unwrap_or(theme::MUTED_TEXT);
    let number: Element<'static, Message> = text(value)
        .font(theme::MONO_FONT)
        .size(STAT_VALUE_SIZE)
        .color(value_color)
        .into();
    // The unit shares the value color at label size: at full size its tall
    // glyphs (notably `%`) read larger than the digits. Leading and trailing
    // flexible space centers the group; uniform spacing doubles as the
    // number-unit gap.
    let mut bottom = row![space::horizontal(), number];
    if let Some(unit) = unit {
        bottom = bottom.push(text(unit).size(theme::BODY_TEXT_SIZE).color(value_color));
    }
    let bottom: Element<'static, Message> = bottom.push(space::horizontal()).spacing(4).into();
    container(
        column![
            text(label)
                .size(theme::BODY_TEXT_SIZE)
                .width(Fill)
                .align_x(Alignment::Center),
            space::vertical(),
            bottom,
        ]
        .height(Fill),
    )
    .padding(Padding {
        top: 14.0,
        right: 10.0,
        bottom: 14.0,
        left: 10.0,
    })
    .width(Fill)
    .height(Length::Fixed(84.0))
    .style(|_theme| theme::group_style())
    .into()
}

/// Horizontal stat tile for the recommendations card: label on the left with
/// a small indent, value hugging the right edge.
fn stat_inline(label: &'static str, value: String, color: Color) -> Element<'static, Message> {
    container(
        row![
            container(text(label).size(theme::BODY_TEXT_SIZE))
                .width(Fill)
                .padding(Padding {
                    left: 4.0,
                    ..Padding::default()
                }),
            // The monospace value renders high next to the UI-font label, so
            // it gets the same one-sided optical nudge as the value boxes,
            // plus a right inset so it never touches the tile edge.
            container(
                text(value)
                    .font(theme::MONO_FONT)
                    .size(STAT_VALUE_SIZE)
                    .color(color),
            )
            .padding(Padding {
                top: 2.0,
                right: 4.0,
                ..Padding::default()
            }),
        ]
        .align_y(Center),
    )
    .padding(10)
    .width(Fill)
    .style(|_theme| theme::group_style())
    .into()
}

fn latencies_card(measurement: MeasurementSnapshot) -> Element<'static, Message> {
    container(
        column![
            text("Measured Axis Latencies").size(theme::HEADING_SIZE),
            text("Counts and P10 / median / P90 from the current snapshot. Near-simultaneous samples (<1 ms) carry no distribution.")
                .size(12)
                .color(theme::MUTED_TEXT),
            container(
                column![
                    table_row([
                        heading("INPUT PATTERN", Length::Fixed(PATTERN_COLUMN), Alignment::Left),
                        heading("SAMPLES", Fill, Alignment::Right),
                        heading("MEDIAN", Fill, Alignment::Right),
                        heading("P10", Fill, Alignment::Right),
                        heading("P90", Fill, Alignment::Right),
                        heading("MIN", Fill, Alignment::Right),
                        heading("MAX", Fill, Alignment::Right),
                    ]),
                    table_hrule(),
                    table_row(pattern_figures(
                        "Neutral transition",
                        theme::PRIMARY_TEXT,
                        measurement.transition_count,
                        [
                            duration(measurement.transition_median_micros),
                            duration(measurement.transition_p10_micros),
                            duration(measurement.transition_p90_micros),
                            duration(measurement.transition_min_micros),
                            duration(measurement.transition_max_micros),
                        ],
                    )),
                    table_hrule(),
                    table_row(pattern_figures(
                        "Physical overlap",
                        theme::WARN_TEXT,
                        measurement.overlap_count,
                        [
                            duration(measurement.overlap_median_micros),
                            duration(measurement.overlap_p10_micros),
                            duration(measurement.overlap_p90_micros),
                            duration(measurement.overlap_min_micros),
                            duration(measurement.overlap_max_micros),
                        ],
                    )),
                    table_hrule(),
                    table_row(pattern_figures(
                        "Near-simultaneous",
                        theme::ERROR_TEXT,
                        measurement.near_simultaneous_count,
                        [
                            "<1 ms".into(),
                            "—".into(),
                            "—".into(),
                            "—".into(),
                            "—".into(),
                        ],
                    )),
                ]
                .spacing(theme::ROW_GAP),
            )
            .padding(theme::GROUP_PADDING)
            .width(Fill)
            .style(|_theme| theme::group_style()),
        ]
        .spacing(theme::ROW_GAP),
    )
    .padding(theme::CARD_PADDING)
    .width(Fill)
    .style(|_theme| theme::card_style())
    .into()
}

fn recommendations_card(measurement: MeasurementSnapshot) -> Element<'static, Message> {
    container(
        column![
            row![
                column![
                    text("Suggested SOCD settings").size(theme::HEADING_SIZE),
                    text("Suggestions use P10 through P50 after excluding near-simultaneous samples. Both configured axes and directions are combined.")
                        .size(12)
                        .color(theme::MUTED_TEXT),
                ]
                .width(Fill)
                .spacing(4),
                button("Apply Recommendations to Settings")
                    .style(theme::primary_button)
                    .on_press(Message::ApplyRecommendations),
            ]
            .align_y(Center),
            row![
                stat_inline(
                    "SOCD Transition Delay",
                    timing_range(measurement.recommended_transition),
                    theme::OK_TEXT,
                ),
                stat_inline(
                    "Preserved Overlap Duration",
                    timing_range(measurement.recommended_overlap),
                    theme::OK_TEXT,
                ),
            ]
            .spacing(theme::ROW_GAP),
        ]
        .spacing(theme::SECTION_GAP),
    )
    .padding(theme::CARD_PADDING)
    .width(Fill)
    .style(|_theme| theme::card_style())
    .into()
}

const PATTERN_COLUMN: f32 = 150.0;

/// Statistic value size. The bottom-row suffix shares it so both sit on one
/// baseline instead of looking like separate lines.
const STAT_VALUE_SIZE: f32 = 18.0;

/// Table header shares the card title color instead of the muted tone.
fn heading(label: &'static str, width: Length, align: Alignment) -> Element<'static, Message> {
    text(label).size(11).width(width).align_x(align).into()
}

/// One table row: seven cells sharing the same widths and spacing, so
/// columns line up down the table.
fn table_row(cells: [Element<'static, Message>; 7]) -> Element<'static, Message> {
    let [pattern, samples, median, p10, p90, minimum, maximum] = cells;
    row![pattern, samples, median, p10, p90, minimum, maximum]
        .spacing(6)
        .align_y(Center)
        .width(Fill)
        .into()
}

/// Pattern label plus sample count plus median / P10 / P90 / minimum /
/// maximum figures, in table-cell order.
fn pattern_figures(
    label: &'static str,
    color: Color,
    count: u32,
    figures: [String; 5],
) -> [Element<'static, Message>; 7] {
    let [median, p10, p90, minimum, maximum] = figures;
    [
        row![dot(color), text(label)]
            .spacing(6)
            .align_y(Center)
            .width(Length::Fixed(PATTERN_COLUMN))
            .into(),
        figure(count.to_string(), None),
        figure(median, Some(theme::PRIMARY_TEXT)),
        figure(p10, None),
        figure(p90, None),
        figure(minimum, None),
        figure(maximum, None),
    ]
}

fn table_hrule() -> Element<'static, Message> {
    rule::horizontal(1).style(theme::table_rule).into()
}

/// Numeric table cell: right-aligned so decimal places line up. The pattern
/// label column stays left-aligned.
fn figure(value: String, color: Option<Color>) -> Element<'static, Message> {
    text(value)
        .size(12)
        .font(theme::MONO_FONT)
        .color(color.unwrap_or(theme::MUTED_TEXT))
        .width(Fill)
        .align_x(Alignment::Right)
        .into()
}

fn disconnected_view(error: Option<&String>) -> Element<'_, Message> {
    let mut content = column![
        text("The settings UI is waiting for LastKey.exe."),
        button("Request snapshot")
            .style(theme::secondary_button)
            .on_press(Message::RequestSnapshot),
    ]
    .spacing(theme::ROW_GAP);
    // The action bar (which now carries error feedback) needs a snapshot, so
    // surface connection errors here while disconnected.
    if let Some(error) = error {
        content = content.push(text(error).size(12).color(theme::ERROR_TEXT));
    }
    content.into()
}

fn key_slot_index(slot: KeySlot) -> usize {
    match slot {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}

fn requested_view() -> UiView {
    requested_view_from(std::env::args())
}

/// Maps window-unfocus events to a facade-rearming message. `Event` here is
/// the iced runtime event, not the IPC one.
fn window_unfocused(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::WindowUnfocused),
        _ => None,
    }
}

fn requested_view_from(arguments: impl IntoIterator<Item = impl AsRef<str>>) -> UiView {
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument.as_ref() != "--view" {
            continue;
        }
        return match arguments.next().as_ref().map(AsRef::as_ref) {
            Some("measurement") => UiView::Measurement,
            _ => UiView::Settings,
        };
    }
    UiView::Settings
}

/// Milliseconds mirror of the single timing ceiling in `Settings::validate`,
/// which is the rule for every entry path — this only clamps typed text.
/// Clamp before scaling: `(x * 10.0).round() as u32` saturates at `u32::MAX`
/// for large input, and the following `* 100` would then overflow.
const MAX_TIMING_MILLIS: f32 = crate::settings::MAX_TIMING_MICROS as f32 / 1_000.0;

fn millis_to_micros(milliseconds: f32) -> u32 {
    (milliseconds.clamp(0.0, MAX_TIMING_MILLIS) * 10.0).round() as u32 * 100
}

fn format_ms(micros: u32) -> String {
    format!("{:.1}", micros as f32 / 1_000.0)
}

fn format_rate(rate: u8) -> String {
    rate.to_string()
}

fn parse_ms_text(input: &str) -> Option<u32> {
    let value: f32 = input.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(millis_to_micros(value))
}

fn parse_rate_text(input: &str) -> Option<u8> {
    let value: f32 = input.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(value.round().clamp(1.0, 100.0) as u8)
}

/// States the buffer commit policy once: parse, store, and normalize the
/// buffer on success; restore the buffer from the stored value on failure.
fn commit_text<T: Copy>(
    buffer: &mut String,
    value: &mut T,
    parse: fn(&str) -> Option<T>,
    format: fn(T) -> String,
) -> bool {
    match parse(buffer) {
        Some(parsed) => {
            *value = parsed;
            *buffer = format(parsed);
            true
        }
        None => {
            *buffer = format(*value);
            false
        }
    }
}

fn duration(micros: Option<u64>) -> String {
    micros.map_or_else(
        || "—".into(),
        |value| format!("{:.1} ms", value as f64 / 1_000.0),
    )
}

fn timing_range(range: Option<crate::protocol::TimingRange>) -> String {
    range.map_or_else(
        || format!("Collect at least {MIN_RECOMMENDATION_SAMPLES} samples"),
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
        "—".into()
    } else {
        format!("{:.1}", f64::from(count) * 100.0 / f64::from(total))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        protocol::UiView,
        settings::{Settings, TimingSettings},
    };

    use super::{
        TimingInputs, format_ms, format_rate, millis_to_micros, parse_ms_text, parse_rate_text,
        requested_view_from,
    };

    #[test]
    fn slider_values_are_rounded_to_tenth_milliseconds() {
        assert_eq!(millis_to_micros(1.94), 1_900);
        assert_eq!(millis_to_micros(1.95), 2_000);
        assert_eq!(millis_to_micros(1_001.0), 1_000_000);
    }

    #[test]
    fn view_argument_requires_the_named_flag_and_value() {
        assert_eq!(
            requested_view_from(["lastkey-settings", "--view", "measurement"]),
            UiView::Measurement
        );
        assert_eq!(
            requested_view_from(["lastkey-settings", "measurement"]),
            UiView::Settings
        );
        assert_eq!(
            requested_view_from(["lastkey-settings", "--view", "unknown"]),
            UiView::Settings
        );
    }

    #[test]
    fn typed_millisecond_values_round_and_clamp_like_sliders() {
        assert_eq!(parse_ms_text("2.0"), Some(2_000));
        assert_eq!(parse_ms_text("1.95"), Some(2_000));
        assert_eq!(parse_ms_text(""), None);
        assert_eq!(parse_ms_text("abc"), None);
        assert_eq!(parse_ms_text("1.2.3"), None);
        assert_eq!(parse_ms_text("2000"), Some(1_000_000));
        assert_eq!(format_ms(2_000), "2.0");
    }

    #[test]
    fn typed_preservation_rates_clamp_to_whole_percent() {
        assert_eq!(parse_rate_text("50"), Some(50));
        assert_eq!(parse_rate_text("49.6"), Some(50));
        assert_eq!(parse_rate_text("0"), Some(1));
        assert_eq!(parse_rate_text("101"), Some(100));
        assert_eq!(parse_rate_text(""), None);
        assert_eq!(format_rate(50), "50");
    }

    #[test]
    fn apply_stops_before_ipc_when_typed_text_is_invalid() {
        let mut app = super::SettingsApp::new();
        app.draft = Some(Settings::default());
        app.inputs = super::TimingInputs::from_timing(&Settings::default().timing);
        app.inputs.transition_minimum = "abc".into();

        let _ = app.update(super::Message::Apply);

        assert_eq!(app.error.as_deref(), Some(super::INVALID_TIMING_TEXT));
        assert_eq!(app.inputs.transition_minimum, "2.0");
        assert_eq!(
            app.status, "Connecting to the LastKey runtime...",
            "no command may leave while the gate is closed"
        );
    }

    #[test]
    fn successful_submit_preserves_an_unrelated_server_error() {
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        app.error = Some("boom".into());

        let _ = app.update(super::Message::TimingTextSubmitted(
            super::TimingField::TransitionMinimum,
        ));
        assert_eq!(app.error.as_deref(), Some("boom"));

        // A locally produced parse error still clears on the next success.
        app.inputs.transition_minimum = "abc".into();
        let _ = app.update(super::Message::TimingTextSubmitted(
            super::TimingField::TransitionMinimum,
        ));
        assert_eq!(app.error.as_deref(), Some(super::INVALID_TIMING_TEXT));
        let _ = app.update(super::Message::TimingTextSubmitted(
            super::TimingField::TransitionMinimum,
        ));
        assert_eq!(app.error, None);
    }

    #[test]
    fn apply_is_blocked_by_local_validation_before_any_ipc() {
        let mut app = super::SettingsApp::new();
        let mut invalid = Settings::default();
        invalid.timing.socd_transition_min_micros = 4_000;
        invalid.timing.socd_transition_max_micros = 2_000;
        app.inputs = super::TimingInputs::from_timing(&invalid.timing);
        app.draft = Some(invalid);

        let _ = app.update(super::Message::Apply);

        assert_eq!(
            app.error.as_deref(),
            Some("a timing minimum cannot exceed its maximum")
        );
        assert_eq!(
            app.status, "Connecting to the LastKey runtime...",
            "no command may leave while the gate is closed"
        );
    }

    fn baseline_snapshot() -> crate::protocol::UiSnapshot {
        use crate::{
            core::PhysicalKey,
            protocol::{DisplayKey, UiSnapshot},
        };
        let keys = std::array::from_fn(|_| DisplayKey {
            physical: PhysicalKey::new(0x11, false),
            name: "W".into(),
        });
        UiSnapshot {
            saved: Settings::default(),
            draft: Settings::default(),
            keys,
            capture_slot: None,
            measurement_active: false,
            measurement: None,
        }
    }

    #[test]
    fn disabled_slider_drags_leave_the_draft_untouched() {
        // The muted slider still emits drags while its group is off; the
        // update arms must drop them so a gray control stays inert.
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        let _ = app.update(super::Message::TransitionDelayToggled(true));
        let _ = app.update(super::Message::TimingSliderChanged(
            super::TimingField::TransitionMinimum,
            9.0,
        ));
        assert_eq!(
            app.draft
                .as_ref()
                .expect("draft is kept")
                .timing
                .socd_transition_min_micros,
            9_000
        );

        let _ = app.update(super::Message::TransitionDelayToggled(false));
        let _ = app.update(super::Message::TimingSliderChanged(
            super::TimingField::TransitionMinimum,
            3.0,
        ));
        let draft = app.draft.as_ref().expect("draft is kept");
        assert_eq!(draft.timing.socd_transition_min_micros, 9_000);
        assert_eq!(app.inputs.transition_minimum, "9.0");
    }

    #[test]
    fn inflight_snapshot_preserves_newer_local_timing_edit() {
        use crate::protocol::UiEvent;
        let mut app = super::SettingsApp::new();
        let old = baseline_snapshot();
        app.set_snapshot(old.clone());
        // A syncing request went out with the baseline values; before its
        // reply arrives, the user flips a timing control.
        let _ = app.update(super::Message::TransitionDelayToggled(true));
        let _ = app.handle_event(UiEvent::Snapshot(old));
        assert!(
            app.draft
                .as_ref()
                .expect("draft is kept")
                .timing
                .socd_transition_delay_enabled
        );
    }

    #[test]
    fn revert_resets_the_draft_and_buffers_at_click_time() {
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        let _ = app.update(super::Message::TransitionDelayToggled(true));

        let _ = app.update(super::Message::Revert);

        let draft = app.draft.as_ref().expect("draft is kept");
        assert!(!draft.timing.socd_transition_delay_enabled);
        assert_eq!(app.inputs, super::TimingInputs::from_timing(&draft.timing));
    }

    #[test]
    fn older_snapshot_after_revert_leaves_reverted_values_in_place() {
        use crate::protocol::UiEvent;
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        let _ = app.update(super::Message::TransitionDelayToggled(true));
        let _ = app.update(super::Message::Revert);
        // A stale reply to an earlier request arrives after the revert.
        let mut stale = baseline_snapshot();
        stale.draft.timing.socd_transition_delay_enabled = true;
        let _ = app.handle_event(UiEvent::Snapshot(stale));
        assert!(
            !app.draft
                .as_ref()
                .expect("draft is kept")
                .timing
                .socd_transition_delay_enabled
        );
    }

    #[test]
    fn stale_measurement_update_does_not_revive_a_stopped_session() {
        use crate::protocol::{MeasurementSnapshot, UiEvent};
        let mut app = super::SettingsApp::new();
        app.snapshot = Some(baseline_snapshot());
        let update = MeasurementSnapshot {
            observed_event_count: 9,
            ..MeasurementSnapshot::default()
        };
        let _ = app.handle_event(UiEvent::MeasurementUpdated(update));

        let snapshot = app.snapshot.as_ref().expect("snapshot is kept");
        assert!(!snapshot.measurement_active);
        assert!(snapshot.measurement.is_none());

        app.snapshot
            .as_mut()
            .expect("snapshot is kept")
            .measurement_active = true;
        let _ = app.handle_event(UiEvent::MeasurementUpdated(update));
        let snapshot = app.snapshot.as_ref().expect("snapshot is kept");
        assert!(snapshot.measurement_active);
        assert_eq!(snapshot.measurement, Some(update));
    }

    #[test]
    fn uncommitted_text_counts_as_dirty() {
        let timing = TimingSettings::default();
        let mut inputs = super::TimingInputs::from_timing(&timing);

        assert_eq!(inputs, super::TimingInputs::from_timing(&timing));
        inputs.transition_minimum = "9.9".into();
        assert_ne!(inputs, super::TimingInputs::from_timing(&timing));
    }

    #[test]
    fn input_buffers_round_trip_through_draft_precision() {
        let timing = TimingSettings {
            socd_transition_min_micros: 1_900,
            overlap_preservation_rate: 35,
            ..TimingSettings::default()
        };
        let inputs = TimingInputs::from_timing(&timing);

        assert_eq!(inputs.transition_minimum, "1.9");
        assert_eq!(inputs.preservation_rate, "35");
    }

    #[test]
    fn window_size_is_unified_for_both_views() {
        assert_eq!(super::WINDOW_SIZE.width, 780.0);
        assert_eq!(super::WINDOW_SIZE.height, 760.0);
    }

    #[test]
    fn settings_actions_stay_visible_without_a_snapshot() {
        let app = super::SettingsApp::new();
        assert!(app.settings_actions().is_none());

        let mut ready = super::SettingsApp::new();
        ready.set_snapshot(baseline_snapshot());
        assert!(ready.settings_actions().is_some());
    }

    fn test_app() -> super::SettingsApp {
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        app
    }

    #[test]
    fn restore_timing_defaults_resets_only_timing() {
        use crate::settings::TimingSettings;
        let mut app = test_app();
        {
            let draft = app.draft.as_mut().expect("draft is kept");
            draft.timing.socd_transition_delay_enabled = true;
        }
        let _ = app.update(super::Message::TimingSliderChanged(
            super::TimingField::TransitionMinimum,
            9.0,
        ));
        let _ = app.update(super::Message::RestoreTimingDefaults);

        let draft = app.draft.as_ref().expect("draft is kept");
        assert_eq!(draft.timing, TimingSettings::default());
        assert_eq!(
            draft.bindings,
            crate::settings::Settings::default().bindings
        );
        assert_eq!(
            app.inputs,
            super::TimingInputs::from_timing(&TimingSettings::default())
        );
    }

    #[test]
    fn zero_preservation_rate_disables_overlap_on_apply() {
        let mut app = test_app();
        {
            let draft = app.draft.as_mut().expect("draft is kept");
            draft.timing.socd_transition_delay_enabled = true;
            draft.timing.preserve_overlap = true;
        }
        app.inputs.preservation_rate = "0".into();
        let _ = app.update(super::Message::Apply);

        let draft = app.draft.as_ref().expect("draft is kept");
        assert!(!draft.timing.preserve_overlap);
        // The stored share is kept for the next enable; only the toggle flips.
        assert_eq!(app.inputs.preservation_rate, "50");
    }

    #[test]
    fn recommendations_open_settings_with_results_in_the_draft() {
        use crate::protocol::{MeasurementSnapshot, TimingRange, UiView};
        let mut app = test_app();
        app.current_view = UiView::Measurement;
        app.snapshot.as_mut().expect("snapshot is kept").measurement = Some(MeasurementSnapshot {
            recommended_transition: Some(TimingRange {
                min_micros: 2_100,
                max_micros: 3_000,
            }),
            ..MeasurementSnapshot::default()
        });
        let _ = app.update(super::Message::ApplyRecommendations);

        assert_eq!(app.current_view, UiView::Settings);
        let draft = app.draft.as_ref().expect("draft is kept");
        assert_eq!(draft.timing.socd_transition_min_micros, 2_100);
        assert_eq!(draft.timing.socd_transition_max_micros, 3_000);
        assert!(app.notice.is_some());
    }

    #[test]
    fn value_box_ids_are_unique_per_field() {
        use super::TimingField;
        let ids = TimingField::ALL.map(super::value_box_id);
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(format!("{id:?}")));
        }
    }

    #[test]
    fn timing_field_indices_match_all_order() {
        for (index, field) in super::TimingField::ALL.iter().enumerate() {
            assert_eq!(field.index(), index);
        }
    }

    #[test]
    fn timing_pair_flags_minimum_above_maximum() {
        assert!(super::timing_pair_invalid(4_000, 2_000));
        assert!(!super::timing_pair_invalid(2_000, 2_000));
        assert!(!super::timing_pair_invalid(2_000, 4_000));
    }

    #[test]
    fn duplicate_slots_flag_every_sharer() {
        use crate::core::PhysicalKey;
        let distinct = [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1F, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x20, false),
        ];
        assert_eq!(super::duplicate_slots(&distinct), [false; 4]);

        let pair = [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x20, false),
        ];
        assert_eq!(super::duplicate_slots(&pair), [true, true, false, false]);

        let triple = [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x11, false),
        ];
        assert_eq!(super::duplicate_slots(&triple), [true, false, true, true]);
    }

    #[test]
    fn value_box_activation_marks_editing() {
        use super::TimingField;
        let mut app = test_app();
        assert!(!app.editing[TimingField::TransitionMinimum.index()]);

        let _ = app.update(super::Message::ValueBoxActivated(
            TimingField::TransitionMinimum,
        ));

        assert!(app.editing[TimingField::TransitionMinimum.index()]);
        assert!(!app.editing[TimingField::PreservationRate.index()]);
    }

    #[test]
    fn focus_moves_rearm_value_boxes() {
        use super::TimingField;
        let mut app = test_app();
        let _ = app.update(super::Message::ValueBoxActivated(
            TimingField::TransitionMinimum,
        ));
        assert!(app.editing[TimingField::TransitionMinimum.index()]);

        // Pressing another control moves focus away: the next press on any
        // box selects all again.
        let _ = app.update(super::Message::TransitionDelayToggled(true));
        assert!(!app.editing[TimingField::TransitionMinimum.index()]);

        // Losing the window rearms as well.
        let _ = app.update(super::Message::ValueBoxActivated(
            TimingField::TransitionMinimum,
        ));
        let _ = app.update(super::Message::WindowUnfocused);
        assert!(!app.editing[TimingField::TransitionMinimum.index()]);

        // Typing and scrolling leave the armed box alone.
        let _ = app.update(super::Message::ValueBoxActivated(
            TimingField::TransitionMinimum,
        ));
        let _ = app.update(super::Message::TimingTextChanged(
            TimingField::TransitionMinimum,
            "9.9".into(),
        ));
        assert!(app.editing[TimingField::TransitionMinimum.index()]);
    }
}
