use super::MeasurementStatistics;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TimingRecommendation {
    pub transition_micros: Option<u64>,
    pub overlap_micros: Option<u64>,
}

/// Converts aggregate measurements into suggested timing values without depending on
/// platform capture, UI state, or persisted settings.
pub fn recommend(statistics: MeasurementStatistics) -> TimingRecommendation {
    TimingRecommendation {
        transition_micros: statistics
            .average_transition_micros()
            .map(|value| value.unsigned_abs()),
        overlap_micros: statistics
            .average_overlap_micros()
            .map(|value| value.unsigned_abs()),
    }
}
