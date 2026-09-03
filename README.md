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

When opposing physical keys overlap, LastKey normally resolves them into a neutral SOCD
transition. Enabling **SOCD Transition Delay** applies the configured randomized neutral gap used
for that resolution. Natural neutral transitions are left unchanged. **Preserve Overlap** becomes
available only while SOCD Transition Delay is enabled and can retain
a configured percentage of detected physical overlaps for a randomized **Preserved Overlap
Duration**; the remaining overlaps still use the SOCD Transition Delay. With preservation
disabled, LastKey retains its immediate Last Input Priority path while preserving the configured
timing values for later use.

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
2. Run the executable. A system tray icon will appear.
3. Right-click the tray icon to open the menu:
   - **Create desktop shortcut**: Creates a desktop shortcut.
   - **Exit**: Stops LastKey.

If **UIPI** blocks `SendInput` to apps with higher privileges, **Exit** the current instance first, then use **Run as administrator** on the desktop shortcut.

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

LastKey operates entirely offline and never sends your data to the developer or third parties. Configured directional inputs are processed solely in memory and are never logged or stored. For more details, see the [Privacy Policy](PRIVACY.md).

The optional input-timing measurement mode observes only physical edges for the four configured pair keys while it is active. Timing samples exist only in memory for the active session and are used to calculate transition and overlap distributions. LastKey does not write timing samples, key history, or typed text to disk.

## Build and package

Open a **Visual Studio developer command prompt** in the project directory.

```bat
cargo build --locked --release --target x86_64-pc-windows-msvc
```

Output: `target\x86_64-pc-windows-msvc\release\lastkey.exe`

To create and validate an unsigned Store-submission MSIX, install the Windows SDK and run:

```powershell
.\msix\package-msix.ps1 -Version 1.0.0 -OutputDirectory release
.\msix\validate-msix.ps1 -Package .\release\LastKey-1.0.0.msix -Version 1.0.0
```

The Microsoft Store signs submitted packages. Local MSIX output is intentionally unsigned.

## Customize

Use the settings window to choose four unique physical keys, configure transition or overlap timing, and start an in-memory timing measurement session. The active settings are saved as `settings.toml` beside `lastkey.exe`; raw timing samples and typed text are never persisted.

## Tests

```bat
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Credits and license

LastKey is distributed under the [GNU General Public License, version 3 only](LICENSE) (`GPL-3.0-only`), including its use of Slint under GPLv3. Portions are derived from [Hitboxer by Valentin Ignatev](https://github.com/valignatev/hitboxer); the original MIT copyright and license notice are preserved in [LICENSES/MIT.txt](LICENSES/MIT.txt). See [LICENSE.md](LICENSE.md) for the complete licensing overview.

The LastKey code and icon were created with Codex AI.
