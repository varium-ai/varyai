# PR #44 provider notes fixture verification

Date: 2026-11-06. Checked base: `00c94e67d696eb6247d744ad85451ac1418c553e`
plus the one-line Rust fixture repair and this report. Checks ran in an isolated
worktree; concurrent changes in the original checkout are excluded.

## Failure evidence and repair

[CI run 42179901692](https://github.com/varium-ai/varyai/actions/runs/42179901692),
Linux job `127766880614`, failed only
`database::routing_upstreams_compat_tests::provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration`.
It reported `routingUpstreams.invalid`; all integration test binaries passed.
The original fixture also failed locally with the same error before the repair.

The fixture passed the editor's intentionally redacted provider record directly
to `resolve_provider`, whose validation requires the private API key. It now uses
`Database::routing_discovery_provider` with the saved ID and revision, which
resolves the private stored record through the existing database path. The
assertions for saving and editing notes, sanitized debug output, and exclusion
from executable configuration remain intact. No production behavior, credential
redaction, workflow, assertion or ignored-test setting changed.

## Local verification

Host: macOS 26.6.2 arm64; Node.js 22.22.0; pnpm 10.12.3; Rust/Cargo 1.98.1.
All commands ran against the isolated final source tree. Cargo reused the local
target directory with `CARGO_INCREMENTAL=0`; build-directory locking serialized
it with other local builds. Logs and per-command exit results are retained under
`/tmp/varyai-pr44-ci-verification` for this session.

| Check                                                              | Result                                 |
| ------------------------------------------------------------------ | -------------------------------------- |
| Focused provider notes regression                                  | Failed before repair; passed afterward |
| `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`       | Passed                                 |
| `pnpm test:unit`                                                   | 279 files, 3,476 tests passed          |
| `pnpm test:dev`, `pnpm test:trae-bridge`                           | Passed                                 |
| `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`     | Passed                                 |
| `pnpm test:agent-rewards`, `pnpm test:qoder-codec`                 | Passed                                 |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | 5 passed                               |
| `pnpm audit --prod`                                                | No known vulnerabilities               |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                 |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                 |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,053 passed, 18 ignored               |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`   | 6,216 passed, 18 ignored               |
| `git diff --check`                                                 | Passed                                 |

Rust linking emitted the existing nonfatal compact-unwind size warning. No
installed application was launched, replaced or restarted.

At commit preparation, the original revision's frontend and Windows-to-WSL2 jobs
had passed; Linux had failed as described, and macOS and Windows were pending.
Cross-platform acceptance requires the subsequent pushed revision's CI to finish;
the local macOS checks do not establish Linux, Windows or WSL2 acceptance.
