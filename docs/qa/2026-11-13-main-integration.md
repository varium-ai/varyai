# Main integration and routing UI resolution

## Checked source

Integrated `origin/main` at `f80ef365063ef774424ffa177ae0052953e44f9e` into
`codex/intent-recognition`, starting from `4a5e90aba7919b028242f44d2cf134bbea15e501`.
The checked source tree before this report is `aa95a5efd60fde1c965b4811a617c020e7a5e14a`.
SHA-256 of the staged binary diff against the starting revision, excluding this
report: `b1a4dbdbdbe60247db49826552167a00d30398fb02a3d5b2c0aa9bef403f0501`.

The user explicitly selected main's simplified routing list. The merge retains
that layout and retires the More menu, sharing, import and JSON export controls.
The old local export regression was removed with its retired UI; main's absence
assertions for removed controls remain. The initial typecheck exposed that old
export test after main removed its user-event import. Final typechecking and the
complete renderer suite passed after removing the retired test.

Local Agent routing overwrite, inline errors, remembered model selections,
shared-profile restoration, profile-name confirmation and DLP changes remain.
The integrated main changes include routing restoration, drag ordering, provider
loading, update progress, model picker presentation and Codex model cache expiry.

## Local verification

Host: macOS 26.6.2 (25G83), Apple Silicon. Node 22.12.0, pnpm 10.12.3,
Rust/Cargo 1.98.1. Checks ran in an isolated worktree whose staged tree matched
the merge index. Rust used `CARGO_INCREMENTAL=0` and disabled dev/test debug
symbols. A client interruption stopped one incomplete full Rust run; its final
rerun completed successfully.

| Check                                                              | Result                                                     |
| ------------------------------------------------------------------ | ---------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                     |
| `pnpm format:check`                                                | Passed                                                     |
| `pnpm build:renderer`                                              | Passed                                                     |
| `pnpm test:unit`                                                   | 306 files, 4,087 tests passed                              |
| All eight development/client helper suites                         | Passed; Qoder codec suite: 3 passed, zero skipped          |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                                     |
| `pnpm audit --prod`                                                | No known vulnerabilities                                   |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                     |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                     |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 7,500 passed, 21 ignored                                   |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                     |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 7,665 passed, zero failures, 21 ignored across all targets |

The separate unstaged manual-acceptance record was preserved and excluded from
this merge commit. Tests used synthetic fixtures, temporary storage and dynamic
loopback ports. No installed application was launched or replaced by these checks.

## Remote acceptance boundary

This record captures the local checkpoint before pushing PR #55. Linux, Windows,
macOS and the applicable Windows-to-WSL2 CI results must be checked for the exact
pushed revision. Local success does not establish those results or live-client,
installer, signing or notarization acceptance.

## Linux Desktop matrix follow-up

[CI #167](https://github.com/varium-ai/varyai/actions/runs/43399168907)
reproduced a failure in the new all-Agent explicit-connection regression:
Claude Desktop projection is intentionally unsupported on Linux, but the matrix
expected a successful connection for every platform. The existing portable
projection contract already requires Linux to reject Desktop projection.

The matrix now asserts the expected error and absence of an owned receipt on
unsupported Desktop platforms, then continues testing all portable Agents.
macOS and Windows retain the full Desktop overwrite, restoration and stale-catalog
checks. The test remains enabled everywhere; no assertion is suppressed for
supported Agents and no production behavior changed.

Validation starts from `7aaf7b189f33ae1602ebf88e5fcd73fa66eb6144`.
SHA-256 of `src-tauri/src/services/agent_connections.rs`:
`95ade57e8ed4a8769dd017f43cee4b537b4699c2781b4cf60ab239f6bd9bba32`. The host, toolchain and isolated worktree are the same as above.
Rust formatting, compilation, Clippy and the complete library/integration suites
were rerun successfully: 7,500 library tests and 7,665 tests across all targets
passed, with 21 existing ignored tests. Renderer, helper and dependency sources
are unchanged from the preceding verified integration.

The first repeated full Rust run returned a synthetic upstream 502 in
`codex_grok_chat_normalizes_tool_schema_patterns`. That test passed immediately
in isolation and the subsequent unchanged full run passed. The failure record
was retained; no Grok implementation or assertion was changed. The cause of that
single failed attempt was not established.

The preceding CI run passed frontend, Windows backend and Windows-to-WSL2 checks.
The new pushed revision still requires its own complete CI results.
