# Development and verification

## Architecture

| Location | Responsibility |
| --- | --- |
| [scheduler.rs](../src/scheduler.rs) | Pure, deterministic grace/cadence/pause/session/power policy and tests. |
| [native/mod.rs](../src/native/mod.rs) | Single instance, hidden top-level window, message dispatch, menu, coalescable timer. |
| [native/input.rs](../src/native/input.rs) | Session/desktop checks, idle samples, held-key guard, zero-motion input packet. |
| [native/tray.rs](../src/native/tray.rs) | Original generated icons, tooltip state, notification-icon ownership. |
| [native/startup.rs](../src/native/startup.rs) | Read/write only this utility's per-user Run value. |

There is one application thread and one timer. `GetMessageW` blocks between events.
During sustained activity, a timer usually rechecks around every 30 seconds; during idle
operation, after each pulse it sleeps until the four-minute deadline. Pause, lock and
suspend kill the action timer. Coalescing may delay a timer slightly; every wake rechecks
eligibility instead of assuming a deadline grants permission to inject.

The window is a hidden **top-level window**, not a message-only window: Explorer's
`TaskbarCreated` broadcast and power/session messages need to reach it. If Explorer is
not ready, retry icon installation every five seconds and send no input until the icon
and session-notification registration are available. A real Explorer restart is recovered
without changing manual pause state.

The window procedure only queues typed events. The outer loop owns mutable app state.
This avoids aliased mutable references when native menus/dialogs re-enter the procedure.
Messages delivered synchronously post `WM_NULL` to wake the outer loop, even when paused.
Do not replace this with an unchecked global `&mut App` in a callback.

Use uptime, not wall clock. `LASTINPUTINFO.dwTime` is a wrapping 32-bit uptime value;
deadlines use `GetTickCount64`. Ambiguous/future input timestamps fail closed. No attempt
is made to subtract our previous packet from the input history: the four-minute cadence
already exceeds the 30-second inactivity guard. Our own event therefore cannot create a
tight keep-alive loop.

No persistent execution-state request or power-plan change is needed. `SetThreadExecutionState`
alone would not implement the requested input-idle behavior or stop a screensaver. The
synthetic event remains tagged and visible to input observers; do not disguise it as hardware.

## Automated checks

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
.\scripts\smoke-test.ps1
```

Normal tests exercise scheduling and packet contents **without injecting input**. The
smoke test starts its own release process, checks for no visible window, validates that
a duplicate exits, dispatches simulated session/power notifications, records a short
resource snapshot, and closes its own process. It refuses to stop an existing instance.
Simulated notifications are not proof of a real lock/unlock or sleep/wake cycle.

The executable must use the Windows subsystem, but test harnesses must keep the console
subsystem so assertions and test counts remain visible. Build with `--locked`. Keep
release artifacts and machine-local files out of Git. The static MSVC CRT setting makes
the release executable portable without a VC++ redistributable installation.

## Opt-in live input check

This test sends **one real zero-motion event** on the current desktop. Stop other
keep-alive utilities first. Leave the desktop unlocked, release all keys/buttons, and
leave input idle. The test waits up to two minutes for 30 seconds of genuine inactivity.
Do not run it in CI, in parallel with interactive work, or against someone else's session.

```powershell
cargo test --locked live_zero_motion -- --ignored --nocapture --test-threads=1
```

It compares cursor coordinates and Windows idle time before/after the same packet used
in production. An inactivity timeout is not an input-backend failure; no input was sent.
If concurrent physical input occurs, the measurement is inconclusive: rerun when quiet.
A passing packet test proves the packet shape; this live test is required before claiming
Windows actually acknowledged the idle reset.

## Interactive acceptance checklist

Use the release executable and stop competing mouse movers so their pulses cannot
confound the results. Do not automate real system lock/sleep or terminate Explorer on a
user's working desktop without agreement.

1. Launch: one tray icon, no console/window/taskbar button/Alt-Tab entry. Launch again:
   still one process. Check the tray overflow if the icon is not immediately visible.
2. Click/right-click/keyboard-activate the icon. Pause switches to amber and **Resume**;
   resume switches back. Closing the menu must not leave it stuck on screen.
3. While actively typing/moving/dragging or holding a key/button, no keep-alive is sent.
   Test the held-button case beyond 30 seconds. An input observer can distinguish our
   `MMOV` tag; avoid global input logging or collecting key contents.
4. While idle, first eligibility is after the startup grace; subsequent successful
   input-idle resets are at least 240 seconds apart. Cursor coordinates stay unchanged.
5. Pause for longer than four minutes: no idle reset. Resume and check the new grace.
6. Lock/unlock, disconnect/reconnect, and sleep/wake: no input while unavailable, fresh
   grace on return, manual pause retained. Try a secure UAC desktop without elevating
   this utility. Denied input should not cause rapid retries or notification spam.
7. Restart Explorer when safe: icon returns, including while manually paused.
8. From a stable path containing spaces, enable startup, read back the quoted Run value,
   and sign out/in when convenient. Check actual enabled startup, not only registry output.
   Toggle it off and verify only this app's value disappears. Restore the original setting.
9. Exit: no process, timer, or tray icon remains. No power settings were changed.

For efficiency measurements, use an optimized build. Sample process CPU-time deltas over
several minutes in both active-use and idle cases, plus private memory/working set and
handle counts. A launch-time resource snapshot is not a sustained CPU benchmark. Windows
helper threads and working-set trimming are normal; do not equate thread count with
application polling or promise exact memory use across machines.

## Win32 references

- [GetLastInputInfo](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getlastinputinfo)
- [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- [MOUSEINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput)
- [Notification-area guidance](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area)
- [OpenInputDesktop](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openinputdesktop)
- [SetThreadExecutionState limitations](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate)