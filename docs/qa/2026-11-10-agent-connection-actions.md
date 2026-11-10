# Agent connection action verification

Date: 2026-11-10. Host: macOS / Darwin 25.6.0 arm64. Base revision:
`c092e24eb20c4f30e44b9b3f8c5d77f78fc85098`, with existing Agent update and
check-in changes preserved in the shared working tree. Node 22.22.0,
pnpm 10.12.3, Rust/Cargo 1.98.1.

## Behavior

- Normal TRAE bridge delivery or cleanup waiting no longer exposes Apply again.
  No new Open action was introduced. Stopped, current, unknown, and unsupported
  desktop states do not offer Restart merely because delivery is pending.
- Restart remains available for a verified running client that predates its
  launch-settings change. Existing configuration-repair guards remain in place.
- TRAE restart freshness now uses its actual launch configuration timestamp,
  rather than the delivery outbox timestamp. Retrying synchronization no longer
  makes a manually reopened client look stale. Restart validation rechecks both
  launch settings and the delivery intention before taking action.
- Bridge unavailability is distinguished from an acknowledged native
  configuration mismatch. Only a verified mismatch persists the existing
  configuration-changed recovery reason. Offline observation does not claim that
  the user changed configuration. Single-Agent and aggregate status reads retain
  this distinction. Explicit repair remains available for actual mismatches;
  foreign native changes are not silently overwritten.
- Existing five-second connection query refreshes remove the Restart action
  after observing a manual reopen. No click-history flag is used. Existing
  background bridge delivery remains automatic.

Regression tests failed before the changes for pending bridge action visibility,
restart freshness after a manual reopen, and native mismatch classification.
Tests use temporary directories, synthetic process times, mocked connection
queries, and bridge callbacks. They do not launch, restart, or modify installed
Agents.

## Verification

Commands ran from the repository root. pnpm used
`PATH=/Users/baomi/.nvm/versions/node/v22.22.0/bin:$PATH`; Cargo used the default
target directory. Logs are under `/private/tmp/vary-trae-actions-*.log` and
`/private/tmp/vary-trae-restart-*.log`.

The 19-file modified-source manifest is
`/private/tmp/vary-trae-actions-source-sha256.txt`, SHA-256
`f9be2ee1892b01c71d68c4a7939f3d627f26e9b2fbd26b02353eb4cfeaec42c1`.
It includes pre-existing shared-tree source changes and excludes documentation.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | 3,623 tests passed |
| `pnpm audit --prod` | No known vulnerabilities |
| Development, TRAE bridge, WorkBuddy auth, Doubao check-in, Agent rewards, and Qoder codec helper suites | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,240 tests passed, 19 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,240 library and 163 integration tests passed, 19 ignored |
| Running renderer observation | TRAE showed only bridge waiting, with neither Apply again nor Restart |

The existing unused test-fixture variable and macOS debug linker compact-unwind
warnings remain nonfatal. Production Clippy passed with warnings denied.

No application launch, replacement, restart, commit, or push was performed for
this follow-up. The running renderer reflects the action visibility fix through
hot reload. The Rust backend requires its normal rebuild/restart to load the
freshness and mismatch-classification fixes. Linux, Windows, and WSL2 remote CI,
live client lifecycle acceptance, packaging, signing, and installation remain
unverified.
