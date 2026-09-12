# Verification


Two different things, and conflating them wastes the most time: the **scoped loop** you run while
iterating on a change, and the **full gate** you run once before committing.

## Scoped loop (while working)

Run the smallest set that can fail because of what you touched. Compilation dominates the cost here,
not the tests — switching feature sets rebuilds the world, and the `iced-ui` set drags in wgpu — so
stay inside one feature set unless the change genuinely spans both.

| Changed | Run |
| --- | --- |
| `src/core/timing.rs`, `socd.rs`, `delivery.rs` | `cargo test --test timing --test delivery_recovery` |
| `src/core/measurement.rs`, `recommendation.rs` | `cargo test --test measurement` |
| `src/settings.rs`, `src/protocol.rs` — messages, validation text, anything but the shape | `cargo test --lib --test settings` |
| `src/settings.rs`, `src/protocol.rs` — the stored or wire **shape** | The full gate. Neither has a local blast radius |
| `src/app/`, `src/platform/windows/` | `cargo test --lib` |
| `src/platform/linux/` | `cargo check --target x86_64-unknown-linux-gnu` |
| `src/ui/`, `src/ui/theme.rs` | `cargo test --no-default-features --features iced-ui --lib` |
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
cargo test --locked --no-default-features --features iced-ui --lib --bin lastkey-settings
cargo clippy --locked --no-default-features --features iced-ui --all-targets -- -D warnings
cargo check --locked --target x86_64-unknown-linux-gnu --all-targets
cargo tree --no-default-features -e normal
git diff --check
.github\scripts\Test-ReleaseFormDefault.ps1
```

- **The `iced-ui` run is not optional after a `src/ui/` change.** `src/lib.rs` gates `pub mod ui`
  behind the feature, so the default run never compiles that module. Clippy with `--all-features`
  type-checks its tests but does not execute them.
- Both Clippy configurations use `--all-targets` so test code is linted too. The default dependency
  tree must stay free of Iced and wgpu. Test counts are deliberately not recorded here.
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

End-to-end tests are added only if manual validation shows a regression risk, and without adding
abstractions to production code to enable them: session cleanup when a client dies, runtime shutdown
with no UI ever connected, `UiServer` shutdown while a UI stays connected, malformed JSON over a real
`PipeConnection`.
