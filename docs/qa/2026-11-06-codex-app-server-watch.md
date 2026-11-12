# Codex app-server routing watcher

Date: November 6, 2026. Checked base:
`417def030f4c23ec6a4577f3492ab8409468ecde` plus this working-tree change.
Pre-existing shared-picker source/tests and intent-routing documentation changes
were preserved. No commit or push was made.

## Behavior

On macOS and Windows, a background worker observes the effective Codex provider, endpoint,
provider credential, catalog and login identity. Configuration changes settle
before a restart decision. Access-token rotation, inactive provider edits and
ordinary selected-model changes do not trigger a restart.

Only previously observed desktop-owned stdio app-servers in the configured Codex
home are eligible. Native discovery verifies the parent application, arguments,
home, PID and process start time. A replacement from Magpie or the application
is adopted. The worker waits for active rollout and paginated-history turns,
rechecks configuration and activity before process termination, and observes
replacement for a bounded period. It never launches or quits the desktop window.
Unreadable activity defers termination. A replacement-process observation alone
does not prove authentication or inference success.

The worker starts with a baseline rather than restarting existing processes on
startup. Runtime process control is enabled on macOS and Windows. Its lifetime
follows the application service; it does not hold request-forwarding database
locks. Diagnostics contain fixed messages without configuration or credentials.

The Windows adapter recognizes the Store package, the cached bundled executable
and supported classic installations. Native APIs validate the user SID, desktop
parent, arguments, selected home and process creation time. Native and WOW64 PEB
layouts are handled separately when reading the process environment. After the
activity recheck, Windows terminates the old app-server and its verified
descendants, allowing inherited stdout handles to close. Each descendant's PID,
creation time and ownership are revalidated. No shell or desktop application is
launched by discovery or restart.

Reference behavior was reviewed in Magpie revision
`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`, including
`internal/provider/codex_app_server.go`. Local incident logs showed seven Magpie
app-server terminations following route changes, alongside desktop identity
initialization failures. This motivated replacement detection and settling; it
does not establish live acceptance of the repair.

## Initial macOS verification

Host: macOS 15.3.1 arm64 / Darwin 24.3.0. Rust/Cargo 1.98.1, Node.js 24.11.0,
pnpm 10.12.3. Node differs from the repository pin of 22.12.0.

The initial crate regression run failed six assertions before implementation.
The final focused suite passed all 12 tests, covering settling, busy-turn
deferral, a final-recheck race, external replacement, PID reuse, timeout,
profile selection, catalog/account changes, token refresh, both history formats,
unreadable activity, native argument parsing and process exclusions. Fixtures
use temporary homes and synthetic data. A separate read-only inspection
confirmed the running ChatGPT server's expected parent, stdio and home shape;
it sent no process signal.

| Command                                                            | Result                                |
| ------------------------------------------------------------------ | ------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                |
| `pnpm build:renderer`                                              | Passed                                |
| `pnpm test:unit`                                                   | 279 files, 3,475 tests passed         |
| `pnpm format:check`                                                | Passed                                |
| `pnpm audit --prod`                                                | No known vulnerabilities              |
| CI Node helper suites                                              | 94 passed, 1 existing skip            |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | 5 passed                              |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,037 passed, 18 ignored              |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 16 binaries; 6,200 passed, 18 ignored |
| Changed-document Prettier and `git diff --check`                   | Passed                                |

The first full Cargo run was interrupted; its rerun completed successfully. The
macOS debug linker emitted a nonfatal compact-unwind size warning.

The seven checked implementation/troubleshooting files remained unchanged through
the successful checks. The SHA-256 of the canonical sorted per-file hash map is
`30d26d1dad2bf0912847c951cf711a9db871f29818300d2e32b82ee9f249fd97`.
Command results and source fingerprints are retained locally in
`/tmp/vary-codex-restart-verification/`. This report was added afterward without
application-source changes.

No application was installed or replaced. No live provider switch or app-server
signal was issued for validation. The existing development watcher could rebuild
from source edits. Actual Magpie-to-VaryAI desktop recovery, authentication,
inference, other operating systems and remote CI remain unverified.

## Windows follow-up

The watcher is now wired into the Windows application service as well. The
Windows adapter uses Toolhelp process snapshots, native process metadata,
same-user token checks and bounded environment reads. Restart cleans up verified
descendants so an inherited output pipe does not keep the desktop disconnected.
Native process handles and creation-time checks protect replacement processes.
The repository's Rust 1.85 minimum remains unchanged.

On the macOS host, Windows x64 application cross-compilation and production
Clippy passed. A later, isolated Windows-target Clippy check against the final
adapter sources also passed, and all six portable Windows process-selection,
home-selection, timestamp and descendant-tree tests passed. No Windows process
was launched or terminated for these checks.

Final shared-tree verification is incomplete. Concurrent work changed the common
watcher, its tests, Desktop restart logic and client activity tracking while
verification was running. An earlier full host test compilation failed during
those Desktop edits; the Windows test-compilation rerun was interrupted before
completion. Earlier complete-check results above do not validate those later
changes. The task-specific cross-compilation target cache was removed after the
interruption to release disk space.

Windows follow-up logs and final adapter fingerprints are retained in
`/tmp/vary-codex-windows-verification/`. Native Windows execution, an actual
Magpie-to-VaryAI switch and required remote platform CI remain unverified.
