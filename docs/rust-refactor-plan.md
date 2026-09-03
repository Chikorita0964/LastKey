# LastKey Rust Refactor and Feature Expansion Plan

**Status:** Proposed RFC  
**Audience:** Maintainers and contributors  
**Purpose:** Move LastKey to a Rust-first, cross-platform architecture while preserving its default user-facing SOCD behavior and low-latency input path.

## 1. Goals

LastKey will be rebuilt around a shared Rust core and a Slint configuration UI.

- Preserve Last Input Priority SOCD behavior for the default W/S and A/D mappings.
- Keep Windows input capture based on `WH_KEYBOARD_LL` and output injection based on `SendInput`.
- Keep the default disabled timing path direct and independent from delayed scheduling.
- Add runtime key mapping, configurable transition and overlap timing, and opt-in input timing measurement.
- Add Linux support later through `evdev` input and `uinput` output.
- Retain Windows MSIX and Microsoft Store distribution.
- Keep all input processing local, with no keystroke logging or network transmission.

This is a Rust-first refactor. The existing C++ implementation remains a behavioral and recovery-rule reference during migration, but Rust is the new implementation baseline rather than a line-by-line port.

## 2. Architecture Principles

The SOCD engine must not know which platform captures or emits keyboard events. The UI must not make SOCD or timing decisions.

```text
                    Slint UI
                       |
                 App Controller
                       |
                LastKey Core
       +---------------+---------------+
       |               |               |
   SocdState     TimingController   Measurement
       |               |               ^
       +---- desired --+               |
                       |               |
             Platform input/output layer
                       |
             +---------+---------+
             |                   |
          Windows               Linux
    WH_KEYBOARD_LL / SendInput  evdev / uinput
```

The physical input flow is:

```text
Physical event
  -> platform backend
  -> mapping and input routing
  -> SocdState determines the desired output
  -> TimingController determines when to transition
  -> platform backend emits the output event
```

Measurement observes eligible physical events at the input boundary. It never observes LastKey-generated synthetic events.

## 3. Component Responsibilities

### SocdState

`SocdState` is pure shared logic. It tracks the configured logical pairs and determines the desired output for each axis using Last Input Priority.

- It has no Win32, Linux, Slint, timer, or I/O dependencies.
- Its desired result for each axis is always zero or one key.
- Repeated key-down events do not change priority.
- Releasing the current winner restores the still-held opposing key when applicable.

### TimingController

`TimingController` reconciles the desired output with the actual emitted output.

- With timing disabled, it selects the immediate path.
- For an opposing-key switch, it chooses either a neutral transition gap or a temporary overlap.
- It owns per-axis pending-transition tokens or generations.
- It cancels stale delayed work after relevant input or settings changes.
- It never blocks the keyboard hook.

### Platform Input/Output Layer

Platform backends capture physical input, translate physical keys, emit output, and report delivery outcomes.

- Windows uses `WH_KEYBOARD_LL` and `SendInput`.
- Linux will use `evdev` and `uinput`.
- Backends own platform-specific event loops, injection tagging, device handling, and scheduler integration.
- The core receives platform-neutral physical and logical key representations.

### App Controller and Slint UI

The App Controller applies validated settings, coordinates lifecycle operations, and publishes UI state. Slint is the configuration and status interface only.

- The UI never runs SOCD logic or latency-sensitive scheduling.
- Applying key mapping changes safely invalidates pending work, releases existing output, resets input state, and atomically installs the new mapping.
- Tray integration may remain platform-specific.

## 4. Threading and Event Loops

Windows UI and input handling will use separate execution paths.

- The UI thread owns Slint windows and UI updates.
- The Windows input thread owns the low-level hook and its required message loop.
- The scheduler uses a worker or native timer mechanism and remains idle without pending work.
- Communication between these components is limited to explicit messages and small shared state.

The hook callback must remain minimal. It must not sleep, perform file I/O, wait on long locks, execute UI work, or busy-spin.

## 5. Output Delivery and Failure Recovery

Input correctness is more important than attempting every requested transition. The Rust design must preserve safe behavior when platform output delivery fails.

The output layer tracks both key state and output ownership:

```text
NotHeld
SyntheticHeld
PhysicalPassThroughHeld
```

This distinction allows the router to safely decide whether an original physical event may pass through when synthetic delivery fails.

The Milestone 1 test suite will specify recovery behavior for at least:

- a desired key-down that cannot be injected;
- a previous key-up that cannot be injected during a switch;
- a desired key-down that fails after the previous output was released;
- a key-up that cannot be injected;
- repeated physical input events;
- LastKey-generated synthetic events; and
- shutdown while output is held.

The invariant is that a delivery failure must not create conflicting opposing output or leave a key stuck.

## 6. Default Behavior and Performance

Default settings preserve the existing user-facing behavior:

```text
Pair 1: W <-> S
Pair 2: A <-> D

SOCD Transition Delay: off (configured 2.0..4.0 ms)
Preserve Overlap: off
Overlap Preservation Rate: 50% configured, 0% effective
Preserved Overlap Duration: 2.0..6.0 ms
```

With these settings, a configured input follows the direct path:

```text
WH_KEYBOARD_LL -> mapping -> SocdState -> SendInput
```

The direct path must not enqueue scheduler work merely because timing support is available. UI rendering, measurement, persistence, and Linux abstractions must not be on this path.

Rust is not required to duplicate the C++ implementation internally. It must instead meet the following behavioral and safety requirements:

- Last Input Priority is correct for both axes.
- Generated input is not processed recursively.
- The hook is non-blocking.
- Failed output delivery remains safe.
- The default path avoids delayed scheduling.

## 7. Runtime Key Mapping and Settings

Users can change all four pair keys without recompiling.

- Bindings use physical keyboard identity where practical, not layout-dependent text.
- Windows may use scan code plus extended-key information internally.
- The UI displays human-readable names such as `W`, `Left Arrow`, `Right Ctrl`, and `Numpad 4`.
- Mouse input cannot become a pair binding.
- All four configured keys must be valid and unique.
- Apply validates before modifying active settings.
- Cancel leaves active settings unchanged.
- Restore Defaults returns to W/S and A/D.

Persisted settings may include mappings and timing configuration. The persistence format, location, schema version, and corrupt-file recovery behavior remain modular decisions.

## 8. Timing Features

Timing policy is evaluated only when opposing physical keys actually overlap. Natural neutral
transitions are not delayed or converted into overlaps.

- **SOCD Transition:** release the old output, wait for a randomized SOCD Transition Delay,
  then press the new output.
- **Preserved Overlap:** press the new output, keep both outputs pressed for a randomized
  Preserved Overlap Duration, then release the old output.

With SOCD Transition Delay disabled, every detected physical overlap is resolved immediately and
Preserve Overlap is unavailable. Enabling SOCD Transition Delay activates its configured range
and makes Preserve Overlap available. With Preserve Overlap enabled, Overlap Preservation Rate
independently determines which detected physical overlaps are preserved; the remainder use the
SOCD Transition Delay. A rate of 100% replaces the former Full Overlap option. Disabled features
retain their configured numeric values so they can be enabled again without re-entering them.

During normal operation and Transition, an axis has at most one actual output held. During an intentional Overlap, an axis may temporarily have both opposing outputs held. This is an explicit feature state, not a general SOCD invariant.

Each axis has independent pending work. A horizontal transition must not delay, replace, or cancel a vertical transition.

## 9. Input Timing Measurement

Measurement is an explicit, user-started mode for configured pair keys only.

- It observes physical key edges only while measurement is active.
- Synthetic LastKey output is excluded.
- It uses a high-resolution monotonic timestamp source at the input boundary.
- A positive result represents a neutral transition; a negative result represents overlap.
- Timing values exist only in memory for the active measurement session.
- No text history, raw keystroke log, or raw timing samples are written to disk or transmitted.
- Only user-approved final settings may be persisted.

The measurement UI reports the median, P10, P90, minimum, and maximum for physical neutral
transitions and physical overlaps. Paired edges less than 1 ms apart are classified separately
as near-simultaneous and excluded from both distributions because keyboard scanning and OS event
batching cannot reliably establish their intended order. After at least ten classified samples
of a pattern, the isolated recommendation layer uses P10 as the suggested minimum and the median
(P50) as the suggested maximum. Both are rounded to the nearest 0.1 ms without applying a fixed
recommendation floor, and the resulting range is capped strictly below P90. Timing settings are
entered in 0.1 ms increments and scheduled with integer microsecond durations. P90 remains visible
as reference data and acts only as the exclusive upper bound. Both configured axes and directions
are combined because the current timing settings are global.

Session duration remains intentionally open. Pairing and recommendation remain isolated from
event capture so their algorithms can evolve without changing privacy or input-routing behavior.

## 10. Platform Scope

### Windows

Windows remains the first supported platform.

- Capture: `WH_KEYBOARD_LL`
- Output: `SendInput`
- UI: Slint
- Distribution: MSIX and Microsoft Store
- Existing injection tagging/filtering behavior is preserved.

### Linux

Linux work begins only after the Windows Rust implementation is stable.

- Capture: `evdev`
- Output: `uinput`
- Physical devices may need to be grabbed and proxied so applications do not receive both original and virtual pair-key events.
- The design must cover permissions, device discovery, hotplugging, multiple keyboards, ordinary-key forwarding, and safe recovery after unexpected exit.

## 11. Delivery Milestones

1. **Rust foundation and Windows parity**
   - Establish the Rust build, shared core types, tests, Windows hook backend, direct output path, and delivery-recovery model.
   - Keep C++ available as the behavior reference until this checkpoint is accepted.

2. **Configuration and Slint UI**
   - Add validated settings, local persistence, runtime mapping, restore defaults, Slint UI, and tray integration.

3. **Timing engine**
   - Add timing models, random selection, asynchronous scheduling, stale-work cancellation, timing UI, and regression tests.

4. **Measurement**
   - Add opt-in physical-event measurement, statistics, UI workflow, and an isolated recommendation layer.

5. **Linux support**
   - Implement and validate the `evdev`/`uinput` backend and platform lifecycle handling.

6. **Packaging and documentation**
   - Automate Rust/Slint MSIX packaging, update README and privacy documentation when features ship, and perform release validation.

## 12. Non-Goals

Unless separately approved, this project will not:

- replace the Windows SOCD path with Raw Input;
- add a kernel driver;
- sleep or busy-wait in the low-level keyboard hook;
- use Slint timers for latency-sensitive output scheduling;
- persist raw measurement samples or typed text;
- send measurement or input data over the network;
- implement Linux before Windows Rust behavior is stable; or
- combine the Rust migration and all new features in one patch.

## 13. Acceptance Criteria

The refactor is ready to progress when the active milestone's behavior is covered by deterministic tests and its defaults remain safe.

- The Rust core is platform-neutral and testable without real keyboard input.
- Default Windows behavior provides correct Last Input Priority for W/S and A/D.
- Timing-disabled input uses the direct path and does not block the hook.
- Output delivery failures cannot produce opposing output or stuck keys.
- Runtime mappings are validated, unique, and safely applied.
- Intentional overlap is limited to the configured timing state.
- Measurement is opt-in, physical-only, memory-only, and local.
- Windows MSIX distribution remains available before release.

## 14. Open Decisions

The following are intentionally modular and require a later focused decision:

- the exact `SendInput` failure-recovery state machine;
- scheduler/timer implementation and supported delay bounds;
- settings storage location and file format;
- measurement pairing, near-zero threshold, and recommendation algorithm;
- Linux tray, packaging, multi-keyboard, and device-recovery policy; and
- final Slint page design.
