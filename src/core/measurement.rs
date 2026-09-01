use std::time::{Duration, Instant};

use super::{KeyAction, LogicalKey};

const MAX_PAIR_GAP: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasurementStatistics {
    sample_count: u32,
    transition_count: u32,
    overlap_count: u32,
    transition_total_micros: i128,
    overlap_total_micros: i128,
}

impl MeasurementStatistics {
    pub fn sample_count(self) -> u32 {
        self.sample_count
    }
    pub fn transition_count(self) -> u32 {
        self.transition_count
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
}

/// Opt-in, in-memory physical edge measurement for the two configured axes.
pub struct MeasurementSession {
    held: [bool; 4],
    released_at: [Option<Instant>; 4],
    pressed_at: [Option<Instant>; 4],
    statistics: MeasurementStatistics,
}

impl MeasurementSession {
    pub const fn new() -> Self {
        Self {
            held: [false; 4],
            released_at: [None; 4],
            pressed_at: [None; 4],
            statistics: MeasurementStatistics {
                sample_count: 0,
                transition_count: 0,
                overlap_count: 0,
                transition_total_micros: 0,
                overlap_total_micros: 0,
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
        self.statistics.sample_count += 1;
        if overlap {
            self.statistics.overlap_count += 1;
            self.statistics.overlap_total_micros += i128::from(micros);
        } else {
            self.statistics.transition_count += 1;
            self.statistics.transition_total_micros += i128::from(micros);
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
