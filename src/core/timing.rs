use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::debug_log;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TimingMode {
    Transition,
    Overlap,
}

#[derive(Clone, Copy, Debug)]
struct PendingTransition {
    due: Instant,
    kind: PendingKind,
    mode: TimingMode,
    trace_id: u64,
    axis: Axis,
    old: LogicalKey,
    new: LogicalKey,
    scheduled_at: Instant,
    requested_delay: Duration,
}

/// Reconciles SOCD decisions with immediate or delayed output. The platform calls
/// `poll` from its scheduler; this controller never sleeps or creates a thread.
pub struct TimingController {
    socd: SocdState,
    output: [DeliveryState; 4],
    settings: TimingSettings,
    pending: [Option<PendingTransition>; 2],
    random_state: u64,
    next_trace_id: u64,
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
            next_trace_id: 1,
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
        let axis = axis_index(decision.axis);
        if let Some(cancelled) = self.pending[axis].take() {
            debug_log::write(format_args!(
                "timing trace={} pending-cancelled mode={:?} axis={:?} old={:?} new={:?} pending={:?} reason=new-physical-input",
                cancelled.trace_id,
                cancelled.mode,
                cancelled.axis,
                cancelled.old,
                cancelled.new,
                cancelled.kind
            ));
        }
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
            let actual_delay = now.saturating_duration_since(pending.scheduled_at);
            let lateness = actual_delay.saturating_sub(pending.requested_delay);
            debug_log::write(format_args!(
                "timing trace={} timer-fired mode={:?} axis={:?} old={:?} new={:?} pending={:?} requested_delay_us={} actual_delay_us={} lateness_us={}",
                pending.trace_id,
                pending.mode,
                pending.axis,
                pending.old,
                pending.new,
                pending.kind,
                pending.requested_delay.as_micros(),
                actual_delay.as_micros(),
                lateness.as_micros()
            ));
            let emitted = match pending.kind {
                PendingKind::Press(key) => self.press(key, emitter),
                PendingKind::Release(key) => {
                    if !self.release(key, emitter) {
                        let (first, second) = LogicalKey::axis_keys(key.axis());
                        let new = if key == first { second } else { first };
                        self.release(new, emitter)
                    } else {
                        true
                    }
                }
            };
            debug_log::write(format_args!(
                "timing trace={} delayed-output-completed mode={:?} pending={:?} success={emitted}",
                pending.trace_id, pending.mode, pending.kind
            ));
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
            let trace_id = self.next_trace_id;
            self.next_trace_id = self.next_trace_id.wrapping_add(1).max(1);
            let (overlap_selected, probability_roll) = self.choose_overlap();
            let selected_mode = if overlap_selected {
                TimingMode::Overlap
            } else {
                TimingMode::Transition
            };
            debug_log::write(format_args!(
                "timing trace={trace_id} decision source=physical-overlap axis={:?} old={old:?} new={new:?} mode={selected_mode:?} transition_delay_enabled={} preserve_overlap={} configured_rate={} effective_rate={} roll={} socd_transition_range_ms={}-{} preserved_overlap_range_ms={}-{}",
                decision.axis,
                self.settings.socd_transition_delay_enabled,
                self.settings.preserve_overlap,
                self.settings.overlap_preservation_rate,
                self.settings.effective_overlap_preservation_rate(),
                probability_roll.map_or_else(|| "none".to_owned(), |roll| roll.to_string()),
                format_millis(self.settings.socd_transition_min_micros),
                format_millis(self.settings.socd_transition_max_micros),
                format_millis(self.settings.preserved_overlap_min_micros),
                format_millis(self.settings.preserved_overlap_max_micros)
            ));
            if overlap_selected {
                if self.press(new, emitter) {
                    let delay = self.random_delay(
                        self.settings.preserved_overlap_min_micros,
                        self.settings.preserved_overlap_max_micros,
                    );
                    self.pending[axis_index(decision.axis)] = Some(PendingTransition {
                        due: now + delay,
                        kind: PendingKind::Release(old),
                        mode: TimingMode::Overlap,
                        trace_id,
                        axis: decision.axis,
                        old,
                        new,
                        scheduled_at: now,
                        requested_delay: delay,
                    });
                    debug_log::write(format_args!(
                        "timing trace={trace_id} scheduled mode=Overlap immediate={new:?}:Down delayed={old:?}:Up requested_delay_ms={:.1} requested_delay_us={}",
                        delay.as_secs_f64() * 1_000.0,
                        delay.as_micros()
                    ));
                } else {
                    debug_log::write(format_args!(
                        "timing trace={trace_id} schedule-aborted mode=Overlap failed={new:?}:Down"
                    ));
                }
            } else if self.release(old, emitter) {
                let delay = self.random_delay(
                    self.settings.socd_transition_min_micros,
                    self.settings.socd_transition_max_micros,
                );
                self.pending[axis_index(decision.axis)] = Some(PendingTransition {
                    due: now + delay,
                    kind: PendingKind::Press(new),
                    mode: TimingMode::Transition,
                    trace_id,
                    axis: decision.axis,
                    old,
                    new,
                    scheduled_at: now,
                    requested_delay: delay,
                });
                debug_log::write(format_args!(
                    "timing trace={trace_id} scheduled mode=Transition immediate={old:?}:Up delayed={new:?}:Down requested_delay_ms={:.1} requested_delay_us={}",
                    delay.as_secs_f64() * 1_000.0,
                    delay.as_micros()
                ));
            } else {
                debug_log::write(format_args!(
                    "timing trace={trace_id} schedule-aborted mode=Transition failed={old:?}:Up"
                ));
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
        self.settings.socd_transition_delay_enabled
    }
    fn choose_overlap(&mut self) -> (bool, Option<u8>) {
        let preservation_rate = self.settings.effective_overlap_preservation_rate();
        if preservation_rate == 0 {
            return (false, None);
        }
        if preservation_rate == 100 {
            return (true, None);
        }
        let roll = (self.next_random() % 100) as u8;
        (roll < preservation_rate, Some(roll))
    }
    fn random_delay(&mut self, min_micros: u32, max_micros: u32) -> Duration {
        let step_count = u64::from((max_micros - min_micros) / 100);
        let selected_step = self.next_random() % (step_count + 1);
        Duration::from_micros(u64::from(min_micros) + selected_step * 100)
    }
    fn next_random(&mut self) -> u64 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        self.random_state
    }
}

fn format_millis(micros: u32) -> String {
    format!("{}.{:01}", micros / 1_000, (micros % 1_000) / 100)
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::Vertical => 0,
        Axis::Horizontal => 1,
    }
}
