# Agent route restart recovery — 2026-11-11

## Scope and behavior

The checked tree is based on `7a00b1869bce94b0c341f0f199f9a8c6993923aa` plus
this task's source and documentation changes. The initial working tree was clean.
No commit, push, application installation or live client restart was performed.

Normal exit suspended shared Agent receipts, but startup checked their current
connected status instead of the saved connection intent. Startup now includes
suspended receipts and both TRAE editions. Configuration ownership checks still
run during publication; a saved receipt does not authorize overwriting external
configuration changes.

Codex keeps a private connection identity next to its configuration receipt.
Recovery records the owned local bearer credential, historical model mappings
and catalog paths before removing the configuration receipt. Startup or explicit
reconnection can reuse that identity. This supports both quitting/reopening
VaryAI and explicitly switching Codex routing off/on without requiring the user
to quit and reopen Codex. Identity files use the existing private atomic writer,
are bound to the selected Codex configuration, and retain the existing mapping
and catalog limits. No upstream credential or login file is added to this record.

Switching routing off clears the active menu. The saved identity alone neither
accepts local menu requests nor resumes a disconnected connection at startup.
An interrupted response stream cannot be resumed by reopening the gateway; the
next message or a retry is a new request. Removed models and invalid upstream
authentication remain normal recovery conditions. Codex's app-server observer
starts after startup restoration so temporary cleanup is not mistaken for a
live route change. These lifecycle changes use the same code on macOS and Windows.

## Regression evidence

Before implementation, the shared lifecycle test failed when changed to invoke
the actual startup function instead of manually starting and synchronizing the
service. The international-only bridge startup test also failed. After isolating
the HTTP fixture and initializing its synthetic security runtime, the Codex
restart test failed because the new process replaced the cached credential.
Earlier fixture setup failures are not counted as reproductions of the defect.

The final Codex HTTP test uses a temporary configuration, an in-memory database,
a mock Responses upstream and a dynamically allocated gateway port. A fresh
manager/runtime resumes the same gateway address while the simulated client
keeps its credential and model name. Both the old model name after a catalog
rename and the current model forward successfully. Switching off rejects the
cached credential; explicitly switching on again accepts it and forwards another
old-conversation request. Additional tests verify private file permissions,
configuration binding, malformed-record rejection and no automatic startup
reconnection after explicit disconnection. No installed client is controlled.

## Verification

Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1 stable; pnpm 10.12.3. Final frontend
checks use the pinned Node.js 22.12.0, downloaded to a temporary directory and
verified against the official archive checksum. Preliminary Node.js 20.20.2
checks are not the final frontend results. Rust checks use cached dependencies
with `--offline`, a temporary `CC_SWITCH_TEST_HOME`, and incremental compilation
disabled. Backend source inventories remained identical throughout the checks.
Logs, command results and source hashes are under
`/tmp/vary-restart-verification/`.

| Check                                                                           | Result                                                     |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `pnpm typecheck`                                                                | Passed                                                     |
| `pnpm build:renderer`                                                           | Passed                                                     |
| `pnpm format:check` and changed-document Prettier                               | Passed                                                     |
| `pnpm test:unit --maxWorkers=2`                                                 | 3 failed, 3,845 passed; 291 files passed, 1 failed         |
| `pnpm test:dev`, `test:macos-dmg`, `test:model-default`                         | Passed                                                     |
| `pnpm test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards` | Passed                                                     |
| `pnpm test:qoder-codec`                                                         | Passed, 1 existing skipped case                            |
| `pnpm test:trae-bridge`                                                         | 55 passed, 3 failed                                        |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                           | Passed                                                     |
| `pnpm audit --prod`                                                             | Passed; no known production vulnerabilities                |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                        | Passed                                                     |
| `cargo check --manifest-path src-tauri/Cargo.toml`                              | Passed                                                     |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`              | Passed                                                     |
| Focused Codex client, shared connections, app-server and startup tests          | 254 passed, 1 existing ignored case                        |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                         | Aborted in a TRAE fixture; incomplete                      |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`                | Failed: library and `profile_bundle` TRAE fixtures aborted |
| `git diff --check`                                                              | Passed                                                     |

The frontend failures are the unchanged authorized-model-save cases for WorkBuddy,
WorkBuddy AI and Qoder in `AgentConnectionsPage.test.tsx`. An isolated run with one
worker reproduced all three. The unchanged TRAE Node tests fail the resident
bridge bootstrap case and two renderer model-identity cases. The Rust library
test `trae_reconnection_delivers_latest_offline_selection_without_enabling_audit`
rejects an operation in its fake bridge, then aborts during destructor cleanup;
an isolated run reproduces the abort. The `profile_bundle` integration test
`trae_profiles_deliver_when_connected_without_binding_preview_to_runtime` likewise
rejects an unexpected bridge operation and aborts during cleanup. The failing
fixtures, their assertions and the TRAE implementation were not changed or
disabled by this task. These failures are verification gaps, not passing checks.

The nonfatal macOS compact-unwind linker warning remains. Linux, native Windows,
WSL2, remote CI, a real Codex conversation and packaged-application acceptance
have not been verified. The source fix must be included in the running backend
before live recovery can be accepted.
