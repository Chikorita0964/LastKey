# Non-Goals and Declined Changes

Reopening one of these is undoing a decision, not finding a defect.

## Non-Goals

- Replacing `WH_KEYBOARD_LL` with Raw Input as the primary path
- Changing `SendInput` ownership or the SOCD algorithm without separate justification
- Redesigning timing semantics or the measurement recommendation policy
- Linking Iced or wgpu into the resident runtime, or running them permanently
- Making the UI process authoritative for settings persistence
- Introducing a general-purpose distributed-systems-style IPC framework
- Reintroducing input logging or raw key-history diagnostics, including behind a feature gate. If new
  diagnostics are needed, design them around aggregate counters and explicit consent.
- Splitting the crate into a Cargo workspace purely to rearrange files. Logical boundaries already
  hold through `src/app`, `src/protocol.rs`, separate binaries, and Cargo features.
- Animating the live key-capture display or the measurement readouts. Those surfaces report timing at
  millisecond scale and redraw on every physical key edge, so decorative motion or any transition that
  delays a value works against invariant 1. Motion belongs to the surrounding chrome instead: mode
  switching, view transitions, toasts.

## Evaluated and Declined

Reopening one of these is undoing a decision, not finding a defect.

| Change | Why not |
| --- | --- |
| Merge the two IPC pump loops | Same cadence, different bodies; unifying needs generics plus callbacks and would bury the deadlock rule the comments state plainly |
| A `with_engine` helper for the `ENGINE.with(..)` sites | The hook and timer paths must keep `try_borrow_mut`; a convenience helper is what a future edit would reach for there too |
| A `with_emitter` helper in `InputEngine` | The rebuilds exist because the borrow checker splits `&self.settings` from `&mut self.timing`; hiding that helps nobody |
| Embed `SampleStats` in `MeasurementSnapshot` | Roughly 25 UI call sites for no behavioral gain, and it couples the wire format to a core type |
| Attribute-driven serde for `TimingSettings` | Saves ~50 lines but moves the stored-file contract out of one visible struct into scattered attributes |
| `[String; 5]` for `TimingInputs` | Deletes two matches, costs the named access that `from_timing` and the tests read by |
| A separate `DurationField` enum | Removes one unreachable `expect` in `ms_field` at the cost of a second enum every reader must relate to the first. Revisit only if a third caller appears |
| Incremental percentiles or a smarter sample structure | `O(n)` insert on a human-bounded sample count; a heap buys nothing measurable |
| Split `src/platform/windows/input.rs` | Every part serves one thread and one `thread_local` engine; the file's comments carry that invariant continuously |
| Split `src/ui/app.rs` | Repeated unification already flattened the dense parts; a split now would be motion, not improvement |
| `[target.'cfg(windows)'.build-dependencies]` for `png` | Cargo resolves build-dependency `cfg` against the **host**, so this drops `png` from a Linux-hosted Windows cross-build and breaks `build.rs`. The feature axis is the correct one |
