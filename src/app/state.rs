use crate::{
    core::{LogicalKey, MeasurementStatistics, PhysicalKey, TimingRecommendation},
    settings::Settings,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedKey {
    pub physical: PhysicalKey,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasurementUpdate {
    pub observed_event_count: u32,
    pub statistics: MeasurementStatistics,
    pub recommendation: TimingRecommendation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    pub saved: Settings,
    pub draft: Settings,
    pub capture_slot: Option<LogicalKey>,
    pub measurement_active: bool,
    pub measurement: Option<MeasurementUpdate>,
}

pub(super) struct AppState {
    pub saved: Settings,
    pub draft: Settings,
    pub capture_slot: Option<LogicalKey>,
    pub capture_generation: u64,
    pub measurement_active: bool,
    pub measurement_generation: u64,
    pub measurement: Option<MeasurementUpdate>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            saved: settings.clone(),
            draft: settings,
            capture_slot: None,
            capture_generation: 0,
            measurement_active: false,
            measurement_generation: 0,
            measurement: None,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            saved: self.saved.clone(),
            draft: self.draft.clone(),
            capture_slot: self.capture_slot,
            measurement_active: self.measurement_active,
            measurement: self.measurement,
        }
    }

    pub fn invalidate_capture(&mut self) {
        self.capture_generation = self.capture_generation.wrapping_add(1);
        self.capture_slot = None;
    }

    pub fn invalidate_measurement(&mut self) {
        self.measurement_generation = self.measurement_generation.wrapping_add(1);
        self.measurement_active = false;
    }
}
