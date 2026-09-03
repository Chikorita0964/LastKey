use super::MeasurementStatistics;

pub const MIN_RECOMMENDATION_SAMPLES: u32 = 10;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecommendedTimingRange {
    pub min_micros: u32,
    pub max_micros: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TimingRecommendation {
    pub socd_transition: Option<RecommendedTimingRange>,
    pub preserved_overlap: Option<RecommendedTimingRange>,
}

/// Converts aggregate measurements into suggested timing values without depending on
/// platform capture, UI state, or persisted settings.
pub fn recommend(statistics: MeasurementStatistics) -> TimingRecommendation {
    TimingRecommendation {
        socd_transition: recommended_range(
            statistics.transition_count(),
            statistics.transition_p10_micros(),
            statistics.transition_median_micros(),
            statistics.transition_p90_micros(),
        ),
        preserved_overlap: recommended_range(
            statistics.overlap_count(),
            statistics.overlap_p10_micros(),
            statistics.overlap_median_micros(),
            statistics.overlap_p90_micros(),
        ),
    }
}

fn recommended_range(
    sample_count: u32,
    p10_micros: Option<u64>,
    median_micros: Option<u64>,
    p90_micros: Option<u64>,
) -> Option<RecommendedTimingRange> {
    if sample_count < MIN_RECOMMENDATION_SAMPLES {
        return None;
    }
    let p90_micros = p90_micros?;
    let exclusive_p90_ceiling = p90_micros.saturating_sub(1) / 100 * 100;
    let max_micros = round_to_tenth_millisecond(median_micros?)
        .min(u32::try_from(exclusive_p90_ceiling).unwrap_or(u32::MAX / 100 * 100));
    let min_micros = round_to_tenth_millisecond(p10_micros?).min(max_micros);
    Some(RecommendedTimingRange {
        min_micros,
        max_micros,
    })
}

fn round_to_tenth_millisecond(micros: u64) -> u32 {
    let tenths = micros.saturating_add(50) / 100;
    u32::try_from(tenths.saturating_mul(100)).unwrap_or(u32::MAX / 100 * 100)
}
