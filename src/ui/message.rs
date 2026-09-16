//! Everything the view can ask for. A view function may only push one of
//! these; it may not save settings, send IPC, or mutate state directly.

use crate::{
    protocol::KeySlot,
    settings::{SocdMode, TimingSettings},
};

use super::language::Language;

/// Which timing value a message addresses. The discriminant doubles as an
/// index into the per-field flag arrays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TimingField {
    TransitionMinimum = 0,
    TransitionMaximum = 1,
    PreservationRate = 2,
    PreservedMinimum = 3,
    PreservedMaximum = 4,
}

impl TimingField {
    pub const ALL: [Self; 5] = [
        Self::TransitionMinimum,
        Self::TransitionMaximum,
        Self::PreservationRate,
        Self::PreservedMinimum,
        Self::PreservedMaximum,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    /// The one definition of "this row is live". The view mounts only live
    /// groups and `update` drops messages for the rest, so a message queued
    /// before a mode switch cannot edit a value the new mode hides.
    /// Each mode uses exactly the values it acts on: Random Mix is the only
    /// mode that uses all three groups, and Immediate uses none.
    pub const fn is_editable(self, timing: &TimingSettings) -> bool {
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
    pub fn micros_mut(self, timing: &mut TimingSettings) -> Option<&mut u32> {
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
    pub fn micros(self, timing: &TimingSettings) -> Option<u32> {
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
    pub fn pair_invalid(self, timing: &TimingSettings) -> bool {
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

pub fn timing_pair_invalid(min_micros: u32, max_micros: u32) -> bool {
    min_micros > max_micros
}

/// Which mode-preview control was used. Display-only; the preview never reads
/// or predicts monitor output.
#[derive(Clone, Copy, Debug)]
pub enum PreviewAction {
    Previous,
    Next,
    Toggle,
    Tick,
}

/// A key event reduced to what the press tracker needs. The view layer owns
/// the translation from its own key type; `update` never sees an egui type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyPress {
    /// The typed character, when the key produced one.
    pub character: Option<String>,
    /// The physical key name, e.g. `ArrowUp`, `KeyW`, `Numpad4`.
    pub physical: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Ipc(IpcEvent),
    RequestSnapshot,
    ToggleFilter,
    OpenProfiles,
    OpenLanguages,
    SelectLanguage(Language),
    CloseProfiles,
    SlotHovered(u8),
    SlotUnhovered(u8),
    LoadProfile(u8),
    ConfirmProfile(u8),
    EditProfileName(u8),
    ProfileNameChanged(String),
    SaveProfileName,
    /// Commit an open rename, if there is one. Sent by the panel's own press
    /// path: a press that lands anywhere inside the panel other than the field
    /// itself is the "click somewhere else" that ends an edit.
    SaveProfileNameIfEditing,
    /// The pointer entered or left a card's name box.
    NameHovered(u8, bool),
    ToggleMonitor,
    Preview(PreviewAction),
    CancelCapture,
    ResetMeasurement,
    Capture(KeySlot),
    ModeSelected(SocdMode),
    MixChanged(f32),
    TimingSliderChanged(TimingField, f32),
    TimingTextChanged(TimingField, String),
    TimingTextSubmitted(TimingField),
    ValueBoxActivated(TimingField),
    WindowFocused,
    WindowUnfocused,
    KeyboardPressed(KeyPress),
    KeyboardReleased(KeyPress),
    Apply,
    Revert,
    RestoreMappingDefaults,
    RestoreTimingDefaults,
    RestoreAllDefaults,
    ToggleMeasurement,
    ApplyRecommendations,
}

/// The IPC channel's own events, as the state layer sees them. The transport
/// handle stays in the view layer; `update` only needs to know that a
/// connection exists.
#[derive(Clone, Debug)]
pub enum IpcEvent {
    Connected,
    Message(Box<crate::protocol::UiEvent>),
    Disconnected(String),
}
