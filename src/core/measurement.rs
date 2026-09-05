use std::time::{Duration, Instant};

use super::{KeyAction, LogicalKey};

const MAX_PAIR_GAP: Duration = Duration::from_secs(1);
pub const NEAR_SIMULTANEOUS_THRESHOLD_MICROS: u64 = 1_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasurementStatistics {
    sample_count: u32,
    near_simultaneous_count: u32,
    transition_count: u32,
    overlap_count: u32,
    transition_total_micros: i128,
    overlap_total_micros: i128,
    transition_min_micros: Option<u64>,
    transition_max_micros: Option<u64>,
    transition_latest_micros: Option<u64>,
    transition_p10_micros: Option<u64>,
    transition_median_micros: Option<u64>,
    transition_p90_micros: Option<u64>,
    overlap_min_micros: Option<u64>,
    overlap_max_micros: Option<u64>,
    overlap_latest_micros: Option<u64>,
    overlap_p10_micros: Option<u64>,
    overlap_median_micros: Option<u64>,
    overlap_p90_micros: Option<u64>,
}

impl MeasurementStatistics {
    pub fn sample_count(self) -> u32 {
        self.sample_count
    }
    pub fn transition_count(self) -> u32 {
        self.transition_count
    }
    pub fn near_simultaneous_count(self) -> u32 {
        self.near_simultaneous_count
    }
    pub fn overlap_count(self) -> u32 {
        self.overlap_count
    }
    pub fn average_transition_micros(self) -> Option<i64> {
        average(self.transition_total_micros, self.transition_count)
    }
    pub fn average_overlap_micros(self) -> Option<i64> {
        average(self.overlap_total_micros, self.overlap_count)
    }
    pub fn transition_min_micros(self) -> Option<u64> {
        self.transition_min_micros
    }
    pub fn transition_max_micros(self) -> Option<u64> {
        self.transition_max_micros
    }
    pub fn transition_latest_micros(self) -> Option<u64> {
        self.transition_latest_micros
    }
    pub fn transition_p10_micros(self) -> Option<u64> {
        self.transition_p10_micros
    }
    pub fn transition_median_micros(self) -> Option<u64> {
        self.transition_median_micros
    }
    pub fn transition_p90_micros(self) -> Option<u64> {
        self.transition_p90_micros
    }
    pub fn overlap_min_micros(self) -> Option<u64> {
        self.overlap_min_micros
    }
    pub fn overlap_max_micros(self) -> Option<u64> {
        self.overlap_max_micros
    }
    pub fn overlap_latest_micros(self) -> Option<u64> {
        self.overlap_latest_micros
    }
    pub fn overlap_p10_micros(self) -> Option<u64> {
        self.overlap_p10_micros
    }
    pub fn overlap_median_micros(self) -> Option<u64> {
        self.overlap_median_micros
    }
    pub fn overlap_p90_micros(self) -> Option<u64> {
        self.overlap_p90_micros
    }
}

/// Opt-in, in-memory physical edge measurement for the two configured axes.
pub struct MeasurementSession {
    held: [bool; 4],
    released_at: [Option<Instant>; 4],
    pressed_at: [Option<Instant>; 4],
    transition_samples: Vec<u64>,
    overlap_samples: Vec<u64>,
    statistics: MeasurementStatistics,
}

impl MeasurementSession {
    pub const fn new() -> Self {
        Self {
            held: [false; 4],
            released_at: [None; 4],
            pressed_at: [None; 4],
            transition_samples: Vec::new(),
            overlap_samples: Vec::new(),
            statistics: MeasurementStatistics {
                sample_count: 0,
                near_simultaneous_count: 0,
                transition_count: 0,
                overlap_count: 0,
                transition_total_micros: 0,
                overlap_total_micros: 0,
                transition_min_micros: None,
                transition_max_micros: None,
                transition_latest_micros: None,
                transition_p10_micros: None,
                transition_median_micros: None,
                transition_p90_micros: None,
                overlap_min_micros: None,
                overlap_max_micros: None,
                overlap_latest_micros: None,
                overlap_p10_micros: None,
                overlap_median_micros: None,
                overlap_p90_micros: None,
            },
        }
    }

    pub fn observe(
        &mut self,
        key: LogicalKey,
        action: KeyAction,
        now: Instant,
    ) -> Option<MeasurementStatistics> {
        let other = opposing_key(key);
        match action {
            KeyAction::Down if self.held[key.index()] => return None,
            KeyAction::Up if !self.held[key.index()] => return None,
            KeyAction::Down => {
                self.held[key.index()] = true;
                if self.held[other.index()] {
                    self.pressed_at[key.index()] = Some(now);
                } else if let Some(released) = self.released_at[other.index()].take()
                    && now.saturating_duration_since(released) <= MAX_PAIR_GAP
                {
                    return Some(self.record(now, released, false));
                }
            }
            KeyAction::Up => {
                self.held[key.index()] = false;
                // The second-pressed key released first ends the overlap now.
                // Without this, its start time lingers and the later release
                // of the first key over-reports the overlap.
                if let Some(pressed) = self.pressed_at[key.index()].take() {
                    self.pressed_at[other.index()] = None;
                    return Some(self.record(pressed, now, true));
                }
                if let Some(pressed) = self.pressed_at[other.index()].take() {
                    return Some(self.record(pressed, now, true));
                }
                self.released_at[key.index()] = Some(now);
            }
        }
        None
    }

    pub fn statistics(&self) -> MeasurementStatistics {
        self.statistics
    }

    fn record(&mut self, press: Instant, release: Instant, overlap: bool) -> MeasurementStatistics {
        let micros = if overlap {
            -(release.saturating_duration_since(press).as_micros() as i64)
        } else {
            press.saturating_duration_since(release).as_micros() as i64
        };
        let magnitude = micros.unsigned_abs();
        self.statistics.sample_count += 1;
        if magnitude < NEAR_SIMULTANEOUS_THRESHOLD_MICROS {
            self.statistics.near_simultaneous_count += 1;
            return self.statistics;
        }
        if overlap {
            self.statistics.overlap_count += 1;
            self.statistics.overlap_total_micros += i128::from(micros);
            update_range(
                &mut self.statistics.overlap_min_micros,
                &mut self.statistics.overlap_max_micros,
                magnitude,
            );
            self.statistics.overlap_latest_micros = Some(magnitude);
            insert_sorted(&mut self.overlap_samples, magnitude);
            self.statistics.overlap_p10_micros = percentile(&self.overlap_samples, 10);
            self.statistics.overlap_median_micros = percentile(&self.overlap_samples, 50);
            self.statistics.overlap_p90_micros = percentile(&self.overlap_samples, 90);
        } else {
            self.statistics.transition_count += 1;
            self.statistics.transition_total_micros += i128::from(micros);
            update_range(
                &mut self.statistics.transition_min_micros,
                &mut self.statistics.transition_max_micros,
                magnitude,
            );
            self.statistics.transition_latest_micros = Some(magnitude);
            insert_sorted(&mut self.transition_samples, magnitude);
            self.statistics.transition_p10_micros = percentile(&self.transition_samples, 10);
            self.statistics.transition_median_micros = percentile(&self.transition_samples, 50);
            self.statistics.transition_p90_micros = percentile(&self.transition_samples, 90);
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

fn average(total: i128, count: u32) -> Option<i64> {
    (count > 0).then(|| (total / i128::from(count)) as i64)
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
