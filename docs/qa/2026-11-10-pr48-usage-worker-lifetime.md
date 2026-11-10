# PR 48 usage worker and Windows module-path verification

Date: November 10, 2026 (Asia/Shanghai).
Base revision: `30710c5c1fed79331c62c2baf5a03fb6993396f0`, plus the source
fingerprints below. Checked in `/tmp/varyai-pr48-ci-20261110`.

## CI evidence and reproduction

[Linux backend job](https://github.com/varium-ai/varyai/actions/runs/42825510588/job/129861718831)
failed the native-agent forwarding/accounting regression while waiting for
asynchronous usage persistence. The same test passed on the preceding revision;
no accounting code changed between those revisions.

The shared usage sender outlived the Tokio runtime that created its consumer.
A different runtime could successfully enqueue work immediately before the
consumer's runtime shut down, discarding queued jobs. A deterministic regression
now creates a worker inside a temporary current-thread runtime, drops that
runtime, submits one synthetic job and checks persistence. Before the fix it
failed immediately with `usage worker stopped with its creator runtime`.
After the fix it passed and retained the exact 11-input/4-output token assertions.
This reproduces a queue lifetime defect; CI's generic timeout does not identify
which individual runtime transition occurred in that failed run.

## Change

The worker owns a dedicated named background thread and receives from the same
bounded 1,000-job channel. All pricing resolution and SQLite writes remain on
that background thread. Forwarding retains `try_send`, the existing sanitized
failure categories and best-effort accounting. Each job retains its database,
agent identity, requested/observed models, tokens, status and latency.

The shared implementation covers macOS, Windows and Linux. No protocol fields,
response semantics, token assertions or test wait limits were relaxed. The
original end-to-end test remains enabled. Process-exit flushing is outside the
scope of this change.

## Windows PowerShell follow-up

The [Windows job for the base revision](https://github.com/varium-ai/varyai/actions/runs/42825510588/job/129861718929)
failed the six original bridge tests and the new ACL inspection. Restricting
the descriptor update to the Access section did not resolve those failures.
The logs still suppress raw PowerShell output, so no specific exception is claimed.

Microsoft documents that Windows PowerShell launched through an intermediate
process can inherit PowerShell 7 module paths and fail to load shared system
modules. Its recommended child-process fix is to remove `PSModulePath`:
[about_PSModulePath](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psmodulepath?view=powershell-7.3).
The bridge's Windows PowerShell process and ACL inspection now do so. The ACL
regression runs in an isolated copy of the test process with an empty synthetic
module directory inherited as `PSModulePath`, then verifies the complete folder
and child-file DACL and unchanged owner/group. It leaves the parent test process's
environment unchanged. Windows execution remains pending for the new revision.

## Verification

Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1; Node 22.22.0; pnpm 10.12.3.
Cargo used `CARGO_TARGET_DIR=/Users/baomi/Work/varium-ai/varyai/src-tauri/target`
as a build cache while compiling and testing this worktree's sources.

| Command                                                                  | Result                                            |
| ------------------------------------------------------------------------ | ------------------------------------------------- |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                 | Passed                                            |
| `cargo check --manifest-path src-tauri/Cargo.toml`                       | Passed                                            |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`       | Passed                                            |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`          | Passed                                            |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml --quiet` | Passed                                            |
| New worker-lifetime regression                                           | Failed before the worker change; passed afterward |
| `git diff --check` and changed-document Prettier checks                  | Passed                                            |

The final library suite passed 6,304 tests with 19 ignored. The full run passed
the same library tests and 163 integration tests. Renderer, helper and dependency
files are unchanged; their local results are recorded in
[the initial CI fix note](2026-11-10-pr48-ci-fixes.md). The frontend and WSL2 jobs
also passed remotely for the base revision.

Linux, Windows and macOS CI must still pass for the new pushed revision. These
local results do not establish cross-platform or packaged-release acceptance.

## Source fingerprints

| File                                    | SHA-256                                                            |
| --------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/proxy/qoder_usage.rs`    | `877264ed8daeebb05911fb63d8166f33ad966e1a46fee2d9161c8d63be4b4de2` |
| `docs/architecture/storage.md`          | `4efcdc5c96df2495db56e4f38549c61692e5cf71e0f4922c8eb217b6bdbdb04e` |
| `src-tauri/src/services/trae_bridge.rs` | `e74ed82d7e36d84a8e7d158b4185537aa608dfa4e318c0da2e66f1ca627c7744` |
