# Provider and Agent integration verification

Date: 2026-11-10 (Asia/Shanghai).
Host: macOS / Darwin arm64; Node v24.11.0, pnpm 10.12.3, Rust and Cargo 1.98.1.

## Integration scope

The working-tree changes were preserved while advancing
`codex/intent-recognition` from `23a4b2be4` to `bc9d14ca1`, retaining the six remote
commits. The existing Agent-page conflict was resolved by retaining the local
native-model and recovery layout, the remote check-in state queries, and the
remote rule that a pending TRAE bridge must not display a repair action. The
remote removal of Pi's redundant individual-settings label was retained.

The staged source, tests, fixtures, localization resources, attribution and
existing documentation were reviewed before commit. No local preview entries,
generated binaries, debug logs or detected credential material were included.
The reduced public model-context snapshot retains its third-party attribution.

The pre-report tested Git tree is
`02cdc6ae914e4872164eeb2f051f3639654d58d3`, based on `bc9d14ca1`.
This report is the only additional file relative to that tested tree.
The complete source-file SHA-256 manifest and command logs are retained locally
in `/tmp/vary-final-push-checks/`.

## Local verification

| Command                                                            | Result                                |
| ------------------------------------------------------------------ | ------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                |
| `pnpm format:check`                                                | Passed                                |
| `pnpm build:renderer`                                              | Passed                                |
| `pnpm test:unit`                                                   | Passed: 3761 tests                    |
| `pnpm audit --prod`                                                | Passed; no known vulnerabilities      |
| `pnpm test:dev`                                                    | Passed                                |
| `pnpm test:macos-dmg`                                              | Passed                                |
| `pnpm test:trae-bridge`                                            | Passed                                |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                |
| `pnpm test:doubao-checkin`                                         | Passed                                |
| `pnpm test:agent-rewards`                                          | Passed                                |
| `pnpm test:qoder-codec`                                            | Passed; installed-client test skipped |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 6,291 tests; 19 ignored       |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 6,454 tests; 19 ignored       |

`git diff --check` and `git diff --cached --check` passed. Ignored Rust
tests and the installed-client Qoder test were not executed and are not
reported as passed.

The additional merge-focused run passed 286 tests across
`AgentConnectionsPage.test.tsx`, `AgentUpdates.test.tsx`,
`CopyAgentSetupButton.test.tsx` and `RoutingUpstreamsPanel.test.tsx`.
It exercises the overlapping Agent behavior and real copy-feedback translations.

No application launch, installation, signing or notarization was performed.
Local macOS checks do not establish Windows, Linux or WSL2 acceptance.
Remote CI must be checked against the actual pushed revision; its results are
reported separately after push.
