# LastKey Architecture and Working Reference

The only document in `docs/`: the current architecture, the contracts the implementation must
preserve, and the working rules for changes. Keep it limited to what is true of the current tree.
Change records, review logs, and finished migrations do not belong here — they live in git history.

## Process Architecture

```text
LastKey.Settings.exe                    LastKey.exe
on-demand Iced + wgpu                   resident tray + AppController
Settings / Measurement UI    <IPC>      settings and measurement lifecycle
display-only state                      InputService
                                                |
                                     SOCD / timing / delivery
                                                |
                                     Windows backend (primary)
                                     Linux backend (experimental)
```

`LastKey.exe` owns the tray and single-instance enforcement, saved settings and their persistence,
`AppController` with the capture and measurement lifecycles, the Windows hook, Raw Input, SOCD,
delivery, timing and synthetic output, and the IPC server including launching and focusing the
settings process.

`LastKey.Settings.exe` owns the Iced application and window state, screen composition and editing
widgets, and the IPC client. It never owns settings files, SOCD, hooks, `SendInput`, the scheduler,
platform handles, or raw measurement. It does not run during normal gameplay: no Iced thread,
allocation, wgpu device, or GPU context exists while only the runtime is running.

| Path | Responsibility |
| --- | --- |
| `src/core/` | Platform-neutral SOCD state, timing, delivery vocabulary, measurement, recommendation |
| `src/app/` | `AppController`, its `SettingsStore` / `RuntimeService` ports, snapshots and errors |
| `src/protocol.rs` | Versioned IPC commands, events, and framing |
| `src/platform/windows/` | Hook, Raw Input, `SendInput`, waitable timer, named pipe, UI server |
| `src/platform/linux/` | evdev capture and uinput output (experimental) |
| `src/ui/` | Iced application and IPC client (`iced-ui` feature only) |
| `src/bin/` | `lastkey` (runtime) and `lastkey-settings` (UI) binaries |

Start a task by reading this file, then `src/bin/lastkey.rs`, `src/platform/windows/input.rs`,
`ui_server.rs`, `ipc.rs`, `src/app/controller.rs`, `src/core/timing.rs`, and `src/settings.rs`.

## Runtime Invariants

1. Input correctness takes priority over UI convenience and architectural elegance.
2. `WH_KEYBOARD_LL` stays the primary Windows SOCD capture mechanism.
3. Raw Input is only a supporting path for mapping capture, measurement, and hook-health observation.
4. Synthetic output stays tagged `SendInput`, and that output never re-enters physical processing —
   on the hook path via `LLKHF_INJECTED` + `INJECTION_TAG`, on the raw path via the null-`hDevice`
   filter (`is_injected`).
5. Low-level hook callbacks never sleep, busy-wait, perform file I/O or UI work, or wait on IPC.
6. The direct low-latency path is preserved when timing is disabled.
7. The SOCD core stays platform-neutral, with independent horizontal and vertical axes.
8. Stale delayed work is cancelled on new physical state, settings changes, and measurement start
   or stop.
9. The UI never determines SOCD winners or latency-sensitive timing.
10. Measurement starts explicitly and observes only the configured physical keys.
11. Release builds never store typed characters or raw key histories.
12. UI crashes or IPC disconnections never interrupt SOCD processing.
13. The runtime stays fully functional without the settings UI running.
14. Linux abstractions are never expanded at the expense of Windows stability.

## Implementation Contracts

### AppController and the Apply transaction

The UI sends editing intent; it never owns authoritative settings.

```text
Iced widget state -> draft intent -> IPC -> AppController validation
  -> persist -> runtime activation -> authoritative snapshot -> Iced display state
```

`AppController` holds saved and draft settings separately, plus capture and measurement generations.
It keeps no mirror of the running configuration: the engine is authoritative, and
`reconcile_apply_outcome` queries it through the `ActiveSettings` fence rather than trusting a local
copy. Apply validates the draft, persists the candidate, activates the runtime synchronously, then
publishes the snapshot. On activation failure the previous file is written back; if that rollback
also fails, both errors are reported together. The UI never reports Apply success before the runtime
confirms persistence and activation.

### IPC

- `src/protocol.rs` defines version 2 of a length-prefixed JSON protocol with a 1 MiB frame limit.
  Version 2 dropped `UiSnapshot.active`, which always equalled `saved` and was never read. Both
  binaries ship together, so the exact-match version check turns a mismatched pair into a clear
  error instead of a deserialization failure.
- Frame length and protocol version are validated before deserialization.
- The named pipe rejects remote clients and carries a protected DACL allowing only the owning user
  and `SYSTEM`.
- Input service acknowledgements have a 5-second startup limit and a 2-second command limit. A failed
  Apply is reconciled through an `ActiveSettings` fence queued behind it: engine runs the candidate,
  adopt it; engine runs the old settings, roll back; fence unreachable, report `RuntimeUnconfirmed`
  rather than claiming a rollback.
- Accept failures back off for 100 ms; server shutdown uses `CancelSynchronousIo` with a 2-second
  limit.
- Each session performs every pipe syscall from exactly one thread: wait on the outbound queue with a
  timeout, drain outbound, `PeekNamedPipe` gated inbound read. Splitting one synchronous pipe across
  a reader and a writer thread deadlocks, because a pending blocking read stalls writes on a
  duplicate handle of the same file object. Server workers enqueue events for the pump thread rather
  than writing directly.
- The pump waits on its outbound channel (`recv_timeout`), so queued messages leave immediately. Only
  inbound discovery is bounded by the poll interval: 2 ms while active, backing off to 50 ms after
  250 ms of silence. Polling exists only while a settings session is connected; latency-sensitive
  input never crosses this pipe.

### Settings storage

- Installed mode: `%LOCALAPPDATA%\LastKey\settings.toml`
- Portable marker present: `settings.toml` beside the executable
- Primary file absent: a legacy fallback reads the existing file beside the executable
- Saving: write and `sync_all` a temporary file in the same directory, then replace the destination
  with `MoveFileExW` (`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`); remove the temporary
  file on failure

The MSIX `WindowsApps` directory is not writable, and a direct `fs::write` could leave a truncated
TOML file after an interrupted shutdown. The legacy file is never deleted automatically. The parser
recognizes only current field names: no released build ever wrote a settings file, so there is
nothing to migrate. A file that fails `validate()` — including one hand-edited past
`MAX_TIMING_MICROS` — is rejected whole: the runtime shows one error dialog and starts from full
defaults rather than clamping individual fields.

### Timing and measurement semantics

- Timing policy applies only when physical opposing keys actually overlap; transitions that are
  already neutral are left alone.
- Full overlap is expressed as a preservation rate of 100%, not a separate control.
- The UI works in 0.1 ms units; internally everything is integer microseconds, so scheduling never
  depends on floating point.
- Recommendations use the P10 minimum and P50 maximum with P90 as an exclusive ceiling, which keeps
  slow tails from widening the range.
- Samples below 1 ms are classified separately as near-simultaneous, because keyboard scanning and OS
  batching make ordering unreliable at that scale.
- Overlap samples longer than `MAX_PAIR_GAP` (1 s) are discarded, symmetric with neutral transitions,
  so a long hold cannot skew the recommended durations.
- Applied timing values share one ceiling, `MAX_TIMING_MICROS` (1000 ms), enforced by
  `Settings::validate` for the slider, typed, and recommendation paths alike. The UI's
  `MAX_TIMING_MILLIS` is derived from it; keep any future ceiling a round millisecond count so the
  `f32` derivation stays exact.
- `SampleStats` owns one distribution (count, min/max/latest, P10/P50/P90) and `push` is its only
  writer. Its fields are public for the protocol mapping; if it ever gains an invariant, reintroduce
  accessors at the same time.

The policy is fixed by the implementation and deterministic tests. The 1 ms threshold, the 10-sample
requirement, and aggregation across both axes still need testing on real hardware. Do not redesign
this policy as a side effect of unrelated work.

### Scheduling

Delayed work uses a high-resolution waitable timer on the Windows input thread rather than a general
`Scheduler` trait. The core exposes only `next_deadline()` and the platform owns the native timer,
which keeps the callback latency and synchronization contracts explicit.

### Delivery failure recovery

`SendInput` can fail — UIPI blocks injection into higher-privilege windows. `TimingController`
returns `EventDisposition::PassThrough` on exactly three paths, and in none of them has a synthetic
emission for that event succeeded, so replaying the original cannot double-deliver:

1. A key-up for a key that is neither physically held nor held in output
2. A failed release of the event's own key-up
3. A failed press of the original key-down

Windows honors `PassThrough` by calling `CallNextHookEx`; Linux honors it by replaying the original
event through the virtual device. The behavior table in `README.md` describes the user-visible result
and must stay in sync with `reconcile_immediate`. The delayed (timing-enabled) path has no physical
pass-through, because both keys are already held and the delayed release is still pending; that
exception is noted in the code.

### Settings UI

Design source: the preview under `stitch_ui_redesign_and_enhancement/` (ignored, not part of the
product). Everything on screen maps onto widgets and style structs the pinned Iced revision
(`f8127c8`) already provides — system fonts, no icon font, no canvas, no new dependency.
`src/ui/theme.rs` holds the palette, widget styles, and metrics; `src/ui/app.rs` holds the state
machine and both views.

- One window size (780×760) for both views, with fixed two-column layouts, so switching views never
  resizes. A manually resized window keeps its size; there is no responsive reflow. Iced's
  `responsive` widget would rebuild children inside a closure on every layout pass, which is not
  worth obscuring `settings_view` for a case the default size never hits.
- The page is `column![header, body]` plus a pinned action bar; there is no footer. The header holds
  the status dot and status text on the left and the Settings/Measurement switch on the right;
  connection state lives only there and in the disconnected body.
- Error and success feedback is plain text inside the action bar, never a toggling banner. The page
  is diffed positionally, so a banner appearing above the scrollable hands its state slot to another
  widget and resets the scroll offset; swapping only text moves nothing. While disconnected, errors
  surface in the waiting body instead, because the action bar needs a snapshot.
- `TimingField` is the single axis of variation for the timing card: `is_editable` defines the
  enabled gate for both the widget tree and `update`, and `micros` / `micros_mut` / `pair_invalid` /
  `buffer` derive every row value from the field, so a row cannot display one field while acting on
  another. `ms_field` builds duration rows only; the preservation rate is genuinely special (the
  "0 disables" rule) and keeps its own path.
- An untouched value box renders as a `facade_button` lookalike; pressing it swaps in the real input
  already focused with its content selected, so no caret flashes. Later presses hit the real box and
  place the caret natively. `editing` flags rearm on any focus move and on window unfocus, so the
  next press selects all again, Explorer-style. `value_box` is shared by the millisecond rows and the
  rate box so the two can never drift apart.
- Disabled timing groups stay on screen grayed out instead of collapsing, so toggling never moves the
  layout. The muted slider has no disabled widget state, so `update` additionally ignores its drags.
- A timing minimum above its maximum blushes both value boxes live; unparseable text blushes its own
  box. Only enabled groups highlight. A binding that duplicates another slot renders its row in
  `slot_error_style`.
- `ApplyRecommendations` copies ranges into the local draft only and switches to Settings scrolled to
  the timing card. Nothing is committed silently.
- Optical padding constants (`VALUE_BOX_PADDING`, `KBD_PADDING`, the inline value's top padding)
  compensate for glyph ink sitting high in its line box. They were tuned against screenshots at the
  current DPI; a different monospace face or scaling may need a pixel of adjustment. Each lives in
  one named constant.
- Two CSS properties have no Iced equivalent: `letter-spacing` (uppercase alone carries the axis
  titles) and `border-bottom-width` (the keycap's thicker bottom edge is a one-pixel shadow).
- winit registers its window class without icons and Iced exposes no icon API, so a throwaway thread
  in `src/bin/lastkey-settings.rs` sends this process's `LastKey*` windows the exe's embedded icon
  (`winres` id 1) as the small icon. It exits once the windows are found (10 s cap), never fails the
  app, and matches by title prefix — a renamed window would silently keep the generic glyph.

Known cosmetic limits, all harmless to behavior: the measurement view is shorter than 760 px so the
shared window leaves whitespace below it; a narrow window crowds rather than reflowing (the status
line ellipsizes); very large measurement counts could overflow the fixed 84 px stat tiles.

### Linux backend (experimental)

- Devices exposing all four configured keys are grabbed exclusively; every other key event is
  replayed through the uinput virtual device.
- The virtual device is excluded from the candidate scan by name, so LastKey never grabs its own
  output.
- Windows scan codes and evdev keycodes coincide for plain keys but not extended ones, so
  `linux_keycode` / `physical_from_linux` translate the arrow keys explicitly and reject bindings
  with no known Linux mapping instead of resolving them to the wrong key.
- The main loop waits until the next timing deadline but never longer than 5 ms, because `std` mpsc
  has no `select()` and a deadline-only wait would starve the other channel. Reader threads use
  non-blocking reads plus a short sleep so shutdown can join them without an extra wakeup fd.

Completing Linux hotplug, permissions, tray, packaging, and release support is out of scope.

## Contracts Hardened by Review — Do Not Regress

Each item looks removable until its reason is known. Changing one requires a replacement test that
fails against the old behavior.

### Capture and input

| Contract | Why it exists |
| --- | --- |
| Entering capture reconciles output, physical state, and pending work (`begin_capture`) | Otherwise a pre-held key's repeat is captured while its release is consumed, leaving output held with nothing down |
| Modifiers are never captured (`is_capture_eligible`, both input paths) | A bound modifier would be swallowed globally; the UI guidance promises they stay available |
| Capture consumes the next key-up for the captured key (`captured_key_awaiting_release`) | Capture already consumed the key-down; releasing only the key-up would deliver an unmatched event elsewhere |
| While capture is pending, a key-up for a *different* key passes through | A key held before capture started has no consumed key-down, so its release must reach applications |
| `process_hook` and `process_raw` share the same leading guards | Asymmetry lets auto-repeat of a captured key reach `observe_raw` as a false miss and trigger a spurious hook reinstall |
| Hook-health records only configured keys, outside capture and measurement | Otherwise every keystroke system-wide does queue work inside the latency-critical hook callback |
| Injected events are filtered by `LLKHF_INJECTED` + `INJECTION_TAG` before any borrow | Keeps LastKey's own output out of physical processing, and makes the common re-entry case allocation-free |
| Raw input with a null device handle is dropped before the union read (`is_injected`) | Covers hook-health, capture, and measurement with one guard. On systems where legitimate keystrokes carry no handle (some RDP/remote stacks), measurement and raw-fallback capture degrade silently while SOCD itself is unaffected, because the hook path does not use `hDevice` |
| `try_borrow_mut` on the hook and timer paths | `SendInput` from the hook's own thread can re-enter the hook; a panic there would abort across the `extern "system"` boundary and leave a synthetic key stuck down. The command path may use a plain borrow; these two may not |
| A lost hook releases output, then notifies once via thread message | With no hook, no release event will ever arrive to clear held output; notification fires on lost/recovered transitions only, after consecutive failures spaced by the reinstall cooldown, never on a timer |
| A failed scheduled release releases the opposite key instead | Never leave both directions of one axis held together |
| Delivery recovery is tested through `TimingController` | The retired `InputRouter` was a second copy of this policy that no shipping path executed |

### Lifecycle and IPC

| Contract | Why it exists |
| --- | --- |
| Every arming transient command carries a deadline (`Apply`, `Capture`, `StartMeasurement`) | A command queued behind a stall must not arm for a caller whose acknowledgement wait already expired. Disarming commands (`CancelCapture`, `StopMeasurement`) deliberately carry none: arriving late they only disarm, which is idempotent and desirable |
| A failed Apply is resolved by the `ActiveSettings` fence | It queues behind any late Apply, so its answer is authoritative; unreachable means `RuntimeUnconfirmed`, not a claimed rollback |
| Stopping measurement with no active session touches no timing state | `close_ui_session` stops unconditionally and also runs on a UI crash or disconnect; resetting there would release live output and interrupt SOCD (invariant 12) |
| `TimingController::reset_state()` at measurement boundaries | `release_all` alone leaves `physically_held` set, so a key released during measurement is treated as a repeat afterwards |
| The engine drops a measurement session whose consumer disconnected | Running on would bypass SOCD with no owner |
| Capture completions validate on the pump (`ServerEvent::KeyCaptureDone`) | Validating in the worker races a Revert processed between acceptance and enqueue |
| Measurement updates validate on the pump (`ServerEvent::MeasurementUpdated`) | A worker-validated update can otherwise surface inside the next session; the pump orders validation, application, and reply |
| `MeasurementUpdated` applies only while `measurement_active` | An update queued just before Stop is written after the stop reply; without this guard it revives the stopped session and overwrites the final statistics |
| Repeat and duplicate key events produce no measurement update | `observe` discards them, so sending was pure duplication — roughly thirty identical messages per second per held key, each costing a lock, a serialization, a pipe write, and a re-render |
| A re-pressed key retires its release candidate (`released_at`) | Otherwise the stale timestamp forms a phantom neutral-transition sample after an overlap |
| Single-threaded IPC pump per session | A pending blocking read stalls writes on a duplicate handle of the same synchronous pipe |
| Bounded input-service acknowledgements and IPC shutdown | An unbounded wait hangs the message loop or the runtime shutdown |
| Owner/SYSTEM-only pipe DACL | Another local user could otherwise read snapshots or occupy the single instance |
| Atomic settings replacement | A truncated TOML file loses all settings on the next launch |

### Settings UI

| Contract | Why it exists |
| --- | --- |
| Timing is locally authoritative until Apply; resets apply locally at click time | Nothing correlates a reply with its request, so an older Snapshot must not undo a revert; every Snapshot merges local timing while bindings converge on the last reply |
| Uncommitted text buffers count toward the dirty state | Typed text reaches the draft only on submit, so comparing drafts alone left Apply disabled — and the click dead — after typing into a clean window |
| Apply passes `draft.validate()` locally before any IPC | Turns a round-trip `ValidationFailed` into immediate feedback while the server stays the authoritative gate |
| A successful submit clears only the parse error it produced | The action bar is the only place a runtime failure is shown; clearing every error would hide a server message on an unrelated keystroke |

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

### Evaluated and declined

Reopening one of these is undoing a decision, not finding a defect. Each was measured against the
current tree and rejected for the reason given.

| Change | Why not |
| --- | --- |
| Merge the two IPC pump loops | Same cadence, different bodies; unifying needs generics plus callbacks and would bury the deadlock rule the comments state plainly |
| A `with_engine` helper for the `ENGINE.with(..)` sites | The hook and timer paths must keep `try_borrow_mut`; a convenience helper is what a future edit would reach for there too |
| A `with_emitter` helper in `InputEngine` | The rebuilds exist because the borrow checker splits `&self.settings` from `&mut self.timing`; hiding that helps nobody |
| Embed `SampleStats` in `MeasurementSnapshot` | Roughly 25 UI call sites for no behavioral gain, and it couples the wire format to a core type |
| Attribute-driven serde for `TimingSettings` | Saves ~50 lines but moves the stored-file contract out of one visible struct into scattered attributes |
| `[String; 5]` for `TimingInputs` | Deletes two matches, costs the named access that `from_timing` and the tests read by |
| A separate `DurationField` enum | Would remove one unreachable `expect` in `ms_field` at the cost of a second enum every reader must relate to the first. Revisit only if a third caller appears |
| Incremental percentiles or a smarter sample structure | `O(n)` insert on a human-bounded sample count; a heap buys nothing measurable |
| Split `src/platform/windows/input.rs` | Every part serves one thread and one `thread_local` engine; the file's comments carry that invariant continuously |
| Split `src/ui/app.rs` | Repeated unification already flattened the dense parts; a split now would be motion, not improvement |

## Packaging and Release

- Requirements: `Windows.Desktop` 10.0.19041 or later, x64 only, `runFullTrust`. Store submission
  packages are unsigned and rely on Store signing.
- `Cargo.toml` always names a version that has **not** shipped. The release workflow stamps the tag
  version before the `--locked` build, restores the files, then bumps the tree to the next version in
  the post-release commit. `winres` derives `FileVersion` from `CARGO_PKG_VERSION`, so there is one
  source of truth, and `validate-msix.ps1` compares all four numeric parts of both executables
  against the package version.
- Launching the installed settings UI from the tray requires a one-time user click check, because the
  process is created from inside the package. This is expected for an executable that is not an
  application entry point.
- The first settings UI launch can take 10–20 seconds for framework initialization; later launches
  connect in about a second.

## Verification

```powershell
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-default-features --features iced-ui --lib --bin lastkey-settings
cargo clippy --locked --no-default-features --features iced-ui --all-targets -- -D warnings
cargo check --locked --target x86_64-unknown-linux-gnu --all-targets
cargo tree --no-default-features -e normal
git diff --check
.github\scripts\Test-ReleaseFormDefault.ps1
```

- **The `iced-ui` run is not optional after a `src/ui/` change.** `src/lib.rs` gates `pub mod ui`
  behind the feature, so the default-feature test run never compiles that module. Clippy with
  `--all-features` type-checks its tests but does not execute them.
- Both Clippy configurations use `--all-targets` so test code is linted too.
- The default dependency tree must stay free of Iced and wgpu.
- Rebuilding fails with OS error 5 while the settings UI or the runtime holds `target\debug\*.exe`.
  Close it first, or run the `--lib` and integration targets, which do not link the binaries.
- Test counts are deliberately not recorded here; they change with every commit.

Before builds that need MSVC linking:

```bat
call "C:\Program Files\Microsoft Visual Studio\18\Insiders\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
```

Manual Windows validation has passed for tray launch and focus restoration, navigation and DPI
scaling, four-key capture and Apply, Revert and both restore scopes, measurement start/stop, normal
and forced UI closure, runtime operation with no UI, and pipe rejection for a separate local user.

## Remaining Work

- Store submission, plus coverage on 10.0.19041 (the minimum) and Windows 11.
- Real-hardware validation of the 1 ms near-simultaneous threshold and the 10-sample recommendation
  requirement.
- Live Linux validation: exclusive `grab` and virtual-device replay are verified by code review and
  cross-compilation only.
- A live-fire check that injected input is what the null-`hDevice` filter drops; the rule rests on
  documented behavior plus a throwaway probe, not an in-tree end-to-end run.
- End-to-end tests only if manual validation shows a regression risk, and without adding abstractions
  to production code to enable them: session cleanup when a client dies, runtime shutdown with no UI
  ever connected, `UiServer` shutdown while a UI stays connected, malformed JSON over a real
  `PipeConnection`.

## Working Rules

- Repository: `C:\Users\Administrator\Documents\GitHub\LastKey`, branch `dev`. Work inside it; do not
  create a separate worktree.
- Existing edits and untracked files may be user work: do not reset, check out, bulk-format,
  overwrite, or delete them without asking. Do not stage, commit, or push unless asked.
- `.claude/` (agent session state) and `stitch_ui_redesign_and_enhancement/` (UI mockups) are ignored
  and are not part of the product.
- Preserve CRLF line endings and UTF-8 without BOM in text files.

### Commit messages — Conventional Commits

```text
<type>(<scope>): <description>

[optional body]

[optional footer]
```

- **Types**: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `ci`, `build`, `chore`.
- **Scope**: the area touched — `core`, `timing`, `socd`, `input`, `ipc`, `ui`, `settings`,
  `measurement`, `linux`, `msix`, `packaging`, `release`. Omit only for a repository-wide change.
- **Description**: imperative mood, lowercase, no trailing period, subject within ~72 characters.
- **Body**: explain *why*, not *what*. Name the invariant or contract a change protects.
- **Breaking changes**: `!` after the scope plus a `BREAKING CHANGE:` footer. Bumping
  `PROTOCOL_VERSION` is always breaking.

The release workflow depends on this convention: it commits the version bump as
`chore(release): prepare <tag>`.
