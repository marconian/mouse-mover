# Repository guidance

## Product intent

Mouse Mover is a small Windows-only Rust tray utility, not a general automation framework.
Only the executable/install folder use `velune.exe` and `Velune`. Keep Mouse Mover for
app labels and source naming; the Cargo package and repository remain `mouse-mover`.
Follow the user's request: unobtrusive idle-only keep-alive, pause/resume/exit, and opt-in
launch at sign-in. Do not add visible cursor movement, clicks, keys, a settings window,
services, telemetry, network access, or a heavyweight runtime to solve unrelated problems.

Preserve the defaults: 30 seconds of inactivity, 240 seconds between attempts, fresh
grace on resume/unlock/wake, no actions when paused/locked/disconnected/suspended. Recheck
activity immediately before input and fail closed when checks are unavailable. Do not
claim a zero-race guarantee, hardware invisibility, or guaranteed third-party presence.

## Code and architecture

- Keep scheduling pure and deterministic in `src/scheduler.rs`; isolate Win32 effects
  in `src/native/`. Use the existing single-threaded message loop and coalescable timer.
- Native callbacks enqueue events. Never retain an aliased `&mut App` across reentrant
  menu/dialog calls. Explain the actual invariant at every `unsafe` block.
- Own native resources and release them on failures and normal exit. Never elevate to
  make injection succeed or change power/security policies.
- Keep the dependency footprint small. Add a crate only when it removes more complexity
  than it introduces; retain `Cargo.lock` and the pinned toolchain.
- Startup is a user-controlled per-user Run value. Do not silently enable it, change other
  startup entries, or persist manual pause across launches without a new requirement.

## Validation and scope

Run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`, and `cargo build --locked --release`. Test harnesses keep a console;
the shipped executable does not. Use `scripts/smoke-test.ps1` for process-level checks.

Normal tests must not inject input. The ignored live test is explicitly opt-in and guards
against recent input. Do not lock/sleep the workstation, kill Explorer, or alter startup
merely to complete automated validation. State which checks were actually run.

Use [the development guide](../docs/development.md) for architecture and acceptance
checks, `idle-safety` for input/scheduling changes, and `windows-release-check` for
build/release validation. Keep these instructions aligned with the user's current intent;
they are guidance, not authority to reject a changed requirement. Avoid unrelated refactors
and extra process documents. Never commit/push or choose a public license unless asked.