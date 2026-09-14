//! The monitor timeline's data model. Framework-free on purpose: the painter
//! that draws it (T5) is added beside this, and the state layer depends only
//! on what is here.

use std::{collections::VecDeque, time::Instant};

use crate::protocol::{KeySlot, MonitorDecision, MonitorEdge, MonitorSnapshot};

/// How much history the graph keeps on screen.
pub const WINDOW_MICROS: u64 = 1_000_000;
const MAX_INTERVALS: usize = 512;

#[derive(Default)]
pub enum MonitorState {
    #[default]
    Stopped,
    Starting,
    Recording(Timeline),
    Stopping(Timeline),
}

impl MonitorState {
    pub fn timeline(&self) -> Option<&Timeline> {
        match self {
            Self::Recording(timeline) | Self::Stopping(timeline) => Some(timeline),
            _ => None,
        }
    }

    pub fn resynchronize(&mut self) {
        if let Self::Recording(timeline) | Self::Stopping(timeline) = self {
            timeline.clear();
        }
    }
}

#[derive(Clone, Copy)]
struct Interval {
    key: usize,
    start: u64,
    end: u64,
}

pub struct Timeline {
    intervals: VecDeque<Interval>,
    held_since: [Option<u64>; 4],
    anchor: Option<(Instant, u64)>,
    pub decision: MonitorDecision,
    pub physical: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            intervals: VecDeque::with_capacity(MAX_INTERVALS),
            held_since: [None; 4],
            anchor: None,
            decision: MonitorDecision::Immediate,
            physical: false,
        }
    }
}

impl Timeline {
    pub fn clear(&mut self) {
        self.intervals.clear();
        self.held_since = [None; 4];
        self.anchor = None;
        self.decision = MonitorDecision::Immediate;
    }

    pub fn accept(&mut self, event: MonitorSnapshot, measuring: bool, received: Instant) {
        let physical = !event.filter_enabled || measuring;
        if self.physical != physical {
            self.clear();
        }
        self.physical = physical;
        // Keep backend deltas intact; arrival time only positions the first event on screen.
        if self.anchor.is_none() {
            self.anchor = Some((received, event.elapsed_micros));
        }
        self.decision = event.decision;
        if physical {
            if let Some(edge) = event.physical {
                self.edge(edge, event.elapsed_micros);
            }
        } else {
            for edge in event.outputs {
                self.edge(edge, event.elapsed_micros);
            }
        }
        let cutoff = event.elapsed_micros.saturating_sub(WINDOW_MICROS);
        while self
            .intervals
            .front()
            .is_some_and(|interval| interval.end < cutoff)
        {
            self.intervals.pop_front();
        }
    }

    fn edge(&mut self, edge: MonitorEdge, at: u64) {
        let key = index(edge.key);
        if edge.pressed {
            self.held_since[key].get_or_insert(at);
        } else if let Some(start) = self.held_since[key].take() {
            if self.intervals.len() == MAX_INTERVALS {
                self.intervals.pop_front();
            }
            self.intervals.push_back(Interval {
                key,
                start,
                end: at,
            });
        }
    }

    pub fn now(&self) -> u64 {
        self.anchor.map_or(0, |(arrival, offset)| {
            offset.saturating_add(arrival.elapsed().as_micros() as u64)
        })
    }

    pub fn held(&self, key: KeySlot) -> bool {
        self.held_since[index(key)].is_some()
    }

    pub fn held_mask(&self) -> [bool; 4] {
        std::array::from_fn(|key| self.held_since[key].is_some())
    }

    /// Visible `(start, end, still_held)` spans for one lane. A held key ends
    /// at `now`, which is what makes its block and any overlap grow live.
    pub fn spans(&self, key: usize, now: u64) -> Vec<(u64, u64, bool)> {
        let mut spans: Vec<(u64, u64, bool)> = self
            .intervals
            .iter()
            .filter(|interval| {
                interval.key == key && now.saturating_sub(interval.end) <= WINDOW_MICROS
            })
            .map(|interval| (interval.start, interval.end, false))
            .collect();
        if let Some(start) = self.held_since[key] {
            spans.push((start, now, true));
        }
        spans
    }

    /// Last-press-wins resolution between two opposing keys, for the D-pad's
    /// center dot: when both are held, the key pressed later takes it.
    pub fn winner(&self, first: KeySlot, second: KeySlot) -> Option<KeySlot> {
        match (
            self.held_since[index(first)],
            self.held_since[index(second)],
        ) {
            (Some(a), Some(b)) => Some(if b >= a { second } else { first }),
            (Some(_), None) => Some(first),
            (None, Some(_)) => Some(second),
            (None, None) => None,
        }
    }
}

pub const fn index(key: KeySlot) -> usize {
    match key {
        KeySlot::VerticalFirst => 0,
        KeySlot::VerticalSecond => 1,
        KeySlot::HorizontalFirst => 2,
        KeySlot::HorizontalSecond => 3,
    }
}
