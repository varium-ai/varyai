# Routing save performance and experimental-policy withdrawal

This record covers the first implementation and its exact manifest below.
Subsequent live saves exposed a separate TRAE bridge bootstrap delay; see
[the live follow-up](2026-11-12-routing-save-live-followup.md). The synthetic
metadata timings here were not live end-to-end acceptance of the user's save.

Date: 2026-11-12. Base revision: `ab5cf647b304d080370af4838f96778f063ca5f0`
plus the current working-tree changes. Host: macOS arm64, Node 20.20.2,
pnpm 10.12.3, Rust 1.98.1. No commit or push was requested.

Final changed source/test manifest SHA-256:
`44874ad29e221da833b3da21ab9230758861e678ea9c7e37987009bd8497460d`.
The manifest hashes the sorted changed/untracked paths under `src/`, `src-tauri/`
and `tests/`, each followed by its content hash. The 32-file manifest remained
unchanged through final verification and is recorded in
`/private/tmp/vary-routing-save-20261112/final-sources.json`.

## Diagnosis

The save command persists a group, publishes global routes, synchronizes connected
clients and refreshes an active Codex menu before returning. The renderer then
waits for its status queries. This work is deliberately still awaited.

Two isolated unoptimized Rust timing fixtures identify separate costs:

- `services::proxy::tests::routing_save_timing_profile`: 12 synthetic upstreams,
  temporary client files, an in-memory database, a dynamic proxy port and an
  activated Codex menu. Three original saves took 295.6, 296.9 and 297.3 ms.
  Persistence took 6.5–6.7 ms, global refresh 117–118 ms, client synchronization
  4 ms and the Codex menu rebuild 166–168 ms. Unit-test provider preparation
  deliberately omits the device's public-facts cache, so these totals do not
  include the second fixture's metadata cost.
- `services::intent_routing::model_context::tests::routing_model_enrichment_timing_profile`:
  12 synthetic providers with 32 unknown models each, 2,614 capacity records and
  189 fact records. These counts match the inspected public-cache dimensions;
  no account or private configuration was copied. Original enrichment took
  2,417.6, 2,387.7 and 2,325.7 ms. Missing or differently spelled model IDs caused
  every lookup to scan and normalize the complete public capacity directory.
  Provider preparation is repeated during publication, Codex refresh and UI
  queries, multiplying this cost.

These are controlled reproductions, not timings of the user's live save. Runtime
phase logs make the corresponding live stages observable without recording
credentials, group names, model IDs or configuration payloads.

The final built library test binary repeats both fixtures successfully:

| Fixture                             | Original samples               | Final samples            |
| ----------------------------------- | ------------------------------ | ------------------------ |
| Public metadata enrichment          | 2,417.6 / 2,387.7 / 2,325.7 ms | 14.9 / 6.9 / 7.0 ms      |
| Save without the public-facts cache | 295.6 / 296.9 / 297.3 ms       | 287.1 / 281.2 / 287.4 ms |

The first indexed lookup constructs the indexes; later samples reuse them.
The basic-save timing difference is small and does not establish a separate
performance improvement. The material improvement is the removed metadata scan
that this basic fixture deliberately excludes. Real latency depends on catalog
contents, connected clients, disk and lock contention.

## Change

Public facts and context-window catalogs now construct immutable normalized
indexes on their first fallback lookup. Exact IDs keep precedence. Ambiguous
normalized context capacities still choose the smallest value; price/fact
collisions still use the first sorted spelling. Indexes are omitted from the
serialized cache, and replacing a catalog creates fresh indexes.

Publication order, generation checks, stale-target revocation, client switch locks,
connected-client synchronization and the active Codex menu update remain in place.
The global gateway and Codex catalogs retain their different publication rules;
they are not combined into a single snapshot. No detached save worker was added.

## Magpie reference

Inspected public revision `8a705a5e5f68febde70c93950220a277e03e0c39`:

- [Group save handler](https://github.com/yetone/magpie/blob/8a705a5e5f68febde70c93950220a277e03e0c39/internal/gui/groups.go):
  ordinary save calls `provider.SaveGroup` and returns the group state. Renames
  separately reseat affected agents.
- [Provider persistence](https://github.com/yetone/magpie/blob/8a705a5e5f68febde70c93950220a277e03e0c39/internal/provider/provider.go):
  writes the private provider file and marks the catalog touched. It does not
  synchronously rebuild each connected client's configuration in this handler.

VaryAI retains its explicit synchronous publication contract while reducing the
preparation cost; Magpie's complete save latency was not benchmarked locally.

## Experimental-policy withdrawal

The requested subagent removed the four-policy UI, comparison checkbox, policy
execution and policy-specific import/export behavior. The original three
templates, nine strategies and advanced settings remain. Concurrent export
feedback, discovery layout/busy states, unavailable-model handling and hidden-group
recovery changes were preserved.

Minimal read compatibility accepts retired `policy` JSON without executing or
serializing it, and reads retired `expiring` as `ordered`. A regression test covers
this case. The user's database was not modified for cleanup. The Chinese design
document remains as a withdrawn implementation proposal.

## Verification

All final local checks passed:

| Command                                                                                | Result                                      |
| -------------------------------------------------------------------------------------- | ------------------------------------------- |
| `pnpm typecheck`                                                                       | Passed                                      |
| `pnpm build:renderer`                                                                  | Passed                                      |
| `pnpm format:check`                                                                    | Passed                                      |
| `pnpm audit --prod`                                                                    | Passed; no known vulnerabilities            |
| `pnpm test:unit`                                                                       | 299 files; 3,897 tests passed               |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                               | Passed                                      |
| `CARGO_INCREMENTAL=0 cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                      |
| `CARGO_INCREMENTAL=0 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                      |
| `CARGO_INCREMENTAL=0 cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,351 passed; 21 ignored                    |
| `CARGO_INCREMENTAL=0 cargo test --manifest-path src-tauri/Cargo.toml`                  | 6,516 passed; 21 ignored across 16 binaries |
| Final built library binary: `timing_profile --ignored --nocapture`                     | Both manual timing fixtures passed          |
| CI helper tests                                                                        | All seven passed                            |
| `git diff --check` and documentation Prettier checks                                   | Passed                                      |

The helper commands were `pnpm test:dev`, `pnpm test:macos-dmg`,
`pnpm test:trae-bridge`, `pnpm test:model-default`,
`pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin` and
`pnpm test:qoder-codec`. Library tests include normalized-lookup collision and
serialization compatibility, retired experimental metadata, global publication,
resource revocation and coexisting Codex menu refresh coverage.

Logs are under `/private/tmp/vary-routing-save-20261112/`. An initial verification
attempt exhausted disk space during Rust compilation, causing unrelated frontend
fixture creation and dependency transforms to fail. Those interrupted checks were
rerun successfully after space was available. Incremental Rust compilation was
disabled to limit subsequent build-cache growth; optimization level was unchanged.
The existing macOS debug linker compact-unwind warning remains nonfatal.

The 21 ignored library tests comprise 19 existing live-client/service,
installed-SDK and performance/corpus checks plus the two new manual timing
fixtures, which were explicitly executed separately. No failing test was skipped
or weakened.

No installed application was launched, installed or replaced as a test. Remote
Linux, Windows, macOS CI and the conditional WSL2 contract are not claimed.
