# Routing group drag handoff

Date: 2026-11-13. Base revision: `f64d5015be1622bc52740b67ae68c39b0691cac7` with concurrent, unrelated routing restoration work preserved. Host: macOS 15.3.1, arm64. Toolchain: Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

The group sorter removed its floating preview and cell transforms before the asynchronous ordering request and catalog refresh updated the list. Releasing a row therefore exposed the original order before the saved order appeared. The pointer position could also differ from the final row position at release.

The panel now projects the requested order immediately and retains it across queued catalog mutations and refetches. It still computes the persisted, complete group order from the latest cache inside the existing mutation queue. Successful writes update only the cached order; a failed subsequent refresh retains the confirmed order. Failed writes clear the projection without restoring an old catalog snapshot, preserving concurrent group metadata changes.

The drag hook measures visible cells before cleanup, commits the drop synchronously, and animates the cells from those visible positions to their final positions over 180 ms. Reduced motion skips this handoff animation. New drags and unmounts cancel outstanding animations. Pointer thresholds, capture cleanup, click suppression, cancellation and scrolling retain their existing behavior. The renderer implementation is shared by macOS and Windows.

Regression tests were added before each fix. The initial drop-order cases failed against the original implementation; the handoff-animation case failed before animation support; a separate failed-refresh case reproduced a confirmed order reverting. Coverage includes deferred persistence and refresh, save rejection, refresh rejection, concurrent metadata updates, complete inert previews, reduced motion, cancellation, unmount cleanup and auto-scrolling.

An isolated renderer page using synthetic rows and the production hook/CSS verified upward and downward pointer sorting while persistence remained pending. This was a browser check, not packaged Tauri or Windows visual acceptance. Its temporary source files were removed; the installed application was not launched or replaced.

Verification of the original drag fix, before the toast follow-up:

| Command                                                            | Result                                                                                           |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `pnpm typecheck`                                                   | Passed.                                                                                          |
| `pnpm build:renderer`                                              | Passed.                                                                                          |
| `pnpm format:check`                                                | Passed.                                                                                          |
| Prettier check for the changed tests and this report               | Passed.                                                                                          |
| Focused panel and drag-hook tests                                  | Passed: 125 tests.                                                                               |
| `pnpm test:unit`                                                   | Passed on the final retry: 304 files, 4,058 tests.                                               |
| `pnpm test:dev`                                                    | Passed: 8 tests after granting localhost access.                                                 |
| `pnpm test:macos-dmg`                                              | Passed.                                                                                          |
| `pnpm test:trae-bridge`                                            | Passed.                                                                                          |
| `pnpm test:model-default`                                          | Passed.                                                                                          |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed.                                                                                          |
| `pnpm test:doubao-checkin`                                         | Passed.                                                                                          |
| `pnpm test:agent-rewards`                                          | Passed.                                                                                          |
| `pnpm test:qoder-codec`                                            | Passed.                                                                                          |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed.                                                                                          |
| `pnpm audit --prod`                                                | Passed; no known production vulnerabilities.                                                     |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed.                                                                                          |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed.                                                                                          |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed.                                                                                          |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed with localhost access: 7,471 passed, 21 ignored.                                          |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Incomplete: disk exhaustion, then missing build artifacts and killed build scripts during retry. |

The first sandboxed Rust test run could not bind isolated loopback listeners; it is not a successful test result. The rerun with localhost access passed. An initial full frontend run passed before the additional refresh-failure regression. The next full frontend run passed 4,057 tests, but one suite could not import because the concurrently running Rust compilation exhausted disk space (`ENOSPC`). Its isolated rerun and the subsequent full frontend retry passed. The first full Rust compilation exhausted disk space and its duplicate retry was stopped. When free space recovered, a further full Rust attempt failed with missing build-output directories and SIGKILL errors in dependency build scripts. Full backend integration verification therefore remains incomplete; these attempts do not establish a source regression. Logs are under `/tmp/vary-drag-*.log`.

The initially tested drag implementation has SHA-256 `ad68a6e12b6c863c2fabe54e536c63bcf26d251fdb367799096412bfe4c02a45`; the initially tested panel has SHA-256 `7e676f233e3c7fda1ff14d8fc9cee95552fb3e8dacf78848f8a5d9f92e7c7049`. Both remained unchanged through that verification. Unrelated restoration changes in the shared checkout were preserved. No commit or push was made; remote CI and packaged macOS/Windows acceptance remain unverified.

## Order save feedback follow-up

Successful pointer and keyboard ordering now show the shared Sonner success toast as soon as the ordering API confirms persistence. The message uses `routingPage.orderSaved` in all four runtime locales (English: “Group order saved”). Pending requests, rejected writes and unchanged ordering do not show success. A subsequent refresh failure does not suppress or duplicate the confirmation of an already saved order.

The existing ordering regressions now check those toast boundaries, including exactly one success before a deferred refresh completes. Three success assertions failed before implementation, while the rejected-write case passed. Follow-up verification on the same host and toolchain passed: `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`, focused panel/drag tests (125 tests), and `pnpm test:unit` (304 files, 4,058 tests). Changed tests and this report also pass Prettier, and `git diff --check` passes. The final panel SHA-256 is `bbf16a28a1e9468e2a4420cce07acbc6f476c0a4560f78e16e1cd27663d95edf`; the panel test SHA-256 is `e8cf86f92bfcceb6e7ede710a07f410b3850e658d2eb2896c40d7b2f178b3fbe`. The follow-up changes only renderer feedback, the four locale files, regression assertions and this report; the earlier backend verification limitations remain unchanged.

## Combined submission verification

The unchanged backend sources subsequently passed `cargo test --manifest-path
src-tauri/Cargo.toml` in the same checkout: 16 targets, 7,636 passed and 21 ignored.
This successful run supersedes the earlier incomplete full-test attempts above.
The final routing panel, tests and drag hook still match the SHA-256 values recorded
here. After the last concurrent UI edits, the entire panel suite passed 111 tests,
and type checking, renderer build and formatting passed again. The full renderer
suite passed all 304 files and 4,058 tests after cache cleanup.

Both routing fixes are submitted together; the complete local check record and
toolchain limitations are in [Removed routing group restoration](2026-11-13-routing-group-restore.md).
Remote CI for the submitted revision must establish the repository-pinned Node
runtime, Linux, Windows, macOS and Windows-to-WSL2 acceptance.
