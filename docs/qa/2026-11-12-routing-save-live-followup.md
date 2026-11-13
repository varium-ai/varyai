# Live routing save latency follow-up

Date: 2026-11-12. Host: macOS arm64. The earlier
[metadata-index verification](2026-11-12-routing-save-performance.md) established
a reproducible metadata improvement but did not validate the user's full live
save. This follow-up uses phase logs from the running development application.

## Live evidence

| Live save                                       | Persistence | Global publication | Client synchronization | Total     |
| ----------------------------------------------- | ----------- | ------------------ | ---------------------- | --------- |
| 16:34:03 Asia/Shanghai                          | 4 ms        | 107 ms             | 16,647 ms              | 16,760 ms |
| 16:36:43 Asia/Shanghai                          | 4 ms        | 125 ms             | 14,256 ms              | 14,386 ms |
| 17:04:13 Asia/Shanghai, fixed development build | 3 ms        | 129 ms             | 512 ms                 | 646 ms    |

The active Codex menu was off in these samples (`codex_ms=0`). Both slow saves
already contained the metadata index and timing changes. The `--no-watch`
launcher does not automatically rebuild after further source changes.

At the user's request, the current process was sampled while the user performed
the second save. The save's shared TRAE synchronization called `prepare_shared`
and the international edition's `Client::prepare`, reaching `ensure_ready_for`.
Samples show helper polling sleeps and startup-lock waits inside these paths.
The periodic reconnect worker also used these same catalog switch locks. Thus a
route edit could initiate bridge preparation or wait behind periodic preparation.
No client configuration, account identity or secret appears in this record.

## Scope and correction

Creating a route can change connected clients' selectable model catalogs.
Publishing that catalog does not require preparing the native connection again
or rewriting its launch arguments. Catalog delivery now keeps its existing
outbox, token, selection-preservation and runtime-binding logic, while connection
preparation remains on explicit connect/reapply paths.

The existing periodic recovery loop reconnects previously authorized desktops
outside catalog switch locks. This reconnect does not rewrite launch settings.
It still delivers the latest catalog after reconnecting. Both China and
international editions use the same separation; Unix sockets and Windows named
pipes retain their existing delivery protocols. Missing bridges leave a durable
pending catalog and do not become falsely configured.

Per-client elapsed-time logs now distinguish Codex, each TRAE edition and other
clients, without group names, model IDs, credentials or configuration payloads.

## Verification status

The new regression failed against the old synchronization path: no latest catalog
was queued for the offline China edition. The fixture uses a temporary home,
in-memory database, dynamic proxy port and synthetic client launch settings.
It verifies queued catalogs for both editions, preservation of launch arguments,
absence of newly created launch backups and no false active runtime binding.
An additional portable test checks that explicit bridge preparation requires
existing connection intent and consent.

The focused TRAE suite passed: 38 passed, 3 existing live checks ignored. A complete
library run passed 6,357 tests, with 21 ignored. Check and Clippy passed on the
same backend source. During the subsequent shared-directory run, compiled
artifacts disappeared: 17 tests that re-execute the test executable failed with
`NotFound`/`Unavailable`, and the development build failed to encode missing
metadata. The user then reported `swift-rs` failing because its referenced
`libserde-08a5add3bd38d4cb.rlib` was absent. These interrupted runs do not count as
acceptance; no assertion was weakened to suppress them.

The stale `swift-rs` build outputs were cleared after build processes stopped.
A dependency-only `cargo check -p swift-rs` encountered a Cargo feature-resolver
panic; the complete application build was used instead. The development build
then succeeded and launched with:

```bash
CARGO_TARGET_DIR=/private/tmp/varyai-routing-save-target-20261112 \
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 pnpm dev --no-watch
```

The test-home override was removed for the actual development application.
No installed application was replaced, no credentials were copied, and normal
application storage was retained. Compilation finished in 2m 06s. Debug symbols
were disabled to limit cache size; optimization level remained unchanged.

The user performed another save in the rebuilt application. The live total was
646 ms, with 512 ms for all client synchronization. Each TRAE edition took 2 ms.
Other connected clients still ran their normal catalog updates, including
selection preservation; a pending/offline bridge is not claimed as delivered.
These measurements are a single post-fix live sample, not a latency guarantee.

## Final local acceptance

Final verification used the private target directory above. Toolchains were
Node 20.20.2, pnpm 10.12.3 and Rust 1.98.1 on macOS arm64.

| Command                                                            | Result                                          |
| ------------------------------------------------------------------ | ----------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                          |
| `pnpm build:renderer`                                              | Passed                                          |
| `pnpm test:unit`                                                   | 299 files, 3,906 tests passed                   |
| `pnpm format:check`                                                | Passed                                          |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                          |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                          |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                          |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`   | 6,525 passed, 21 ignored across 16 test targets |

The full Cargo run includes the library suite: 6,360 passed and 21 ignored.
The ignored checks include existing live/client/SDK checks and two manual timing
profiles exercised separately during this investigation. Concurrent security
source changes invalidated an earlier test snapshot; the updated security module
passed all 16 tests, followed by the final successful full run above. Earlier
interrupted or superseded runs are not treated as final acceptance.

The checked working tree was based on
`ab5cf647b304d080370af4838f96778f063ca5f0`. Its 68 changed or untracked source/test
files had manifest SHA-256
`adb8e6a360999bef0f4b3253dfbca4cef41d4a88f4e7bec29f71234db0136a2f`.
The manifest and command logs are under
`/private/tmp/vary-routing-save-20261112/`; source contents were checked again
after all checks completed. No commit or push was performed. Remote Linux,
Windows and macOS CI, installer behavior and release acceptance remain unverified.
