use super::{AxisDecision, KeyAction, LogicalKey, SocdState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDisposition {
    Consume,
    PassThrough,
}

/// The effective output state. A held key can originate from a successful synthetic
/// event or from an original event that was deliberately allowed through after a
/// delivery failure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DeliveryState {
    #[default]
    NotHeld,
    SyntheticHeld,
    PhysicalPassThroughHeld,
}

impl DeliveryState {
    const fn is_held(self) -> bool {
        !matches!(self, Self::NotHeld)
    }
}

pub trait OutputEmitter {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool;
}

/// Direct, timing-disabled delivery reconciliation. TimingController will replace
/// this direct reconciliation in the timing milestone while retaining its recovery
/// policy at the platform boundary.
pub struct InputRouter {
    socd: SocdState,
    output: [DeliveryState; 4],
}

impl InputRouter {
    pub const fn new() -> Self {
        Self {
            socd: SocdState::new(),
            output: [DeliveryState::NotHeld; 4],
        }
    }

    pub fn process<E: OutputEmitter>(
        &mut self,
        key: LogicalKey,
        action: KeyAction,
        emitter: &mut E,
    ) -> EventDisposition {
        if action == KeyAction::Up
            && !self.socd.physically_held(key)
            && !self.output[key.index()].is_held()
        {
            return EventDisposition::PassThrough;
        }

        let decision = self.socd.apply(key, action);
        self.reconcile(decision, key, action, emitter)
    }

    pub fn output_state(&self, key: LogicalKey) -> DeliveryState {
        self.output[key.index()]
    }

    pub fn release_all<E: OutputEmitter>(&mut self, emitter: &mut E) {
        for key in LogicalKey::ALL {
            if self.output[key.index()].is_held() && emitter.emit(key, KeyAction::Up) {
                self.output[key.index()] = DeliveryState::NotHeld;
            }
        }
    }

    fn reconcile<E: OutputEmitter>(
        &mut self,
        decision: AxisDecision,
        original_key: LogicalKey,
        action: KeyAction,
        emitter: &mut E,
    ) -> EventDisposition {
        let (first, second) = LogicalKey::axis_keys(decision.axis);
        let mut released_previous_output = false;

        for key in [first, second] {
            if self.output[key.index()].is_held() && decision.desired != Some(key) {
                if !emitter.emit(key, KeyAction::Up) {
                    if action == KeyAction::Up && original_key == key {
                        self.output[key.index()] = DeliveryState::NotHeld;
                        return EventDisposition::PassThrough;
                    }
                    return EventDisposition::Consume;
                }
                self.output[key.index()] = DeliveryState::NotHeld;
                released_previous_output = true;
            }
        }

        if let Some(desired) = decision.desired
            && !self.output[desired.index()].is_held()
        {
            if emitter.emit(desired, KeyAction::Down) {
                self.output[desired.index()] = DeliveryState::SyntheticHeld;
            } else if action == KeyAction::Down
                && original_key == desired
                && !released_previous_output
            {
                self.output[desired.index()] = DeliveryState::PhysicalPassThroughHeld;
                return EventDisposition::PassThrough;
            }
        }

        EventDisposition::Consume
    }
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}
