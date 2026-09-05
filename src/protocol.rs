use std::{error::Error, fmt};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    app::{AppSnapshot, MeasurementUpdate},
    core::{LogicalKey, PhysicalKey, RecommendedTimingRange},
    settings::Settings,
};

pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Envelope<T> {
    pub version: u16,
    pub message: T,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum KeySlot {
    VerticalFirst,
    VerticalSecond,
    HorizontalFirst,
    HorizontalSecond,
}

impl From<KeySlot> for LogicalKey {
    fn from(slot: KeySlot) -> Self {
        match slot {
            KeySlot::VerticalFirst => Self::VerticalFirst,
            KeySlot::VerticalSecond => Self::VerticalSecond,
            KeySlot::HorizontalFirst => Self::HorizontalFirst,
            KeySlot::HorizontalSecond => Self::HorizontalSecond,
        }
    }
}

impl From<LogicalKey> for KeySlot {
    fn from(key: LogicalKey) -> Self {
        match key {
            LogicalKey::VerticalFirst => Self::VerticalFirst,
            LogicalKey::VerticalSecond => Self::VerticalSecond,
            LogicalKey::HorizontalFirst => Self::HorizontalFirst,
            LogicalKey::HorizontalSecond => Self::HorizontalSecond,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UiCommand {
    RequestSnapshot,
    BeginKeyCapture(KeySlot),
    UpdateDraft(Settings),
    Apply,
    Revert,
    RestoreMappingDefaults,
    RestoreAllDefaults,
    StartMeasurement,
    StopMeasurement,
    CloseUiSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UiEvent {
    Snapshot(UiSnapshot),
    KeyCaptured { slot: KeySlot, key: DisplayKey },
    MeasurementUpdated(MeasurementSnapshot),
    ValidationFailed(ErrorView),
    ApplySucceeded(UiSnapshot),
    RuntimeError(ErrorView),
    FocusRequested(UiView),
    RuntimeShuttingDown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UiView {
    Settings,
    Measurement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DisplayKey {
    pub physical: PhysicalKey,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UiSnapshot {
    pub saved: Settings,
    pub draft: Settings,
    pub keys: [DisplayKey; 4],
    pub capture_slot: Option<KeySlot>,
    pub measurement_active: bool,
    pub measurement: Option<MeasurementSnapshot>,
}

impl UiSnapshot {
    pub fn from_app(snapshot: AppSnapshot, key_names: [String; 4]) -> Self {
        let keys = std::array::from_fn(|index| DisplayKey {
            physical: snapshot.draft.bindings[index],
            name: key_names[index].clone(),
        });
        Self {
            saved: snapshot.saved,
            draft: snapshot.draft,
            keys,
            capture_slot: snapshot.capture_slot.map(Into::into),
            measurement_active: snapshot.measurement_active,
            measurement: snapshot.measurement.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimingRange {
    pub min_micros: u32,
    pub max_micros: u32,
}

impl From<RecommendedTimingRange> for TimingRange {
    fn from(range: RecommendedTimingRange) -> Self {
        Self {
            min_micros: range.min_micros,
            max_micros: range.max_micros,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MeasurementSnapshot {
    pub observed_event_count: u32,
    pub sample_count: u32,
    pub transition_count: u32,
    pub near_simultaneous_count: u32,
    pub overlap_count: u32,
    pub transition_min_micros: Option<u64>,
    pub transition_max_micros: Option<u64>,
    pub transition_latest_micros: Option<u64>,
    pub transition_p10_micros: Option<u64>,
    pub transition_median_micros: Option<u64>,
    pub transition_p90_micros: Option<u64>,
    pub overlap_min_micros: Option<u64>,
    pub overlap_max_micros: Option<u64>,
    pub overlap_latest_micros: Option<u64>,
    pub overlap_p10_micros: Option<u64>,
    pub overlap_median_micros: Option<u64>,
    pub overlap_p90_micros: Option<u64>,
    pub recommended_transition: Option<TimingRange>,
    pub recommended_overlap: Option<TimingRange>,
}

impl From<MeasurementUpdate> for MeasurementSnapshot {
    fn from(update: MeasurementUpdate) -> Self {
        let statistics = update.statistics;
        Self {
            observed_event_count: update.observed_event_count,
            sample_count: statistics.sample_count,
            transition_count: statistics.transition.count,
            near_simultaneous_count: statistics.near_simultaneous_count,
            overlap_count: statistics.overlap.count,
            transition_min_micros: statistics.transition.min_micros,
            transition_max_micros: statistics.transition.max_micros,
            transition_latest_micros: statistics.transition.latest_micros,
            transition_p10_micros: statistics.transition.p10_micros,
            transition_median_micros: statistics.transition.median_micros,
            transition_p90_micros: statistics.transition.p90_micros,
            overlap_min_micros: statistics.overlap.min_micros,
            overlap_max_micros: statistics.overlap.max_micros,
            overlap_latest_micros: statistics.overlap.latest_micros,
            overlap_p10_micros: statistics.overlap.p10_micros,
            overlap_median_micros: statistics.overlap.median_micros,
            overlap_p90_micros: statistics.overlap.p90_micros,
            recommended_transition: update.recommendation.socd_transition.map(Into::into),
            recommended_overlap: update.recommendation.preserved_overlap.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErrorView {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug)]
pub enum ProtocolError {
    FrameTooLarge(usize),
    InvalidFrameLength,
    VersionMismatch { expected: u16, actual: u16 },
    Serialization(serde_json::Error),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge(size) => write!(formatter, "IPC frame is too large: {size} bytes"),
            Self::InvalidFrameLength => write!(formatter, "IPC frame length is invalid"),
            Self::VersionMismatch { expected, actual } => write!(
                formatter,
                "IPC protocol version mismatch: expected {expected}, received {actual}"
            ),
            Self::Serialization(error) => write!(formatter, "IPC message is invalid: {error}"),
        }
    }
}

impl Error for ProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, ProtocolError> {
    let payload = serde_json::to_vec(&Envelope {
        version: PROTOCOL_VERSION,
        message,
    })
    .map_err(ProtocolError::Serialization)?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge(payload.len()));
    }
    let length = u32::try_from(payload.len()).expect("maximum frame size fits in u32");
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode<T: DeserializeOwned>(frame: &[u8]) -> Result<T, ProtocolError> {
    let length_bytes: [u8; 4] = frame
        .get(..4)
        .ok_or(ProtocolError::InvalidFrameLength)?
        .try_into()
        .expect("the frame prefix has exactly four bytes");
    let payload_length = u32::from_le_bytes(length_bytes) as usize;
    if payload_length > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge(payload_length));
    }
    let payload = frame.get(4..).ok_or(ProtocolError::InvalidFrameLength)?;
    if payload.len() != payload_length {
        return Err(ProtocolError::InvalidFrameLength);
    }
    let envelope: Envelope<T> =
        serde_json::from_slice(payload).map_err(ProtocolError::Serialization)?;
    if envelope.version != PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch {
            expected: PROTOCOL_VERSION,
            actual: envelope.version,
        });
    }
    Ok(envelope.message)
}

#[cfg(test)]
mod tests {
    use super::{
        Envelope, MAX_FRAME_SIZE, PROTOCOL_VERSION, ProtocolError, UiCommand, decode, encode,
    };

    #[test]
    fn commands_round_trip_through_a_versioned_frame() {
        let command = UiCommand::RestoreMappingDefaults;
        let encoded = encode(&command).expect("command encodes");

        assert_eq!(
            decode::<UiCommand>(&encoded).expect("command decodes"),
            command
        );
    }

    #[test]
    fn truncated_frames_are_rejected_before_deserialization() {
        let mut encoded = encode(&UiCommand::Apply).expect("command encodes");
        encoded.pop();

        assert!(matches!(
            decode::<UiCommand>(&encoded),
            Err(ProtocolError::InvalidFrameLength)
        ));
    }

    #[test]
    fn oversized_frames_are_rejected_before_allocation() {
        let length = u32::try_from(MAX_FRAME_SIZE + 1)
            .expect("test maximum fits in u32")
            .to_le_bytes();

        assert!(matches!(
            decode::<UiCommand>(&length),
            Err(ProtocolError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn mismatched_protocol_versions_are_rejected() {
        let payload = serde_json::to_vec(&Envelope {
            version: PROTOCOL_VERSION + 1,
            message: UiCommand::RequestSnapshot,
        })
        .expect("envelope serializes");
        let mut frame = Vec::from(
            u32::try_from(payload.len())
                .expect("payload length fits")
                .to_le_bytes(),
        );
        frame.extend_from_slice(&payload);

        assert!(matches!(
            decode::<UiCommand>(&frame),
            Err(ProtocolError::VersionMismatch { .. })
        ));
    }
}
