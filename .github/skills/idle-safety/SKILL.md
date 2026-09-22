---
name: idle-safety
description: 'Change or review Mouse Mover idle detection, pulse cadence, SendInput, held-key guards, session lock, sleep/resume, and Win32 callback safety. Use when modifying keep-alive behavior or investigating unwanted activity.'
---

# Idle-safe behavior changes

1. Read `src/scheduler.rs` and `src/native/input.rs`, then the affected dispatch path in
   `src/native/mod.rs`. Identify the user-visible behavior being changed; do not silently
   reinterpret the 30-second guard or four-minute cadence.
2. Add or update deterministic scheduler tests before changing policy. Cover activity
   just before a deadline, held input at the native boundary, failed idle sampling,
   tick rollover, manual pause, independent session/power gates, and resume grace.
3. Keep pulse contents at relative `(0, 0)`, `MOUSEEVENTF_MOVE` only, system timestamp,
   no button/wheel/key flags, and the explicit synthetic-input tag. Do not "restore"
   the cursor: restoration races with physical input and can move it backwards.
4. Preserve the final idle recheck, interactive-desktop/session guard and held-key check.
   Missing/ambiguous observations must skip input; errors must not cause rapid retries.
   Eligibility checks and `SendInput` cannot be atomic: document rather than hide this.
5. Inspect lifetimes and reentrancy if callbacks, timers or menus changed. Dispatch app
   state outside the window procedure; test stale queued ticks after pause/lock. Keep
   the action timer stopped when policy says stop and no active input without tray control.
6. Run formatting, strict Clippy, regular tests and the release build. Tests normally
   inspect policy and packet contents, never the user's live input stream.
7. If the packet/backend changed, use the opt-in `live_zero_motion` test only when the
   interactive desktop has been quiet for 30 seconds and no competing mover is running.
   Record separately: packet assertions, observed Windows idle reset, unchanged cursor,
   and untested third-party/system-policy behavior. Do not turn a serializer test into
   a claim about Windows or chat presence.