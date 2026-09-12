use super::{KeyAction, LogicalKey};

/// One key edge on the monitor timeline. `synthetic` separates what the game
/// receives from what the hands did; with the filter off every edge is
/// physical and the output lane mirrors the input lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorEdge {
    pub key: LogicalKey,
    pub action: KeyAction,
    pub synthetic: bool,
}

/// How one overlap resolved. Exact badge data: the frontend renders it
/// without inferring anything from timing gaps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonitorDecision {
    Immediate,
    PressDelayed { delay_micros: u32 },
    ReleaseDelayed { delay_micros: u32 },
}

/// One step of filter behavior for the monitor timeline. `physical` is the
/// trigger when there is one; poll-fired delayed completions carry `None`
/// with their outputs. Lifecycle output changes (apply, filter toggles) are
/// deliberately not streamed: consumers resynchronize held-output state on
/// snapshots and filter-change notifications instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonitorEvent {
    /// Microseconds since the monitor session started, on the backend clock.
    /// The frontend anchors these offsets onto its own arrival timeline; the
    /// residual skew is invisible on a one-second window.
    pub elapsed_micros: u64,
    /// Engine filter state when the event was recorded.
    pub filter_enabled: bool,
    pub physical: Option<MonitorEdge>,
    pub outputs: Vec<MonitorEdge>,
    pub decision: MonitorDecision,
}
