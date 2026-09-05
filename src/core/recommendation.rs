use super::{MeasurementStatistics, SampleStats};

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
        socd_transition: recommended_range(&statistics.transition),
        preserved_overlap: recommended_range(&statistics.overlap),
    }
}

fn recommended_range(stats: &SampleStats) -> Option<RecommendedTimingRange> {
    if stats.count < MIN_RECOMMENDATION_SAMPLES {
        return None;
    }
    let p90_micros = stats.p90_micros?;
    let exclusive_p90_ceiling = p90_micros.saturating_sub(1) / 100 * 100;
    let max_micros = round_to_tenth_millisecond(stats.median_micros?)
        .min(u32::try_from(exclusive_p90_ceiling).unwrap_or(u32::MAX / 100 * 100));
    let min_micros = round_to_tenth_millisecond(stats.p10_micros?).min(max_micros);
    Some(RecommendedTimingRange {
        min_micros,
        max_micros,
    })
}

fn round_to_tenth_millisecond(micros: u64) -> u32 {
    let tenths = micros.saturating_add(50) / 100;
    u32::try_from(tenths.saturating_mul(100)).unwrap_or(u32::MAX / 100 * 100)
}
