# Verification


Two different things, and conflating them wastes the most time: the **scoped loop** you run while
iterating on a change, and the **full gate** you run once before committing.

## Scoped loop (while working)

Run the smallest set that can fail because of what you touched. Compilation dominates the cost here,
not the tests — switching feature sets rebuilds the world, and the `egui-ui` set drags in wgpu — so
stay inside one feature set unless the change genuinely spans both.

| Changed | Run |
| --- | --- |
| `src/core/timing.rs`, `socd.rs`, `delivery.rs` | `cargo test --test timing --test delivery_recovery` |
| `src/core/measurement.rs`, `recommendation.rs` | `cargo test --test measurement` |
| `src/settings.rs`, `src/protocol.rs` — messages, validation text, anything but the shape | `cargo test --lib --test settings` |
| `src/settings.rs`, `src/protocol.rs` — the stored or wire **shape** | The full gate. Neither has a local blast radius |
| `src/app/`, `src/platform/windows/` | `cargo test --lib` |
| `src/platform/linux/` | `cargo check --target x86_64-unknown-linux-gnu` |
| `src/ui/` | `cargo test --no-default-features --features egui-ui --lib --test ui_semantic` |
| `build.rs`, dependency edits in `Cargo.toml` | `cargo tree --no-default-features -e normal` and one clean build |
| Packaging scripts | `.github\scripts\Test-ReleaseFormDefault.ps1` |
| Markdown only | Nothing. `git diff --check` at most |

`cargo fmt --all` is cheap enough to run unconditionally. Clippy belongs in the gate, not the loop:
it re-lints every target and rarely says anything new between two edits to the same function.

## Full gate (once, before committing)

```powershell
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-default-features --features egui-ui --lib --bin lastkey-settings --test ui_semantic
cargo clippy --locked --no-default-features --features egui-ui --all-targets -- -D warnings
cargo check --locked --target x86_64-unknown-linux-gnu --all-targets
cargo tree --no-default-features -e normal
git diff --check
.github\scripts\Test-ReleaseFormDefault.ps1
```

- **The `egui-ui` run is not optional after a `src/ui/` change.** `src/lib.rs` gates `pub mod ui`
  behind the feature, so the default run never compiles that module. Clippy with `--all-features`
  type-checks its tests but does not execute them.
- **`--test ui_semantic` must stay on the `egui-ui` commands.** `--lib --bin` selects only those two
  targets, so dropping the flag silently stops the label-driven tests from running; the default-
  feature run compiles `tests/ui_semantic.rs` but its crate-level cfg leaves it empty there. A lost
  accessible name is then invisible to both the gate and CI.
- Both Clippy configurations use `--all-targets` so test code is linted too. The default dependency
  tree must stay free of egui and wgpu. Test counts are deliberately not recorded here.
- Rebuilding fails with OS error 5 while the settings UI or the runtime holds `target\debug\*.exe`.
  Close it, run the `--lib` and integration targets (which do not link the binaries), or pass a
  separate `--target-dir`, which stays inside the `target/` ignore rule.

MSVC linking needs a `VsDevCmd.bat -arch=x64 -host_arch=x64` call first; the install path differs per
machine, so it is not pinned here.

## Unverified Areas

What the command set above does not cover. Each line is a known gap, not a plan.

- Manual Windows validation predates the settings-UI rebuild and the mode rework. Treat the last pass
  as dated; view changes need a fresh one.
- Real-hardware validation of the 1 ms near-simultaneous threshold and the 10-sample recommendation
  requirement.
- Live Linux validation: exclusive `grab` and virtual-device replay are verified by code review and
  cross-compilation only.
- A live-fire check that injected input is what the null-`hDevice` filter drops; the rule rests on
  documented behavior plus a throwaway probe, not an in-tree end-to-end run.
- Native visual checks: the single-page layout at its initial and minimum sizes, scrolling with
  live results, the measurement empty state, the feedback bar on a real Start/Stop failure, and the
  title-bar icon plus its DPI scaling. The UI scoped tests and settings executable build passed for
  the composition slice; they do not verify these on-screen results.
- The 2026-09-13 UI detail slice adds the timing example, profile chips, hover-scroll labels, and
  reference copy. Its tests, both clippy configurations, Linux check, format check, dependency tree,
  release-form script, and settings executable build passed. Native inspection of this slice was
  blocked by `GetCursorPos failed: Access is denied (0x80070005)` and a black window capture.
  Preview playback, hover interruption, translated layout, and minimum-size rendering still need
  a desktop session that the native UI tool can access; test success is not visual evidence.
- The same day's fidelity pass reworked the page against `target/ui-design-comparison.md`:
  conditional timing groups, collapsed stopped timeline, anchored profile/language panels, timing
  pills, mechanism steps, preview chrome, the D-pad center tile, latency column order, suggestion
  tiles, icon-only header controls, and the amber dirty badge. The UI scoped loop (107 tests), both
  clippy configurations, and fmt passed. None of it has been seen on screen: the panel anchor offset
  `PROFILE_PANEL_TOP`, card height matching, and every restyled control need a native visual pass,
  and the comparison document's own unverified list still applies.
- Native-font glyph coverage: `theme::fonts()` leads the proportional family with Segoe UI and falls
  back to eframe's bundled Ubuntu-Light plus the two emoji faces (NotoEmoji-Regular and
  emoji-icon-font). None of those covers Hangul or CJK, so `src/ui/language/zh.rs` strings render
  tofu until a face that covers them is registered; registering one is deliberately out of scope for
  the correction that recorded this gap. No on-screen pass has measured the missing glyphs.

End-to-end tests are added without adding abstractions to production code to enable them. Four now
run against a real named pipe in `src/platform/windows/ipc.rs`: a malformed payload, an oversized
length prefix, a client that dies mid-frame, and the peek that reports a disconnected client. A fifth
covers the self-connect that releases a parked `accept`. Each opens its own pipe instance keyed by
process and thread, and each hands the server the session before the client goes away — a pipe whose
client closed first fails `ConnectNamedPipe` outright and tests a different path.

Two remain untested and cannot be reached from a unit test as the code stands. `UiServer` shutdown
with a UI connected, and with no UI ever connected, both need a `UiServer`, and
`type Controller = Arc<Mutex<AppController<FileSettingsStore, InputService>>>` monomorphizes it
against the real input service: constructing one installs a `WH_KEYBOARD_LL` hook in the test
process. Making `UiServer` generic to admit a mock is the abstraction this rule forbids, and the
existing `MockRuntime` is `Rc`-based and not `Send` besides. Leave both to manual validation unless
that trade is deliberately reopened.
