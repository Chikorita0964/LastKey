# LastKey

A standalone SOCD (Simultaneous Opposite Cardinal Direction) filter for Windows.

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

## If `SendInput` fails

Resolved inputs are sent using Windows `SendInput`. If the target app runs with higher privileges (such as an Administrator), UIPI (User Interface Privilege Isolation) may block these inputs. Windows does not clearly report when this happens, so you can try running this tool with the same privileges if inputs are not registered.

If forwarding fails, conflicting inputs are suppressed:

| What happens | What the application receives |
| --- | --- |
| A cannot be forwarded | A is held |
| A releases, but D fails | Neither key is held |
| A cannot be released for D | A remains held |
| Releasing A cannot be forwarded | A is released |

Original physical events pass through only when safe, preventing simultaneous opposing inputs even if forwarding fails.

## Usage

1. Download from GitHub Releases (or build it from source).
2. Run the executable. A system tray icon will appear.
3. Right-click the tray icon to open its menu:
   - **Open file location** opens File Explorer with `LastKey.exe` selected.
   - **Exit** stops the filter.

## Compatibility and policies

Antivirus or anti-cheat software may block keyboard hooks or simulated input. Also, some games and communities prohibit third-party tools or specific SOCD rules, so please check their policies before using it.

## Privacy

LastKey operates entirely offline and never sends your data to the developer or third parties. Configured directional inputs are processed solely in memory and are never logged or stored. For more details, see the [Privacy Policy](PRIVACY.md).

## Build

Open a **Visual Studio developer command prompt** in the project directory.

### Using the batch script

```bat
build.cmd
```

Output: `out\LastKey.exe`

### Using CMake directly

```bat
cmake -G Ninja -S . -B out\cmake-release -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=OFF
cmake --build out\cmake-release
```
Output: `out\cmake-release\LastKey.exe`

## Customize

To change the mapping, edit the `kKeys` declaration near the top of `LastKey.cpp`:

```cpp
constexpr KeySpec kKeys[kKeyCount] = {
    {0x11, false}, // W
    {0x1F, false}, // S
    {0x1E, false}, // A
    {0x20, false}, // D
};
```

Each entry uses a hardware scan code and an extended flag (not a virtual-key code). For arrow keys, set the extended flag to `true` as they are extended keys.

## Tests

The table-based state-transition tests simulate key events along with both successful and failed output attempts without requiring a physical keyboard:

```bat
cmake -G Ninja -S . -B out\cmake-tests -DCMAKE_BUILD_TYPE=Debug -DBUILD_TESTING=ON
cmake --build out\cmake-tests
ctest --test-dir out\cmake-tests --output-on-failure
```

## Credits and license

LastKey is derived from [Hitboxer by Valentin Ignatev](https://github.com/valignatev/hitboxer) and is distributed under the MIT License. The original copyright and license notice are preserved in [LICENSE](https://github.com/Chikorita0964/LastKey/blob/main/LICENSE).

The LastKey code and icon were created with Codex AI.
