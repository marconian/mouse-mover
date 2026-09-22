---
name: windows-release-check
description: 'Build, test, smoke-test, profile or prepare a Windows release of the Rust Mouse Mover tray utility. Use for release readiness, executable size, idle CPU/memory checks, startup registration and tray lifecycle verification.'
---

# Windows release validation

1. Inspect Git status and preserve existing work. Verify the Rust pin, MSVC/Windows SDK,
   `Cargo.lock`, and static CRT configuration. Use the existing VS Code tasks where
   available. Do not install system prerequisites through unattended elevation.
2. Run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
   `cargo test --locked`, and `cargo build --locked --release`. Confirm visible test
   output: a Windows-subsystem test harness can hide all its assertion output.
3. Exit only a test instance you own. Run `scripts/smoke-test.ps1`; it refuses to terminate
   an existing user instance. Report actual executable size and process measurements,
   distinguishing a brief launch snapshot from a sustained performance benchmark.
4. Use the interactive acceptance checklist in `docs/development.md` for tray activation,
   pause/resume/exit, long held-input cases, session changes, and Explorer recovery.
   Simulated messages do not prove a real lock/wake cycle. Do not disrupt the desktop
   just to tick a box; identify untested items explicitly.
5. If authorized to test real input, run the ignored `live_zero_motion` test on an idle
   interactive desktop. It sends exactly one event. Never run it in CI or weaken its
   guard merely to make it pass.
6. For startup changes, preserve the original value, use a path containing spaces, verify
   the quoted per-user command and removal, and restore the setting. Registry read-back
   is not proof of successful sign-in startup; Task Manager/policy may override it.
7. Use `scripts/install.ps1` for a stable per-user copy. Installation must not enable
   startup unless the user requests it. A build artifact and local install are not a
   published release; do not tag, commit, push, or publish without a request.