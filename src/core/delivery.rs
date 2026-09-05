use super::{KeyAction, LogicalKey};

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
    pub(crate) const fn is_held(self) -> bool {
        !matches!(self, Self::NotHeld)
    }
}

/// Delivery vocabulary shared with the shipping timing path.
/// `TimingController` owns the SOCD and output state.
pub trait OutputEmitter {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool;
}
