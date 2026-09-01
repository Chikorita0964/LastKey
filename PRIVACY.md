# Privacy Policy

Last updated: September 1, 2026

LastKey is a local, open-source Windows application with an experimental Linux input backend, developed and maintained by Chikorita0964. Its source code is available on GitHub under the GNU General Public License, version 3 only.

## Keyboard Input

On Windows, the application uses a `WH_KEYBOARD_LL` low-level keyboard hook. On Linux, the experimental backend reads configured keyboard devices through `evdev` and emits resolved keys through a local `uinput` virtual keyboard. Both backends process only configured directional keys (W/S, A/D by default) to resolve simultaneous opposing directional inputs.

All key processing occurs entirely in-memory during execution. Keystrokes are never logged, written to disk, or transmitted over a network. The Windows `SendInput` API and Linux `uinput` device are used only to output resolved directional key events. Optional timing measurement stores aggregate transition and overlap values only in memory for the active session; it never persists raw samples or key history.

## Data Collection and Network Activity

**Fully local and offline.** No analytics, telemetry, crash reporting, ads, accounts, cloud services, or outbound network connections—including update checks. Updates are obtained manually from GitHub Releases or through the Microsoft Store. No file, Registry, or keystroke logging.

## Contact

If you have any questions about this Privacy Policy, please open an issue in the project's GitHub repository.
