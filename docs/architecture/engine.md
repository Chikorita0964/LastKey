# Engine Contracts

Backend implementation contracts: controller, IPC, storage, timing, delivery, profiles, and the
Linux backend. UI contracts live in `ui.md`. Read with `../lastkey-architecture.md`, which owns
the process map and the runtime invariants these contracts serve.

## AppController and the Apply transaction

The UI sends editing intent; it never owns authoritative settings.

```text
Iced widget state -> draft intent -> IPC -> AppController validation
  -> persist -> runtime activation -> authoritative snapshot -> Iced display state
```

`AppController` holds saved and draft settings separately, plus capture and measurement generations.
It keeps no mirror of the running configuration: the engine is authoritative, and
`reconcile_apply_outcome` queries it through the `ActiveSettings` fence. Apply validates the draft,
persists the candidate, activates the runtime synchronously, then publishes the snapshot. On
activation failure the previous file is written back; if that rollback also fails, both errors are
reported together. The UI never reports Apply success before the runtime confirms both steps.

## IPC

- `src/protocol.rs` defines version 4 of a length-prefixed JSON protocol with a 1 MiB frame limit.
  Frame length and version are validated before deserialization. Both binaries ship together, so an
  exact-match version check turns a mismatched pair into a clear error, not a parse failure.
- The named pipe rejects remote clients and carries a DACL allowing only the owning user and
  `SYSTEM`; another local user could otherwise read snapshots or occupy the single instance.
- Bounded everywhere, because an unbounded wait hangs the message loop or shutdown: 5 s for input
  service startup, 2 s per command, 100 ms back-off on accept failure, and `CancelSynchronousIo`
  with a 2 s limit on shutdown.
- A failed Apply is reconciled through an `ActiveSettings` fence queued behind it: engine runs the
  candidate, adopt it; engine runs the old settings, roll back; fence unreachable, report
  `RuntimeUnconfirmed` rather than claiming a rollback.
- Each session performs every pipe syscall from exactly one thread: wait on the outbound queue with a
  timeout, drain outbound, `PeekNamedPipe`-gated inbound read. Splitting one synchronous pipe across
  a reader and a writer thread deadlocks, because a pending blocking read stalls writes on a
  duplicate handle of the same file object. Server workers enqueue events for the pump rather than
  writing directly.
- The pump waits on its outbound channel (`recv_timeout`), so queued messages leave immediately. Only
  inbound discovery is bounded by the poll interval: 2 ms while active, backing off to 50 ms after
  250 ms of silence. Polling exists only while a settings session is connected; latency-sensitive
  input never crosses this pipe.

## Settings storage

- Installed mode: `%LOCALAPPDATA%\LastKey\settings.toml`; with a portable marker, `settings.toml`
  beside the executable; if the primary file is absent, a legacy fallback reads the file beside the
  executable, and that file is never deleted automatically.
- Saving writes and `sync_all`s a temporary file in the same directory, then replaces the destination
  with `MoveFileExW` (`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`), removing the temporary
  file on failure. The MSIX `WindowsApps` directory is not writable, and a plain `fs::write` could
  leave a truncated TOML file that loses every setting on the next launch.
- The parser recognizes only current field names, with one exception: `socd_transition_delay_enabled`
  and `preserve_overlap`, written by v1.0.0 through v1.0.2, are read when `mode` is absent and mapped
  to the mode that build actually behaved as. They are never written back, so a file upgrades itself
  on the next save. A file that fails `validate()` is rejected whole — one error dialog, then full
  defaults — rather than clamping individual fields.

## Timing and measurement semantics

- Timing policy applies only when physical opposing keys actually overlap; already-neutral
  transitions are left alone.
- `SocdMode` is the whole timing state space: `Immediate`, `PressDelay`, `ReleaseDelay`, `RandomMix`.
  The modes are independent — none gates another — so every stored value is meaningful and the engine
  needs no defensive guard for an undefined combination. `TimingSettings::release_delay_share` turns
  the mode into the one number `TimingController` asks for: 0 for `Immediate` and `PressDelay`, 100
  for `ReleaseDelay`, the configured ratio for `RandomMix`. Only `RandomMix` ever draws a number.
- Each mode reads only the values it acts on, and the settings card grays out the rest without
  discarding them, so switching modes never loses a configured range.
- The UI works in 0.1 ms units; internally everything is integer microseconds, so scheduling never
  depends on floating point.
- Recommendations use the P10 minimum and P50 maximum with P90 as an exclusive ceiling, keeping slow
  tails from widening the range.
- The engine calls the sub-threshold bucket `near_simultaneous`; the UI labels it **Indistinguishable**.
  The identifier is deliberately threshold-independent so it survives a change to the 1 ms constant,
  while the label states what the reader needs: the order could not be determined. Do not rename one
  to match the other.
- Samples below 1 ms are classified separately as near-simultaneous, because keyboard scanning and OS
  batching make ordering unreliable at that scale. They never enter `SampleStats`, so a recommended
  minimum cannot fall under the 100 µs floor `validate()` enforces.
- Overlap samples longer than `MAX_PAIR_GAP` (1 s) are discarded, symmetric with neutral transitions,
  so a long hold cannot skew the recommended durations.
- All applied timing shares one ceiling, `MAX_TIMING_MICROS` (1000 ms), enforced by
  `Settings::validate` for the slider, typed, and recommendation paths alike. It equals `MAX_PAIR_GAP`
  on purpose: rounding the largest acceptable sample lands exactly on the ceiling, so
  `ApplyRecommendations` cannot produce a draft its own validator rejects. The UI's
  `MAX_TIMING_MILLIS` derives from it; keep any future ceiling a round millisecond count so that
  `f32` derivation stays exact.
- `SampleStats` owns one distribution (count, min/max/latest, P10/P50/P90) and `push` is its only
  writer. Its fields are public for the protocol mapping; if it ever gains an invariant, reintroduce
  accessors at the same time.

The policy is fixed by the implementation and deterministic tests. The 1 ms threshold, the 10-sample
requirement, and aggregation across both axes still need real-hardware testing. Do not redesign this
policy as a side effect of unrelated work.

## Scheduling

Delayed work uses a high-resolution waitable timer on the Windows input thread rather than a general
`Scheduler` trait. The core exposes only `next_deadline()` and the platform owns the native timer,
which keeps the callback latency and synchronization contracts explicit.

## Delivery failure recovery

`SendInput` can fail — UIPI blocks injection into higher-privilege windows. `TimingController`
returns `EventDisposition::PassThrough` on exactly three paths, and in none of them has a synthetic
emission for that event succeeded, so replaying the original cannot double-deliver:

1. A key-up for a key that is neither physically held nor held in output
2. A failed release of the event's own key-up
3. A failed press of the original key-down

Windows honors `PassThrough` by calling `CallNextHookEx`; Linux replays the original event through
the virtual device. The behavior table in `README.md` describes the user-visible result and must stay
in sync with `reconcile_immediate`. The delayed (timing-enabled) path has no physical pass-through,
because both keys are already held and the delayed release is still pending; the code notes that.

## Filter control and opt-in timeline

The engine owns the session-only filter state, initially enabled. The header and tray display only
queried or acknowledged engine state; neither performs optimistic toggles. Tray notifications enqueue
a request to query current state in the IPC pump, avoiding replay of an older toggle. UI toggles post
a scalar thread message to refresh the tray label on its owning thread. The tooltip still reports
hook health only.

The user approved this narrowly scoped exception to the no-history contract: Start timeline opts in
to a volatile history of only the four mapped keys. The UI retains at most 512 completed intervals
and four current holds, displays a one-second window, and discards everything on Stop, Apply,
profile activation, disconnect, or window close. No timeline data is written to disk. The backend
streams its clock offsets, physical triggers, successful synthetic outputs, and exact delay decisions;
the UI never simulates SOCD or draws its own random decisions. While filtering is disabled or
measurement bypasses filtering, the display uses physical edges. Otherwise it uses backend outputs.
It is an engine-event display, not an acknowledgment that another application received an event.

Monitor events are forwarded by a session-owned worker, then accepted by controller generation in
the single IPC pump. Stop invalidates that generation. Toggling survives the monitor session;
Snapshot and FilterChanged clear held display state because lifecycle output is not streamed.
Start and Stop have explicit pending and failed states, and late events cannot revive a stopped UI.
The UI redraws while holds or the last one-second interval remain visible and stops redrawing idle
history. The channel pumps exist only while a settings session is connected.

## Profiles

Settings may contain one boxed, four-slot profile bank with an active slot index. Legacy files with
no bank keep their current settings; the first profile operation seeds slot 1 from that configuration
and the other three from the factory modes. The same Settings validator checks each stored slot.
Names contain 1–64 characters and no control characters. The bank lives in the existing settings file;
there is no second persistence owner.

Apply synchronizes the active slot with the draft before the existing persistence/activation
transaction. Load is an explicit immediate activation through that same transaction. The UI confirms
discard of unapplied edits before Load and replaces its local buffers only after ProfileLoaded.
A failed load retains the previous draft and reports the transaction error. Renaming persists only
metadata, preserving unapplied edits and never activating them. Restore all defaults resets the
current draft configuration and preserves the saved slots until Apply updates the active one.

## Linux backend (experimental)

- Devices exposing all four configured keys are grabbed exclusively; every other key event is
  replayed through the uinput virtual device, which is excluded from the candidate scan by name so
  LastKey never grabs its own output.
- Windows scan codes and evdev keycodes coincide for plain keys but not extended ones, so
  `linux_keycode` / `physical_from_linux` translate the arrow keys explicitly and reject bindings
  with no known Linux mapping instead of resolving them to the wrong key.
- The main loop waits until the next timing deadline but never longer than 5 ms, because `std` mpsc
  has no `select()` and a deadline-only wait would starve the other channel. Reader threads use
  non-blocking reads plus a short sleep so shutdown can join them without an extra wakeup fd.

Completing Linux hotplug, permissions, tray, packaging, and release support is out of scope.

## Do Not Regress

Each item looks removable until its reason is known. Changing one requires a replacement test that fails against the old behavior.

### Capture and input

| Contract | Why it exists |
| --- | --- |
| Entering capture reconciles output, physical state, and pending work (`begin_capture`) | Otherwise a pre-held key's repeat is captured while its release is consumed, leaving output held with nothing down |
| Modifiers are never captured (`is_capture_eligible`, both paths) | A bound modifier would be swallowed globally; the UI guidance promises they stay available |
| Capture consumes the next key-up for the captured key (`captured_key_awaiting_release`) | The key-down was already consumed; releasing only the key-up would deliver an unmatched event elsewhere |
| While capture is pending, a key-up for a *different* key passes through | A key held before capture started has no consumed key-down, so its release must reach applications |
| `process_hook` and `process_raw` share the same leading guards | Asymmetry lets auto-repeat of a captured key reach `observe_raw` as a false miss and trigger a spurious reinstall |
| Hook-health records only configured keys, outside capture and measurement | Otherwise every keystroke system-wide does queue work inside the latency-critical hook callback |
| Injected events are filtered by `LLKHF_INJECTED` + `INJECTION_TAG` before any borrow | Keeps LastKey's own output out of physical processing, and makes the common re-entry case allocation-free |
| Raw input with a null device handle is dropped before the union read (`is_injected`) | One guard covers hook-health, capture, and measurement. Where legitimate keystrokes carry no handle (some RDP/remote stacks) those three degrade silently while SOCD is unaffected, because the hook path never reads `hDevice` |
| `try_borrow_mut` on the hook and timer paths | `SendInput` from the hook's own thread can re-enter the hook; a panic there would abort across the `extern "system"` boundary and leave a synthetic key stuck down. The command path may use a plain borrow; these two may not |
| A lost hook releases output, then notifies once via thread message | With no hook, no release event will ever arrive to clear held output. Notification fires on lost/recovered transitions only, after consecutive failures spaced by the reinstall cooldown, never on a timer |
| A failed scheduled release releases the opposite key instead | Never leave both directions of one axis held together |
| The four SOCD modes are one enum, not a switch gating a sub-switch | The boolean pair had eight states for four behaviors; the unreachable ones needed runtime guards that silently rewrote a stored preference. An enum makes every state defined and every mode reachable on its own |
| Delivery recovery is tested through `TimingController`, not a parallel router | A second copy of this policy that no shipping path executes drifts from the real one |

### Lifecycle and IPC

| Contract | Why it exists |
| --- | --- |
| Every arming transient command carries a deadline (`Apply`, `Capture`, `StartMeasurement`) | A command queued behind a stall must not arm for a caller whose acknowledgement wait already expired. Disarming commands (`CancelCapture`, `StopMeasurement`) deliberately carry none: arriving late they only disarm, which is idempotent and desirable |
| A failed Apply is resolved by the `ActiveSettings` fence | It queues behind any late Apply, so its answer is authoritative; unreachable means `RuntimeUnconfirmed`, not a claimed rollback |
| Stopping measurement with no active session touches no timing state | `close_ui_session` stops unconditionally and also runs on a UI crash or disconnect; resetting there would release live output and interrupt SOCD (invariant 12) |
| `TimingController::reset_state()` at measurement boundaries | `release_all` alone leaves `physically_held` set, so a key released during measurement is treated as a repeat afterwards |
| The engine drops a measurement session whose consumer disconnected | Running on would bypass SOCD with no owner |
| Capture completions and measurement updates validate on the pump, not in the worker | Worker validation races a Revert processed between acceptance and enqueue, and lets an update surface inside the *next* session; the pump orders validation, application, and reply |
| `MeasurementUpdated` applies only while `measurement_active` | An update queued just before Stop is written after the stop reply; without this guard it revives the stopped session and overwrites the final statistics |
| Repeat and duplicate key events produce no measurement update | `observe` discards them, so sending was pure duplication — roughly thirty identical messages per second per held key, each costing a lock, a serialization, a pipe write, and a re-render |
| A re-pressed key retires its release candidate (`released_at`) | Otherwise the stale timestamp forms a phantom neutral-transition sample after an overlap |
| A consumed overlap candidate returns immediately, accepted or discarded | Only a plain release with no candidate may seed `released_at`. Letting a discarded long overlap fall through leaves a stale release behind, and the next press fabricates a neutral transition that never happened |
