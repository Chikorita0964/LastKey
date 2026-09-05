# LastKey Architecture and Working Reference

## Purpose

This is the single reference document for LastKey: the current architecture, the contracts the
implementation must preserve, verification results, and the working rules for changes.

Earlier planning and migration narrative — the C++ tray program, the Slint-based single process, the
phase-by-phase Iced migration, and the GPL-to-MIT transition — has been removed. Those transitions
are finished and remain in git history. Keep this file limited to what is true of the current tree,
and update it when a decision changes.

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

`LastKey.Settings.exe` does not run during normal gameplay. No Iced thread, allocation, wgpu device,
or GPU context exists while only the runtime is running.

### Ownership by process

`LastKey.exe` owns:

- The tray and runtime single-instance enforcement
- Authoritative saved and active settings, and file persistence
- `AppController`, plus the capture and measurement lifecycles
- Windows hooks, Raw Input, SOCD, delivery, timing, and synthetic output
- The IPC server, and launching, focusing, and shutting down the settings process

`LastKey.Settings.exe` owns:

- The Iced application and window state
- Screen composition, theme, editing widgets, and display snapshots
- The IPC client and error presentation

The settings process never owns settings files, SOCD, hooks, `SendInput`, the scheduler, platform
handles, or raw measurement.

### Source layout

| Path | Responsibility |
| --- | --- |
| `src/core/` | Platform-neutral SOCD state, timing, delivery vocabulary, measurement, recommendation |
| `src/app/` | `AppController`, its `SettingsStore` / `RuntimeService` ports, snapshots and errors |
| `src/protocol.rs` | Versioned IPC commands, events, and framing |
| `src/platform/windows/` | Hook, Raw Input, `SendInput`, waitable timer, named pipe, UI server |
| `src/platform/linux/` | evdev capture and uinput output (experimental) |
| `src/ui/` | Iced application and IPC client (`iced-ui` feature only) |
| `src/bin/` | `lastkey` (runtime) and `lastkey-settings` (UI) binaries |

## Runtime Invariants

1. Input correctness takes priority over UI convenience and architectural elegance.
2. `WH_KEYBOARD_LL` stays the primary Windows SOCD capture mechanism.
3. Raw Input is only a supporting path for mapping capture, measurement, and hook-health observation.
4. Synthetic output stays tagged `SendInput`, and that output never re-enters physical processing.
5. Low-level hook callbacks never sleep, busy-wait, perform file I/O or UI work, or wait on IPC.
6. The direct low-latency path is preserved when timing is disabled.
7. The SOCD core stays platform-neutral, with independent horizontal and vertical axes.
8. Stale delayed work is cancelled on new physical state, settings changes, and measurement
   start or stop.
9. The UI never determines SOCD winners or latency-sensitive timing.
10. Measurement starts explicitly and observes only the configured physical keys.
11. Release builds never store typed characters or raw key histories.
12. UI crashes or IPC disconnections never interrupt SOCD processing.
13. The runtime stays fully functional without the settings UI running.
14. Linux abstractions are never expanded at the expense of Windows stability.

Behavior covered by these invariants: Last Input Priority, axis independence, repeated key-down
handling, delivery failure recovery, four-key mapping, Transition Delay / Preserve Overlap /
probability semantics, stale timer cancellation, measurement classification with P10/P50/P90, the
recommendation algorithm, high-resolution waitable timers, hook health recovery, and a single
runtime instance.

## Implementation Contracts

### AppController and the Apply transaction

The UI sends editing intent; it never owns authoritative settings.

```text
Iced widget state -> draft intent -> IPC -> AppController validation
  -> persist -> runtime activation -> authoritative snapshot -> Iced display state
```

`AppController` holds saved, active, and draft settings separately, plus capture and measurement
generations. Apply runs in this order: validate the draft, persist the candidate, activate the
runtime synchronously, then publish the snapshot. If activation fails, the previous settings file is
written back; if that rollback also fails, both errors are reported together. The UI never reports
Apply success before the runtime confirms persistence and activation.

### IPC

- `src/protocol.rs` defines version 1 of a length-prefixed JSON protocol with a 1 MiB frame limit.
- Frame length and protocol version are validated before deserialization.
- The named pipe rejects remote clients and carries a protected DACL allowing only the owning user
  and `SYSTEM`.
- Input service acknowledgements have a 5-second startup limit and a 2-second command limit.
- Accept failures back off for 100 ms; server shutdown uses `CancelSynchronousIo` with a 2-second
  limit.
- Each session performs every pipe syscall from exactly one thread: drain outbound, `PeekNamedPipe`
  gated inbound read, then sleep for `IPC_POLL_INTERVAL` (2 ms). Splitting one synchronous pipe
  across a reader and a writer thread deadlocks, because a pending blocking read stalls writes on a
  duplicate handle of the same file object. Server workers enqueue events for the pump thread rather
  than writing directly.
- The 2 ms poll exists only while a settings session is connected. Latency-sensitive input never
  crosses this pipe.

### Settings storage

- Installed mode: `%LOCALAPPDATA%\LastKey\settings.toml`
- Portable marker present: `settings.toml` beside the executable
- Primary file absent: a legacy fallback reads the existing file beside the executable
- Saving: write and `sync_all` a temporary file in the same directory, then replace the destination
  with `MoveFileExW` (`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`); remove the temporary
  file on failure

The MSIX `WindowsApps` directory is not writable, and a direct `fs::write` could leave a truncated
TOML file after an interrupted shutdown. The legacy file is never deleted automatically.

### Timing and measurement semantics

| Earlier model | Current decision | Reason |
| --- | --- | --- |
| A general delay including neutral transitions | Apply timing policy only when physical opposing keys actually overlap | Do not alter transitions that are already neutral |
| A separate Full Overlap checkbox | Full overlap is a preservation rate of 100% | One probability model instead of two overlapping controls |
| Integer milliseconds | 0.1 ms UI units, integer microseconds internally | Represent short physical intervals without floating-point scheduling |
| Recommendations centered on averages | P10 minimum, P50 maximum, P90 as an exclusive ceiling | Reduce the influence of slow tails |
| All short samples included | Samples below 1 ms classified separately as near-simultaneous | Keyboard scanning and OS batching make ordering unreliable at that scale |

The policy is fixed by the implementation and deterministic tests. The 1 ms threshold, the 10-sample
requirement, aggregation across both axes, and perceived quality still need testing on real hardware.
Do not redesign this policy as a side effect of unrelated work.

### Input capture and scheduling

Delayed work uses a high-resolution waitable timer on the Windows input thread rather than a general
`Scheduler` trait. The core exposes only `next_deadline()`, and the platform owns the native timer,
which keeps the callback latency and synchronization contracts explicit. The Linux backend has its
own polling loop; see below.

### Delivery failure recovery

`SendInput` can fail — UIPI blocks injection into higher-privilege windows. `TimingController`
returns `EventDisposition::PassThrough` on exactly three paths, and in none of them has a synthetic
emission for that event succeeded, so replaying the original event cannot double-deliver:

1. A key-up for a key that is neither physically held nor held in output
2. A failed release of the event's own key-up
3. A failed press of the original key-down

Windows honors `PassThrough` by calling `CallNextHookEx`; Linux honors it by replaying the original
event through the virtual device. The behavior table in `README.md` describes the user-visible
result and must stay in sync with `reconcile_immediate`. The delayed (timing-enabled) path has no
physical pass-through, because both keys are already held and the delayed release is still pending;
that exception is noted in the code.

### Linux backend (experimental)

- Devices that expose all four configured keys are grabbed exclusively; every other key event is
  replayed through the uinput virtual device.
- The virtual device is excluded from the candidate scan by name, so LastKey never grabs its own
  output.
- Windows scan codes and evdev keycodes coincide for plain keys but not for extended ones, so
  `linux_keycode` / `physical_from_linux` translate the arrow keys explicitly and reject bindings
  with no known Linux mapping instead of resolving them to the wrong key.
- The main loop waits until the next timing deadline but never longer than 5 ms, because `std` mpsc
  has no `select()` and a deadline-only wait would starve the other channel. Reader threads use
  non-blocking reads plus a short sleep so shutdown can join them without an extra wakeup fd.

Completing Linux hotplug, permissions, tray, packaging, and release support is out of scope.

## Contracts Hardened by Review — Do Not Regress

Each item below looks removable until its reason is known. Changing any of them requires a
replacement test that fails against the old behavior.

| Contract | Why it exists |
| --- | --- |
| Capture consumes the next key-up for the captured key (`captured_key_awaiting_release`) | Capture already consumed the key-down; releasing only the key-up would deliver an unmatched event to other applications |
| While capture is pending, a key-up for a *different* key passes through | A key held before capture started has no consumed key-down, so its release must reach applications |
| `process_hook` and `process_raw` share the same leading guards | Asymmetry lets auto-repeat of a captured key reach `observe_raw` as a false miss and trigger a spurious hook reinstall |
| Hook-health records only configured keys, outside capture and measurement | Otherwise every keystroke system-wide does queue work inside the latency-critical hook callback |
| `TimingController::reset_state()` at measurement boundaries | `release_all` alone leaves `physically_held` set, so a key released during measurement is treated as a repeat afterwards |
| `try_borrow_mut` on the hook and timer paths | `SendInput` from the hook's own thread can re-enter the hook; a panic there would abort across the `extern "system"` boundary and leave a synthetic key stuck down |
| Injected events are filtered by `LLKHF_INJECTED` + `INJECTION_TAG` before any borrow | Keeps LastKey's own output out of physical processing, and makes the common re-entry case allocation-free |
| A failed scheduled release releases the opposite key instead | Never leave both directions of one axis held together |
| Single-threaded IPC pump per session | A pending blocking read stalls writes on a duplicate handle of the same synchronous pipe |
| Atomic settings replacement | A truncated TOML file loses all settings on the next launch |
| Owner/SYSTEM-only pipe DACL | Another local user could otherwise read snapshots or occupy the single instance |
| Bounded input-service acknowledgements and IPC shutdown | An unbounded wait hangs the message loop or the runtime shutdown |
| Delivery recovery tested through `TimingController` | The retired `InputRouter` was a second copy of this policy that no shipping path executed |

## Non-Goals

- Replacing `WH_KEYBOARD_LL` with Raw Input as the primary path
- Changing `SendInput` ownership or the SOCD algorithm without separate justification
- Redesigning timing semantics or the measurement recommendation policy
- Linking Iced or wgpu into the resident runtime, or running them permanently
- Making the UI process authoritative for settings persistence
- Introducing a general-purpose distributed-systems-style IPC framework
- Reintroducing input logging or raw key-history diagnostics, including behind a feature gate. If
  new diagnostics are needed, design them around aggregate counters and explicit consent.
- Splitting the crate into a Cargo workspace purely to rearrange files. Logical boundaries already
  hold through `src/app`, `src/protocol.rs`, separate binaries, and Cargo features. Reconsider only
  if a concrete coupling problem or an independent distribution requirement appears.

## Beta Compatibility Baseline

Verification environment: Windows 10 Pro build 10.0.26200 x64; application version 0.1.0
(runtime plus Iced settings UI). Package requirements: `Windows.Desktop` 10.0.19041 or later, x64
only, `runFullTrust`.

| Flow | Result |
| --- | --- |
| Portable ZIP with marker | Apply round trips persist to `settings.toml` beside the executable |
| Installed storage location | `%LOCALAPPDATA%\LastKey\settings.toml`, no virtualization |
| Sideload installation and removal | Install, launch, and removal verified with a test ID; an existing 1.0.2 installation was unaffected |
| Start menu registration | Registered on install, cleaned up on removal |
| IPC snapshot and Apply round trips | Verified in installed and development builds |
| Runtime without UI | Tray residency and continued pipe acceptance verified |
| 15-minute idle soak | Memory stable at 11 MB, handles stable at 154, cumulative CPU time 0 s, 15/15 pipe connections accepted |

Known limitations:

- Launching the installed settings UI from the tray requires a one-time user click check, because
  the process is created from inside the package. Direct launch from outside the package is blocked
  by platform policy; this is expected for an executable that is not an application entry point.
- The first settings UI launch can take 10–20 seconds for framework initialization. Later launches
  connect in about a second.
- Window resizing when switching between Settings and Measurement keeps the OS's asynchronous
  behavior. Each view retains its own window size.
- Store submission packages are unsigned and rely on Store signing. Local installation tests need
  separate signing, so the Beta uses the Store distribution path.

Environment coverage to complete during Beta:

| OS build | Architecture | Installation | Result |
| --- | --- | --- | --- |
| 10.0.26200 Pro | x64 | Sideload with test ID | Passed (baseline) |
| 10.0.19041 or later | x64 | Store | Not verified (minimum version) |
| Windows 11 | x64 | Store / ZIP | Not verified |

## Verification

```powershell
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --no-default-features --features iced-ui --lib --bin lastkey-settings
cargo clippy --no-default-features --features iced-ui --lib --bin lastkey-settings -- -D warnings
cargo check --target x86_64-unknown-linux-gnu --lib
cargo tree --no-default-features -e normal
git diff --check
```

Latest results: 81 tests pass (lib 39, delivery 9, measurement 9, settings 13, timing 11); Clippy is
clean with warnings as errors on both configurations; formatting passes. The default dependency tree
contains no Iced or wgpu. Unsigned 0.1.0 MSIX creation, extraction, and required-file checks pass via
`msix/package-msix.ps1` and `msix/validate-msix.ps1`.

Manual Windows integration validation has passed for all of: tray launch and focus restoration;
navigation, window size, DPI, and scrolling; four-key capture and Apply; Revert and both Restore
scopes; measurement start/stop and repeated sessions; normal and forced UI closure and runtime
shutdown; runtime operation with no UI; perceived performance; and pipe rejection for a separate
local user, which confirms the owner/SYSTEM-only DACL.

Before builds that need MSVC linking:

```bat
call "C:\Program Files\Microsoft Visual Studio\18\Insiders\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
```

## Remaining Work

- Store submission and the additional environment coverage listed above.
- End-to-end tests that are not yet automated, to be added only if manual validation shows a
  regression risk — and without adding abstractions to production code merely to enable them:
  session cleanup when a connected client terminates unexpectedly; runtime shutdown when the UI has
  never connected; `UiServer` shutdown within its limit while a directly launched UI stays
  connected; connection isolation for malformed JSON over a real `PipeConnection`.
- Real-hardware validation of the 1 ms near-simultaneous threshold and the 10-sample recommendation
  requirement.
- Live Linux validation: exclusive `grab` plus virtual-device replay have been verified by code and
  cross-target compilation only.

## Working Rules

- Repository: `C:\Users\Administrator\Documents\GitHub\LastKey`, branch `dev`.
- The working tree intentionally carries extensive changes. Do not reset, check out, bulk-format,
  overwrite, or delete existing edits and untracked files; they may be user work. `.claude/` is
  untracked user state — leave it alone.
- Work inside the repository above; do not create a separate worktree.
- Preserve CRLF line endings and UTF-8 without BOM in text files.
- Do not stage, commit, or push unless asked.

### Commit messages — Conventional Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) for every commit:

```text
<type>(<scope>): <description>

[optional body]

[optional footer]
```

- **Types**: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `ci`, `build`, `chore`.
- **Scope**: the area touched — `core`, `timing`, `socd`, `input`, `ipc`, `ui`, `settings`,
  `measurement`, `linux`, `msix`, `packaging`, `release`. Omit it only when a change is genuinely
  repository-wide.
- **Description**: imperative mood, lowercase, no trailing period, and short enough that the subject
  line stays within about 72 characters.
- **Body**: explain *why*, not *what* — the diff already shows what changed. Reference the invariant
  or contract a change protects when one applies.
- **Breaking changes**: append `!` after the scope (`feat(protocol)!: ...`) and add a
  `BREAKING CHANGE:` footer. Bumping `PROTOCOL_VERSION` is always breaking.

Examples from this repository's history:

```text
feat(timing): refine input timing and measurement workflow
feat(linux): add evdev and uinput input backend
ci(packaging): automate Rust MSIX release validation
chore(release): prepare v1.0.3
```

The release workflow depends on this convention: `.github/workflows/release.yml` commits the next
version bump as `chore(release): prepare <tag>`.

## Files to Read Before Starting a Task

1. `docs/lastkey-architecture.md` (this file)
2. `src/bin/lastkey.rs`
3. `src/platform/windows/input.rs`
4. `src/platform/windows/ui_server.rs`
5. `src/platform/windows/ipc.rs`
6. `src/app/controller.rs`
7. `src/core/timing.rs`
8. `src/settings.rs`
9. `src/ui/app.rs`
