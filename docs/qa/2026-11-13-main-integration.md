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

| Check                                                              | Result                                                                        |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                                        |
| `pnpm format:check`                                                | Passed                                                                        |
| `pnpm build:renderer`                                              | Passed                                                                        |
| `pnpm test:unit`                                                   | 306 files, 4,087 tests passed                                                 |
| All eight development/client helper suites                         | Passed; installed Qoder codec acceptance skipped because its worker is absent |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                                                        |
| `pnpm audit --prod`                                                | No known vulnerabilities                                                      |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                                        |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                                        |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 7,500 passed, 21 ignored                                                      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                                        |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 7,665 passed, zero failures, 21 ignored across all targets                    |

The separate unstaged manual-acceptance record was preserved and excluded from
this merge commit. Tests used synthetic fixtures, temporary storage and dynamic
loopback ports. No installed application was launched or replaced by these checks.

## Remote acceptance boundary

This record captures the local checkpoint before pushing PR #55. Linux, Windows,
macOS and the applicable Windows-to-WSL2 CI results must be checked for the exact
pushed revision. Local success does not establish those results or live-client,
installer, signing or notarization acceptance.
