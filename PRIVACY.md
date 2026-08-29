# Privacy Policy

Last updated: August 28, 2026

LastKey is a local, open-source Windows system tray application developed and maintained by Chikorita0964. Its source code is available on GitHub under the MIT License.

## Keyboard Input

The application uses a Windows `WH_KEYBOARD_LL` low-level keyboard hook to process only configured directional keys. While Windows delivers system-wide keyboard events to this hook, it strictly inspects and processes the state of configured directional keys (W/S, A/D by default) solely to resolve simultaneous opposing directional inputs.

All key processing occurs entirely in-memory during execution. Keystrokes are never logged, written to disk, or transmitted over a network. The Windows `SendInput` API is used exclusively to output the resolved directional key events.

## Data Collection and Network Activity

**Fully local and offline.** No analytics, telemetry, crash reporting, ads, accounts, cloud services, or outbound network connections—including update checks. Updates are obtained manually from GitHub Releases or through the Microsoft Store. No file, Registry, or keystroke logging.

## Contact

If you have any questions about this Privacy Policy, please open an issue in the project's GitHub repository.
