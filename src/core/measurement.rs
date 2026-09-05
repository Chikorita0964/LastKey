use std::time::{Duration, Instant};

use super::{KeyAction, LogicalKey};

const MAX_PAIR_GAP: Duration = Duration::from_secs(1);
pub const NEAR_SIMULTANEOUS_THRESHOLD_MICROS: u64 = 1_000;

/// One per-distribution sample summary. Transition and overlap share the
/// same shape, so they share this type instead of repeating six prefixed
/// fields each.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleStats {
    pub count: u32,
    pub min_micros: Option<u64>,
    pub max_micros: Option<u64>,
    pub latest_micros: Option<u64>,
    pub p10_micros: Option<u64>,
    pub median_micros: Option<u64>,
    pub p90_micros: Option<u64>,
}

impl SampleStats {
    fn push(&mut self, magnitude: u64, samples: &mut Vec<u64>) {
        self.count += 1;
        update_range(&mut self.min_micros, &mut self.max_micros, magnitude);
        self.latest_micros = Some(magnitude);
        insert_sorted(samples, magnitude);
        self.p10_micros = percentile(samples, 10);
        self.median_micros = percentile(samples, 50);
        self.p90_micros = percentile(samples, 90);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasurementStatistics {
    pub sample_count: u32,
    pub near_simultaneous_count: u32,
    pub transition: SampleStats,
    pub overlap: SampleStats,
}

/// Opt-in, in-memory physical edge measurement for the two configured axes.
pub struct MeasurementSession {
    held: [bool; 4],
    released_at: [Option<Instant>; 4],
    pressed_at: [Option<Instant>; 4],
    edge_count: u32,
    transition_samples: Vec<u64>,
    overlap_samples: Vec<u64>,
    statistics: MeasurementStatistics,
}

impl MeasurementSession {
    pub fn new() -> Self {
        Self {
            held: [false; 4],
            released_at: [None; 4],
            pressed_at: [None; 4],
            edge_count: 0,
            transition_samples: Vec::new(),
            overlap_samples: Vec::new(),
            statistics: MeasurementStatistics::default(),
        }
    }

    pub fn observe(
        &mut self,
        key: LogicalKey,
        action: KeyAction,
        now: Instant,
    ) -> Option<MeasurementStatistics> {
        let other = opposing_key(key);
        // Auto-repeat downs and duplicate ups are not physical edges and
        // must never inflate the count.
        if action == KeyAction::Down && self.held[key.index()] {
            return None;
        }
        if action == KeyAction::Up && !self.held[key.index()] {
            return None;
        }
        self.edge_count += 1;
        match action {
            KeyAction::Down => {
                self.held[key.index()] = true;
                // A re-press retires this key's old release candidate: it was
                // either consumed already (None) or is stale, so it must never
                // become the start of a later neutral-transition sample.
                self.released_at[key.index()] = None;
                if self.held[other.index()] {
                    self.pressed_at[key.index()] = Some(now);
                } else if let Some(released) = self.released_at[other.index()].take() {
                    let gap = now.saturating_duration_since(released);
                    if gap <= MAX_PAIR_GAP {
                        return Some(self.record(gap, false));
                    }
                }
            }
            KeyAction::Up => {
                self.held[key.index()] = false;
                // The second-pressed key released first ends the overlap now.
                // Without this, its start time lingers and the later release
                // of the first key over-reports the overlap.
                if let Some(pressed) = self.pressed_at[key.index()].take() {
                    self.pressed_at[other.index()] = None;
                    let gap = now.saturating_duration_since(pressed);
                    if gap <= MAX_PAIR_GAP {
                        return Some(self.record(gap, true));
                    }
                }
                if let Some(pressed) = self.pressed_at[other.index()].take() {
                    let gap = now.saturating_duration_since(pressed);
                    if gap <= MAX_PAIR_GAP {
                        return Some(self.record(gap, true));
                    }
                }
                self.released_at[key.index()] = Some(now);
            }
        }
        None
    }

    pub fn statistics(&self) -> MeasurementStatistics {
        self.statistics
    }

    pub fn edge_count(&self) -> u32 {
        self.edge_count
    }

    /// Records a pre-checked pair gap. Callers apply the `MAX_PAIR_GAP`
    /// filter first; this stores whatever gap it is given.
    fn record(&mut self, gap: Duration, overlap: bool) -> MeasurementStatistics {
        let magnitude = gap.as_micros() as u64;
        self.statistics.sample_count += 1;
        if magnitude < NEAR_SIMULTANEOUS_THRESHOLD_MICROS {
            self.statistics.near_simultaneous_count += 1;
            return self.statistics;
        }
        if overlap {
            self.statistics
                .overlap
                .push(magnitude, &mut self.overlap_samples);
        } else {
            self.statistics
                .transition
                .push(magnitude, &mut self.transition_samples);
        }
        self.statistics
    }
}

impl Default for MeasurementSession {
    fn default() -> Self {
        Self::new()
    }
}

fn opposing_key(key: LogicalKey) -> LogicalKey {
    let (first, second) = LogicalKey::axis_keys(key.axis());
    if key == first { second } else { first }
}

fn update_range(minimum: &mut Option<u64>, maximum: &mut Option<u64>, value: u64) {
    *minimum = Some(minimum.map_or(value, |current| current.min(value)));
    *maximum = Some(maximum.map_or(value, |current| current.max(value)));
}

fn insert_sorted(samples: &mut Vec<u64>, value: u64) {
    let index = samples.partition_point(|sample| *sample <= value);
    samples.insert(index, value);
}

fn percentile(samples: &[u64], percent: usize) -> Option<u64> {
    let last = samples.len().checked_sub(1)?;
    let position = last * percent;
    let lower_index = position / 100;
    let remainder = position % 100;
    let lower = samples[lower_index];
    let upper = samples[(lower_index + 1).min(last)];
    let interpolated = (u128::from(upper - lower) * remainder as u128 + 50) / 100;
    Some(lower + interpolated as u64)
}
