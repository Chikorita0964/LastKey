# LastKey

A minimal and standalone SOCD (Simultaneous Opposite Cardinal Direction) filter for Windows.

It tracks inputs using a [`WH_KEYBOARD_LL`](https://learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc) hook, resolves opposing directions via an internal state machine, and tags generated [`SendInput`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput) events to prevent re-entrant hook loops.

## Behavior

For each opposing key pair, the filter forwards only one key at a time.
Using the A/D pair as an example:

| What you do | What the application receives |
| --- | --- |
| Press A | A is held |
| Keep holding A, then press D | A is released and D is held |
| Keep holding A, then release D | D is released and A is held again |
| Release A | Neither key is held |

The same rule applies in reverse, and independently to W/S.
Key repeat does not change priority or generate additional output transitions.

## Physical overlap handling

The input timing card provides four independent modes:

| Mode | What happens during a physical overlap |
| --- | --- |
| **Immediate** | Release the old direction and press the new one immediately. |
| **Press Delay** | Release the old direction now and press the new one after a randomized neutral gap. |
| **Release Delay** | Press the new direction now and release the old one after a randomized overlap. |
| **Random Mix** | Choose press delay or release delay for each overlap using the configured share. |

Natural neutral transitions are unchanged. Switching modes preserves the configured ranges; unused
controls stay muted. Range handles cover 0–20 ms, and numeric editors accept durations up to 1000 ms
in 0.1 ms steps. Release delay has a 0.1 ms minimum.

## If `SendInput` fails

Resolved inputs are sent using Windows `SendInput`. If the target app runs with higher privileges (such as an Administrator), **UIPI** (User Interface Privilege Isolation) may block these inputs. Windows does not clearly report when this happens, so you can try running this tool with the same privileges if inputs are not registered.

If forwarding fails, conflicting inputs are suppressed:

| What happens | What the application receives |
| --- | --- |
| A cannot be forwarded | A is held |
| A releases, but D fails | Neither key is held |
| A cannot be released for D | A remains held |
| Releasing A cannot be forwarded | A is released |

Original physical events pass through only when safe, preventing simultaneous opposing inputs even if forwarding fails.

## Usage

1. Download the signed MSIX from the Microsoft Store, or the ZIP release from GitHub Releases.
2. Run `LastKey.exe`. The input filter starts immediately and a system tray icon appears; the settings renderer is not loaded until requested.
3. Right-click the tray icon to open the menu:
   - **Settings**: Opens the on-demand settings process.
   - **Disable / Enable**: Toggles the filter for this runtime session.
   - **Exit**: Stops LastKey.

Opening **Settings** again focuses the existing settings window. Key mappings and input timing appear side by side, with measurement and results below on the same scrollable page. **Restore all defaults**, **Revert**, and **Apply** stay at the bottom of the window while you scroll. Measurement starts only when you select **Start measurement**. Closing that window stops transient capture, measurement, and timeline work without stopping the input filter.

The header on/off button controls the same engine state as the tray. State changes appear after the
engine confirms them; restarting LastKey starts with filtering enabled.

The language button switches the settings window between English, Chinese, and Spanish for the
current window session. Key names, profile names, and runtime diagnostics retain their original text.

**Profiles** offers four saved slots. **Load** immediately saves and activates the selected slot;
unapplied edits require a discard confirmation. **Apply** saves edits to the active slot, and
**Rename** changes a slot name without activating other edits. Existing settings become slot 1 on
the first profile operation.

**Start timeline** explicitly enables a one-second view of the four mapped keys. It displays engine
outputs while filtering, or physical input while disabled or measuring. Delay badges come from the
engine. Stop, Apply, profile load, disconnect, and window close discard its memory-only history.
**Reset session** clears measurement results or starts a fresh session when measurement is running.

If **UIPI** blocks `SendInput` to apps with higher privileges, **Exit** the current instance first, then use **Run as administrator** on `LastKey.exe`.

## Compatibility and policies

Antivirus or anti-cheat software may block keyboard hooks or simulated input. Also, some games and communities prohibit third-party tools or specific SOCD rules, so please check their policies before using it.

## Linux backend

Linux uses `evdev` to exclusively grab candidate keyboards and `uinput` to provide a
virtual keyboard. This lets LastKey filter configured pair keys while forwarding ordinary
key events. The process needs read/write access to `/dev/input/event*` and `/dev/uinput`;
configure udev permissions or use an appropriate privileged service instead of running a
desktop session as root. Connected keyboards are discovered when the service starts; unplugged
devices release their grab automatically, and connecting a new keyboard currently requires a
service restart.

## Privacy

LastKey operates entirely offline and never sends your data to the developer or third parties. Configured directional inputs are processed solely in memory and are never written to disk. For more details, see the [Privacy Policy](PRIVACY.md).

The optional input-timing measurement mode observes only physical edges for the four configured pair keys while it is active. Timing samples exist only in memory for the active session and are used to calculate transition and overlap distributions. The separate opt-in timeline retains at most 512 completed four-key intervals plus current holds in memory. LastKey does not write timing samples, key history, or typed text to disk.

## Build and package

Open a **Visual Studio developer command prompt** in the project directory.

```bat
cargo build --locked --release --target x86_64-pc-windows-msvc --bin lastkey
cargo build --locked --release --target x86_64-pc-windows-msvc --no-default-features --features iced-ui --bin lastkey-settings
```

Outputs: `target\x86_64-pc-windows-msvc\release\lastkey.exe` and `target\x86_64-pc-windows-msvc\release\lastkey-settings.exe`. Keep both files together; packaged builds name the second file `LastKey.Settings.exe`.

To create and validate an unsigned Store-submission MSIX, install the Windows SDK and run:

```powershell
$version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
.\msix\package-msix.ps1 -Version $version -OutputDirectory release
.\msix\validate-msix.ps1 -Package ".\release\LastKey-$version.msix" -Version $version
```

The Microsoft Store signs submitted packages. Local MSIX output is intentionally unsigned.

## Customize

Use the settings window to choose four unique physical keys, configure transition or overlap timing, and start an in-memory timing measurement session. Installed builds save active settings in `%LOCALAPPDATA%\LastKey\settings.toml`. To use a self-contained ZIP as a portable installation, create an empty `lastkey.portable` marker beside `LastKey.exe`; settings will then be saved beside the executable. Existing executable-adjacent settings are still read as a migration fallback. Raw timing samples and typed text are never persisted.

## Tests

```bat
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --no-default-features --features iced-ui --lib --bin lastkey-settings
cargo clippy --no-default-features --features iced-ui --lib --bin lastkey-settings -- -D warnings
```

## Credits and license

LastKey is distributed under the [MIT License](LICENSE) (`MIT`). Portions are derived from [Hitboxer by Valentin Ignatev](https://github.com/valignatev/hitboxer); the original MIT copyright and license notice are preserved in [LICENSES/MIT.txt](LICENSES/MIT.txt). See [LICENSE.md](LICENSE.md) for the complete licensing overview.

The LastKey code and icon were created with AI.
