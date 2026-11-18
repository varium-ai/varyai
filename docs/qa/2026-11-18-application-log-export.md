# Application log export verification

Date: 2026-11-18. Base revision: `eb24a0e3b947ba89bd9aa668148e23b0e9bbe59c`.
Checks run against the uncommitted working tree in this checkout, including its
existing proxy replay and Agent UI changes. No commit or push was requested.

## Scope

The user explicitly replaced the earlier all-Agent ZIP requirement with export
of the current `varyai.log` only. The Settings command reads one complete file
snapshot, sanitizes it and atomically publishes a `.log` destination. It does not
scan native logs, configuration, rotations, crash files or databases. Source and
hardlink destinations are rejected; errors preserve an existing destination.
A distinct RPC prevents an updated frontend from invoking the old ZIP backend.

Supplemental diagnostics record version, OS, architecture and build mode, plus
fixed start/result/duration events for connection, disconnection, reapplication,
confirmed restart and model-control authorization. Only canonical Agent IDs and
allowlisted reasons are logged. Model selections and native error details are
excluded. Existing operation return values, security policies and native adapters
remain unchanged. The shared logging matrix covers all 20 supported Agent IDs;
this is not a native-client acceptance claim.

Owned implementation paths include `services/diagnostics/{mod,text_logs,operation,
tests}.rs`, `commands/diagnostics.rs`, their module/command registrations, startup
metadata in `lib.rs`, connection-command wrappers, the diagnostic text sanitizer,
`LogConfigPanel.tsx`, the settings API, four locale bundles and their regressions.
`same-file` provides portable file identity checks on macOS and Windows. The
existing Agent page restart-error callback received an `unknown` error annotation
to retain its string-error handling while satisfying TypeScript. Other concurrent
Agent UI and proxy changes were preserved.

## Host and checks

Host: macOS 15.3.1 arm64. Node 24.11.0, pnpm 10.12.3, rustc/cargo 1.98.1.
Rust checks use `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
`CARGO_INCREMENTAL=0`.

- `pnpm typecheck`: passed after correcting the restart callback error annotation.
- `pnpm build:renderer`: passed.
- `pnpm format:check`: passed.
- `pnpm test:unit`: 324 files / 4,456 tests passed.
- Focused settings/API/tray regressions: 3 files / 87 tests passed.
- `pnpm audit --prod`: passed; no known vulnerabilities reported.
- Development launcher, CI policy, macOS DMG, TRAE bridge, model-default,
  Pi session, WorkBuddy auth, Doubao check-in, Agent rewards, Qoder codec and
  shared-v23 recovery helper tests: passed.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7,732 passed,
  21 ignored, zero failures.
- `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`: passed;
  7,897 passed, 21 ignored, zero failures across library and integration harnesses.
- `git diff --check`: passed.

The initial single-file backend suite passed 10 tests. Its fixtures verify full
content above the former tail limits, long lines, line order/endings, final lines,
fixed-size snapshots, premature EOF rejection, invalid encoding, disabled-policy
privacy, sensitive blocks, unavailable privacy context, missing/empty logs,
hardlinks and symlink destinations. All 11 diagnostic regressions, including the additional structured-array privacy
regression, passed in the final backend run. Component tests cover native
save cancellation, duplicate-click locking, fixed private error messages, all
four runtime languages, missing/empty logs and outdated backend handling.

Earlier Rust attempts were interrupted by disk exhaustion and are not acceptance
evidence. Obsolete generated test binaries, libraries and abandoned crate codegen
objects were removed before rerunning checks. Source files, user data and the
installed/running application were not deleted or replaced by a verification
command. Earlier broad-ZIP results do not validate this replacement.

## Remaining acceptance

The current native save dialog has not been manually exercised. Real Agent
self-tests, Windows/Linux execution, Windows-to-WSL2 filesystem behavior and the
required remote macOS/Windows/Linux CI jobs remain unverified. No application
installation, signing, notarization, release or live deployment is accepted by
these local synthetic checks.
