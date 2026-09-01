use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::settings::TimingSettings;

use super::{
    Axis, AxisDecision, DeliveryState, EventDisposition, KeyAction, LogicalKey, OutputEmitter,
    SocdState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingKind {
    Press(LogicalKey),
    Release(LogicalKey),
}

#[derive(Clone, Copy, Debug)]
struct PendingTransition {
    due: Instant,
    kind: PendingKind,
}

/// Reconciles SOCD decisions with immediate or delayed output. The platform calls
/// `poll` from its scheduler; this controller never sleeps or creates a thread.
pub struct TimingController {
    socd: SocdState,
    output: [DeliveryState; 4],
    settings: TimingSettings,
    pending: [Option<PendingTransition>; 2],
    random_state: u64,
}

impl TimingController {
    pub fn new(settings: TimingSettings) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |duration| duration.as_nanos() as u64);
        Self::with_seed(settings, seed)
    }

    pub fn with_seed(settings: TimingSettings, seed: u64) -> Self {
        Self {
            socd: SocdState::new(),
            output: [DeliveryState::NotHeld; 4],
            settings,
            pending: [None, None],
            random_state: seed,
        }
    }

    pub fn process<E: OutputEmitter>(
        &mut self,
        key: LogicalKey,
        action: KeyAction,
        now: Instant,
        emitter: &mut E,
    ) -> EventDisposition {
        if action == KeyAction::Up
            && !self.socd.physically_held(key)
            && !self.output[key.index()].is_held()
        {
            return EventDisposition::PassThrough;
        }
        if action == KeyAction::Down && self.socd.physically_held(key) {
            return EventDisposition::Consume;
        }
        let decision = self.socd.apply(key, action);
        self.pending[axis_index(decision.axis)] = None;
        self.reconcile(decision, key, action, now, emitter)
    }

    pub fn poll<E: OutputEmitter>(&mut self, now: Instant, emitter: &mut E) {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let index = axis_index(axis);
            let Some(pending) = self.pending[index] else {
                continue;
            };
            if pending.due > now {
                continue;
            }
            self.pending[index] = None;
            match pending.kind {
                PendingKind::Press(key) => {
                    let _ = self.press(key, emitter);
                }
                PendingKind::Release(key) => {
                    if !self.release(key, emitter) {
                        let (first, second) = LogicalKey::axis_keys(key.axis());
                        let new = if key == first { second } else { first };
                        let _ = self.release(new, emitter);
                    }
                }
            }
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending
            .iter()
            .flatten()
            .map(|pending| pending.due)
            .min()
    }

    pub fn is_enabled(&self) -> bool {
        self.settings_enabled()
    }
    pub fn output_state(&self, key: LogicalKey) -> DeliveryState {
        self.output[key.index()]
    }

    pub fn reset<E: OutputEmitter>(&mut self, emitter: &mut E) {
        self.release_all(emitter);
        *self = Self::new(self.settings.clone());
    }

    pub fn release_all<E: OutputEmitter>(&mut self, emitter: &mut E) {
        self.pending = [None, None];
        for key in LogicalKey::ALL {
            let _ = self.release(key, emitter);
        }
    }

    fn reconcile<E: OutputEmitter>(
        &mut self,
        decision: AxisDecision,
        original: LogicalKey,
        action: KeyAction,
        now: Instant,
        emitter: &mut E,
    ) -> EventDisposition {
        let (first, second) = LogicalKey::axis_keys(decision.axis);
        let held = [first, second]
            .into_iter()
            .find(|key| self.output[key.index()].is_held());
        if let (Some(old), Some(new)) = (held, decision.desired)
            && old != new
            && self.settings_enabled()
        {
            if self.choose_overlap() {
                if self.press(new, emitter) {
                    let delay = self
                        .random_delay(self.settings.overlap_min_ms, self.settings.overlap_max_ms);
                    self.pending[axis_index(decision.axis)] = Some(PendingTransition {
                        due: now + delay,
                        kind: PendingKind::Release(old),
                    });
                }
            } else if self.release(old, emitter) {
                let delay = self.random_delay(
                    self.settings.transition_min_ms,
                    self.settings.transition_max_ms,
                );
                self.pending[axis_index(decision.axis)] = Some(PendingTransition {
                    due: now + delay,
                    kind: PendingKind::Press(new),
                });
            }
            return EventDisposition::Consume;
        }
        self.reconcile_immediate(decision, original, action, emitter)
    }

    fn reconcile_immediate<E: OutputEmitter>(
        &mut self,
        decision: AxisDecision,
        original: LogicalKey,
        action: KeyAction,
        emitter: &mut E,
    ) -> EventDisposition {
        let (first, second) = LogicalKey::axis_keys(decision.axis);
        let mut released = false;
        for key in [first, second] {
            if self.output[key.index()].is_held() && decision.desired != Some(key) {
                if !self.release(key, emitter) {
                    if action == KeyAction::Up && original == key {
                        self.output[key.index()] = DeliveryState::NotHeld;
                        return EventDisposition::PassThrough;
                    }
                    return EventDisposition::Consume;
                }
                released = true;
            }
        }
        if let Some(desired) = decision.desired
            && !self.output[desired.index()].is_held()
            && !self.press(desired, emitter)
            && action == KeyAction::Down
            && original == desired
            && !released
        {
            self.output[desired.index()] = DeliveryState::PhysicalPassThroughHeld;
            return EventDisposition::PassThrough;
        }
        EventDisposition::Consume
    }

    fn press<E: OutputEmitter>(&mut self, key: LogicalKey, emitter: &mut E) -> bool {
        if self.output[key.index()].is_held() {
            return true;
        }
        if emitter.emit(key, KeyAction::Down) {
            self.output[key.index()] = DeliveryState::SyntheticHeld;
            true
        } else {
            false
        }
    }

    fn release<E: OutputEmitter>(&mut self, key: LogicalKey, emitter: &mut E) -> bool {
        if !self.output[key.index()].is_held() {
            return true;
        }
        if emitter.emit(key, KeyAction::Up) {
            self.output[key.index()] = DeliveryState::NotHeld;
            true
        } else {
            false
        }
    }

    fn settings_enabled(&self) -> bool {
        self.settings.full_overlap
            || self.settings.overlap_probability > 0
            || self.settings.transition_max_ms > 0
    }
    fn choose_overlap(&mut self) -> bool {
        self.settings.full_overlap
            || (self.settings.overlap_probability > 0
                && self.next_random() % 100 < self.settings.overlap_probability as u64)
    }
    fn random_delay(&mut self, min: u32, max: u32) -> Duration {
        Duration::from_millis(min as u64 + self.next_random() % (u64::from(max - min) + 1))
    }
    fn next_random(&mut self) -> u64 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        self.random_state
    }
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::Vertical => 0,
        Axis::Horizontal => 1,
    }
}
