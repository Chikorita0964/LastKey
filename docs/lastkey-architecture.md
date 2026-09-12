# LastKey Architecture and Working Reference

Entry point for the product documentation. This file owns the process map and the runtime
invariants; every contract that elaborates on them lives in `architecture/`. Keep each file limited
to what is true of the current tree. Change records, review logs, and finished migrations belong in
git history, not here.

## Reading Map

Read this file, then only the row that matches the change.

| Change | Read |
| --- | --- |
| Engine, IPC, storage, timing, delivery, profiles, Linux | `architecture/engine.md` |
| Settings UI: composition, rendering, localization | `architecture/ui.md` |
| Any change, before calling it done | `architecture/verification.md` |
| Packaging, release, commit messages | `architecture/release.md` |
| Proposing something the project already rejected | `architecture/decisions.md` |

Each contract file carries a **Do Not Regress** section for its own area. Those entries look
removable until their reason is known; changing one requires a replacement test that fails against
the old behavior.

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

`LastKey.exe` owns everything authoritative: tray and single-instance enforcement, settings
persistence, `AppController` with the capture and measurement lifecycles, hook, Raw Input, SOCD,
delivery, timing, synthetic output, and the IPC server that launches and focuses the settings
process. `LastKey.Settings.exe` owns only the Iced application, screen composition, and the IPC
client — never settings files, SOCD, hooks, `SendInput`, the scheduler, platform handles, or raw
measurement. It does not run during normal gameplay: no Iced thread, allocation, wgpu device, or GPU
context exists while only the runtime is running.

| Path | Responsibility |
| --- | --- |
| `src/core/` | Platform-neutral SOCD state, timing, delivery vocabulary, measurement, recommendation |
| `src/app/` | `AppController`, its `SettingsStore` / `RuntimeService` ports, snapshots and errors |
| `src/protocol.rs` | Versioned IPC commands, events, and framing |
| `src/platform/windows/` | Hook, Raw Input, `SendInput`, waitable timer, named pipe, UI server |
| `src/platform/linux/` | evdev capture and uinput output (experimental) |
| `src/ui/` | Iced application and IPC client (`iced-ui` feature only) |
| `src/bin/` | `lastkey` (runtime) and `lastkey-settings` (UI) binaries |

Start a task by reading this file and the row above, then `src/bin/lastkey.rs`, `src/platform/windows/input.rs`,
`ui_server.rs`, `ipc.rs`, `src/app/controller.rs`, `src/core/timing.rs`, and `src/settings.rs`.

## Runtime Invariants

1. Input correctness takes priority over UI convenience and architectural elegance.
2. `WH_KEYBOARD_LL` stays the primary Windows SOCD capture mechanism.
3. Raw Input is only a supporting path for mapping capture, measurement, and hook-health observation.
4. Synthetic output stays tagged `SendInput` and never re-enters physical processing — on the hook
   path via `LLKHF_INJECTED` + `INJECTION_TAG`, on the raw path via the null-`hDevice` filter.
5. Low-level hook callbacks never sleep, busy-wait, perform file I/O or UI work, or wait on IPC.
6. The direct low-latency path is preserved in `Immediate` mode.
7. The SOCD core stays platform-neutral, with independent horizontal and vertical axes.
8. Stale delayed work is cancelled on new physical state, settings changes, and measurement start
   or stop.
9. The UI never determines SOCD winners or latency-sensitive timing.
10. Measurement starts explicitly and observes only the configured physical keys.
11. Typed characters and key histories are never persisted. The explicit timeline exception below
    allows bounded, memory-only display history for the four mapped keys.
12. UI crashes or IPC disconnections never interrupt SOCD processing.
13. The runtime stays fully functional without the settings UI running.
14. Linux abstractions are never expanded at the expense of Windows stability.
