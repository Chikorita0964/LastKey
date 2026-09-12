use iced::{
    Center, Color, Element, Fill, Length, Padding, Size, Subscription, Task, Theme,
    widget::{
        Id, button, column, container, opaque, operation, row, rule, scrollable, slider, space,
        stack, text,
        text::{Alignment, Ellipsis, Wrapping},
        text_input,
    },
    window,
};

use crate::{
    core::MIN_RECOMMENDATION_SAMPLES,
    protocol::{KeySlot, MeasurementSnapshot, UiCommand, UiEvent, UiSnapshot, UiView},
    settings::{Settings, SocdMode, TimingSettings},
};

use super::{
    icons::{self, Name as Icon},
    ipc_client::{self, Connection, Event},
    language::Language,
    theme,
    timeline::{self, MonitorState, Timeline},
    widgets,
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
        // `window` replaces the whole window settings while `window_size`
        // only merges, so the size lives here next to the icon: a later
        // `window_size` call would be equally correct, but a single site
        // keeps the one-size invariant obvious.
        .window(iced::window::Settings {
            size: WINDOW_SIZE,
            min_size: Some(Size::new(960.0, 600.0)),
            icon: window_icon(),
            ..iced::window::Settings::default()
        })
        .run()
}

// RGBA pixels for the native window icon, unpacked from the application ICO
// at build time (see build.rs). A missing or invalid asset degrades to no
// icon rather than failing the settings app.
#[cfg(windows)]
include!(concat!(env!("OUT_DIR"), "/lastkey_icon.rs"));

/// Builds the native window icon from the build-time RGBA pixels.
#[cfg(windows)]
fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_rgba(
        WINDOW_ICON_RGBA.to_vec(),
        WINDOW_ICON_WIDTH,
        WINDOW_ICON_HEIGHT,
    )
    .ok()
}

/// The single page starts wide enough for the two settings cards.
/// Section navigation preserves any size chosen by the user.
const WINDOW_SIZE: Size = Size::new(1040.0, 800.0);

/// Stable id shared by settings and measurement in the single page.
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

struct SettingsApp {
    connection: Option<Connection>,
    monitor: MonitorState,
    pending_filter: Option<bool>,
    profiles: ProfileDialog,
    language: Language,
    snapshot: Option<UiSnapshot>,
    draft: Option<Settings>,
    inputs: TimingInputs,
    /// Defer launch/focus navigation until the first snapshot mounts the body.
    pending_section: Option<UiView>,
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
    /// Each mode uses exactly the values it acts on: Random Mix is the only
    /// mode that uses all three groups, and Immediate uses none.
    const fn is_editable(self, timing: &TimingSettings) -> bool {
        match self {
            Self::TransitionMinimum | Self::TransitionMaximum => {
                matches!(timing.mode, SocdMode::PressDelay | SocdMode::RandomMix)
            }
            Self::PreservationRate => matches!(timing.mode, SocdMode::RandomMix),
            Self::PreservedMinimum | Self::PreservedMaximum => {
                matches!(timing.mode, SocdMode::ReleaseDelay | SocdMode::RandomMix)
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

#[derive(Default)]
enum ProfileDialog {
    #[default]
    Closed,
    List,
    Languages,
    Confirm(u8),
    Rename {
        slot: u8,
        name: String,
    },
    Loading,
    Renaming,
}

#[derive(Clone, Debug)]
enum Message {
    Ipc(Event),
    RequestSnapshot,
    ToggleFilter,
    OpenProfiles,
    OpenLanguages,
    SelectLanguage(Language),
    CloseProfiles,
    LoadProfile(u8),
    ConfirmProfile(u8),
    EditProfileName(u8),
    ProfileNameChanged(String),
    SaveProfileName,
    ToggleMonitor,
    CancelCapture,
    ResetMeasurement,
    Capture(KeySlot),
    ModeSelected(SocdMode),
    MixChanged(f32),
    TimingSliderChanged(TimingField, f32),
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
        Self {
            connection: None,
            monitor: MonitorState::Stopped,
            pending_filter: None,
            profiles: ProfileDialog::Closed,
            language: Language::default(),
            snapshot: None,
            draft: None,
            inputs: TimingInputs::default(),
            pending_section: Some(requested_view()),
            editing: [false; 5],
            status: "Connecting to the LastKey runtime...".into(),
            notice: None,
            error: None,
        }
    }

    fn title(&self) -> String {
        "LastKey Settings".into()
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
        if !matches!(self.profiles, ProfileDialog::Closed)
            && !matches!(
                &message,
                Message::Ipc(_)
                    | Message::OpenLanguages
                    | Message::SelectLanguage(_)
                    | Message::OpenProfiles
                    | Message::CloseProfiles
                    | Message::LoadProfile(_)
                    | Message::ConfirmProfile(_)
                    | Message::EditProfileName(_)
                    | Message::ProfileNameChanged(_)
                    | Message::SaveProfileName
                    | Message::WindowUnfocused
            )
        {
            return Task::none();
        }
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
                self.monitor = MonitorState::Stopped;
                self.pending_filter = None;
                self.profiles = ProfileDialog::Closed;
                self.status = "The LastKey runtime is disconnected.".into();
                self.error = Some(error);
            }
            Message::OpenLanguages => {
                self.profiles = ProfileDialog::Languages;
            }
            Message::SelectLanguage(language) => {
                self.language = language;
                self.profiles = ProfileDialog::Closed;
            }
            Message::OpenProfiles => {
                self.profiles = ProfileDialog::List;
            }
            Message::CloseProfiles => {
                if !matches!(
                    self.profiles,
                    ProfileDialog::Loading | ProfileDialog::Renaming
                ) {
                    self.profiles = ProfileDialog::Closed;
                }
            }
            Message::LoadProfile(slot) => {
                if self.is_dirty() {
                    self.profiles = ProfileDialog::Confirm(slot);
                } else {
                    self.load_profile(slot);
                }
            }
            Message::ConfirmProfile(slot) => self.load_profile(slot),
            Message::EditProfileName(slot) => {
                if let Some(snapshot) = &self.snapshot {
                    let bank = snapshot.saved.profile_bank();
                    if let Some(profile) = bank.slots.get(usize::from(slot)) {
                        self.profiles = ProfileDialog::Rename {
                            slot,
                            name: profile.name.clone(),
                        };
                    }
                }
            }
            Message::ProfileNameChanged(value) => {
                if let ProfileDialog::Rename { name, .. } = &mut self.profiles {
                    *name = value;
                }
            }
            Message::SaveProfileName => {
                if let ProfileDialog::Rename { slot, name } = &self.profiles {
                    let command = UiCommand::RenameProfile {
                        slot: *slot,
                        name: name.clone(),
                    };
                    self.profiles = ProfileDialog::Renaming;
                    self.send(command);
                }
            }
            Message::ToggleFilter => {
                if let Some(snapshot) = &self.snapshot
                    && self.pending_filter.is_none()
                    && self.connection.is_some()
                {
                    let enabled = !snapshot.filter_enabled;
                    self.pending_filter = Some(enabled);
                    self.send(UiCommand::SetFilterEnabled(enabled));
                }
            }
            Message::ToggleMonitor => {
                if self.connection.is_none() {
                    return Task::none();
                }
                self.monitor = match std::mem::take(&mut self.monitor) {
                    MonitorState::Stopped => {
                        self.send(UiCommand::StartMonitor);
                        MonitorState::Starting
                    }
                    MonitorState::Recording(timeline) => {
                        self.send(UiCommand::StopMonitor);
                        MonitorState::Stopping(timeline)
                    }
                    pending => pending,
                };
            }
            Message::CancelCapture => {
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.capture_slot.is_some())
                {
                    self.send(UiCommand::CancelKeyCapture);
                }
            }
            Message::ResetMeasurement => {
                self.send(UiCommand::ResetMeasurement);
            }
            Message::RequestSnapshot => {
                self.send(UiCommand::RequestSnapshot);
            }
            Message::Capture(slot) => {
                // Timing stays local until Apply; the answering Snapshot
                // merges instead of replacing it (see set_snapshot).
                self.send(UiCommand::BeginKeyCapture(slot));
            }
            Message::MixChanged(press_share) => {
                if let Some(draft) = self.draft.as_mut()
                    && draft.timing.mode == SocdMode::RandomMix
                {
                    draft.timing.overlap_preservation_rate =
                        100 - press_share.round().clamp(1.0, 99.0) as u8;
                    self.inputs.preservation_rate =
                        format_rate(draft.timing.overlap_preservation_rate);
                }
            }
            Message::ModeSelected(mode) => {
                if let Some(draft) = self.draft.as_mut() {
                    draft.timing.mode = mode;
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
                let defaults = Settings {
                    profiles: self.draft.as_ref().and_then(|draft| draft.profiles.clone()),
                    ..Settings::default()
                };
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
            // The rate is a percentage, not a duration, so it keeps its own
            // parse and format. "Off" is a mode, not a rate of zero.
            return commit_text(
                &mut self.inputs.preservation_rate,
                &mut draft.timing.overlap_preservation_rate,
                parse_rate_text,
                format_rate,
            );
        }
        // All duration fields share one path.
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
        self.show_section(UiView::Settings, false)
    }

    fn handle_event(&mut self, event: UiEvent) -> Task<Message> {
        match event {
            UiEvent::ProfileLoaded(snapshot) => {
                self.draft = None;
                self.set_snapshot(snapshot);
                self.profiles = ProfileDialog::Closed;
                self.notice = Some("Profile loaded and activated.".into());
                self.error = None;
            }
            UiEvent::FilterChanged(enabled) => {
                if let Some(snapshot) = self.snapshot.as_mut() {
                    snapshot.filter_enabled = enabled;
                }
                self.pending_filter = None;
                self.monitor.resynchronize();
            }
            UiEvent::MonitorStateChanged(active) => {
                self.monitor = if active {
                    MonitorState::Recording(Timeline::default())
                } else {
                    MonitorState::Stopped
                };
            }
            UiEvent::MonitorUpdated(event) => {
                if let MonitorState::Recording(timeline) = &mut self.monitor
                    && let Some(snapshot) = &self.snapshot
                    && event.filter_enabled == snapshot.filter_enabled
                {
                    timeline.accept(
                        event,
                        snapshot.measurement_active,
                        std::time::Instant::now(),
                    );
                }
            }
            UiEvent::Snapshot(snapshot) => {
                self.set_snapshot(snapshot);
                if matches!(self.profiles, ProfileDialog::Renaming) {
                    self.profiles = ProfileDialog::List;
                    self.notice = Some("Profile renamed.".into());
                }
                self.status = "Settings are synchronized with the runtime.".into();
                self.error = None;
                if let Some(section) = self.pending_section.take() {
                    return self.show_section(section, false);
                }
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
                match error.code.as_str() {
                    "filter-failed" => {
                        self.pending_filter = None;
                        self.send(UiCommand::RequestSnapshot);
                    }
                    "filter-state-failed" => {
                        self.pending_filter = None;
                    }
                    "profile-load-failed" | "profile-rename-failed" => {
                        self.profiles = ProfileDialog::List;
                    }
                    "monitor-start-failed" => {
                        self.monitor = MonitorState::Stopped;
                        self.send(UiCommand::StopMonitor);
                    }
                    "monitor-stop-failed" => {
                        if let MonitorState::Stopping(mut timeline) =
                            std::mem::take(&mut self.monitor)
                        {
                            timeline.clear();
                            self.monitor = MonitorState::Recording(timeline);
                        }
                    }
                    _ => {}
                }
                self.error = Some(error.message);
            }
            UiEvent::FocusRequested(view) => {
                return self.show_section(view, true);
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
        self.monitor.resynchronize();
        self.pending_filter = None;
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

    fn show_section(&mut self, view: UiView, focus: bool) -> Task<Message> {
        let scroll = if self.snapshot.is_some() {
            match view {
                UiView::Settings => {
                    operation::snap_to(SETTINGS_BODY_ID, operation::RelativeOffset::START)
                }
                UiView::Measurement => operation::snap_to_end(SETTINGS_BODY_ID),
            }
        } else {
            self.pending_section = Some(view);
            Task::none()
        };
        if !focus {
            return scroll;
        }
        scroll.chain(window::latest().and_then(move |id| {
            Task::batch([
                window::set_mode(id, window::Mode::Windowed),
                window::gain_focus(id),
            ])
        }))
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
        let connected = self.connection.is_some();
        let state_color = if connected {
            theme::OK_TEXT
        } else {
            theme::MUTED_TEXT
        };
        let header = container(
            row![
                widgets::logo(WINDOW_ICON_RGBA, WINDOW_ICON_WIDTH),
                text(self.language.text("LastKey"))
                    .size(theme::HEADING_SIZE)
                    .font(theme::UI_FONT_BOLD),
                row![
                    dot(state_color),
                    text(self.language.text(&self.status))
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
                button(
                    row![
                        icons::icon(Icon::Layers, 14.0, None),
                        text(self.active_profile_name())
                            .size(12)
                            .wrapping(Wrapping::None)
                            .ellipsis(Ellipsis::End)
                    ]
                    .spacing(6)
                    .align_y(Center)
                )
                .width(240)
                .style(theme::secondary_button)
                .padding([10, 14])
                .on_press_maybe(
                    (connected && self.snapshot.is_some()).then_some(Message::OpenProfiles)
                ),
                button(icons::icon(Icon::Languages, 16.0, None))
                    .padding(10)
                    .style(theme::secondary_button)
                    .on_press_maybe(self.snapshot.is_some().then_some(Message::OpenLanguages)),
                button(
                    row![
                        icons::icon(Icon::Power, 14.0, None),
                        text(
                            self.language.text(if self.pending_filter.is_some() {
                                "Updating…"
                            } else if self
                                .snapshot
                                .as_ref()
                                .is_some_and(|snapshot| snapshot.filter_enabled)
                            {
                                "ON"
                            } else {
                                "OFF"
                            })
                        )
                        .font(theme::UI_FONT_BOLD)
                    ]
                    .spacing(6)
                    .align_y(Center)
                )
                .padding([8, 14])
                .style(theme::secondary_button)
                .on_press_maybe(
                    (self.connection.is_some()
                        && self.snapshot.is_some()
                        && self.pending_filter.is_none())
                    .then_some(Message::ToggleFilter)
                ),
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

        let body = self.settings_view();
        let actions = self.settings_actions();
        // The connection state lives in the header status line, so no footer
        // strip is needed.
        let mut page = column![header, body]
            .spacing(theme::SECTION_GAP)
            .padding(theme::PAGE_PADDING)
            .height(Fill);
        if let Some(actions) = actions {
            page = page.push(actions);
        }
        let page: Element<'_, Message> = container(page)
            .height(Fill)
            .width(Fill)
            .style(|_theme| theme::canvas_style())
            .into();
        // Keep the page at the same stack index, preserving scroll and editor state.
        let overlay = self.profile_dialog();
        stack![page, overlay].into()
    }

    fn is_dirty(&self) -> bool {
        self.snapshot
            .as_ref()
            .zip(self.draft.as_ref())
            .is_some_and(|(snapshot, draft)| {
                draft != &snapshot.saved || self.inputs != TimingInputs::from_timing(&draft.timing)
            })
    }

    fn load_profile(&mut self, slot: u8) {
        self.error = None;
        self.profiles = ProfileDialog::Loading;
        self.send(UiCommand::LoadProfile(slot));
    }

    fn active_profile_name(&self) -> String {
        self.snapshot.as_ref().map_or_else(
            || self.language.text("Profiles").into(),
            |snapshot| {
                let bank = snapshot.saved.profile_bank();
                format!(
                    "{} {}  ·  {}",
                    self.language.text("Profile"),
                    bank.active + 1,
                    bank.slots[usize::from(bank.active)].name
                )
            },
        )
    }

    fn profile_dialog(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return space::horizontal().width(0).into();
        };
        if matches!(self.profiles, ProfileDialog::Closed) {
            return space::horizontal().width(0).into();
        }
        let bank = snapshot.saved.profile_bank();
        let mut body = column![
            text(
                self.language
                    .text(if matches!(self.profiles, ProfileDialog::Languages) {
                        "Language"
                    } else {
                        "Profiles"
                    })
            )
            .size(22)
            .font(theme::UI_FONT_BOLD)
        ]
        .spacing(16);
        match &self.profiles {
            ProfileDialog::Languages => {
                for language in Language::ALL {
                    body = body.push(
                        button(icon_label(
                            if self.language == language {
                                Icon::Check
                            } else {
                                Icon::Languages
                            },
                            language.name(),
                            self.language,
                        ))
                        .style(theme::secondary_button)
                        .on_press(Message::SelectLanguage(language)),
                    );
                }
            }
            ProfileDialog::List => {
                body = body.push(text(self.language.text("Load a slot to activate it immediately. Apply saves edits to the active slot.")).size(12).color(theme::MUTED_TEXT));
                for (index, profile) in bank.slots.iter().enumerate() {
                    let slot = index as u8;
                    body = body.push(
                        container(
                            row![
                                column![
                                    text(format!(
                                        "{}  ·  {}{}",
                                        index + 1,
                                        profile.name,
                                        if bank.active == slot {
                                            "  — Active"
                                        } else {
                                            ""
                                        }
                                    ))
                                    .font(theme::UI_FONT_BOLD),
                                    text(mode_label(profile.timing.mode, self.language))
                                        .size(12)
                                        .color(theme::MUTED_TEXT),
                                ]
                                .spacing(4)
                                .width(Fill),
                                button(icon_label(Icon::Edit, "Rename", self.language))
                                    .style(theme::secondary_button)
                                    .on_press(Message::EditProfileName(slot)),
                                button(icon_label(Icon::Layers, "Load", self.language))
                                    .style(theme::primary_button)
                                    .on_press(Message::LoadProfile(slot)),
                            ]
                            .spacing(10)
                            .align_y(Center),
                        )
                        .padding(14)
                        .style(|_| theme::group_style()),
                    );
                }
            }
            ProfileDialog::Confirm(slot) => {
                body = body
                    .push(text(format!(
                        "Load profile {} and discard unapplied changes?",
                        slot + 1
                    )))
                    .push(
                        text(
                            self.language
                                .text("The saved slot will become active immediately."),
                        )
                        .size(12)
                        .color(theme::MUTED_TEXT),
                    )
                    .push(
                        button(icon_label(
                            Icon::Layers,
                            "Discard edits and load",
                            self.language,
                        ))
                        .style(theme::primary_button)
                        .on_press(Message::ConfirmProfile(*slot)),
                    );
            }
            ProfileDialog::Rename { name, .. } => {
                body = body
                    .push(text(self.language.text("Profile name · 1–64 characters")).size(12))
                    .push(
                        text_input("Profile name", name)
                            .on_input(Message::ProfileNameChanged)
                            .on_submit(Message::SaveProfileName),
                    )
                    .push(
                        button(icon_label(Icon::Check, "Save name", self.language))
                            .style(theme::primary_button)
                            .on_press(Message::SaveProfileName),
                    );
            }
            ProfileDialog::Loading => {
                body = body.push(text(self.language.text("Loading and activating profile…")));
            }
            ProfileDialog::Renaming => {
                body = body.push(text(self.language.text("Saving profile name…")));
            }
            ProfileDialog::Closed => {}
        }
        if let Some(error) = &self.error {
            body = body.push(
                text(self.language.text(error))
                    .size(12)
                    .color(theme::ERROR_TEXT),
            );
        }
        if !matches!(
            self.profiles,
            ProfileDialog::Loading | ProfileDialog::Renaming
        ) {
            body = body.push(
                button(icon_label(Icon::Close, "Close", self.language))
                    .style(theme::secondary_button)
                    .on_press(Message::CloseProfiles),
            );
        }
        opaque(
            container(
                container(body)
                    .width(600)
                    .padding(24)
                    .style(|_| theme::card_style()),
            )
            .center_x(Fill)
            .center_y(Fill)
            .padding(20)
            .style(|_| container::Style {
                background: Some(
                    Color {
                        a: 0.35,
                        ..Color::BLACK
                    }
                    .into(),
                ),
                ..Default::default()
            }),
        )
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let (Some(snapshot), Some(draft)) = (&self.snapshot, &self.draft) else {
            return disconnected_view(self.error.as_ref(), self.language);
        };
        let timing = &draft.timing;

        let mappings = container(
            column![
                row![
                    section_title(Icon::Keyboard, "Key mappings", self.language).width(Fill),
                    button(icon_label(Icon::Restore, "Restore defaults", self.language))
                        .style(theme::secondary_button)
                        .on_press(Message::RestoreMappingDefaults),
                ]
                .align_y(Center),
                text(
                    self.language
                        .text("Hardware scan codes the SOCD filter uses.")
                )
                .size(12)
                .color(theme::MUTED_TEXT),
                mapping_pad(snapshot, self.monitor.timeline(), self.language),
                icon_label(
                    Icon::Edit,
                    "Click a keycap to rebind; click again to cancel.",
                    self.language
                ),
                text(
                    self.language
                        .text("Modifiers like Shift, Ctrl, and Alt are not captured.")
                )
                .size(12)
                .color(theme::MUTED_TEXT),
            ]
            .spacing(theme::SECTION_GAP),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_| theme::card_style());

        let timing_card = container(
            column![
                row![
                    section_title(Icon::Timer, "Input timings", self.language).width(Fill),
                    button(icon_label(Icon::Restore, "Restore defaults", self.language))
                        .style(theme::secondary_button)
                        .on_press(Message::RestoreTimingDefaults),
                ]
                .align_y(Center),
                text(
                    self.language
                        .text("How opposite-direction overlaps resolve.")
                )
                .size(12)
                .color(theme::MUTED_TEXT),
                container(
                    column![
                        mode_selector(timing.mode, self.language),
                        rate_group(timing, &self.inputs, &self.editing, self.language),
                        duration_range(
                            TimingField::TransitionMinimum,
                            TimingField::TransitionMaximum,
                            self.language.text("New Key Press Delay"),
                            timing,
                            &self.inputs,
                            &self.editing,
                            theme::PRIMARY_TEXT
                        ),
                        duration_range(
                            TimingField::PreservedMinimum,
                            TimingField::PreservedMaximum,
                            self.language.text("Previous Key Release Delay"),
                            timing,
                            &self.inputs,
                            &self.editing,
                            theme::RELEASE_TEXT
                        ),
                        container(
                            column![
                                text(self.language.text("How it works"))
                                    .size(12)
                                    .font(theme::UI_FONT_BOLD),
                                text(mode_description(timing.mode, self.language))
                                    .size(12)
                                    .color(theme::MUTED_TEXT),
                            ]
                            .spacing(8)
                        )
                        .padding(12)
                        .width(Fill)
                        .style(|_| theme::slot_style()),
                    ]
                    .spacing(12)
                )
                .padding(theme::GROUP_PADDING)
                .width(Fill)
                .style(|_| theme::group_style()),
            ]
            .spacing(theme::SECTION_GAP),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_| theme::card_style());

        // One scroll owner keeps measurement below the settings cards.
        // The action bar stays outside it, including when results grow.
        scrollable(
            column![
                row![mappings, timing_card].spacing(theme::SECTION_GAP),
                self.timeline_section(),
                self.measurement_section(),
            ]
            .spacing(theme::SECTION_GAP)
            .padding(Padding {
                right: 12.0,
                ..Padding::ZERO
            }),
        )
        .id(SETTINGS_BODY_ID)
        .height(Fill)
        .into()
    }

    fn settings_actions(&self) -> Option<Element<'_, Message>> {
        let (Some(snapshot), Some(draft)) = (&self.snapshot, &self.draft) else {
            return None;
        };
        let _ = (snapshot, draft);
        let dirty = self.is_dirty();
        let revert = if dirty {
            button(icon_label(Icon::Revert, "Revert", self.language))
                .style(theme::secondary_button)
                .on_press(Message::Revert)
        } else {
            button(icon_label(Icon::Revert, "Revert", self.language)).style(theme::secondary_button)
        };
        let apply = if dirty {
            button(icon_label(Icon::ArrowRight, "Apply", self.language))
                .style(theme::primary_button)
                .on_press(Message::Apply)
        } else {
            button(icon_label(Icon::ArrowRight, "Apply", self.language))
                .style(theme::primary_button)
        };
        // Error and notice feedback lives in this bar as plain text rather
        // than as toggling banners above the scrollable: the page is diffed
        // positionally, so a banner appearing or disappearing above the body
        // hands the scrollable state slot to another widget and resets the
        // scroll offset. Swapping only this text never moves any widget.
        let feedback = self.feedback_element();
        let actions = container(
            row![
                button(icon_label(
                    Icon::Restore,
                    "Restore all defaults",
                    self.language
                ))
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

    /// Error and notice feedback shared by the whole page. Rendered as plain text
    /// with a blank placeholder when empty, so its presence never moves any
    /// widget (see `settings_actions`).
    fn feedback_element(&self) -> Element<'_, Message> {
        match (&self.error, &self.notice) {
            (Some(error), _) => iced::widget::tooltip(
                row![
                    icons::icon(Icon::Warning, 14.0, Some(theme::ERROR_TEXT)),
                    text(self.language.text(error))
                        .size(theme::BODY_TEXT_SIZE)
                        .font(theme::UI_FONT_BOLD)
                        .color(theme::ERROR_TEXT)
                        .width(Fill)
                        .align_x(Alignment::Right)
                        .wrapping(Wrapping::None)
                        .ellipsis(Ellipsis::End)
                ]
                .spacing(6)
                .align_y(Center)
                .width(Fill),
                text(self.language.text(error)),
                iced::widget::tooltip::Position::Top,
            )
            .into(),
            (None, Some(notice)) => text(self.language.text(notice))
                .size(theme::BODY_TEXT_SIZE)
                .font(theme::UI_FONT_BOLD)
                .color(theme::OK_TEXT)
                .width(Fill)
                .align_x(Alignment::Right)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::End)
                .into(),
            (None, None) => container(icon_label(
                if self.is_dirty() {
                    Icon::Edit
                } else {
                    Icon::Check
                },
                if self.is_dirty() {
                    "Unsaved draft changes"
                } else {
                    "Synchronized"
                },
                self.language,
            ))
            .width(Fill)
            .align_right(Fill)
            .into(),
        }
    }

    fn timeline_section(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return space::horizontal().into();
        };
        let timeline = self.monitor.timeline();
        let label = match &self.monitor {
            MonitorState::Stopped => "Start timeline",
            MonitorState::Starting => "Starting…",
            MonitorState::Recording(_) => "Stop timeline",
            MonitorState::Stopping(_) => "Stopping…",
        };
        let ready = matches!(
            self.monitor,
            MonitorState::Stopped | MonitorState::Recording(_)
        );
        let source = if snapshot.measurement_active || !snapshot.filter_enabled {
            "Physical input"
        } else {
            "Filter output"
        };
        let decision = timeline.map_or_else(
            || self.language.text("No input yet").into(),
            |timeline| match timeline.decision {
                crate::protocol::MonitorDecision::Immediate => {
                    self.language.text("Immediate").into()
                }
                crate::protocol::MonitorDecision::PressDelayed { delay_micros } => {
                    format!(
                        "{} · {} ms",
                        self.language.text("Press delay"),
                        format_ms(delay_micros)
                    )
                }
                crate::protocol::MonitorDecision::ReleaseDelayed { delay_micros } => {
                    format!(
                        "{} · {} ms",
                        self.language.text("Release delay"),
                        format_ms(delay_micros)
                    )
                }
            },
        );
        let labels = column(snapshot.keys.iter().map(|key| {
            container(text(&key.name).size(12).font(theme::UI_FONT_BOLD))
                .height(36)
                .center_y(30)
                .into()
        }))
        .width(60);
        container(
            column![
                row![
                    column![
                        section_title(Icon::Target, "Key Input Timeline", self.language),
                        text(self.language.text(
                            "Last 1 second · mapped keys only · memory cleared when stopped"
                        ))
                        .size(12)
                        .color(theme::MUTED_TEXT)
                    ]
                    .spacing(4)
                    .width(Fill),
                    button(icon_label(
                        if matches!(self.monitor, MonitorState::Recording(_)) {
                            Icon::Stop
                        } else {
                            Icon::Play
                        },
                        label,
                        self.language
                    ))
                    .style(theme::secondary_button)
                    .on_press_maybe(ready.then_some(Message::ToggleMonitor))
                ]
                .align_y(Center),
                row![
                    text(self.language.text(source))
                        .size(12)
                        .font(theme::UI_FONT_BOLD)
                        .width(Fill),
                    text(decision).size(12).color(theme::PRIMARY_TEXT)
                ],
                row![labels, timeline::graph(timeline)].spacing(12),
                row![
                    text(self.language.text("−1000 ms")).size(11),
                    space::horizontal(),
                    text(self.language.text("−500 ms")).size(11),
                    space::horizontal(),
                    text(self.language.text("Now")).size(11)
                ],
            ]
            .spacing(12),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_| theme::card_style())
        .into()
    }

    fn measurement_section(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return disconnected_view(self.error.as_ref(), self.language);
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
                        section_title(Icon::Chart, "Input timing measurement", self.language),
                        text(
                            self.language
                                .text("Records your mapped key-pair timing for this session.")
                        )
                        .size(12)
                        .color(theme::MUTED_TEXT),
                    ]
                    .width(Fill)
                    .spacing(4),
                    button(icon_label(Icon::Restore, "Reset session", self.language))
                        .style(theme::secondary_button)
                        .on_press(Message::ResetMeasurement),
                    button(icon_label(
                        if snapshot.measurement_active {
                            Icon::Stop
                        } else {
                            Icon::Play
                        },
                        button_label,
                        self.language
                    ))
                    .style(theme::primary_button)
                    .on_press(Message::ToggleMeasurement),
                ]
                .align_y(Center),
                {
                    let stats: Element<_> = match snapshot.measurement {
                        Some(measurement) => row![
                            stat_box(
                                self.language.text("Physical key edges"),
                                measurement.observed_event_count.to_string(),
                                None,
                                None,
                            ),
                            stat_box(
                                self.language.text("Valid paired samples"),
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
                                self.language.text("Indistinguishable share"),
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
                        None => text(self.language.text("No measurement results yet.")).into(),
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
            content = content.push(latencies_card(measurement, self.language));
            content = content.push(recommendations_card(measurement, self.language));
        }
        content.into()
    }
}

impl Drop for SettingsApp {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            let _ = connection.send(UiCommand::CloseUiSession);
        }
    }
}

fn icon_label(
    name: Icon,
    label: impl Into<String>,
    language: Language,
) -> Element<'static, Message> {
    row![
        icons::icon(name, 14.0, None),
        text(language.text(&label.into()).to_owned()).size(12)
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}

fn section_title(
    name: Icon,
    label: impl Into<String>,
    language: Language,
) -> iced::widget::Row<'static, Message> {
    row![
        icons::icon(name, 16.0, Some(theme::PRIMARY_TEXT)),
        text(language.text(&label.into()).to_owned())
            .size(theme::HEADING_SIZE)
            .font(theme::UI_FONT_BOLD)
    ]
    .spacing(8)
    .align_y(Center)
}

/// Small status or legend mark.
fn dot(color: Color) -> Element<'static, Message> {
    container(space::horizontal())
        .width(Length::Fixed(8.0))
        .height(Length::Fixed(8.0))
        .style(move |_theme| theme::dot_style(color))
        .into()
}

fn mapping_pad<'a>(
    snapshot: &'a UiSnapshot,
    timeline: Option<&Timeline>,
    language: Language,
) -> Element<'a, Message> {
    let duplicates = duplicate_slots(&snapshot.draft.bindings);
    let up = keycap(
        "UP",
        Icon::ArrowUp,
        KeySlot::VerticalFirst,
        snapshot,
        duplicates[0],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::VerticalFirst)),
        language,
    );
    let down = keycap(
        "DOWN",
        Icon::ArrowDown,
        KeySlot::VerticalSecond,
        snapshot,
        duplicates[1],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::VerticalSecond)),
        language,
    );
    let left = keycap(
        "LEFT",
        Icon::ArrowLeft,
        KeySlot::HorizontalFirst,
        snapshot,
        duplicates[2],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::HorizontalFirst)),
        language,
    );
    let right = keycap(
        "RIGHT",
        Icon::ArrowRight,
        KeySlot::HorizontalSecond,
        snapshot,
        duplicates[3],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::HorizontalSecond)),
        language,
    );
    let hint = if snapshot.capture_slot.is_some() {
        "Press a key to assign it"
    } else {
        "Click a keycap to rebind"
    };
    container(
        column![
            text(language.text(hint)).size(12).color(theme::MUTED_TEXT),
            container(up).center_x(Fill),
            row![
                left,
                container(text(language.text("+")).size(40).color(theme::MUTED_TEXT))
                    .width(80)
                    .height(80)
                    .center_x(80)
                    .center_y(80),
                right
            ]
            .spacing(12)
            .align_y(Center),
            container(down).center_x(Fill),
            row![
                icons::icon(
                    if duplicates.contains(&true) {
                        Icon::Warning
                    } else {
                        Icon::Check
                    },
                    14.0,
                    Some(if duplicates.contains(&true) {
                        theme::ERROR_TEXT
                    } else {
                        theme::OK_TEXT
                    })
                ),
                text(language.text(if duplicates.contains(&true) {
                    "Duplicate key bindings detected."
                } else {
                    "All keys uniquely assigned."
                }))
                .size(12)
                .color(if duplicates.contains(&true) {
                    theme::ERROR_TEXT
                } else {
                    theme::OK_TEXT
                })
            ]
            .spacing(6)
            .align_y(Center),
        ]
        .spacing(12)
        .align_x(Center),
    )
    .padding(theme::GROUP_PADDING)
    .width(Fill)
    .style(|_| theme::group_style())
    .into()
}

fn keycap<'a>(
    label: &'static str,
    arrow: Icon,
    slot: KeySlot,
    snapshot: &'a UiSnapshot,
    duplicate: bool,
    pressed: bool,
    language: Language,
) -> Element<'a, Message> {
    let key = &snapshot.keys[key_slot_index(slot)];
    let selected = snapshot.capture_slot == Some(slot);
    let accent = if matches!(slot, KeySlot::VerticalFirst | KeySlot::VerticalSecond) {
        Color::from_rgb8(37, 99, 235)
    } else {
        theme::PRIMARY_TEXT
    };
    let name = if selected { "…" } else { &key.name };
    iced::widget::tooltip(
        button(
            column![
                row![
                    text(language.text(label)).size(10).width(Fill),
                    icons::icon(arrow, 12.0, None)
                ],
                container(
                    text(name)
                        .size(if name.chars().count() > 4 { 13 } else { 24 })
                        .font(theme::UI_FONT_BOLD)
                        .wrapping(Wrapping::None)
                        .ellipsis(Ellipsis::End)
                )
                .center_x(Fill)
                .center_y(Fill),
            ]
            .spacing(4),
        )
        .width(80)
        .height(80)
        .padding(10)
        .style(move |_, state| theme::keycap(state, selected || pressed, duplicate, accent))
        .on_press(if selected {
            Message::CancelCapture
        } else {
            Message::Capture(slot)
        }),
        text(&key.name),
        iced::widget::tooltip::Position::Top,
    )
    .into()
}

/// A shared pair of numeric editors and a two-handle native range rail.
fn duration_range<'a>(
    minimum: TimingField,
    maximum: TimingField,
    label: &'static str,
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
    accent: Color,
) -> Element<'a, Message> {
    let enabled = minimum.is_editable(timing);
    let min = minimum.micros(timing).expect("duration field") as f32 / 1000.0;
    let max = maximum.micros(timing).expect("duration field") as f32 / 1000.0;
    let invalid = minimum.pair_invalid(timing);
    let editor = |field: TimingField| {
        value_box(
            field,
            inputs.buffer(field),
            enabled,
            editing[field.index()],
            enabled && (invalid || parse_ms_text(inputs.buffer(field)).is_none()),
            56.0,
        )
    };
    container(
        column![
            text(label)
                .size(12)
                .font(theme::UI_FONT_BOLD)
                .color(if enabled { accent } else { theme::MUTED_TEXT }),
            row![
                editor(minimum),
                text("–").color(theme::MUTED_TEXT),
                editor(maximum),
                text("ms").size(12).color(theme::MUTED_TEXT)
            ]
            .spacing(6)
            .align_y(Center),
            widgets::range_slider(
                min,
                max,
                if minimum == TimingField::PreservedMinimum {
                    0.1
                } else {
                    0.0
                },
                enabled,
                accent,
                move |is_min, value| {
                    Message::TimingSliderChanged(if is_min { minimum } else { maximum }, value)
                }
            ),
        ]
        .spacing(6),
    )
    .padding(12)
    .width(Fill)
    .style(|_| theme::slot_style())
    .into()
}

/// Mode picker for the timing card. The four modes are mutually exclusive and
/// each is selectable directly, so no ordering between switches can leave the
/// card in a state the engine treats as a fifth behavior.
fn mode_selector(selected: SocdMode, language: Language) -> Element<'static, Message> {
    let mut segments = row![].spacing(4);
    for mode in SocdMode::ALL {
        let segment = button(
            text(mode_label(mode, language))
                .size(11)
                .width(Fill)
                .align_x(Alignment::Center),
        )
        .width(Fill)
        .padding(theme::ROW_GAP)
        .on_press(Message::ModeSelected(mode));
        segments = segments.push(segment.style(move |_, status| {
            theme::mode_button(status, mode == selected, mode_color(mode))
        }));
    }
    let current = SocdMode::ALL
        .iter()
        .position(|mode| *mode == selected)
        .expect("all modes are listed");
    container(
        column![
            segments,
            row![
                button(icons::icon(Icon::ChevronLeft, 14.0, None))
                    .style(theme::secondary_button)
                    .on_press(Message::ModeSelected(SocdMode::ALL[(current + 3) % 4])),
                space::horizontal(),
                text(mode_label(selected, language))
                    .size(12)
                    .color(mode_color(selected)),
                space::horizontal(),
                button(icons::icon(Icon::ChevronRight, 14.0, None))
                    .style(theme::secondary_button)
                    .on_press(Message::ModeSelected(SocdMode::ALL[(current + 1) % 4])),
            ]
            .spacing(4)
            .align_y(Center),
        ]
        .spacing(8),
    )
    .padding(4)
    .width(Fill)
    .style(|_theme| theme::group_style())
    .into()
}

/// Split between the two delays for Random Mix. The stored rate is the
/// release-delay share, so the press-delay share is shown as its mirror
/// rather than stored twice.
fn rate_group<'a>(
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
    language: Language,
) -> Element<'a, Message> {
    let enabled = TimingField::PreservationRate.is_editable(timing);
    let press_share = 100u8.saturating_sub(timing.overlap_preservation_rate);
    let title = text(language.text("Delay Mix Ratio"))
        .size(12)
        .font(theme::UI_FONT_BOLD);
    let title = if enabled {
        title
    } else {
        title.color(theme::MUTED_TEXT)
    };
    container(
        column![
            title,
            row![
                text(format!("{} {press_share} %", language.text("Press delay")))
                    .size(12)
                    .color(theme::MUTED_TEXT),
                space::horizontal(),
                text(language.text("Release delay")).size(12),
                rate_box(
                    &inputs.preservation_rate,
                    enabled,
                    editing[TimingField::PreservationRate.index()],
                ),
                text(language.text("%")).size(12).color(theme::MUTED_TEXT),
            ]
            .spacing(6)
            .align_y(Center),
            slider(1.0..=99.0, press_share as f32, Message::MixChanged)
                .step(1.0)
                .style(if enabled {
                    theme::mixer_slider
                } else {
                    theme::muted_slider
                }),
        ]
        .spacing(theme::ROW_GAP),
    )
    .padding(theme::GROUP_PADDING)
    .width(Fill)
    .style(|_theme| theme::slot_style())
    .into()
}

const fn mode_color(mode: SocdMode) -> Color {
    match mode {
        SocdMode::Immediate => Color::from_rgb(0.227, 0.333, 0.91),
        SocdMode::PressDelay => theme::PRIMARY_TEXT,
        SocdMode::ReleaseDelay => theme::RELEASE_TEXT,
        SocdMode::RandomMix => theme::MIX_TEXT,
    }
}

fn mode_label(mode: SocdMode, language: Language) -> &'static str {
    language.text(match mode {
        SocdMode::Immediate => "Immediate",
        SocdMode::PressDelay => "Press Delay",
        SocdMode::RandomMix => "Random Mix",
        SocdMode::ReleaseDelay => "Release Delay",
    })
}

fn mode_description(mode: SocdMode, language: Language) -> &'static str {
    language.text(match mode {
        SocdMode::Immediate => {
            "On each opposing-key overlap, drop the previous direction and send the new one \
             with no added delay."
        }
        SocdMode::PressDelay => {
            "On each opposing-key overlap, release the previous direction immediately and \
             press the new one after the configured delay."
        }
        SocdMode::RandomMix => {
            "On each opposing-key overlap, randomly select press delay or release delay \
             using the configured ratio."
        }
        SocdMode::ReleaseDelay => {
            "On each opposing-key overlap, press the new direction immediately and release \
             the previous one after the configured delay."
        }
    })
}

/// One millisecond slider row. Every value in the row is derived from `field`,
/// so a row cannot display one field while acting on another.
fn rate_box<'a>(buffer: &'a str, enabled: bool, editing: bool) -> Element<'a, Message> {
    // Out-of-range numbers clamp into 1-100 on commit, so only genuinely
    // unparseable text counts as invalid here.
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
                    .color(color)
                    .align_x(Alignment::Right),
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

fn latencies_card(
    measurement: MeasurementSnapshot,
    language: Language,
) -> Element<'static, Message> {
    container(
        column![
            section_title(Icon::Measurement, "Measured Input Transitions", language),
            text(
                language
                    .text("Live counts from this session, values freeze when measurement stops.")
            )
            .size(12)
            .color(theme::MUTED_TEXT),
            container(
                column![
                    table_row([
                        heading(
                            language.text("INPUT PATTERN"),
                            Length::Fixed(PATTERN_COLUMN),
                            Alignment::Left
                        ),
                        heading(language.text("SAMPLES"), Fill, Alignment::Right),
                        heading(language.text("MEDIAN"), Fill, Alignment::Right),
                        heading(language.text("P10"), Fill, Alignment::Right),
                        heading(language.text("P90"), Fill, Alignment::Right),
                        heading(language.text("MIN"), Fill, Alignment::Right),
                        heading(language.text("MAX"), Fill, Alignment::Right),
                    ]),
                    table_hrule(),
                    table_row(pattern_figures(
                        language.text("Neutral transition"),
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
                        language.text("Physical overlap"),
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
                        language.text("Indistinguishable"),
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

fn recommendations_card(
    measurement: MeasurementSnapshot,
    language: Language,
) -> Element<'static, Message> {
    container(
        column![
            row![
                column![
                    section_title(Icon::Star, "Suggested delays", language),
                    text(language.text(
                        "Based on P10-P50 input timings, excluding indistinguishable inputs."
                    ))
                    .size(12)
                    .color(theme::MUTED_TEXT),
                ]
                .width(Fill)
                .spacing(4),
                button(icon_label(
                    Icon::ArrowForward,
                    "Apply suggestions",
                    language
                ))
                .style(theme::primary_button)
                .on_press(Message::ApplyRecommendations),
            ]
            .align_y(Center),
            row![
                stat_inline(
                    language.text("SOCD Transition Delay"),
                    timing_range(measurement.recommended_transition),
                    theme::OK_TEXT,
                ),
                stat_inline(
                    language.text("Preserved Overlap Duration"),
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

fn disconnected_view(error: Option<&String>, language: Language) -> Element<'_, Message> {
    let mut content = column![
        text(language.text("The settings UI is waiting for LastKey.exe.")),
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
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            ..
        }) => Some(Message::CancelCapture),
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
        // Two lines by construction: the tile is too narrow for the full
        // sentence, and an explicit break stays put across DPIs where
        // automatic wrapping would not.
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
        "—".into()
    } else {
        format!("{:.1}", f64::from(count) * 100.0 / f64::from(total))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        protocol::UiView,
        settings::{Settings, SocdMode, TimingSettings},
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
            filter_enabled: true,
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
        let _ = app.update(super::Message::ModeSelected(SocdMode::PressDelay));
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

        let _ = app.update(super::Message::ModeSelected(SocdMode::Immediate));
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
        let _ = app.update(super::Message::ModeSelected(SocdMode::PressDelay));
        let _ = app.handle_event(UiEvent::Snapshot(old));
        assert_eq!(
            app.draft.as_ref().expect("draft is kept").timing.mode,
            SocdMode::PressDelay
        );
    }

    #[test]
    fn revert_resets_the_draft_and_buffers_at_click_time() {
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        let _ = app.update(super::Message::ModeSelected(SocdMode::PressDelay));

        let _ = app.update(super::Message::Revert);

        let draft = app.draft.as_ref().expect("draft is kept");
        assert_eq!(draft.timing.mode, SocdMode::Immediate);
        assert_eq!(app.inputs, super::TimingInputs::from_timing(&draft.timing));
    }

    #[test]
    fn older_snapshot_after_revert_leaves_reverted_values_in_place() {
        use crate::protocol::UiEvent;
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        let _ = app.update(super::Message::ModeSelected(SocdMode::PressDelay));
        let _ = app.update(super::Message::Revert);
        // A stale reply to an earlier request arrives after the revert.
        let mut stale = baseline_snapshot();
        stale.draft.timing.mode = SocdMode::PressDelay;
        let _ = app.handle_event(UiEvent::Snapshot(stale));
        assert_eq!(
            app.draft.as_ref().expect("draft is kept").timing.mode,
            SocdMode::Immediate
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
    fn section_navigation_keeps_the_single_page_title() {
        let mut app = test_app();
        let title = app.title();
        let _ = app.show_section(UiView::Measurement, false);
        assert_eq!(app.title(), title);
        assert!(app.settings_actions().is_some());
        let _ = app.show_section(UiView::Settings, false);
        assert_eq!(app.title(), title);
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
            draft.timing.mode = SocdMode::PressDelay;
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
    fn an_out_of_range_mix_ratio_clamps_without_leaving_the_mode() {
        // "Off" is a mode of its own now, so a zero in the ratio box no
        // longer flips a hidden switch: it clamps to the lowest usable share
        // and Random Mix stays selected.
        let mut app = test_app();
        {
            let draft = app.draft.as_mut().expect("draft is kept");
            draft.timing.mode = SocdMode::RandomMix;
        }
        app.inputs.preservation_rate = "0".into();
        let _ = app.update(super::Message::Apply);

        let draft = app.draft.as_ref().expect("draft is kept");
        assert_eq!(draft.timing.mode, SocdMode::RandomMix);
        assert_eq!(draft.timing.overlap_preservation_rate, 1);
        assert_eq!(app.inputs.preservation_rate, "1");
    }

    #[test]
    fn recommendations_open_settings_with_results_in_the_draft() {
        use crate::protocol::{MeasurementSnapshot, TimingRange, UiView};
        let mut app = test_app();
        let _ = app.show_section(UiView::Measurement, false);
        app.snapshot.as_mut().expect("snapshot is kept").measurement = Some(MeasurementSnapshot {
            recommended_transition: Some(TimingRange {
                min_micros: 2_100,
                max_micros: 3_000,
            }),
            ..MeasurementSnapshot::default()
        });
        let _ = app.update(super::Message::ApplyRecommendations);

        let draft = app.draft.as_ref().expect("draft is kept");
        assert_eq!(draft.timing.socd_transition_min_micros, 2_100);
        assert_eq!(draft.timing.socd_transition_max_micros, 3_000);
        assert!(app.notice.is_some());
    }

    #[test]
    fn measurement_errors_use_the_pinned_action_bar() {
        use crate::protocol::{ErrorView, UiEvent};
        let mut app = test_app();
        assert!(app.settings_actions().is_some());
        let _ = app.handle_event(UiEvent::RuntimeError(ErrorView {
            code: "measurement-start-failed".into(),
            message: "raw input registration failed".into(),
            recoverable: true,
        }));
        assert_eq!(app.error.as_deref(), Some("raw input registration failed"));
        assert!(app.settings_actions().is_some());
    }

    #[test]
    fn measurement_updates_keep_apply_feedback_in_the_shared_bar() {
        use crate::protocol::{MeasurementSnapshot, UiEvent};
        let mut app = test_app();
        let mut snapshot = baseline_snapshot();
        snapshot.measurement_active = true;
        snapshot.measurement = Some(MeasurementSnapshot::default());
        let _ = app.handle_event(UiEvent::ApplySucceeded(snapshot));
        assert_eq!(app.error, None);
        assert!(app.notice.is_some());
        assert!(app.settings_actions().is_some());
        let _ = app.handle_event(UiEvent::MeasurementUpdated(MeasurementSnapshot::default()));
        assert_eq!(app.notice.as_deref(), Some("Settings applied."));
        assert!(app.settings_actions().is_some());
    }

    #[test]
    fn window_icon_pixels_build_a_native_icon() {
        // The build-time RGBA blob must satisfy `from_rgba` (length matches
        // dimensions); a broken asset degrades to no icon at runtime.
        let icon = super::window_icon().expect("checked-in icon asset builds");
        let _ = icon;
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
        let _ = app.update(super::Message::ModeSelected(SocdMode::PressDelay));
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

    #[test]
    fn explicit_profile_load_replaces_local_edits_only_on_confirmed_success() {
        let mut app = super::SettingsApp::new();
        let initial = baseline_snapshot();
        app.set_snapshot(initial.clone());
        let _ = app.update(super::Message::ModeSelected(SocdMode::PressDelay));
        let _ = app.update(super::Message::LoadProfile(2));
        assert_eq!(app.snapshot.as_ref().unwrap().saved, initial.saved);
        assert_eq!(
            app.draft.as_ref().unwrap().timing.mode,
            SocdMode::PressDelay
        );
        let mut loaded = initial;
        loaded.saved = loaded.saved.select_profile(2).unwrap();
        loaded.draft = loaded.saved.clone();
        let expected = loaded.saved.clone();
        let _ = app.handle_event(crate::protocol::UiEvent::ProfileLoaded(loaded));
        assert_eq!(app.draft.as_ref(), Some(&expected));
        assert!(!app.is_dirty());
    }

    #[test]
    fn stopped_timeline_rejects_late_updates_and_filter_ack_clears_held_keys() {
        use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot, UiEvent};
        let mut app = super::SettingsApp::new();
        app.set_snapshot(baseline_snapshot());
        let event = MonitorSnapshot {
            elapsed_micros: 0,
            filter_enabled: true,
            physical: None,
            outputs: vec![MonitorEdge {
                key: KeySlot::HorizontalFirst,
                pressed: true,
                synthetic: true,
            }],
            decision: MonitorDecision::Immediate,
        };
        let _ = app.handle_event(UiEvent::MonitorStateChanged(true));
        let _ = app.handle_event(UiEvent::MonitorUpdated(event.clone()));
        assert!(
            app.monitor
                .timeline()
                .unwrap()
                .held(KeySlot::HorizontalFirst)
        );
        let _ = app.handle_event(UiEvent::FilterChanged(false));
        assert!(
            !app.monitor
                .timeline()
                .unwrap()
                .held(KeySlot::HorizontalFirst)
        );
        let _ = app.handle_event(UiEvent::MonitorStateChanged(false));
        let _ = app.handle_event(UiEvent::MonitorUpdated(event));
        assert!(app.monitor.timeline().is_none());
    }
}
