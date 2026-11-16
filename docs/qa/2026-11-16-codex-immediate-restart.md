# Immediate confirmed Codex restart — November 16, 2026

## Behavior

The manual Restart command previously acknowledged stale-catalog refreshes as
soon as it queued an idle-dependent worker action. Active or unreadable task
records could leave the application running after the confirmation closed.

An explicitly confirmed restart now immediately enters the existing native
whole-application lifecycle on macOS and Windows. It does not wait for task
activity to become idle, and the command stays pending until the native result
is available. Quit refusal and launch failure reach the existing localized
error feedback. Configuration and application identity guards still apply.

The idle-dependent worker now handles automatic routing refreshes only. Its
request, rollout and final-recheck activity guards remain intact. While an
automatic refresh waits for idle, the row offers an explicit manual restart.
Catalog-only changes still require confirmation and the worker adopts a new
application's service without terminating it again.

All four runtime languages explain that manual restart immediately interrupts
running tasks and can lose unsent drafts. Every Codex restart confirmation uses
that guidance, including the desktop-age and automatic-waiting cases. The
existing row spinner remains active throughout the native operation and blocks
duplicate clicks. Automatic-refresh status text is separate from manual restart.

## Verification

Base revision: `f9bbcb16b3f7d1e2d4b6ca8ce48840e245ca7e4f`, with preceding
WorkBuddy and Codex tool-discovery changes already present. Their 12 changed
backend files still match the preceding verification's hashes. No commit or
push was requested.

Host: macOS 15.3.1 arm64, Rust/Cargo 1.98.1, Node.js 24.11.0 and pnpm 10.12.3.
The local Node version differs from the repository's 22.12.0 CI pin. Cargo
verification disables incremental compilation and uses process-local loopback
proxy exclusions. Native lifecycle tests use fake runtimes; activity fixtures
use temporary files and databases.

Before the fix, both
`confirmed_codex_restart_enters_native_lifecycle_without_queuing_for_idle` and
`confirmed_codex_restart_stays_pending_until_native_relaunch_finishes` failed:
the command returned without entering the native lifecycle or awaiting its
result. They now pass, together with conflict and stale-state guards. Previous
queued-manual-worker tests were replaced by direct-command and automatic-only
worker coverage; native process-identity and relaunch tests remain unchanged.

| Check                                                                                                     | Result                                                     |
| --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| Agent connection command tests                                                                            | 21 passed                                                  |
| Codex automatic worker tests                                                                              | 46 passed                                                  |
| Native desktop lifecycle tests, including portable Windows cases                                          | 26 passed                                                  |
| Gateway activity guards                                                                                   | 4 passed                                                   |
| Focused UI and confirmation translation tests                                                             | 265 passed                                                 |
| `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`                                              | Passed                                                     |
| `pnpm test:unit`                                                                                          | 4,235 passed in 317 files                                  |
| `pnpm audit --prod`                                                                                       | Passed                                                     |
| Development, DMG, bridge, model-default, credential, check-in, reward, codec and storage-recovery helpers | Passed; existing installed-client codec skip retained      |
| `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings`                                         | Passed                                                     |
| `cargo test --lib`                                                                                        | 7,613 passed; 21 ignored                                   |
| `cargo test`                                                                                              | 7,778 passed (7,613 library + 165 integration); 21 ignored |

Command logs, per-file source hashes and result JSON are under
`/tmp/varyai-codex-immediate-checks/`. Test-first logs use
`/tmp/varyai-codex-immediate-red-{rust,ui}.log`.

No real Codex/ChatGPT application was quit or reopened, and no running VaryAI
installation was replaced. The currently running installed VaryAI has not
loaded this source change. Installed-client restart acceptance and remote
Linux, Windows, macOS and Windows-to-WSL2 CI remain unverified; local synthetic
tests do not establish those results.

The final changed-source and test manifest SHA-256 is
`a3456e43f8409fa027f21d43277b263dcb4ac61c4373556a628aca26379cbde0`. Its per-file hashes matched after all checks.
The test build retained the existing test-only quota helper warning and macOS
linker compact-unwind warning; Clippy completed with warnings denied.
Documentation formatting and `git diff --check` passed. No generated artifacts
were added to Git.
