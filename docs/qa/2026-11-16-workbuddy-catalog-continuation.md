# WorkBuddy catalog and continuation verification

- Base revision: `f9bbcb16b3f7d1e2d4b6ca8ce48840e245ca7e4f`, with the working-tree changes listed below.
- Host: macOS, Apple Silicon (`Darwin arm64`).
- Tools: Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3.
- SHA-256 of the sorted changed Rust paths and contents, separated by NUL bytes: `fac97411a2a5eeaf3a1aa5efb168e5dcbe0e81976bc871d058a06e1d2e9969e6`.
- Changed implementation: Agent model projection and visible IDs; WorkBuddy Chat continuation policy and durable scope restoration.
- Changed regressions: `source_label_tests.rs`, `parity_tests.rs` and `workbuddy_history_tests.rs`.

## Reproductions

Before implementation, the catalog regression published `Model A · legacy:Model A · Relay` instead of `Model A · Relay A`. The continuation regression returned HTTP 409 instead of 200 for a complete visible text follow-up after a saved target configuration changed.

The catalog regression now migrates actual temporary native files for both WorkBuddy editions, retains historical request aliases, verifies the discovery ID, and checks repeated synchronization for stability. The parity regression verifies that a renamed model publishes its current ID while all previous aliases still identify the same target. The HTTP regression covers both editions with JSON and streaming responses: an initial successful turn, a configuration refresh, rejection of sealed reasoning at the changed destination, and a successful visible-text follow-up. Tests use synthetic credentials, temporary storage and dynamic ports.

## Checks

| Command                                                            | Result                                                            |
| ------------------------------------------------------------------ | ----------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                            |
| `pnpm build:renderer`                                              | Passed                                                            |
| `pnpm test:unit`                                                   | Passed: 317 files, 4,225 tests                                    |
| `pnpm format:check`                                                | Passed                                                            |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                                  |
| `pnpm test:dev`                                                    | Passed: 8 tests                                                   |
| `pnpm test:macos-dmg`                                              | Passed: 5 tests                                                   |
| `pnpm test:trae-bridge`                                            | Passed: 66 tests                                                  |
| `pnpm test:model-default`                                          | Passed: 9 tests                                                   |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed: 5 tests                                                   |
| `pnpm test:doubao-checkin`                                         | Passed: 13 tests                                                  |
| `pnpm test:agent-rewards`                                          | Passed: 10 tests                                                  |
| `pnpm test:qoder-codec`                                            | Passed: 2 tests; 1 installed-client test skipped                  |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed: 5 tests                                                   |
| Focused catalog migration regression                               | Passed                                                            |
| WorkBuddy regression suite, before the final test-only expansion   | Passed: 100 tests, 1 ignored                                      |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                            |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 7,608 tests; 21 ignored                                   |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                            |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 7,608 library tests and 165 integration tests; 21 ignored |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                            |

Earlier full Rust runs failed the previous parity assertion that required an unchanged alias map after a model rename. The updated assertion requires a new visible alias and verifies that every historical alias keeps its target. Both final full runs above include that update. The default ignored Rust tests and the installed-client Qoder helper were not executed.

## Acceptance limits

Live diagnostics confirmed a local `routing_unknown_continuation` rejection. At inspection time, the native file and the gateway's read-only model catalog had short display names; the exact repeated screenshot label was not reproduced in those live sources. The historical-ID regression establishes and repairs a persistence path that a display-name-only refresh did not address.

The installed applications were not replaced or restarted for verification. Synthetic success does not establish live WorkBuddy menu or conversation acceptance. Remote Linux, Windows and macOS CI has not been run for this uncommitted tree. No minimum-Rust-version acceptance is claimed.
