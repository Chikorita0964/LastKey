# Privacy Policy

Last updated: September 12, 2026

LastKey is a local, open-source Windows application with an experimental Linux input backend, developed and maintained by Chikorita0964. Its source code is available on GitHub under the MIT License.

## Keyboard Input

On Windows, the application uses a `WH_KEYBOARD_LL` low-level keyboard hook. On Linux, the experimental backend reads configured keyboard devices through `evdev` and emits resolved keys through a local `uinput` virtual keyboard. Both backends process only configured directional keys (W/S, A/D by default) to resolve simultaneous opposing directional inputs.

All key processing occurs entirely in-memory during execution. Keystrokes are never written to disk or transmitted over a network. The Windows `SendInput` API and Linux `uinput` device are used only to output resolved directional key events. Optional timing measurement stores aggregate transition and overlap values only in memory for the active session; it never persists raw samples or key history.

The optional input timeline starts only when the user selects Start timeline. It displays a one-second window of only the four mapped keys, retaining at most 512 completed intervals and four current holds in memory. It shows engine output while filtering, or physical input while filtering is disabled or measurement is active. Stop, Apply, profile activation, disconnection, and window close discard this display history. It records no typed text and has no export or disk log.

## Data Collection and Network Activity

**Fully local and offline.** No analytics, telemetry, crash reporting, ads, accounts, cloud services, or outbound network connections—including update checks. Updates are obtained manually from GitHub Releases or through the Microsoft Store. No file-activity, Registry-activity, or persisted keystroke logging. Saved key mappings, timing settings, and profile names remain in the local settings file.

## Contact

If you have any questions about this Privacy Policy, please open an issue in the project's GitHub repository.
