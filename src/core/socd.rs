use super::{Axis, KeyAction, LogicalKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AxisDecision {
    pub axis: Axis,
    pub desired: Option<LogicalKey>,
}

#[derive(Clone, Copy, Default)]
struct KeyState {
    physically_held: bool,
    press_order: u64,
}

/// Platform-neutral Last Input Priority state. It has no delivery or timing behavior.
pub struct SocdState {
    keys: [KeyState; 4],
    sequence: u64,
}

impl SocdState {
    pub const fn new() -> Self {
        Self {
            keys: [KeyState {
                physically_held: false,
                press_order: 0,
            }; 4],
            sequence: 0,
        }
    }

    pub fn physically_held(&self, key: LogicalKey) -> bool {
        self.keys[key.index()].physically_held
    }

    pub fn apply(&mut self, key: LogicalKey, action: KeyAction) -> AxisDecision {
        let state = &mut self.keys[key.index()];
        match action {
            KeyAction::Down if !state.physically_held => {
                state.physically_held = true;
                self.sequence = self.sequence.wrapping_add(1);
                state.press_order = self.sequence;
            }
            KeyAction::Up => state.physically_held = false,
            KeyAction::Down => {}
        }

        let axis = key.axis();
        AxisDecision {
            axis,
            desired: self.winner_for(axis),
        }
    }

    fn winner_for(&self, axis: Axis) -> Option<LogicalKey> {
        let (first, second) = LogicalKey::axis_keys(axis);
        let first_state = self.keys[first.index()];
        let second_state = self.keys[second.index()];

        match (first_state.physically_held, second_state.physically_held) {
            (true, false) => Some(first),
            (false, true) => Some(second),
            (true, true) if first_state.press_order > second_state.press_order => Some(first),
            (true, true) => Some(second),
            (false, false) => None,
        }
    }
}

impl Default for SocdState {
    fn default() -> Self {
        Self::new()
    }
}
