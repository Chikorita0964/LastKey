use iced::{
    Center, Color, Element, Fill, Length, Padding, Size, Subscription, Task, Theme,
    widget::{
        Id, button, column, container, mouse_area, opaque, operation, row, rule, scrollable,
        slider, space, stack, text,
        text::{Alignment, Ellipsis, Wrapping},
        text_input, toggler,
    },
    window,
};

use crate::{
    core::MIN_RECOMMENDATION_SAMPLES,
    protocol::{KeySlot, MeasurementSnapshot, UiCommand, UiEvent, UiSnapshot, UiView},
    settings::{ProfileSlot, Settings, SocdMode, TimingSettings},
};

use super::{
    hover_text,
    icons::{self, Name as Icon},
    ipc_client::{self, Connection, Event},
    language::Language,
    preview::{self, Preview},
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

/// Top offset anchoring the profile/language panel below the header card:
/// page padding + header height + the gap the reference puts under its
/// dropdowns (8). Iced has no absolute positioning, so the header's height
/// is fixed rather than measured and this offset derives from it; a
/// content-sized header would drift from the offset with font and DPI.
const HEADER_HEIGHT: f32 = 60.0;
const PROFILE_PANEL_TOP: f32 = theme::PAGE_PADDING + HEADER_HEIGHT + 8.0;

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
    preview: Preview,
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
    session_details_open: bool,
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

    /// The one definition of "this row is live". The view mounts only live
    /// groups and `update` drops messages for the rest, so a message queued
    /// before a mode switch cannot edit a value the new mode hides.
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
    Preview(preview::Action),
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
            preview: Preview::default(),
            pending_filter: None,
            profiles: ProfileDialog::Closed,
            language: Language::default(),
            snapshot: None,
            draft: None,
            inputs: TimingInputs::default(),
            pending_section: Some(requested_view()),
            editing: [false; 5],
            session_details_open: false,
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
                    | Message::CancelCapture
                    | Message::WindowUnfocused
            )
        {
            return Task::none();
        }
        self.track_box_focus(&message);
        match message {
            Message::Preview(action) => self.preview.update(action),
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
                // Escape unwinds one layer at a time: a rename or confirm
                // back to the slot list, the list or language menu to
                // closed. In-flight loads and renames finish first, matching
                // the close button's guard.
                match &self.profiles {
                    ProfileDialog::Rename { .. } | ProfileDialog::Confirm(_) => {
                        self.profiles = ProfileDialog::List;
                    }
                    ProfileDialog::List | ProfileDialog::Languages => {
                        self.profiles = ProfileDialog::Closed;
                    }
                    ProfileDialog::Loading | ProfileDialog::Renaming => {}
                    ProfileDialog::Closed => {
                        if self
                            .snapshot
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.capture_slot.is_some())
                        {
                            self.send(UiCommand::CancelKeyCapture);
                        }
                    }
                }
            }
            Message::ResetMeasurement => {
                self.session_details_open = false;
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
                // A drag queued before the mode switched still arrives after
                // its slider unmounts, so the gate lives here too.
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
                self.session_details_open = true;
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
            | Message::Ipc(..)
            | Message::Preview(preview::Action::Tick) => {}
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
        if view == UiView::Measurement {
            self.session_details_open = true;
        }
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
            theme::EMERALD_500
        } else {
            theme::SLATE_300
        };
        let filter_enabled = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.filter_enabled);
        // Compact icon-only controls (reference: header buttons are icons
        // with aria-labels). No hover tooltips: the reference shows no
        // hover descriptions.
        let header = container(
            row![
                widgets::logo(WINDOW_ICON_RGBA, WINDOW_ICON_WIDTH),
                text("LastKey")
                    .size(theme::HEADING_SIZE)
                    .font(theme::UI_FONT_BOLD),
                row![
                    dot(state_color),
                    hover_text::label(
                        self.language.text(&self.status),
                        theme::BODY_TEXT_SIZE,
                        theme::UI_FONT_SEMIBOLD,
                        Some(theme::SLATE_600),
                        false
                    ),
                ]
                .spacing(14)
                .align_y(Center)
                .width(Fill),
                button(icons::icon(Icon::Layers, 14.0, Some(theme::PRIMARY_TEXT)))
                    .padding(theme::HEADER_ICON_PADDING)
                    .height(theme::HEADER_ICON_HEIGHT)
                    .style(theme::secondary_button)
                    .on_press_maybe(
                        (connected && self.snapshot.is_some()).then_some(Message::OpenProfiles)
                    ),
                button(icons::icon(
                    Icon::Languages,
                    14.0,
                    Some(theme::PRIMARY_TEXT)
                ))
                .padding(theme::HEADER_ICON_PADDING)
                .height(theme::HEADER_ICON_HEIGHT)
                .style(theme::secondary_button)
                .on_press_maybe(self.snapshot.is_some().then_some(Message::OpenLanguages)),
                button(icons::icon(
                    Icon::Power,
                    14.0,
                    Some(if filter_enabled {
                        theme::PRIMARY_TEXT
                    } else {
                        theme::ICON_MUTED
                    })
                ))
                .padding(theme::HEADER_ICON_PADDING)
                .height(theme::HEADER_ICON_HEIGHT)
                .style(theme::secondary_button)
                .on_press_maybe(
                    (connected && self.snapshot.is_some() && self.pending_filter.is_none())
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
        .center_y(HEADER_HEIGHT)
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

    fn timing_preview(&self, timing: &TimingSettings) -> Element<'_, Message> {
        let preview = &self.preview;
        let (old, new) = preview.held();
        let mode = [
            SocdMode::Immediate,
            SocdMode::PressDelay,
            SocdMode::ReleaseDelay,
        ][preview.example];
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
        let delay = preview::delay_label(min, max);
        let key = |name: &'static str, arrow, active, accent| {
            container(
                column![
                    text(name).size(18).font(theme::UI_FONT_BOLD),
                    icons::icon(
                        arrow,
                        12.0,
                        Some(if active { Color::WHITE } else { accent })
                    )
                ]
                .spacing(4)
                .align_x(Center),
            )
            .center_x(52)
            .center_y(52)
            .style(move |_| {
                let style =
                    theme::keycap(iced::widget::button::Status::Active, active, false, accent);
                container::Style {
                    text_color: Some(style.text_color),
                    background: style.background,
                    border: style.border,
                    shadow: style.shadow,
                    ..container::Style::default()
                }
            })
        };
        let state = match (old, new) {
            (true, true) => "Game receives A + D",
            (false, false) => "Game receives no direction",
            (true, false) => "Game receives A",
            (false, true) => "Game receives D",
        };
        let indicator: Element<'_, Message> = match (old, new) {
            (true, false) => icons::icon(Icon::ArrowLeft, 24.0, Some(theme::PRIMARY_TEXT)),
            (false, true) => icons::icon(Icon::ArrowRight, 24.0, Some(theme::PURPLE_600)),
            (false, false) => dot(theme::INDIGO_600),
            (true, true) => container(space::horizontal().width(18).height(3))
                .style(|_| container::Style {
                    background: Some(theme::VIOLET_500.into()),
                    border: iced::Border::default().rounded(2),
                    ..container::Style::default()
                })
                .into(),
        };
        let nav = |icon, action| {
            button(icons::icon(icon, 18.0, Some(theme::ICON_MUTED)))
                .padding(9)
                .style(theme::nav_button)
                .on_press(Message::Preview(action))
        };
        // The play pill fills with the example's accent while playing
        // (reference: filled colored pill vs. outlined neutral pill).
        let (pill_label, pill_value, pill_icon) = if preview.playing {
            (
                Color {
                    a: 0.75,
                    ..Color::WHITE
                },
                Color::WHITE,
                Color::WHITE,
            )
        } else {
            (theme::MUTED_TEXT, theme::BODY_TEXT, theme::MUTED_TEXT)
        };
        let content = column![
            button(
                row![
                    text(self.language.text("Preview"))
                        .size(10)
                        .color(pill_label),
                    text(mode_label(mode, self.language))
                        .size(11)
                        .font(theme::UI_FONT_BOLD)
                        .color(pill_value),
                    icons::icon(
                        if preview.playing {
                            Icon::Stop
                        } else {
                            Icon::Play
                        },
                        12.0,
                        Some(pill_icon)
                    ),
                ]
                .spacing(6)
                .align_y(Center)
            )
            .padding([5, 12])
            .style(move |theme_, status| {
                theme::preview_pill(theme_, status, mode_color(mode), preview.playing)
            })
            .on_press(Message::Preview(preview::Action::Toggle)),
            row![
                key("A", Icon::ArrowLeft, old, theme::PRIMARY_TEXT),
                column![
                    container(indicator).center_x(60).center_y(26),
                    container(text(delay).size(10))
                        .padding([2, 6])
                        .style(move |_| container::Style {
                            background: Some(
                                if preview.phase == 1 {
                                    mode_color(mode)
                                } else {
                                    Color::from_rgb8(241, 245, 249)
                                }
                                .into()
                            ),
                            text_color: Some(if preview.phase == 1 {
                                Color::WHITE
                            } else {
                                theme::MUTED_TEXT
                            }),
                            border: iced::Border::default().rounded(8),
                            ..container::Style::default()
                        })
                ]
                .spacing(4)
                .align_x(Center),
                key("D", Icon::ArrowRight, new, theme::PURPLE_600)
            ]
            .spacing(16)
            .align_y(Center),
            text(self.language.text(state))
                .size(12)
                .font(theme::UI_FONT_BOLD),
            // Narrow example-position indicator: one slot per example, the
            // selected one elongated in its own accent (reference dots).
            row((0..3).map(|example| {
                let selected = example == preview.example;
                container(space::horizontal())
                    .width(Length::Fixed(if selected { 20.0 } else { 6.0 }))
                    .height(Length::Fixed(6.0))
                    .style(move |_| {
                        theme::example_dot(if selected {
                            mode_color(
                                [
                                    SocdMode::Immediate,
                                    SocdMode::PressDelay,
                                    SocdMode::ReleaseDelay,
                                ][example],
                            )
                        } else {
                            Color::from_rgb8(0xcb, 0xd5, 0xe1)
                        })
                    })
                    .into()
            }))
            .spacing(4),
            if matches!(self.profiles, ProfileDialog::Closed) {
                preview::clock(preview, Message::Preview(preview::Action::Tick))
            } else {
                space::vertical().height(1).into()
            },
        ]
        .spacing(10)
        .align_x(Center)
        .width(Fill);
        container(
            row![
                nav(Icon::ChevronLeft, preview::Action::Previous),
                content,
                nav(Icon::ChevronRight, preview::Action::Next)
            ]
            .spacing(8)
            .align_y(Center),
        )
        .padding(12)
        .width(Fill)
        .style(|_| theme::slot_style())
        .into()
    }

    fn load_profile(&mut self, slot: u8) {
        self.error = None;
        self.profiles = ProfileDialog::Loading;
        self.send(UiCommand::LoadProfile(slot));
    }

    /// The profile and language menus share one panel anchored below the
    /// header's right edge, like the reference dropdowns. Iced has no
    /// absolute positioning, so the anchor is a fixed offset from the page
    /// corner. A transparent backdrop closes the panel on an outside press;
    /// the panel itself is opaque so hovers never leak to the page beneath.
    fn profile_dialog(&self) -> Element<'_, Message> {
        let Some(snapshot) = &self.snapshot else {
            return space::horizontal().width(0).into();
        };
        if matches!(self.profiles, ProfileDialog::Closed) {
            return space::horizontal().width(0).into();
        }
        let languages = matches!(self.profiles, ProfileDialog::Languages);
        let in_flight = matches!(
            self.profiles,
            ProfileDialog::Loading | ProfileDialog::Renaming
        );
        // The reference nests the heading and its subtitle in one column *beside*
        // the close button, so the 36px button and the 34px of text both sit on
        // the row rather than stacking. Keeping the subtitle inside this column
        // is what stops the button from adding its own height to the header.
        let mut titles = column![
            text(self.language.text(if languages {
                "Language"
            } else {
                "Profile Slots"
            }))
            .size(14)
            .font(theme::UI_FONT_BOLD),
        ]
        .spacing(theme::PROFILE_HEADER_GAP);
        if !languages {
            titles = titles.push(
                text(
                    self.language
                        .text("Changes are saved when you click Apply."),
                )
                .size(11)
                .color(theme::MUTED_TEXT),
            );
        }
        if let Some(error) = &self.error {
            titles = titles.push(
                text(self.language.text(error))
                    .size(12)
                    .color(theme::ERROR_TEXT),
            );
        }
        let mut header = row![
            icons::icon(
                if languages {
                    Icon::Languages
                } else {
                    Icon::Layers
                },
                14.0,
                Some(theme::PRIMARY_TEXT)
            ),
            titles,
            space::horizontal().width(Fill),
        ]
        .spacing(8)
        .align_y(Center);
        // The close button is a borderless circle, not the outlined secondary
        // button used elsewhere, and the reference sizes it on the box: `h-9 w-9`
        // on the slot dialog and `h-7 w-7` on the language menu. The padding is
        // all that is left for the icon once the box is fixed, so it is set to
        // centre a 14px glyph in each rather than being inherited from a metric.
        if !in_flight {
            let (box_size, icon_size) = if languages {
                (28.0, 12.0)
            } else {
                (36.0, 14.0)
            };
            header = header.push(
                button(icons::icon(Icon::Close, icon_size, None))
                    .style(theme::profile_close_button)
                    .width(box_size)
                    .height(box_size)
                    .padding((box_size - icon_size) / 2.0)
                    .on_press(Message::CloseProfiles),
            );
        }
        let header = container(header).padding(theme::PROFILE_HEADER_PADDING);
        // The panel is a measured shell: the header block and the scroller keep
        // the insets they are measured with, so the padding lives on them and
        // not on `panel`.
        let panel = container(column![
            header,
            container(self.profile_slots(snapshot, languages))
                .padding(if languages {
                    theme::LANGUAGE_SCROLLER_PADDING
                } else {
                    theme::PROFILE_SCROLLER_PADDING
                })
                .width(Fill),
        ])
        .width(if languages {
            theme::LANGUAGE_PANEL_WIDTH
        } else {
            theme::PROFILE_PANEL_WIDTH
        })
        // Stands in for the border the panel strokes inside its own bounds, so
        // the inner blocks inset from the drawn edge exactly as they do in the
        // reference.
        .padding(theme::PANEL_PADDING)
        .style(|_| theme::profile_panel());
        stack![
            mouse_area(space::horizontal().width(Fill).height(Fill))
                .on_press(Message::CloseProfiles),
            container(opaque(panel))
                .align_right(Fill)
                .height(Fill)
                .padding(Padding {
                    top: PROFILE_PANEL_TOP,
                    right: theme::PAGE_PADDING,
                    ..Padding::ZERO
                }),
        ]
        .into()
    }

    /// The panel's scroller: the language rows, or one card per profile slot.
    /// Four slots is the whole bank (`ProfileBank::slots` is a `[_; 4]`), so the
    /// reference's `max-h` and its overflow never engage and no scroll padding is
    /// copied for a scrollbar that cannot appear.
    fn profile_slots<'a>(
        &'a self,
        snapshot: &'a UiSnapshot,
        languages: bool,
    ) -> Element<'a, Message> {
        let body: Element<'a, Message> = if languages {
            let mut rows = column![].spacing(theme::LANGUAGE_ROW_GAP);
            for language in Language::ALL {
                let selected = self.language == language;
                rows = rows.push(
                    button(
                        row![
                            container(hover_text::label(
                                language.name(),
                                12.0,
                                theme::UI_FONT_BOLD,
                                None,
                                false
                            ))
                            .width(Fill),
                            if selected {
                                icons::icon(Icon::Check, 12.0, Some(theme::PRIMARY_TEXT))
                            } else {
                                space::horizontal().width(12).into()
                            },
                        ]
                        .spacing(6)
                        .align_y(Center),
                    )
                    .width(Fill)
                    .padding([8, 10])
                    .style(if selected {
                        theme::active_option
                    } else {
                        theme::language_option
                    })
                    .on_press(Message::SelectLanguage(language)),
                );
            }
            rows.into()
        } else if matches!(self.profiles, ProfileDialog::Loading) {
            text(self.language.text("Loading and activating profile…")).into()
        } else if matches!(self.profiles, ProfileDialog::Renaming) {
            text(self.language.text("Saving profile name…")).into()
        } else {
            let bank = snapshot.saved.profile_bank();
            let mut cards = column![].spacing(theme::SLOT_GAP);
            for (index, profile) in bank.slots.iter().enumerate() {
                let slot = index as u8;
                cards = cards.push(self.profile_slot_card(
                    slot,
                    profile,
                    bank.active == slot,
                    snapshot,
                ));
            }
            cards.into()
        };
        body
    }

    /// One slot in the profile panel: a mode-tinted card with the name box
    /// (which doubles as the rename target) on row one and the axis-paired
    /// keycap chips plus mode label on row two. Row two groups chips by axis
    /// pair (vertical pair, then horizontal pair) like the reference and the
    /// timeline, and is the load target for inactive slots; a pending load
    /// covers the card via `stack` so the card never changes height.
    fn profile_slot_card<'a>(
        &'a self,
        slot: u8,
        profile: &ProfileSlot,
        active: bool,
        snapshot: &'a UiSnapshot,
    ) -> Element<'a, Message> {
        // The bank is an owned local copy, so everything the card shows is
        // copied out here; no element may borrow the slot.
        let profile_name = profile.name.clone();
        let bindings = profile.bindings;
        let mode = profile.timing.mode;
        let renaming = matches!(&self.profiles, ProfileDialog::Rename { slot: editing, .. } if *editing == slot);
        let confirming =
            matches!(&self.profiles, ProfileDialog::Confirm(pending) if *pending == slot);
        let name_row: Element<'a, Message> = if let ProfileDialog::Rename {
            slot: editing,
            name,
        } = &self.profiles
            && *editing == slot
        {
            row![
                container(
                    text_input(self.language.text("Profile name"), name)
                        .on_input(Message::ProfileNameChanged)
                        .on_submit(Message::SaveProfileName)
                        .style(|theme_, status| {
                            theme::value_input(theme_, status, theme::PRIMARY_TEXT, false)
                        })
                        .padding(VALUE_BOX_PADDING)
                        .width(Fill),
                )
                .padding(theme::SLOT_NAME_PADDING)
                .width(Fill)
                .style(|_| theme::pill_style(false)),
                button(icons::icon(Icon::Check, 12.0, Some(Color::WHITE)))
                    .style(theme::primary_button)
                    .padding(6)
                    .on_press(Message::SaveProfileName),
            ]
            .spacing(6)
            .align_y(Center)
            .into()
        } else {
            button(
                row![
                    hover_text::label(profile_name, 12.0, theme::UI_FONT_BOLD, None, false),
                    icons::icon(Icon::Edit, 12.0, Some(theme::ICON_MUTED)),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .style(theme::profile_name_button)
            .padding(theme::SLOT_NAME_PADDING)
            .on_press(Message::EditProfileName(slot))
            .into()
        };
        // Reference pairs chips by axis (vertical pair, then horizontal
        // pair) with a divider between the pairs; the mode label hugs
        // the right edge. Bindings are stored vertical-first,
        // vertical-second, horizontal-first, horizontal-second.
        let chips = row![
            row![
                profile_chip(bindings[0], snapshot),
                profile_chip(bindings[1], snapshot),
            ]
            .spacing(4)
            .align_y(Center),
            // Reference draws the pair divider as a left border on the second
            // group rather than a rule, and puts 8px of space on *both* sides of
            // it: `ml-2 pl-2` is 8 + 8 around a 1px edge, so the divider sits on
            // the row's own 8px spacing. Its height is the chips' height, not the
            // row's -- a `rule::vertical` would fill and inflate the card.
            container(space::horizontal().width(1.0))
                .height(theme::CHIP_HEIGHT)
                .style(|_| theme::pair_divider()),
            row![
                profile_chip(bindings[2], snapshot),
                profile_chip(bindings[3], snapshot),
            ]
            .spacing(4)
            .align_y(Center),
            space::horizontal().width(Fill),
            text(mode_label(mode, self.language))
                .size(10)
                .font(theme::UI_FONT_BOLD)
                .color(mode_label_color(mode)),
        ]
        .spacing(8)
        .align_y(Center);
        // The load target is the keycap row itself, so it takes the row's height
        // rather than grown padding. Padding here would make an inactive card
        // taller than the active one, which the reference's own row button does
        // not do -- it carries no padding classes at all.
        let chips: Element<'a, Message> = if active || renaming || confirming {
            chips.into()
        } else {
            button(chips)
                .width(Fill)
                .padding(Padding::ZERO)
                .height(theme::CHIP_HEIGHT)
                .style(theme::ghost_button)
                .on_press(Message::LoadProfile(slot))
                .into()
        };
        let card_body = column![name_row, chips].spacing(theme::SLOT_ROW_GAP);
        let content: Element<'a, Message> = if confirming {
            stack![
                container(card_body)
                    .padding(theme::SLOT_CARD_PADDING)
                    .width(Fill),
                opaque(
                    container(
                        row![
                            column![
                                text(self.language.text("Load this slot?"))
                                    .size(12)
                                    .font(theme::UI_FONT_BOLD),
                                text(
                                    self.language
                                        .text("Unapplied draft changes will be discarded.")
                                )
                                .size(11)
                                .color(theme::MUTED_TEXT),
                            ]
                            .spacing(2)
                            .width(Fill),
                            button(text(self.language.text("Cancel")).size(12))
                                .style(theme::secondary_button)
                                .padding([6, 12])
                                .on_press(Message::OpenProfiles),
                            button(
                                text(self.language.text("Load"))
                                    .size(12)
                                    .font(theme::UI_FONT_BOLD)
                            )
                            .style(theme::primary_button)
                            .padding([6, 12])
                            .on_press(Message::ConfirmProfile(slot)),
                        ]
                        .spacing(8)
                        .align_y(Center),
                    )
                    .padding(12)
                    .width(Fill)
                    .height(Fill)
                    .style(|_| theme::profile_confirm_overlay())
                ),
            ]
            .into()
        } else {
            container(card_body)
                .padding(theme::SLOT_CARD_PADDING)
                .width(Fill)
                .into()
        };
        container(content)
            .width(Fill)
            .style(move |_| theme::tinted_slot(mode_color(mode), active))
            .into()
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
                    button(icon_label(
                        Icon::Restore,
                        "Restore mapping defaults",
                        self.language,
                    ))
                    .style(theme::secondary_button)
                    .on_press(Message::RestoreMappingDefaults),
                ]
                .align_y(Center),
                text(
                    self.language
                        .text("Hardware scan codes the SOCD filter uses")
                )
                .size(12)
                .color(theme::MUTED_TEXT),
                // The reference shows an indigo capture banner between the
                // header and the stage. A zero-height placeholder keeps the
                // column's child indices stable while it is hidden, as the
                // mode-conditional timing groups do.
                if snapshot.capture_slot.is_some() {
                    rebind_banner(self.language)
                } else {
                    space::vertical().height(0).into()
                },
                mapping_pad(snapshot, self.monitor.timeline(), self.language),
                // The assignment status sits in a footer below the inset,
                // hugging the right edge (reference layout).
                rule::horizontal(1).style(theme::table_rule),
                row![
                    space::horizontal().width(Fill),
                    assignment_status(&duplicate_slots(&snapshot.draft.bindings), self.language),
                ]
                .align_y(Center),
            ]
            .spacing(theme::SECTION_GAP),
        )
        .padding(theme::CARD_PADDING)
        .width(Fill)
        .style(|_| theme::card_style());

        // Zero-height placeholders keep child indices stable across mode
        // changes: the page is diffed positionally, so an unmounted group
        // would hand its state slot to the next widget.
        let timing_card = container(
            column![
                row![
                    section_title(Icon::Timer, "Input timings", self.language).width(Fill),
                    button(icon_label(
                        Icon::Restore,
                        "Restore timing defaults",
                        self.language,
                    ))
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
                        if timing.mode == SocdMode::Immediate {
                            self.timing_preview(timing)
                        } else {
                            space::vertical().height(0).into()
                        },
                        if timing.mode == SocdMode::RandomMix {
                            rate_group(timing, &self.inputs, &self.editing, self.language)
                        } else {
                            space::vertical().height(0).into()
                        },
                        if matches!(timing.mode, SocdMode::PressDelay | SocdMode::RandomMix) {
                            duration_range(
                                TimingField::TransitionMinimum,
                                TimingField::TransitionMaximum,
                                self.language.text("New Key Press Delay"),
                                timing,
                                &self.inputs,
                                &self.editing,
                                theme::PRIMARY_TEXT,
                            )
                        } else {
                            space::vertical().height(0).into()
                        },
                        if matches!(timing.mode, SocdMode::ReleaseDelay | SocdMode::RandomMix) {
                            duration_range(
                                TimingField::PreservedMinimum,
                                TimingField::PreservedMaximum,
                                self.language.text("Previous Key Release Delay"),
                                timing,
                                &self.inputs,
                                &self.editing,
                                theme::VIOLET_600,
                            )
                        } else {
                            space::vertical().height(0).into()
                        },
                        // Absent in Random Mix, where the ratio and both
                        // groups already fill the card (reference: mt-auto is
                        // not expressible here; the group is intrinsic-height).
                        if timing.mode == SocdMode::RandomMix {
                            space::vertical().height(0).into()
                        } else {
                            mechanism_steps(timing.mode, timing, self.language)
                        },
                    ]
                    .spacing(12),
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
                .padding(theme::BUTTON_PADDING)
                .style(theme::secondary_button)
                .on_press(Message::Revert)
        } else {
            button(icon_label(Icon::Revert, "Revert", self.language))
                .padding(theme::BUTTON_PADDING)
                .style(theme::secondary_button)
        };
        let apply = if dirty {
            button(icon_label(Icon::Check, "Apply", self.language))
                .padding(theme::BUTTON_PADDING_WIDE)
                .style(theme::primary_button)
                .on_press(Message::Apply)
        } else {
            button(icon_label(Icon::Check, "Apply", self.language))
                .padding(theme::BUTTON_PADDING_WIDE)
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
                .padding(theme::BUTTON_PADDING)
                .style(theme::secondary_button)
                .on_press(Message::RestoreAllDefaults),
                if dirty {
                    container(icon_label(
                        Icon::Edit,
                        "Unsaved Draft Changes",
                        self.language,
                    ))
                    .padding(theme::BUTTON_PADDING)
                    .style(|_| theme::dirty_badge())
                    .into()
                } else {
                    Element::from(space::horizontal().width(0))
                },
                feedback,
                revert,
                apply,
            ]
            .spacing(theme::ROW_GAP)
            .align_y(Center),
        )
        .padding(16.0)
        .width(Fill)
        .style(|_theme| theme::card_style());

        Some(actions.into())
    }

    /// Error and notice feedback shared by the whole page. Rendered as plain text
    /// with a blank placeholder when empty, so its presence never moves any
    /// widget (see `settings_actions`).
    fn feedback_element(&self) -> Element<'_, Message> {
        match (&self.error, &self.notice) {
            (Some(error), _) => row![
                icons::icon(Icon::Warning, 14.0, Some(theme::ERROR_TEXT)),
                text(self.language.text(error))
                    .size(12)
                    .font(theme::UI_FONT_SEMIBOLD)
                    .color(theme::RED_600)
                    .width(Fill)
                    .align_x(Alignment::Right)
                    .wrapping(Wrapping::None)
                    .ellipsis(Ellipsis::End)
            ]
            .spacing(6)
            .align_y(Center)
            .width(Fill)
            .into(),
            (None, Some(notice)) => row![
                icons::icon(Icon::Check, 14.0, Some(theme::OK_TEXT)),
                text(self.language.text(notice))
                    .size(12)
                    .font(theme::UI_FONT_SEMIBOLD)
                    .color(theme::EMERALD_600)
                    .width(Fill)
                    .align_x(Alignment::Right)
                    .wrapping(Wrapping::None)
                    .ellipsis(Ellipsis::End)
            ]
            .spacing(6)
            .align_y(Center)
            .width(Fill)
            .into(),
            (None, None) if self.is_dirty() => container(hover_text::label(
                self.language.text("Click Apply to commit draft edits."),
                12.0,
                theme::UI_FONT_ITALIC,
                Some(theme::ICON_MUTED),
                false,
            ))
            .width(Fill)
            .align_right(Fill)
            .into(),
            (None, None) => container(
                row![
                    icons::icon(Icon::Check, 14.0, Some(theme::OK_TEXT)),
                    text(self.language.text("Synchronized"))
                        .size(12)
                        .color(theme::EMERALD_SYNC),
                ]
                .spacing(4)
                .align_y(Center),
            )
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
        // A stopped timeline collapses to its header and control; the graph
        // mounts only while recording. The subtitle carries the Starting /
        // Stopping lifecycle the toggle itself cannot express. The graph
        // carries its own keycaps, ruler, and needle, so no separate label
        // column or scale row is needed (reference canvas).
        let recording = matches!(self.monitor, MonitorState::Recording(_));
        let subtitle = if ready {
            "Shows how long each key is held and where it overlaps its opposite, live."
        } else {
            label
        };
        let header = row![
            column![
                section_title(Icon::Target, "Key Input Timeline", self.language),
                text(self.language.text(subtitle))
                    .size(12)
                    .color(theme::MUTED_TEXT)
            ]
            .spacing(4)
            .width(Fill),
            toggler(recording)
                .on_toggle_maybe(ready.then_some(|_| Message::ToggleMonitor))
                .size(24)
                .style(theme::monitor_toggler),
        ]
        .spacing(12)
        .align_y(Center);
        let mut card = column![header].spacing(12);
        if recording {
            let names = [
                snapshot.keys[0].name.as_str(),
                snapshot.keys[1].name.as_str(),
                snapshot.keys[2].name.as_str(),
                snapshot.keys[3].name.as_str(),
            ];
            card = card.push(
                container(timeline::graph(timeline, names))
                    .clip(true)
                    .style(|_| theme::graph_frame()),
            );
        }
        container(card)
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
        let button_style = if snapshot.measurement_active {
            theme::warning_button
        } else {
            theme::primary_button
        };
        let is_open = self.session_details_open || snapshot.measurement_active;
        let measurement = snapshot.measurement.unwrap_or_default();

        let header = row![
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
                self.language,
            ))
            .style(button_style)
            .on_press(Message::ToggleMeasurement),
        ]
        .align_y(Center);

        let mut summary_col = column![header];
        if is_open {
            let stats = row![
                stat_box(
                    self.language.text("Physical key edges"),
                    measurement.observed_event_count.to_string(),
                    theme::BODY_TEXT,
                ),
                stat_box(
                    self.language.text("Valid paired samples"),
                    measurement.sample_count.to_string(),
                    theme::INDIGO_600,
                ),
                stat_box(
                    self.language.text("Physical overlap share"),
                    percentage_value(measurement.overlap_count, measurement.sample_count,),
                    theme::WARN_TEXT,
                ),
                stat_box(
                    self.language.text("Indistinguishable share"),
                    percentage_value(
                        measurement.near_simultaneous_count,
                        measurement.sample_count,
                    ),
                    theme::RED_600,
                ),
            ]
            .spacing(theme::ROW_GAP);
            summary_col = summary_col.push(stats);
        }

        let summary = container(summary_col.spacing(theme::SECTION_GAP))
            .padding(theme::CARD_PADDING)
            .width(Fill)
            .style(|_theme| theme::card_style());

        let mut content = column![summary].spacing(theme::SECTION_GAP);
        if is_open {
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
    // Restore affordances use the reference's slate-600 ink; other action
    // icons inherit the button text color so hover states keep working.
    let ink = matches!(name, Icon::Restore).then_some(theme::ICON_SECONDARY);
    row![
        icons::icon(name, 14.0, ink),
        text(language.text(&label.into()).to_owned()).size(12)
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}

fn trailing_icon_label(
    name: Icon,
    label: impl Into<String>,
    language: Language,
) -> Element<'static, Message> {
    row![
        text(language.text(&label.into()).to_owned()).size(12),
        icons::icon(name, 14.0, None)
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

/// One D-pad direction's display identity: its capture slot, sub-legend,
/// arrow, and accent color.
struct Direction {
    slot: KeySlot,
    label: &'static str,
    arrow: Icon,
    accent: Color,
}

const UP: Direction = Direction {
    slot: KeySlot::VerticalFirst,
    label: "UP",
    arrow: Icon::ArrowUp,
    accent: Color::from_rgb8(0x25, 0x63, 0xeb),
};
const DOWN: Direction = Direction {
    slot: KeySlot::VerticalSecond,
    label: "DOWN",
    arrow: Icon::ArrowDown,
    accent: Color::from_rgb8(0x7c, 0x3a, 0xed),
};
const LEFT: Direction = Direction {
    slot: KeySlot::HorizontalFirst,
    label: "LEFT",
    arrow: Icon::ArrowLeft,
    accent: Color::from_rgb8(0x63, 0x66, 0xf1),
};
const RIGHT: Direction = Direction {
    slot: KeySlot::HorizontalSecond,
    label: "RIGHT",
    arrow: Icon::ArrowRight,
    accent: Color::from_rgb8(0x93, 0x33, 0xea),
};

fn mapping_pad<'a>(
    snapshot: &'a UiSnapshot,
    timeline: Option<&Timeline>,
    language: Language,
) -> Element<'a, Message> {
    let duplicates = duplicate_slots(&snapshot.draft.bindings);
    let up = keycap(
        &UP,
        snapshot,
        duplicates[0],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::VerticalFirst)),
        language,
    );
    let down = keycap(
        &DOWN,
        snapshot,
        duplicates[1],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::VerticalSecond)),
        language,
    );
    let left = keycap(
        &LEFT,
        snapshot,
        duplicates[2],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::HorizontalFirst)),
        language,
    );
    let right = keycap(
        &RIGHT,
        snapshot,
        duplicates[3],
        timeline.is_some_and(|timeline| timeline.held(KeySlot::HorizontalSecond)),
        language,
    );
    // No `Fill` height may appear in this subtree. `Container::diff` and
    // `Column::diff` stack a child's height into their own `Length`, so a
    // single fill here turns the whole card into a fill-height child of the
    // body row. That row compresses its cross axis, and the scrollable body
    // gives it no bounded height to distribute, so the card resolves to zero
    // height and paints nothing. The reference's `justify-between` has no
    // intrinsic equivalent; the column's spacing carries the separation.
    container(
        column![
            row![
                icons::icon(Icon::Edit, 12.0, Some(theme::MUTED_TEXT)),
                text(language.text("Click keycap to rebind"))
                    .size(11)
                    .color(theme::MUTED_TEXT),
            ]
            .spacing(6)
            .align_y(Center),
            container(up).center_x(Fill),
            row![left, dpad_center_tile(timeline), right]
                .spacing(16)
                .align_y(Center),
            container(down).center_x(Fill),
        ]
        .spacing(16)
        .align_x(Center),
    )
    .padding(20.0)
    .width(Fill)
    .style(|_| theme::stage_style())
    .into()
}

/// The capture-mode banner from the reference: an indigo bar naming the
/// prompt with an explicit ESC cancel. The reference pulses the bar; the
/// port keeps it still, as with the paused-by-default preview, so no
/// repaint loop outlives the capture it decorates.
fn rebind_banner<'a>(language: Language) -> Element<'a, Message> {
    container(
        row![
            dot(Color::WHITE),
            text(language.text("Press a new key on your keyboard..."))
                .size(12)
                .font(theme::UI_FONT_BOLD)
                .color(Color::WHITE)
                .width(Fill),
            button(
                text(language.text("ESC Cancel"))
                    .size(11)
                    .font(theme::UI_FONT_BOLD),
            )
            .style(theme::banner_cancel_button)
            .padding([4, 10])
            .on_press(Message::CancelCapture),
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([10, 16])
    .width(Fill)
    .style(|_| theme::rebind_banner_style())
    .into()
}

/// The D-pad's center tile: a dashed guide ring, a resting dot, and the
/// moving dot that shifts toward the winning direction (diagonals travel
/// less far, as in the reference). Iced has no absolute positioning, so
/// the dot's offset rides on the padding of its full-size wrapper.
///
/// The dot follows engine output only. While the filter is off or
/// measurement runs, the timeline carries physical input, where both
/// opposing keys can be down with nothing resolving them, so the dot rests.
fn dpad_center_tile(timeline: Option<&Timeline>) -> Element<'static, Message> {
    let resolved = timeline.filter(|timeline| !timeline.physical);
    let (x, y) = resolved.map_or((0, 0), |timeline| {
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
        (x, y)
    });
    let diagonal = x != 0 && y != 0;
    let reach = if diagonal { 13.0 } else { 18.0 };
    let active = x != 0 || y != 0;
    let dot_color = if active {
        mode_color(SocdMode::Immediate)
    } else {
        theme::ICON_MUTED
    };
    const DOT: f32 = 18.0;
    const TILE: f32 = 80.0;
    container(
        stack![
            container(icons::dashed_ring(48.0, theme::GUIDE_RING))
                .center_x(Fill)
                .center_y(Fill),
            container(
                container(space::horizontal().width(8).height(8)).style(|_| {
                    theme::dot_style(Color {
                        a: 0.6,
                        ..Color::from_rgb8(0xcb, 0xd5, 0xe1)
                    })
                })
            )
            .center_x(Fill)
            .center_y(Fill),
            container(
                container(space::horizontal().width(DOT).height(DOT))
                    .style(move |_| theme::dot_style(dot_color))
            )
            .padding(Padding {
                top: (TILE - DOT) / 2.0 + y as f32 * reach,
                left: (TILE - DOT) / 2.0 + x as f32 * reach,
                ..Padding::ZERO
            })
            .width(Fill)
            .height(Fill),
        ]
        .width(Fill)
        .height(Fill),
    )
    .width(TILE)
    .height(TILE)
    .style(|_| theme::dpad_center())
    .into()
}

fn keycap<'a>(
    direction: &Direction,
    snapshot: &'a UiSnapshot,
    duplicate: bool,
    pressed: bool,
    language: Language,
) -> Element<'a, Message> {
    let slot = direction.slot;
    let accent = direction.accent;
    let key = &snapshot.keys[key_slot_index(slot)];
    let selected = snapshot.capture_slot == Some(slot);
    let active = selected || pressed;
    let name: &str = if selected { "…" } else { &key.name };
    let length = name.chars().count();
    // The reference's normal-key letter scale.
    let size = if length <= 2 {
        18.0
    } else if length <= 4 {
        14.0
    } else {
        11.0
    };
    button(
        column![
            row![
                text(language.text(direction.label))
                    .size(10)
                    .font(theme::UI_FONT_BOLD)
                    .color(if active {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.8)
                    } else {
                        theme::ICON_MUTED
                    })
                    .width(Fill),
                icons::icon(
                    direction.arrow,
                    12.0,
                    if active { None } else { Some(accent) }
                )
            ],
            container(hover_text::label(
                name,
                size,
                theme::UI_FONT_BLACK,
                None,
                true
            ))
            .center_x(Fill)
            .center_y(Fill),
        ]
        .spacing(4),
    )
    .width(80)
    .height(80)
    .padding(10)
    .style(move |_, state| theme::keycap(state, active, duplicate, accent))
    .on_press(if selected {
        Message::CancelCapture
    } else {
        Message::Capture(slot)
    })
    .into()
}

/// The unique/duplicate assignment status, rendered in the mapping card's
/// footer below the inset (reference: bottom-right, outside the stage).
fn assignment_status<'a>(duplicates: &[bool; 4], language: Language) -> Element<'a, Message> {
    let duplicate = duplicates.contains(&true);
    row![
        icons::icon(
            if duplicate {
                Icon::Warning
            } else {
                Icon::Check
            },
            14.0,
            Some(if duplicate {
                theme::ERROR_TEXT
            } else {
                theme::GREEN_CHECK
            })
        ),
        text(language.text(if duplicate {
            "Duplicate key bindings detected."
        } else {
            "All keys uniquely assigned."
        }))
        .size(11)
        .color(if duplicate {
            theme::RED_600
        } else {
            theme::EMERALD_700
        })
    ]
    .spacing(6)
    .align_y(Center)
    .into()
}

/// One duration group in the reference's two-row grouping: a label row
/// whose right side is a pill holding both numeric editors, with the
/// two-handle rail directly below. Mounted only in modes that use it, so
/// it is always editable; `update` still gates stray slider drags.
fn duration_range<'a>(
    minimum: TimingField,
    maximum: TimingField,
    label: &'static str,
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
    accent: Color,
) -> Element<'a, Message> {
    let min = minimum.micros(timing).expect("duration field") as f32 / 1000.0;
    let max = maximum.micros(timing).expect("duration field") as f32 / 1000.0;
    let invalid = minimum.pair_invalid(timing);
    // Reference inputs are `w-8` sans-bold at `text-xs`; the pill owns the
    // chrome, so the boxes stay borderless and transparent over it.
    let editor = |field: TimingField| {
        value_box(
            field,
            inputs.buffer(field),
            editing[field.index()],
            invalid || parse_ms_text(inputs.buffer(field)).is_none(),
            32.0,
            accent,
        )
    };
    container(
        column![
            row![
                dot(accent),
                text(label).size(12).font(theme::UI_FONT_BOLD),
                space::horizontal().width(Fill),
                container(
                    row![
                        editor(minimum),
                        text("~").size(12).color(accent),
                        editor(maximum),
                        text("ms").size(12).color(accent),
                    ]
                    .spacing(4)
                    .align_y(Center),
                )
                .padding([1, 6])
                .style(move |_| theme::pill_style(invalid)),
            ]
            .spacing(8)
            .align_y(Center),
            widgets::range_slider(
                min,
                max,
                if minimum == TimingField::PreservedMinimum {
                    0.1
                } else {
                    0.0
                },
                true,
                accent,
                move |is_min, value| {
                    Message::TimingSliderChanged(if is_min { minimum } else { maximum }, value)
                }
            ),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Fill)
    .style(|_| theme::slot_style())
    .into()
}

/// One keycap chip in a profile slot card. The reference draws this as a plain
/// `<kbd>` holding the key's label -- no arrow glyph -- which is why its chip is
/// ~19px wide where the arrow-plus-name version came to ~41px and pushed the
/// card past the width the panel is measured against. The wire supplies display
/// names for the current mapping only, so other physical keys still show their
/// explicit SC:xx or E0:xx scan code; long labels scroll on hover instead of
/// using a tooltip.
fn profile_chip<'a>(
    physical: crate::core::PhysicalKey,
    snapshot: &'a UiSnapshot,
) -> Element<'a, Message> {
    let name = snapshot
        .keys
        .iter()
        .find(|key| key.physical == physical)
        .map(|key| key.name.clone())
        .unwrap_or_else(|| {
            format!(
                "{}{:02X}",
                if physical.extended { "E0:" } else { "SC:" },
                physical.scan_code
            )
        });
    container(
        text(name)
            .size(10.0)
            .font(theme::UI_FONT_BOLD)
            .color(theme::CHIP_TEXT),
    )
    // The reference gets its 16px chip from a `leading-none` line plus `py-0.5`,
    // but iced's default line box for the same 10px label is taller than the
    // line itself, so fixing the box and centring the line inside it reproduces
    // the drawn size without clipping the glyphs.
    .height(theme::CHIP_HEIGHT)
    .padding(theme::CHIP_PADDING)
    .align_y(Center)
    .style(|_| theme::chip_style())
    .into()
}

/// Mode picker for the timing card. The four modes are mutually exclusive and
/// each is selectable directly, so no ordering between switches can leave the
/// card in a state the engine treats as a fifth behavior.
fn mode_selector(selected: SocdMode, language: Language) -> Element<'static, Message> {
    let mut segments = row![].spacing(4);
    for mode in SocdMode::ALL {
        // 12px bold at iced's default 1.3 line height is the reference's 16px
        // line, which is what makes a segment 28px rather than 26px tall.
        let segment = button(
            text(mode_label(mode, language))
                .size(12)
                .font(theme::UI_FONT_BOLD)
                .width(Fill)
                .align_x(Alignment::Center),
        )
        .width(Fill)
        .padding(theme::MODE_PADDING)
        .on_press(Message::ModeSelected(mode));
        segments = segments
            .push(segment.style(move |_, status| theme::mode_button(status, mode == selected)));
    }
    container(segments)
        .padding(4)
        .width(Fill)
        .style(|_theme| theme::group_style())
        .into()
}

/// Split between the two delays for Random Mix, shown as one
/// `press : release` pill on the title row. The stored rate is the
/// release-delay share, so the press share is derived from it rather than
/// stored twice, and only the release side is editable. Mounted only in
/// Random Mix, so it is always editable.
fn rate_group<'a>(
    timing: &TimingSettings,
    inputs: &'a TimingInputs,
    editing: &[bool; 5],
    language: Language,
) -> Element<'a, Message> {
    let press_share = 100u8.saturating_sub(timing.overlap_preservation_rate);
    let invalid = parse_rate_text(&inputs.preservation_rate).is_none();
    container(
        column![
            row![
                dot(theme::MIX_TEXT),
                text(language.text("Delay Mix Ratio"))
                    .size(12)
                    .font(theme::UI_FONT_BOLD),
                space::horizontal().width(Fill),
                container(
                    row![
                        text(format_rate(press_share))
                            .size(12)
                            .font(theme::UI_FONT_BOLD)
                            .color(theme::MIX_TEXT),
                        text(":").size(12).color(theme::MIX_TEXT),
                        rate_box(
                            &inputs.preservation_rate,
                            editing[TimingField::PreservationRate.index()],
                        ),
                        text("%").size(12).color(theme::MIX_TEXT),
                    ]
                    .spacing(4)
                    .align_y(Center),
                )
                .padding([1, 6])
                .style(move |_| theme::pill_style(invalid)),
            ]
            .spacing(8)
            .align_y(Center),
            slider(1.0..=99.0, press_share as f32, Message::MixChanged)
                .step(1.0)
                .style(theme::mixer_slider),
            hover_text::body(
                language.text("Each overlap randomly picks one of the two delays below.")
            ),
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
        SocdMode::Immediate => theme::IMMEDIATE_ACCENT,
        SocdMode::PressDelay => theme::INDIGO_600,
        SocdMode::ReleaseDelay => theme::VIOLET_600,
        SocdMode::RandomMix => theme::MIX_TEXT,
    }
}

/// Mode label ink from the reference (`MODE_TEXT`): the release-delay label
/// uses the darker `text-violet-700` while the card tint keeps the lighter
/// violet. Every other mode labels in its own card color.
const fn mode_label_color(mode: SocdMode) -> Color {
    match mode {
        SocdMode::ReleaseDelay => theme::RELEASE_LABEL,
        mode => mode_color(mode),
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

/// The "How it works" block: the selected mode explained as numbered steps,
/// with the configured range embedded in the wait step and the mode's own
/// steps highlighted in its accent. Never mounted in Random Mix (the caller
/// gates it), where the ratio and both groups already fill the card.
fn mechanism_steps(
    mode: SocdMode,
    timing: &TimingSettings,
    language: Language,
) -> Element<'static, Message> {
    let range = |min_micros: u32, max_micros: u32| {
        format!(
            "{:.1}~{:.1} ms",
            min_micros as f32 / 1_000.0,
            max_micros as f32 / 1_000.0
        )
    };
    let step = |key: &str| language.text(key).to_owned();
    let step_with_range = |key: &str, range: String| language.text(key).replace("{range}", &range);
    let steps: Vec<(String, bool)> = match mode {
        SocdMode::Immediate => vec![
            (step("Detect an opposite-direction overlap."), false),
            (step("Release the previous key output immediately."), false),
            (step("Send the new key immediately. 0 ms added delay"), true),
        ],
        SocdMode::PressDelay => vec![
            (step("Detect an opposite-direction overlap."), false),
            (step("Release the previous key output immediately."), false),
            (
                step_with_range(
                    "Wait a random time within {range}. This gap sends no input",
                    range(
                        timing.socd_transition_min_micros,
                        timing.socd_transition_max_micros,
                    ),
                ),
                true,
            ),
            (step("Send the new key once the wait ends."), true),
        ],
        SocdMode::ReleaseDelay => vec![
            (step("Detect an opposite-direction overlap."), false),
            (step("Send the new key immediately."), false),
            (
                step_with_range(
                    "Wait a random time within {range}. The overlap stays live",
                    range(
                        timing.preserved_overlap_min_micros,
                        timing.preserved_overlap_max_micros,
                    ),
                ),
                true,
            ),
            (
                step("Release the previous key output once the wait ends."),
                true,
            ),
        ],
        SocdMode::RandomMix => vec![],
    };
    let accent = mode_color(mode);
    container(
        column![
            row![
                dot(theme::MUTED_TEXT),
                text(language.text("How it works"))
                    .size(11)
                    .font(theme::UI_FONT_BOLD),
            ]
            .spacing(6)
            .align_y(Center),
            column(
                steps
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, accented))| {
                        row![
                            container(
                                text(format!("{}", index + 1))
                                    .size(9)
                                    .font(theme::UI_FONT_BOLD)
                            )
                            .center_x(16)
                            .center_y(16)
                            .style(move |_| {
                                theme::step_badge(if accented { Some(accent) } else { None })
                            }),
                            text(label).size(11).color(theme::MUTED_TEXT),
                        ]
                        .spacing(8)
                        .align_y(Center)
                        .into()
                    })
            )
            .spacing(6),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Fill)
    .style(|_| theme::slot_style())
    .into()
}

/// The release-share editor in the mix pill. Out-of-range numbers clamp
/// into 1-100 on commit, so only genuinely unparseable text is invalid.
/// Sized like the duration editors (`w-8` in the reference).
fn rate_box<'a>(buffer: &'a str, editing: bool) -> Element<'a, Message> {
    value_box(
        TimingField::PreservationRate,
        buffer,
        editing,
        parse_rate_text(buffer).is_none(),
        32.0,
        theme::RELEASE_TEXT,
    )
}

/// The press-to-edit value box shared by the duration pills and the rate
/// box: a facade until the user activates it, then the live input, already
/// focused and selected. Any fix to the swap (focus ordering, selection,
/// styling) lands here once instead of drifting between two copies.
fn value_box<'a>(
    field: TimingField,
    buffer: &'a str,
    editing: bool,
    invalid: bool,
    width: f32,
    accent: Color,
) -> Element<'a, Message> {
    if !editing {
        return value_facade(buffer, field, width, invalid, accent);
    }
    text_input("", buffer)
        .font(theme::UI_FONT_BOLD)
        .size(12.0)
        .align_x(Alignment::Center)
        .padding(VALUE_BOX_PADDING)
        .style(move |theme_, status| theme::value_input(theme_, status, accent, invalid))
        .id(value_box_id(field))
        .width(Length::Fixed(width))
        .on_input(move |input| Message::TimingTextChanged(field, input))
        .on_submit(Message::TimingTextSubmitted(field))
        .into()
}

/// Press-to-edit lookalike for an untouched value box. It mirrors the live
/// box visuals so the swap is invisible; buttons report presses on release,
/// after any drag settled, which pairs with focusing and selecting below.
fn value_facade<'a>(
    buffer: &'a str,
    field: TimingField,
    width: f32,
    invalid: bool,
    accent: Color,
) -> Element<'a, Message> {
    button(
        text(buffer)
            .font(theme::UI_FONT_BOLD)
            .size(12.0)
            .width(Fill)
            .align_x(Alignment::Center),
    )
    .style(move |theme_, status| theme::facade_button(theme_, status, accent, invalid))
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

fn stat_box(label: &'static str, value: String, value_color: Color) -> Element<'static, Message> {
    let number: Element<'static, Message> = text(value)
        .font(theme::UI_FONT_BOLD)
        .size(19.0)
        .color(value_color)
        .into();
    let bottom: Element<'static, Message> = row![space::horizontal(), number, space::horizontal()]
        .align_y(Center)
        .width(Fill)
        .into();
    container(
        column![
            hover_text::label(
                label,
                11.0,
                theme::UI_FONT_BOLD,
                Some(theme::BODY_TEXT),
                true,
            ),
            space::vertical(),
            bottom,
        ]
        .height(Fill)
        .align_x(Alignment::Center),
    )
    .padding(Padding {
        top: 12.0,
        right: 10.0,
        bottom: 12.0,
        left: 10.0,
    })
    .width(Fill)
    .height(Length::Fixed(76.0))
    .style(|_theme| theme::group_style())
    .into()
}

/// One recommendation tile in the suggestions card: the delay name and its
/// hint stacked on the left, the value hugging the right edge (reference:
/// the hint lives inside the tile). The monospace value gets the same
/// one-sided optical nudge as the value boxes, plus a right inset so it
/// never touches the tile edge.
fn suggestion_tile<'a>(
    label: &'static str,
    hint: &'static str,
    value: String,
    available: bool,
    language: Language,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(language.text(label))
                    .size(12)
                    .font(theme::UI_FONT_BOLD)
                    .color(theme::BODY_TEXT),
                text(language.text(hint)).size(11).color(theme::MUTED_TEXT),
            ]
            .spacing(2)
            .width(Fill),
            container(
                text(value)
                    .font(theme::UI_FONT_BOLD)
                    .size(if available { 17.0 } else { 13.0 })
                    .color(if available {
                        theme::PRIMARY_TEXT
                    } else {
                        theme::ICON_MUTED
                    })
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
    .padding(14)
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
            text(language.text("Updates live while measuring"))
                .size(12)
                .color(theme::MUTED_TEXT),
            container(
                column![
                    table_row([
                        heading(
                            language.text("INPUT PATTERN"),
                            Length::Fixed(PATTERN_COLUMN),
                            Alignment::Left,
                        ),
                        heading(
                            language.text("SAMPLES"),
                            Length::Fixed(SAMPLES_COLUMN),
                            Alignment::Right,
                        ),
                        heading("P10", Fill, Alignment::Right),
                        heading("P50", Fill, Alignment::Right),
                        heading("P90", Fill, Alignment::Right),
                        heading(language.text("MIN"), Fill, Alignment::Right),
                        heading(language.text("MAX"), Fill, Alignment::Right),
                    ]),
                    table_hrule(),
                    table_row(pattern_figures(
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
                    )),
                    table_hrule(),
                    table_row(pattern_figures(
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
                    )),
                    table_hrule(),
                    // The indistinguishable row carries its explanation in
                    // place of the figures, like the reference's colspan.
                    row![
                        row![
                            dot(theme::RED_500),
                            text(language.text("Indistinguishable"))
                                .font(theme::UI_FONT_BOLD)
                                .color(theme::BODY_TEXT)
                        ]
                        .spacing(6)
                        .align_y(Center)
                        .width(Length::Fixed(PATTERN_COLUMN)),
                        figure(
                            measurement.near_simultaneous_count.to_string(),
                            theme::BODY_TEXT,
                            Length::Fixed(SAMPLES_COLUMN),
                            false,
                        ),
                        text(
                            language
                                .text("Unclear input order (<1 ms), excluded from timing ranges.")
                        )
                        .size(11)
                        .color(theme::MUTED_TEXT)
                        .width(Fill)
                        .align_x(Alignment::Right),
                    ]
                    .spacing(6)
                    .align_y(Center)
                    .width(Fill),
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
    let has_suggestions =
        measurement.recommended_transition.is_some() || measurement.recommended_overlap.is_some();
    let apply_button = button(trailing_icon_label(
        Icon::ArrowForward,
        "Apply suggestions",
        language,
    ))
    .style(theme::primary_button);
    let apply_action = if has_suggestions {
        apply_button.on_press(Message::ApplyRecommendations)
    } else {
        apply_button
    };

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
                apply_action,
            ]
            .align_y(Center),
            row![
                suggestion_tile(
                    language.text("New Key Press Delay"),
                    language.text("Based on neutral transitions"),
                    timing_range(measurement.recommended_transition),
                    measurement.recommended_transition.is_some(),
                    language,
                ),
                suggestion_tile(
                    language.text("Previous Key Release Delay"),
                    language.text("Based on physical overlaps"),
                    timing_range(measurement.recommended_overlap),
                    measurement.recommended_overlap.is_some(),
                    language,
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
const SAMPLES_COLUMN: f32 = 70.0;

/// Table header shares the card title color instead of the muted tone.
fn heading(label: &'static str, width: Length, align: Alignment) -> Element<'static, Message> {
    text(label)
        .size(11)
        .font(theme::UI_FONT_BOLD)
        .color(theme::BODY_TEXT)
        .width(width)
        .align_x(align)
        .into()
}

/// One table row: seven cells sharing the same widths and spacing, so
/// columns line up down the table.
fn table_row(cells: [Element<'static, Message>; 7]) -> Element<'static, Message> {
    let [pattern, samples, p10, p50, p90, minimum, maximum] = cells;
    row![pattern, samples, p10, p50, p90, minimum, maximum]
        .spacing(6)
        .align_y(Center)
        .width(Fill)
        .into()
}

/// Pattern label plus sample count plus P10 / P50 / P90 / minimum /
/// maximum figures, in table-cell order; P50 is the highlighted column.
fn pattern_figures(
    label: &'static str,
    color: Color,
    count: u32,
    figures: [(String, bool); 5],
) -> [Element<'static, Message>; 7] {
    let [
        (p10, has_p10),
        (p50, has_p50),
        (p90, has_p90),
        (minimum, has_min),
        (maximum, has_max),
    ] = figures;
    let base_color = theme::BODY_TEXT;
    [
        row![
            dot(color),
            text(label)
                .font(theme::UI_FONT_BOLD)
                .color(theme::BODY_TEXT)
        ]
        .spacing(6)
        .align_y(Center)
        .width(Length::Fixed(PATTERN_COLUMN))
        .into(),
        figure(
            count.to_string(),
            base_color,
            Length::Fixed(SAMPLES_COLUMN),
            false,
        ),
        figure(
            p10,
            if has_p10 {
                base_color
            } else {
                theme::MUTED_TEXT
            },
            Length::Fill,
            false,
        ),
        // The P50 column is highlighted in the row's own accent and bold.
        figure(
            p50,
            if has_p50 { color } else { theme::MUTED_TEXT },
            Length::Fill,
            has_p50,
        ),
        figure(
            p90,
            if has_p90 {
                base_color
            } else {
                theme::MUTED_TEXT
            },
            Length::Fill,
            false,
        ),
        figure(
            minimum,
            if has_min {
                base_color
            } else {
                theme::MUTED_TEXT
            },
            Length::Fill,
            false,
        ),
        figure(
            maximum,
            if has_max {
                base_color
            } else {
                theme::MUTED_TEXT
            },
            Length::Fill,
            false,
        ),
    ]
}

fn table_hrule() -> Element<'static, Message> {
    rule::horizontal(1).style(theme::table_rule).into()
}

/// Numeric table cell: right-aligned so decimal places line up. The pattern
/// label column stays left-aligned.
fn figure(value: String, color: Color, width: Length, bold: bool) -> Element<'static, Message> {
    text(value)
        .size(12)
        .font(if bold {
            theme::UI_FONT_BOLD
        } else {
            theme::MONO_FONT
        })
        .color(color)
        .width(width)
        .align_x(Alignment::Right)
        .into()
}

fn duration_stat(micros: Option<u64>) -> (String, bool) {
    micros.map_or_else(
        || ("-".into(), false),
        |value| (format!("{:.1} ms", value as f64 / 1_000.0), true),
    )
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
        "-".into()
    } else {
        format!("{:.1}%", f64::from(count) * 100.0 / f64::from(total))
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
    fn drags_for_a_hidden_group_leave_the_draft_untouched() {
        // A drag queued before the mode switch arrives after its slider is
        // unmounted; the update arm must drop it so the hidden value stays put.
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
