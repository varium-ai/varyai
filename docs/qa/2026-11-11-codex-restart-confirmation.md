# Confirmed ChatGPT application restart — 2026-11-11

## Final scope

The user clarified that manual Restart must restart the unified ChatGPT/Codex
desktop application and requested no interface changes. Earlier experimental
confirmation-title, localization and dialog-dismissal changes were withdrawn.
Concurrent daily-check-in interface work is outside this task and is preserved.

Read-only installation metadata confirmed that the installed `ChatGPT.app` uses
`com.openai.codex`. The earlier conversational claim that this identifier
necessarily referred to a separate Codex application was incorrect. Existing
native desktop selection already reopens the actual verified application bundle.
Read-only process discovery found an application-owned stdio server using the
selected Codex configuration directory. No live application was quit or launched.

## Backend changes

Confirmed manual desktop restarts accept both stale and current verified ChatGPT
instances. The prior launch-age condition could reject a confirmed request after
a recent application launch. Configuration timestamps, process identity checks,
replacement protection and refusal to launch an unidentified or stopped target
remain in place on macOS and Windows.

A stale internal model catalog is exposed through the existing desktop-restart
state when a current desktop application is available. The fallback queued manual
action now restarts the whole application after the existing idle and final
configuration checks. Automatic route-change actions remain internal-service
reloads. Queued manual requests revalidate their observed service identities
before entering desktop restart. Neither path launches a terminal CLI.

## Verification

Base: `7a00b1869bce94b0c341f0f199f9a8c6993923aa` plus preceding route recovery work.
Host: macOS 26.6.2 arm64, Rust/Cargo 1.98.1. Tests use synthetic process runtimes,
temporary directories and dynamic ports; native application control is disabled
in tests. macOS and portable Windows regressions both reproduced
`agentConnections.restartChanged` for a current application before implementation.
Additional contracts cover manual whole-application action selection, desktop
restart status, replacement protection and automatic service-reload separation.

Final backend checks use cached dependencies with `--offline`, a separate Cargo
target directory, temporary client storage and incremental compilation disabled.
Logs and source hashes are in `/tmp/vary-restart-verification/chatgpt-full-*`.
Renderer checks use Node.js 22.12.0 and pnpm 10.12.3; their logs are in
`/tmp/vary-restart-verification/chatgpt-final-renderer-*`.

| Check                                                                        | Result                                             |
| ---------------------------------------------------------------------------- | -------------------------------------------------- |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                     | Passed                                             |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml`                 | Passed                                             |
| `cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                             |
| Desktop lifecycle tests, including portable Windows cases                    | 20 passed                                          |
| Codex app-server observer tests                                              | 42 passed                                          |
| Agent connection command tests                                               | 14 passed                                          |
| `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`                 | Passed                                             |
| `pnpm audit --prod`                                                          | Passed                                             |
| `pnpm test:unit`                                                             | 3 failed, 3,848 passed; 292 files passed           |
| `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml`            | Aborted in the previously recorded TRAE fixture    |
| `cargo test --offline --no-fail-fast --manifest-path src-tauri/Cargo.toml`   | Library and `profile_bundle` TRAE fixtures aborted |
| Backend source hashes before and after all checks                            | Identical                                          |
| `git diff --check`                                                           | Passed                                             |

The full library run again aborted in
`trae_reconnection_delivers_latest_offline_selection_without_enabling_audit`:
the fake bridge rejects an operation and panics during destructor cleanup. This
fixture was not changed or disabled. Frontend failures are the same recorded
authorized model-save cases for WorkBuddy, WorkBuddy AI and Qoder. Concurrent
daily-check-in UI work was present in the renderer tree and is preserved.
The full integration run also aborted in the previously recorded
`trae_profiles_deliver_when_connected_without_binding_preview_to_runtime`
fake-bridge case in `profile_bundle`; all other test programs passed. No failing
fixture was disabled or weakened. Full checks remain incomplete; results for
withdrawn UI experiments do not validate this final backend implementation.
Native Windows, Linux/WSL2 CI, a real model conversation and installed-application
acceptance remain unverified. No commit, push or application installation was
performed.
