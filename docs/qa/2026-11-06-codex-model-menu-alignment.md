# Codex model-menu reload verification

Date: November 6, 2026. Base revision:
`417def030f4c23ec6a4577f3492ab8409468ecde`, plus the current uncommitted
working tree. Existing watcher, desktop restart, Windows adapter, model-picker,
and localization changes were preserved. No commit or push was requested.

## Behavior reviewed

The Codex app-server monitor is already attached to the application service on
macOS and Windows. Observed route changes settle before an idle reload; a
catalog-only change offers a confirmed manual refresh. Both paths wait for
running turns and complete response bodies, including the quiet interval after
requests. A replacement process is observed separately from a termination
request. Process replacement does not prove authentication or inference.

This follow-up adds the native `/backend-api/codex/responses/compact` endpoint
to the proxy activity guard. Its regression failed before the implementation
change and passed afterward. Without that guard, a native compaction request
could be omitted from the request-level reload deferral. The read-side activity
methods are compiled only for supported desktop platforms and portable tests.
The request wrapper preserves status, headers, data frames and trailers.

The shared tree also now rejects a final reload recheck without a valid model
catalog, rather than treating two failed reads as equal. Its regression failed
in an earlier concurrently changing tree and passes in the verified unit run.
The route and catalog are checked again before any process operation.

Reference behavior was inspected in
[Magpie's app-server watcher at 1d0fc2b](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/provider/codex_app_server.go).
This is the Codex reload contract, not an assertion of parity with every Magpie
integration.

## Local verification

Host: macOS 15.3.1 / Darwin arm64; Rust and Cargo 1.98.1; Node 24.11.0;
pnpm 10.12.3. Node differs from the repository's exact 22.12.0 pin.

| Command                                                                | Result                                                         |
| ---------------------------------------------------------------------- | -------------------------------------------------------------- |
| `pnpm typecheck`                                                       | Passed                                                         |
| `pnpm build:renderer`                                                  | Passed                                                         |
| `pnpm test:unit`                                                       | 279 files, 3,483 tests passed                                  |
| `pnpm format:check`                                                    | Passed                                                         |
| `pnpm audit --prod`                                                    | No known vulnerabilities                                       |
| Development, bridge, WorkBuddy, Doubao, reward and Qoder helper suites | Passed; one existing Qoder codec skip                          |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                  | 5 passed                                                       |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`               | Passed                                                         |
| `cargo check --manifest-path src-tauri/Cargo.toml`                     | Passed                                                         |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`     | Passed                                                         |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                | 6,084 passed, 18 ignored                                       |
| `cargo test -j 1 --manifest-path src-tauri/Cargo.toml`                 | 16 test programs; 6,247 passed, 18 ignored                     |
| Final full run with `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1`           | 16 test programs; 6,247 passed, 18 ignored on the checked tree |

Earlier attempts encountered unfinished concurrent Desktop edits and the invalid
catalog regression. These results are retained separately and are not counted
as passes. The final integration compilation exhausted disk space while
creating an archive. Idle crate incremental caches were removed only after
acquiring the Cargo build lock and their session locks; the full test command
was then restarted with one build job. Later targeted rechecks again exhausted
space while saving an incremental query cache. Failed test compilation objects
were removed under the Cargo build lock, and final checks disable incremental
compilation. The final full repetition includes the subsequent shared-manager
and isolated TLS-test updates. The macOS test linker emitted its existing
nonfatal compact-unwind size warning.

Logs, individual command results and per-file source hashes are retained locally
in `/tmp/varyai-codex-alignment-verification/`. The SHA-256 of the canonical sorted
source hash map is
`c21ae0182cf7fbc502ab832088182bdae1a1a59e49b559484d4ceca49df6929c`.
The final scripted checks compare source hashes before and after each Rust command.
Renderer sources remained unchanged after their successful checks. This report
is added separately from application source changes.

During the final full run, concurrent work changed
`src-tauri/src/services/agent_connections.rs` and
`src-tauri/src/services/deepseek_harness.rs`. The Codex watcher, proxy activity
guard, Codex command bindings and renderer remained unchanged. The passing full
run does not validate these later edits: final whole-tree acceptance remains
incomplete until the affected checks run against a stable tree. The command
results retain the exact before/after file-hash differences. No commit or push
was made from this changing tree.

No installed application was launched, replaced, signalled or quit for these
checks. Native process behavior uses synthetic fixtures and fake runtimes;
HTTP tests use temporary configuration and dynamic loopback ports. Linux,
Windows and macOS remote CI, the Windows-to-WSL2 contract, real-client takeover,
authentication, inference, installation, signing and release acceptance remain
unverified by this local run.
