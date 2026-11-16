# Provider and routing CRUD latency

## Findings

Provider import, create, duplicate, edit, rename, enable/disable and delete commands,
along with routing-group save, ordering, delete and policy changes, awaited the
same post-write publication chain. It rebuilt the global catalog, waited for all
Agent configuration writes, and finally rebuilt and published Codex's menu. Bulk
deletion repeated that chain for every selected row. The frontend held operation
state until multiple invalidated queries had finished as well.

Read-only inspection of existing completed-operation timing records found these
maxima: 17 ms for group persistence, 281 ms for global publication, 2,000 ms for
shared client synchronization, 3,799 ms for Codex publication, and 5,931 ms for a
complete refresh. The stage maxima come from different operations and must not be
added. Samples included 29 complete refreshes and three saves. These are historical
observations, not a benchmark of every client or unfinished waits. Only fixed timing
fields were retained; no private configuration, credentials or model IDs are reported.

Provider list queries also reread the native subscription store for each provider
row. Claude's account list additionally reads account credential files for local
plan metadata. Multiple rows referring to the same subscription kind repeated those
reads. Group lists use cached provider discovery; ordinary CRUD does not request an
upstream model list. Explicit discovery and connectivity tests intentionally perform
network work. The public model-context cache is about 403 KiB on this host; no new
long-lived credential or model cache was introduced.

The comparison used Magpie revision
[`015860f2da6c78e9f9d2cf1fde9a240681776486`](https://github.com/yetone/magpie/tree/015860f2da6c78e9f9d2cf1fde9a240681776486).
Its `internal/provider/provider.go::store` writes the catalog and marks it touched;
`internal/agent/sync.go::SyncCatalogSoon` starts client synchronization in the
background, and `SyncCatalog` merges changes arriving during a running round.
The comments identify busy Windows files and WSL file operations as causes of slow
saves. `sharedAll` and `Hold` avoid rebuilding account discovery for repeated reads
in one request. VaryAI keeps its own SQLite persistence and configuration ownership
rules, while applying the same separation between resource persistence and client
publication, and sharing account observations within a list query.

## Changes and safety

- CRUD completion acknowledges persistence and schedules background work. A fixed
  25 ms batching window merges bursts; each worker has at most one pending rerun.
  An edit arriving during synchronization is not lost. Global preparation and each
  client have separate workers, so an occupied Codex or TRAE lock does not queue
  later changes behind it for other clients. Explicit bulk synchronization and
  restoration retain their existing completion barriers.
- Shutdown pauses client publication before restoring configurations. Each writer
  checks this state under its client lock, preventing a late refresh from resuming
  an already suspended connection while another client's restoration is pending.
  Explicit startup enables publication again; TRAE can still retry pending cleanup.
- Local Codex menus and native projections carry the generation of the resources
  used to build them. Both ingress paths reject a stale managed catalog before
  forwarding, and execution rechecks the generation before dispatch/retry. Menu
  rollback retains its generation. Shared profile activation copies the menu and
  generation atomically. Native passthrough is unchanged.
- The UI updates its query cache with the acknowledged revision, cancels older
  reads that could overwrite it, and releases operation state before background
  invalidation completes. Membership/order writes retain their serialization and
  each routing row retains its own mutation guard. Renames handle a concurrent
  list read that already contains the new ID. Bulk deletion still reports partial
  failures and preserves remaining selections for retry.
- Native subscription summaries are read once per kind per list query, including
  failed reads. A read failure remains unknown instead of declaring sign-out. The
  next query reads current account state again.

The scheduling and generation checks use shared Rust code on macOS and Windows.
Client writes still use the existing platform adapters, locks, ownership checks,
authentication and recovery journals. No timeout was shortened and no installed
client was launched, restarted, disconnected or replaced for verification.

## Verification

The manual `routing_save_timing_profile` uses 12 synthetic API providers, 16
catalog models per provider plus configured defaults, one group and an active
Codex projection. It runs three saves through the blocking publication barrier,
then three through the queued path, draining publication after each measurement.
Save acknowledgment includes persistence and excludes the later background drain.

| Path | Three save acknowledgments (ms) | Median (ms) |
| --- | --- | --- |
| Blocking publication | 309.471, 302.997, 304.355 | 304.355 |
| Queued publication | 6.616, 6.409, 6.375 | 6.409 |

This measures completion of the save command, not faster filesystem writes or
upstream inference. Background synchronization still takes time. Busy-client
regressions separately verify that an occupied lock does not hold acknowledgment
open. The timing test is ignored by default and was run explicitly with
`cargo test --lib --manifest-path src-tauri/Cargo.toml routing_save_timing_profile -- --ignored --nocapture`.

The original busy-Codex regression failed because a resource edit waited on its
switch lock. The two initial UI regressions failed while query invalidations were
pending. The repeated-account-read regression failed with four reads instead of
the required two kinds. These failures were recorded before their respective fixes.
An additional regression reproduced a late Gemini catalog write undoing shutdown
restoration while Claude's lock was occupied, and verifies explicit restart afterward.

Tests use synthetic resources, temporary homes, in-memory databases and dynamic
ports. Coverage includes busy global/client locks, coalescing a 100-edit burst,
delivering later changes to Gemini while Codex remains busy, preserving connection
tokens, rejecting stale requests on both Codex ingress paths without upstream calls,
authentication precedence, rollback generation, and successful forwarding after
current publication. Frontend tests cover immediate acknowledgment, latest revisions,
renames, partial deletions, and independent routing-row operations.

Host: macOS 15.3.1 arm64; Rust/Cargo 1.98.1; Node 24.11.0; pnpm 10.12.3.
Base revision: `015521e022f3b850fdb56a2177bbdb419c68575f` plus the cumulative working-tree
changes from the earlier fixes and this task. Validation logs, before/after source
hashes and timing results are retained under `/tmp/varyai-crud-latency-20261114/`.
The final backend run and repeated timing profile are under `accepted/`; earlier
development runs remain available, including the deliberately failing regressions.
The initial full frontend run exposed an obsolete test stub returning only an ID
and revision for a newly saved provider. It now returns the complete acknowledged
record required by the API, and the full frontend suite was rerun successfully.

Final local checks passed:

- `pnpm typecheck`, `pnpm format:check`, and `pnpm build:renderer`.
- `pnpm test:unit`: 312 files and 4,185 tests passed after the fixture correction.
- `pnpm audit --prod`: no known vulnerabilities found.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`,
  `pnpm test:doubao-checkin`, `pnpm test:qoder-codec`, and `pnpm test:agent-rewards`.
- `cargo check --manifest-path src-tauri/Cargo.toml`.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7,592 passed,
  21 existing tests ignored by default.
- `cargo test --manifest-path src-tauri/Cargo.toml`: the same 7,592 library tests
  plus 165 integration tests passed; the same 21 default ignores remain.
- The explicit timing profile, affected test-file formatting and `git diff --check`.

The SHA-256 of the final manifest of 1,760 code and check-input files is
`505ff5ee024c43d4889e5701cb8e23ed857065952b9a31ce5d4c62f6273b28f3`.
The manifests in `accepted/backend-source-before.json` and
`accepted/backend-source-after.json` match exactly. Documentation is excluded from
that code manifest so final validation results can be recorded afterward. The
renderer source, dependency and helper inputs did not change after their successful
checks; the corrected provider fixture was covered by the complete frontend rerun.
Test builds retain the existing test-only `quota_panel::expiring` and macOS linker
unwind-table warnings.

Live client timings, remote CI, Linux, Windows and Windows-to-WSL2 acceptance remain
unverified. No commit or push is part of this task.
