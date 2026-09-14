//! The settings window's state and its one transition function. No egui, no
//! IPC, no file access: `update` takes a message, mutates state, and returns
//! the side effects for the caller to perform. That is what makes it testable
//! without a window.

use std::time::Instant;

use crate::{
    core::MIN_RECOMMENDATION_SAMPLES,
    protocol::{KeySlot, UiCommand, UiEvent, UiSnapshot, UiView},
    settings::{Settings, SocdMode, TimingSettings},
};

use super::{
    language::Language,
    message::{IpcEvent, KeyPress, Message, TimingField},
    preview::Preview,
    timeline::{MonitorState, Timeline},
};

/// Parse failure reported when a value box cannot commit its text. Kept as a
/// constant so a later successful submit only clears an error it produced
/// itself, leaving server errors on screen until the next snapshot.
pub const INVALID_TIMING_TEXT: &str = "Invalid timing value; reverted to the current draft.";

/// A side effect `update` asks the caller to perform. Everything the Iced
/// version expressed as a `Task` lands here, so the state layer stays free of
/// both the runtime and the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    /// Send a command over the IPC connection.
    Send(UiCommand),
    /// Put the caret in the profile-name box with its whole value selected.
    FocusProfileName,
    /// Same, for a timing value box.
    FocusValueBox(TimingField),
    /// Scroll the single page to a section, optionally raising the window.
    ShowSection { view: UiView, focus: bool },
    /// Let the IPC pump sleep while the window is deactivated.
    PumpAwake(bool),
    /// The runtime is going away; close the window.
    Close,
}

/// Editable numeric buffers shadowing the timing draft. Sliders write straight
/// through to the draft; typed text commits on submit so partial input such as
/// an empty field never corrupts the draft mid-keystroke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimingInputs {
    pub transition_minimum: String,
    pub transition_maximum: String,
    pub preservation_rate: String,
    pub preserved_minimum: String,
    pub preserved_maximum: String,
}

impl TimingInputs {
    pub fn from_timing(timing: &TimingSettings) -> Self {
        Self {
            transition_minimum: format_ms(timing.socd_transition_min_micros),
            transition_maximum: format_ms(timing.socd_transition_max_micros),
            preservation_rate: format_rate(timing.overlap_preservation_rate),
            preserved_minimum: format_ms(timing.preserved_overlap_min_micros),
            preserved_maximum: format_ms(timing.preserved_overlap_max_micros),
        }
    }

    pub fn set_field(&mut self, field: TimingField, value: String) {
        *self.buffer_mut(field) = value;
    }

    pub fn buffer_mut(&mut self, field: TimingField) -> &mut String {
        match field {
            TimingField::TransitionMinimum => &mut self.transition_minimum,
            TimingField::TransitionMaximum => &mut self.transition_maximum,
            TimingField::PreservationRate => &mut self.preservation_rate,
            TimingField::PreservedMinimum => &mut self.preserved_minimum,
            TimingField::PreservedMaximum => &mut self.preserved_maximum,
        }
    }

    pub fn buffer(&self, field: TimingField) -> &str {
        match field {
            TimingField::TransitionMinimum => &self.transition_minimum,
            TimingField::TransitionMaximum => &self.transition_maximum,
            TimingField::PreservationRate => &self.preservation_rate,
            TimingField::PreservedMinimum => &self.preserved_minimum,
            TimingField::PreservedMaximum => &self.preserved_maximum,
        }
    }
}

#[derive(Default)]
pub enum ProfileDialog {
    #[default]
    Closed,
    List,
    Languages,
    Confirm(u8),
    Rename {
        slot: u8,
        name: String,
    },
    /// The rename is dispatched and the server has not answered yet. The slot
    /// and the typed name travel with the state so the card can keep showing
    /// what the user typed: the reference writes synchronously and so never has
    /// an intermediate value to show, but a pipe round trip is at least one
    /// frame, and rendering the stored name for that frame reads as the box
    /// snapping backwards before it settles forward again.
    Renaming {
        slot: u8,
        name: String,
    },
    Loading,
}

/// What an empty rename box does when its edit ends. The reference drops a
/// blank name rather than saving it, so the stored name always survives; these
/// are the two things that can then happen to the box itself.
#[derive(Clone, Copy, Debug)]
enum EmptyName {
    /// Go back to the field so the restored name can be retyped. Used when the
    /// edit ended because the press left the field but not the panel.
    Reopen,
    /// Close the box, leaving the card at rest on the stored name. Used when the
    /// edit ended at the field's own control -- Enter, or the rename pencil.
    Close,
}

/// The card's interaction state, not just whether it is loaded: the reference
/// paints three card classes and hover is the whole-card emphasis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotState {
    Idle,
    Hovered,
    Active,
}

pub struct State {
    /// Whether an IPC connection exists. The handle itself stays in the view
    /// layer, which is what keeps this module transport-free.
    pub connected: bool,
    pub monitor: MonitorState,
    pub preview: Preview,
    pub pending_filter: Option<bool>,
    pub profiles: ProfileDialog,
    /// The slot card the pointer is over, or `None`. Only one card can be
    /// under the pointer, so it is one value rather than a flag per slot.
    pub hovered_slot: Option<u8>,
    /// The name box the pointer is over, or `None`. It is the box's own hover
    /// rather than the card's, so pointing at the keycaps leaves it at rest.
    pub hovered_name: Option<u8>,
    pub language: Language,
    pub snapshot: Option<UiSnapshot>,
    pub draft: Option<Settings>,
    pub inputs: TimingInputs,
    /// Defer launch/focus navigation until the first snapshot mounts the body.
    pub pending_section: Option<UiView>,
    /// Whether each value box is the one being edited. Indexed by field
    /// discriminant. Any message that moves focus elsewhere rearms every box,
    /// so the next press selects all again, Explorer-style.
    pub editing: [bool; 5],
    pub session_details_open: bool,
    pub pressed_keys: [bool; 4],
    pub press_timestamps: [Option<Instant>; 4],
    /// Whether the settings window holds focus. A deactivated window animates
    /// nothing and lets its IPC pump sleep; the filter engine is a separate
    /// process and keeps running either way.
    pub focused: bool,
    pub status: String,
    /// Success notice shown as a toast until the next server snapshot.
    pub notice: Option<String>,
    pub error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            connected: false,
            monitor: MonitorState::Stopped,
            preview: Preview::default(),
            pending_filter: None,
            profiles: ProfileDialog::Closed,
            hovered_slot: None,
            hovered_name: None,
            language: Language::default(),
            snapshot: None,
            draft: None,
            inputs: TimingInputs::default(),
            pending_section: Some(requested_view()),
            editing: [false; 5],
            session_details_open: false,
            pressed_keys: [false; 4],
            press_timestamps: [None; 4],
            focused: true,
            status: "Connecting to the LastKey runtime...".into(),
            notice: None,
            error: None,
        }
    }
}

impl State {
    pub fn is_dirty(&self) -> bool {
        self.snapshot
            .as_ref()
            .zip(self.draft.as_ref())
            .is_some_and(|(snapshot, draft)| {
                draft != &snapshot.saved || self.inputs != TimingInputs::from_timing(&draft.timing)
            })
    }

    pub fn slot_state(&self, slot: u8, active_slot: u8) -> SlotState {
        if slot == active_slot {
            SlotState::Active
        } else if self.hovered_slot == Some(slot) {
            SlotState::Hovered
        } else {
            SlotState::Idle
        }
    }

    /// The name the server last reported for a slot.
    pub fn stored_profile_name(&self, slot: u8) -> Option<String> {
        self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .saved
                .profile_bank()
                .slots
                .get(usize::from(slot))
                .map(|profile| profile.name.clone())
        })
    }
}

/// The one transition function. Returns the side effects to perform, in order.
pub fn update(state: &mut State, message: Message) -> Vec<Effect> {
    if !matches!(state.profiles, ProfileDialog::Closed)
        && !matches!(
            &message,
            Message::Ipc(_)
                | Message::OpenLanguages
                | Message::SelectLanguage(_)
                | Message::OpenProfiles
                | Message::CloseProfiles
                | Message::SlotHovered(_)
                | Message::SlotUnhovered(_)
                | Message::NameHovered(..)
                | Message::LoadProfile(_)
                | Message::ConfirmProfile(_)
                | Message::EditProfileName(_)
                | Message::ProfileNameChanged(_)
                | Message::SaveProfileName
                | Message::SaveProfileNameIfEditing
                | Message::CancelCapture
                | Message::WindowFocused
                | Message::WindowUnfocused
        )
    {
        return Vec::new();
    }
    track_box_focus(state, &message);
    match message {
        Message::Preview(action) => state.preview.update(action),
        Message::Ipc(IpcEvent::Connected) => {
            state.connected = true;
            state.status = "Connected to the LastKey runtime.".into();
            state.error = None;
            // A reconnect while deactivated starts asleep, not awake.
            return vec![
                Effect::PumpAwake(state.focused),
                Effect::Send(UiCommand::RequestSnapshot),
            ];
        }
        Message::Ipc(IpcEvent::Message(event)) => return handle_event(state, *event),
        Message::Ipc(IpcEvent::Disconnected(error)) => {
            state.connected = false;
            state.monitor = MonitorState::Stopped;
            state.pending_filter = None;
            state.profiles = ProfileDialog::Closed;
            state.status = "The LastKey runtime is disconnected.".into();
            state.error = Some(error);
        }
        Message::OpenLanguages => state.profiles = ProfileDialog::Languages,
        Message::SelectLanguage(language) => {
            state.language = language;
            state.profiles = ProfileDialog::Closed;
        }
        Message::OpenProfiles => state.profiles = ProfileDialog::List,
        Message::CloseProfiles => {
            // Only a load holds the panel open. A rename in flight does not:
            // its command is already dispatched and the answer only lands a
            // notice, so the close works the frame it is pressed.
            if !matches!(state.profiles, ProfileDialog::Loading) {
                // Leaving the panel is still "somewhere else", and ends an
                // open edit by saving it. Only Escape and a window
                // deactivation drop it.
                let commit = commit_profile_name(state, EmptyName::Close);
                state.profiles = ProfileDialog::Closed;
                return commit;
            }
        }
        // Only one card can be under the pointer, so entering a card replaces
        // whatever was hovered and exiting only clears the card that is still
        // current -- an exit delivered after a later enter must not cancel it.
        Message::SlotHovered(slot) => state.hovered_slot = Some(slot),
        Message::SlotUnhovered(slot) => {
            if state.hovered_slot == Some(slot) {
                state.hovered_slot = None;
            }
        }
        Message::NameHovered(slot, hovered) => {
            state.hovered_name = if hovered { Some(slot) } else { None };
        }
        Message::LoadProfile(slot) => return load_or_confirm(state, slot),
        Message::ConfirmProfile(slot) => return load_profile(state, slot),
        Message::EditProfileName(slot) => {
            let Some(profile) = state.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .saved
                    .profile_bank()
                    .slots
                    .get(usize::from(slot))
                    .cloned()
            }) else {
                return Vec::new();
            };
            state.profiles = ProfileDialog::Rename {
                slot,
                name: profile.name,
            };
            // Focus and select together so the box arrives ready to overwrite
            // the old name: a caret at the end would append instead.
            return vec![Effect::FocusProfileName];
        }
        Message::ProfileNameChanged(value) => {
            if let ProfileDialog::Rename { name, .. } = &mut state.profiles {
                *name = value;
            }
        }
        Message::SaveProfileName => return commit_profile_name(state, EmptyName::Close),
        Message::SaveProfileNameIfEditing => return commit_profile_name(state, EmptyName::Reopen),
        Message::ToggleFilter => {
            if let Some(snapshot) = &state.snapshot
                && state.pending_filter.is_none()
                && state.connected
            {
                let enabled = !snapshot.filter_enabled;
                state.pending_filter = Some(enabled);
                return vec![Effect::Send(UiCommand::SetFilterEnabled(enabled))];
            }
        }
        Message::ToggleMonitor => {
            if !state.connected {
                return Vec::new();
            }
            let (monitor, effects) = match std::mem::take(&mut state.monitor) {
                MonitorState::Stopped => (
                    MonitorState::Starting,
                    vec![Effect::Send(UiCommand::StartMonitor)],
                ),
                MonitorState::Recording(timeline) => (
                    MonitorState::Stopping(timeline),
                    vec![Effect::Send(UiCommand::StopMonitor)],
                ),
                pending => (pending, Vec::new()),
            };
            state.monitor = monitor;
            return effects;
        }
        Message::CancelCapture => {
            state.pressed_keys = [false; 4];
            state.press_timestamps = [None; 4];
            // Escape unwinds one layer at a time: a rename or confirm back to
            // the slot list, the list or language menu to closed. In-flight
            // loads and renames finish first, matching the close button.
            match &state.profiles {
                ProfileDialog::Rename { .. } | ProfileDialog::Confirm(_) => {
                    state.profiles = ProfileDialog::List;
                }
                ProfileDialog::List | ProfileDialog::Languages => {
                    state.profiles = ProfileDialog::Closed;
                }
                ProfileDialog::Loading | ProfileDialog::Renaming { .. } => {}
                ProfileDialog::Closed => {
                    if state
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.capture_slot.is_some())
                    {
                        return vec![Effect::Send(UiCommand::CancelKeyCapture)];
                    }
                }
            }
        }
        Message::ResetMeasurement => {
            state.session_details_open = false;
            return vec![Effect::Send(UiCommand::ResetMeasurement)];
        }
        Message::RequestSnapshot => return vec![Effect::Send(UiCommand::RequestSnapshot)],
        Message::Capture(slot) => {
            state.pressed_keys = [false; 4];
            state.press_timestamps = [None; 4];
            // Timing stays local until Apply; the answering Snapshot merges
            // instead of replacing it (see set_snapshot).
            return vec![Effect::Send(UiCommand::BeginKeyCapture(slot))];
        }
        Message::MixChanged(press_share) => {
            if let Some(draft) = state.draft.as_mut()
                && draft.timing.mode == SocdMode::RandomMix
            {
                draft.timing.overlap_preservation_rate =
                    100 - press_share.round().clamp(1.0, 99.0) as u8;
                state.inputs.preservation_rate =
                    format_rate(draft.timing.overlap_preservation_rate);
            }
        }
        Message::ModeSelected(mode) => {
            if let Some(draft) = state.draft.as_mut() {
                draft.timing.mode = mode;
            }
        }
        Message::TimingSliderChanged(field, milliseconds) => {
            // A drag queued before the mode switched still arrives after its
            // slider unmounts, so the gate lives here too.
            if let Some(draft) = state.draft.as_mut()
                && field.is_editable(&draft.timing)
                && let Some(slot) = field.micros_mut(&mut draft.timing)
            {
                let micros = millis_to_micros(milliseconds);
                *slot = micros;
                state.inputs.set_field(field, format_ms(micros));
            }
        }
        Message::TimingTextChanged(field, value) => state.inputs.set_field(field, value),
        Message::TimingTextSubmitted(field) => {
            if !commit_field(state, field) {
                state.error = Some(INVALID_TIMING_TEXT.into());
            } else if state.error.as_deref() == Some(INVALID_TIMING_TEXT) {
                state.error = None;
            }
        }
        Message::ValueBoxActivated(field) => {
            // Flag bookkeeping already ran in `track_box_focus`; this only
            // reveals the box with its whole value selected.
            return vec![Effect::FocusValueBox(field)];
        }
        Message::WindowFocused => {
            state.focused = true;
            return vec![Effect::PumpAwake(true)];
        }
        Message::WindowUnfocused => {
            state.focused = false;
            state.pressed_keys = [false; 4];
            state.press_timestamps = [None; 4];
            // Deactivating the window ends an edit in flight the way Escape
            // does: the box returns to the name it had and nothing is sent.
            // The pump sleeps, not the engine: filtering lives in the runtime
            // process and is untouched by this.
            let mut effects = vec![Effect::PumpAwake(false)];
            if cancel_profile_name(state) {
                effects.push(Effect::FocusProfileName);
            }
            return effects;
        }
        Message::KeyboardPressed(press) => {
            if matches!(state.profiles, ProfileDialog::Rename { .. }) {
                return Vec::new();
            }
            if let Some(snapshot) = &state.snapshot {
                if snapshot.capture_slot.is_some() {
                    return Vec::new();
                }
                for (i, display_key) in snapshot.keys.iter().enumerate() {
                    if matches_key(&press, &display_key.name) && !state.pressed_keys[i] {
                        state.pressed_keys[i] = true;
                        state.press_timestamps[i] = Some(Instant::now());
                    }
                }
            }
        }
        Message::KeyboardReleased(press) => {
            if let Some(snapshot) = &state.snapshot {
                for (i, display_key) in snapshot.keys.iter().enumerate() {
                    if matches_key(&press, &display_key.name) {
                        state.pressed_keys[i] = false;
                        state.press_timestamps[i] = None;
                    }
                }
            }
        }
        Message::Apply => {
            // Gate order is explicit: typed text, then local rules, then IPC.
            // The server remains the authoritative gate; this only avoids a
            // round trip for failures we can already name.
            if !commit_inputs(state) {
                state.error = Some(INVALID_TIMING_TEXT.into());
                return Vec::new();
            }
            let Some(draft) = state.draft.clone() else {
                return Vec::new();
            };
            if let Err(error) = draft.validate() {
                state.error = Some(error.to_string());
                return Vec::new();
            }
            state.status = "Applying settings...".into();
            return vec![
                Effect::Send(UiCommand::UpdateDraft(draft)),
                Effect::Send(UiCommand::Apply),
            ];
        }
        Message::Revert => {
            // Timing is locally authoritative: reset now instead of waiting
            // for the reply, which may arrive behind older snapshots that
            // must not undo it.
            if let Some(saved) = state.snapshot.as_ref().map(|s| s.saved.clone()) {
                state.inputs = TimingInputs::from_timing(&saved.timing);
                state.draft = Some(saved);
            }
            return vec![Effect::Send(UiCommand::Revert)];
        }
        Message::RestoreMappingDefaults => {
            return vec![Effect::Send(UiCommand::RestoreMappingDefaults)];
        }
        Message::RestoreTimingDefaults => {
            // No dedicated server command exists, so the defaults ride the
            // regular draft path: bindings stay, timing resets, and the
            // answering Snapshot keeps the local timing until Apply persists.
            let defaults = TimingSettings::default();
            state.inputs = TimingInputs::from_timing(&defaults);
            if let Some(draft) = state.draft.as_mut() {
                draft.timing = defaults;
            }
            return state
                .draft
                .clone()
                .map(|draft| vec![Effect::Send(UiCommand::UpdateDraft(draft))])
                .unwrap_or_default();
        }
        Message::RestoreAllDefaults => {
            let defaults = Settings {
                profiles: state
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.profiles.clone()),
                ..Settings::default()
            };
            state.inputs = TimingInputs::from_timing(&defaults.timing);
            state.draft = Some(defaults);
            return vec![Effect::Send(UiCommand::RestoreAllDefaults)];
        }
        Message::ToggleMeasurement => {
            state.session_details_open = true;
            let active = state
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.measurement_active);
            return vec![Effect::Send(if active {
                UiCommand::StopMeasurement
            } else {
                UiCommand::StartMeasurement
            })];
        }
        Message::ApplyRecommendations => return apply_recommendations(state),
    }
    Vec::new()
}

/// Rearms value-box facades around focus moves. Typing, scrolling, and IPC
/// traffic leave focus alone; activating one box arms it and disarms the rest,
/// while every other message disarms all of them. The next press on a disarmed
/// box therefore selects all again, Explorer-style.
fn track_box_focus(state: &mut State, message: &Message) {
    use super::message::PreviewAction;

    match message {
        Message::TimingTextChanged(..)
        | Message::TimingTextSubmitted(..)
        | Message::Ipc(..)
        | Message::Preview(PreviewAction::Tick) => {}
        Message::ValueBoxActivated(field) => {
            state.editing = [false; 5];
            state.editing[field.index()] = true;
        }
        _ => state.editing = [false; 5],
    }
}

/// Parses one typed buffer into the draft, refreshing the buffer from the
/// stored value. Returns false when the text was invalid and reverted.
fn commit_field(state: &mut State, field: TimingField) -> bool {
    let Some(draft) = state.draft.as_mut() else {
        return true;
    };
    if field == TimingField::PreservationRate {
        // The rate is a percentage, not a duration, so it keeps its own parse
        // and format. "Off" is a mode, not a rate of zero.
        return commit_text(
            &mut state.inputs.preservation_rate,
            &mut draft.timing.overlap_preservation_rate,
            parse_rate_text,
            format_rate,
        );
    }
    let Some(slot) = field.micros_mut(&mut draft.timing) else {
        return true;
    };
    commit_text(
        state.inputs.buffer_mut(field),
        slot,
        parse_ms_text,
        format_ms,
    )
}

fn commit_inputs(state: &mut State) -> bool {
    let mut committed = true;
    for field in TimingField::ALL {
        committed &= commit_field(state, field);
    }
    committed
}

/// Copies the measured recommendations into the local draft only. The user
/// still confirms them with Apply; nothing is committed silently.
fn apply_recommendations(state: &mut State) -> Vec<Effect> {
    let recommendation = state
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
        state.status =
            format!("Collect at least {MIN_RECOMMENDATION_SAMPLES} samples for recommendations.");
        return Vec::new();
    }
    if let Some(draft) = state.draft.as_mut() {
        if let Some(range) = recommendation.0 {
            draft.timing.socd_transition_min_micros = range.min_micros;
            draft.timing.socd_transition_max_micros = range.max_micros;
        }
        if let Some(range) = recommendation.1 {
            draft.timing.preserved_overlap_min_micros = range.min_micros;
            draft.timing.preserved_overlap_max_micros = range.max_micros;
        }
    }
    if let Some(draft) = state.draft.as_ref() {
        state.inputs = TimingInputs::from_timing(&draft.timing);
    }
    // No push here: the answering Snapshot would overwrite the guidance below
    // with "synchronized with the runtime", which reads as already active.
    state.notice = Some("Recommendations written to the draft. Select Apply when ready.".into());
    show_section(state, UiView::Settings, false)
}

fn handle_event(state: &mut State, event: UiEvent) -> Vec<Effect> {
    match event {
        UiEvent::ProfileLoaded(snapshot) => {
            state.draft = None;
            set_snapshot(state, snapshot);
            state.profiles = ProfileDialog::Closed;
            state.notice = Some("Profile loaded and activated.".into());
            state.error = None;
        }
        UiEvent::FilterChanged(enabled) => {
            if let Some(snapshot) = state.snapshot.as_mut() {
                snapshot.filter_enabled = enabled;
            }
            state.pending_filter = None;
            state.monitor.resynchronize();
        }
        UiEvent::MonitorStateChanged(active) => {
            state.monitor = if active {
                MonitorState::Recording(Timeline::default())
            } else {
                MonitorState::Stopped
            };
        }
        UiEvent::MonitorUpdated(event) => {
            if let MonitorState::Recording(timeline) = &mut state.monitor
                && let Some(snapshot) = &state.snapshot
                && event.filter_enabled == snapshot.filter_enabled
            {
                timeline.accept(event, snapshot.measurement_active, Instant::now());
            }
        }
        UiEvent::Snapshot(snapshot) => {
            set_snapshot(state, snapshot);
            if matches!(state.profiles, ProfileDialog::Renaming { .. }) {
                state.profiles = ProfileDialog::List;
                state.notice = Some("Profile renamed.".into());
            }
            state.status = "Settings are synchronized with the runtime.".into();
            state.error = None;
            if let Some(section) = state.pending_section.take() {
                return show_section(state, section, false);
            }
        }
        UiEvent::ApplySucceeded(snapshot) => {
            set_snapshot(state, snapshot);
            state.notice = Some("Settings applied.".into());
            state.status = "Settings applied.".into();
            state.error = None;
        }
        UiEvent::KeyCaptured { slot, key } => {
            state.pressed_keys = [false; 4];
            state.press_timestamps = [None; 4];
            if let Some(draft) = state.draft.as_mut() {
                draft.bindings[key_slot_index(slot)] = key.physical;
            }
            if let Some(snapshot) = state.snapshot.as_mut() {
                snapshot.draft.bindings[key_slot_index(slot)] = key.physical;
                snapshot.keys[key_slot_index(slot)] = key;
                snapshot.capture_slot = None;
            }
            state.status = "Mapping changed. Select Apply when ready.".into();
        }
        UiEvent::MeasurementUpdated(update) => {
            // A late update must not revive a stopped session; the stop
            // snapshot is authoritative about whether measurement runs.
            if let Some(snapshot) = state.snapshot.as_mut()
                && snapshot.measurement_active
            {
                snapshot.measurement = Some(update);
            }
        }
        UiEvent::ValidationFailed(error) | UiEvent::RuntimeError(error) => {
            let mut effects = Vec::new();
            match error.code.as_str() {
                "filter-failed" => {
                    state.pending_filter = None;
                    effects.push(Effect::Send(UiCommand::RequestSnapshot));
                }
                "filter-state-failed" => state.pending_filter = None,
                "profile-load-failed" | "profile-rename-failed" => {
                    state.profiles = ProfileDialog::List;
                }
                "monitor-start-failed" => {
                    state.monitor = MonitorState::Stopped;
                    effects.push(Effect::Send(UiCommand::StopMonitor));
                }
                "monitor-stop-failed" => {
                    if let MonitorState::Stopping(mut timeline) = std::mem::take(&mut state.monitor)
                    {
                        timeline.clear();
                        state.monitor = MonitorState::Recording(timeline);
                    }
                }
                _ => {}
            }
            state.error = Some(error.message);
            return effects;
        }
        UiEvent::FocusRequested(view) => return show_section(state, view, true),
        UiEvent::RuntimeShuttingDown => {
            state.connected = false;
            state.status = "The LastKey runtime is shutting down.".into();
            return vec![Effect::Close];
        }
    }
    Vec::new()
}

fn set_snapshot(state: &mut State, snapshot: UiSnapshot) {
    // Timing and its buffers are locally authoritative until Apply, so every
    // Snapshot merges: bindings, capture slot, measurement, and saved come
    // from the server while local timing stays. Only the first snapshot (no
    // local draft yet) replaces wholesale. `ApplySucceeded` flows through the
    // same rule, so mid-apply edits stay dirty instead of vanishing.
    state.monitor.resynchronize();
    state.pending_filter = None;
    let mut snapshot = snapshot;
    state.notice = None;
    if state.draft.is_none() {
        state.inputs = TimingInputs::from_timing(&snapshot.draft.timing);
    } else if let Some(draft) = state.draft.as_ref() {
        snapshot.draft.timing = draft.timing.clone();
    }
    state.draft = Some(snapshot.draft.clone());
    state.snapshot = Some(snapshot);
}

fn show_section(state: &mut State, view: UiView, focus: bool) -> Vec<Effect> {
    if view == UiView::Measurement {
        state.session_details_open = true;
    }
    if state.snapshot.is_none() {
        state.pending_section = Some(view);
        return Vec::new();
    }
    vec![Effect::ShowSection { view, focus }]
}

fn load_profile(state: &mut State, slot: u8) -> Vec<Effect> {
    state.error = None;
    state.profiles = ProfileDialog::Loading;
    vec![Effect::Send(UiCommand::LoadProfile(slot))]
}

/// Loads a slot, or opens its confirm banner when the draft is dirty. Shared
/// by the keycap row and by the card's own press so the two cannot drift.
fn load_or_confirm(state: &mut State, slot: u8) -> Vec<Effect> {
    if state.is_dirty() {
        state.profiles = ProfileDialog::Confirm(slot);
        Vec::new()
    } else {
        load_profile(state, slot)
    }
}

/// Sends an open rename to the server, or reopens the box on the stored name
/// when the field is empty. `on_empty` says what an empty field should do,
/// which is the one point where the three ways out of an edit differ.
///
/// A blank name is never sent: `Settings::validate` rejects it as
/// `profile-rename-failed`, and the panel would surface an error for a box
/// that was simply left empty.
fn commit_profile_name(state: &mut State, on_empty: EmptyName) -> Vec<Effect> {
    let ProfileDialog::Rename { slot, name } = &state.profiles else {
        return Vec::new();
    };
    let (slot, name) = (*slot, name.trim().to_owned());
    if name.is_empty() {
        let Some(stored) = state.stored_profile_name(slot) else {
            return Vec::new();
        };
        return match on_empty {
            EmptyName::Reopen => {
                state.profiles = ProfileDialog::Rename { slot, name: stored };
                // Writing the state alone would leave the dropped text on
                // screen, so the box is focused and selected instead.
                vec![Effect::FocusProfileName]
            }
            EmptyName::Close => {
                state.profiles = ProfileDialog::List;
                Vec::new()
            }
        };
    }
    state.profiles = ProfileDialog::Renaming {
        slot,
        name: name.clone(),
    };
    vec![Effect::Send(UiCommand::RenameProfile { slot, name })]
}

/// Drops an open rename and puts the stored name back, without telling the
/// server. This is Escape, and a window that lost focus with an edit in flight.
fn cancel_profile_name(state: &mut State) -> bool {
    let ProfileDialog::Rename { slot, .. } = &state.profiles else {
        return false;
    };
    let slot = *slot;
    let Some(stored) = state.stored_profile_name(slot) else {
        return false;
    };
    state.profiles = ProfileDialog::Rename { slot, name: stored };
    true
}

pub fn key_slot_index(slot: KeySlot) -> usize {
    match slot {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}

/// Whether a key event refers to the key a slot is bound to. Matching is by
/// name because the binding is stored as a display name, not a scancode.
fn matches_key(press: &KeyPress, target_name: &str) -> bool {
    let clean_target = target_name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();

    if clean_target.is_empty() {
        return false;
    }

    if let Some(character) = &press.character
        && (character.eq_ignore_ascii_case(target_name)
            || character.eq_ignore_ascii_case(&clean_target))
    {
        return true;
    }

    let Some(physical) = &press.physical else {
        return false;
    };
    let code = physical.to_ascii_uppercase();
    let stripped = code.strip_prefix("KEY").unwrap_or(&code);
    if stripped == clean_target || code == clean_target {
        return true;
    }
    for direction in ["UP", "DOWN", "LEFT", "RIGHT"] {
        if (code == format!("ARROW{direction}") || code == direction)
            && clean_target.contains(direction)
        {
            return true;
        }
    }
    code.replace("PAD", "") == clean_target.replace("PAD", "")
}

pub fn requested_view() -> UiView {
    requested_view_from(std::env::args())
}

pub fn requested_view_from(arguments: impl IntoIterator<Item = impl AsRef<str>>) -> UiView {
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

pub fn millis_to_micros(milliseconds: f32) -> u32 {
    (milliseconds.clamp(0.0, MAX_TIMING_MILLIS) * 10.0).round() as u32 * 100
}

pub fn format_ms(micros: u32) -> String {
    format!("{:.1}", micros as f32 / 1_000.0)
}

pub fn format_rate(rate: u8) -> String {
    rate.to_string()
}

pub fn parse_ms_text(input: &str) -> Option<u32> {
    let value: f32 = input.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(millis_to_micros(value))
}

pub fn parse_rate_text(input: &str) -> Option<u8> {
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
