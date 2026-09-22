# Mouse Mover

A tiny, native Rust keep-alive utility for **Windows 10/11**. No main window, console,
clicks, keystrokes, cursor jiggle, network access, or background service. Its only normal
UI is a notification-area icon with a small menu.

## Behavior

- Starts enabled, with a **30-second grace period**.
- Only sends a keep-alive after at least **30 seconds without keyboard/mouse input**.
- Uses one **zero-distance, relative mouse event**, at most once every **240 seconds**.
  Unlike moving away and back, this never changes the cursor coordinates.
- Checks held keys/buttons and rechecks inactivity immediately before sending.
- Stops during manual pause, session lock/disconnection, and sleep. Resume/unlock/wake
  gets another grace period and never shortens the previous pulse interval.
- Works on battery too, matching the supplied setup. Keeping a machine awake consumes
  more battery than allowing it to sleep.
- Runs one application message-loop thread; Windows may create helper threads. No hooks,
  busy loop, high-resolution timer requests, or periodic disk writes.

The timings match the supplied Move Mouse setup; they are deliberately fixed rather than
adding an unnecessary settings window.

## Use

Build with the **Build release** VS Code task, then open
[target/release/velune.exe](target/release/velune.exe).
There is no window to dismiss. Look in the system tray overflow (`^`) if the icon is hidden;
Windows controls its placement, and you can drag it into the visible tray.

Click or right-click the icon (keyboard activation works too):

| Menu | Effect |
| --- | --- |
| **Pause / Resume** | Stop all keep-alive input / re-enable idle-only operation. |
| **Start with Windows** | Toggle launch at sign-in for this user and executable location. |
| **Exit** | Remove the tray icon and terminate the application. |

The teal mouse means enabled, amber pause bars mean paused/session inactive, and red pause
bars mean an input/API failure with a delayed retry. The tooltip identifies the state.
Ordinary operation shows no notifications; unrecoverable startup failures or explicitly
requested startup-setting failures show an error dialog instead of failing silently.

Opening a second copy does nothing. Pause is for the current run; a new launch starts
enabled. Exit does not disable a previously selected startup registration.

## Install and start automatically

For a stable location outside build output, run [scripts/install.ps1](scripts/install.ps1)
with PowerShell. It builds and copies the executable into the current user's local
application-data Programs folder, as `Velune\velune.exe`. Add `-Launch` to start that copy.
No administrator permission is required. The installer does **not** enable startup.

When ready, select **Start with Windows** in the **installed copy's** tray menu. It uses only
the current user's `Software\Microsoft\Windows\CurrentVersion\Run` registry key and its
`MouseMover` value, with a quoted executable path. On the next sign-in it launches enabled.
Task Manager's Startup apps controls or organization policy can still disable startup.

To update: exit, rerun the installer, and relaunch. To uninstall: uncheck **Start with
Windows**, exit, and delete the installed folder. If you move the executable, enable
startup again from its new location to update the registration.

## Build and verify

Install [Rust with rustup](https://rustup.rs/) and the Visual Studio **Desktop development
with C++** workload / Windows SDK. The repository pins the Rust toolchain. Rust is needed
only to build, not to run the resulting executable; the MSVC runtime is statically linked.

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
.\scripts\smoke-test.ps1
```

Equivalent format/lint/test/build tasks and a Rust Analyzer recommendation are included
for VS Code. Restart VS Code after first installing Rust if the extension cannot find it.
Windows CI runs the same checks and uploads the executable; it never injects live input.
See [docs/development.md](docs/development.md) for interactive checks and architecture.

## What this does not promise

- This is **synthetic input**, not an undetectable hardware mouse. Applications can observe
  or ignore it. Zero movement does not imply no input message: hover/cursor visibility or
  application-specific raw-input behavior can still react to a synthetic event.
- It targets Windows' session input-idle timer, rather than only requesting that the PC
  stay awake. It does not promise a particular Teams/Slack status or override managed
  screen-lock policies. It cannot unlock the machine or bypass the secure desktop/UAC.
- Windows has no atomic "inject only if still idle" API. The app skips observed activity,
  checks held keys/buttons, and rechecks just before injection; a simultaneous physical
  input event can race that final check. Even then, the packet cannot move, click, scroll,
  or type. A literal zero-race guarantee would be misleading.
- The 240-second pulse cadence assumes the relevant timeout is longer than four minutes.
  Very short idle timeouts can still expire. Reading/watching without input counts as idle;
  use **Pause** if that should not generate a keep-alive.
- Explicit lock, sleep, lid-close behavior and system power/security policies remain under
  Windows' control. Protected/elevated foreground applications may block injection.

This is an independent implementation, not a fork of Move Mouse. No original application
code, icons, or branding assets were copied.