# Codex connection latency and background disconnect

## Findings

Both takeover and disconnect call the Codex thread reconciliation code while
holding the Codex configuration-switch lock. Before this change, each paginated
thread opened SQLite connections, committed its provider change individually,
and durably rewrote the growing recovery journal before and after migration.
Restoration also committed and persisted its journal per row. This makes a large
history expensive even though paginated history only needs SQLite metadata
changes. A background reconciliation holding the same lock can also delay a
subsequent connection operation.

The comparison uses public Magpie commit
[`015860f2da6c78e9f9d2cf1fde9a240681776486`](https://github.com/yetone/magpie/blob/015860f2da6c78e9f9d2cf1fde9a240681776486/internal/sessions/codex_provider.go).
Its `retagPaged` reuses a database connection and commits groups of up to 200
threads. Both implementations skip active writers and recent rollout writes.
The 65-second retry interval in VaryAI applies to deferred history recovery; it
is not a mandatory delay before closing a disconnect dialog.

## Changes

- Disconnect confirmation closes immediately. A mutation-cache entry keeps the
  affected Agent row in its disconnecting state across navigation. Other Agents
  remain available. Completion refreshes native state; failures release the
  controls and report a sanitized Agent-specific error without automatic retry.
- Paginated migrations and restorations reuse each database connection and use
  transactions of at most 64 threads, bounding simultaneously held lock handles.
  A durable pending journal precedes migration, and completion is persisted
  before releasing writer locks. A failed batch rolls back all its row changes.
- Conditional updates and transactional recovery checks preserve external
  provider, rollout-path and history-mode changes. Unowned target rows never
  enter the recovery journal. Existing receipts, busy-thread retries, legacy
  file handling and paginated rollout bytes remain compatible.

The implementation is shared by macOS and Windows and retains each platform's
existing writer-lock implementation.

## Synthetic timing

An isolated unoptimized Rust harness used the original thread modules from base
revision `015521e022f3b850fdb56a2177bbdb419c68575f` and the final working-tree
modules, with identical dependencies and filesystem operations. It retained the
production `atomic_write_private` implementation. Only the unrelated home/path
resolvers and error wrapper were replaced with temporary-fixture equivalents.
No native Codex data was modified. The harness and logs were retained locally
under `/tmp/varyai-magpie-diagnosis-20261114/benchmark/`.

The fixture contains 1,000 paginated source threads, holds one thread busy, and
adds an unrelated target thread. It migrates 999 rows, changes one row externally,
then restores 998 rows while preserving both the external and unowned rows.

| Implementation | Transition | Restoration |
| --- | ---: | ---: |
| Original per-row implementation | 20,025 ms | 11,670 ms |
| Bounded batches | 459 ms | 320 ms |

These are single-run local measurements of thread processing, not end-to-end
desktop or Magpie timings. The final comparison ran sequentially after clearing
obsolete compiler intermediates, before starting backend verification. Filesystem
and system load affect the numbers. An earlier complete application test of the
batched workload measured 1,191 ms and 339 ms. Old-code attempts that failed with
a sanitized client error while disk space was constrained are excluded from this
comparison; their underlying errors were not captured.

## Verification

Host: macOS 15.3.1 arm64; Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3. Checked
base: `015521e022f3b850fdb56a2177bbdb419c68575f` plus the current working-tree
changes, including the separate Chat-role compatibility fix and preserved
pre-existing protocol changes.

The initial transaction-rollback regression failed against the old code. The
updated suite covers migration and restoration rollback, interrupted journals,
external changes during recovery, completion after an already-restored row is
written again, busy writers, unowned rows, rollout-byte preservation and the
1,000-thread round trip. Renderer coverage includes compact
and full layouts, immediate dismissal, independent Agent controls, navigation,
completion, sanitized failure and retry.

Frontend verification passed: `pnpm typecheck`, `pnpm build:renderer`,
`pnpm format:check`, and `pnpm test:unit` (312 files, 4,182 tests).

Backend checks passed: `cargo check`, `cargo fmt --check`, `cargo clippy -- -D
warnings`, `cargo test --lib`, and `cargo test`, all with
`--manifest-path src-tauri/Cargo.toml`. Both test commands passed all 7,578 library
tests with 21 existing ignored tests; the full command additionally passed 165
integration tests. Test builds retain the existing test-only
`quota_panel::expiring` dead-code warning and macOS linker unwind-table warning.
Earlier build attempts exhausted disk space. Obsolete incremental snapshots and
leftover compiler object intermediates were removed before the successful final
checks; source files, native application data and installed binaries were kept.

Final source SHA-256:

- `src-tauri/src/services/intent_routing/client/threads.rs`:
  `bec0ef055844934f3e0ea313f8f593c04121443605c14e09deb4dbdafc4d3c27`
- `src-tauri/src/services/intent_routing/client/threads/paginated.rs`:
  `79f0a157042b7bb774fb4867dc3235e8a2a58dcc8f201882d1155c979ac23758`
- `src/components/agents/AgentConnectionsPage.tsx`:
  `fe55bc634be796c74091afb641337831a8b7858fb6a5b1da72900e3fb84e093b`
- `tests/components/AgentConnectionsPage.test.tsx`:
  `9b9f551b1cbc9444d23e1c54ffebde66858a0065c84b3149e831c4e216d3f118`

The source manifest and check logs are retained locally under
`/tmp/varyai-magpie-diagnosis-20261114/checks/`; the final backend logs use the
`-latest.log` suffix. The source manifest was rechecked after all tests and
matches the final working tree. The helper checks and production
dependency audit are recorded in the [role-compatibility report](2026-11-14-codex-chat-instruction-roles.md);
their inputs were unchanged by the UI and batching work.

No application was manually launched or installed for verification. No commit or
push is included. Linux, Windows, Windows-to-WSL2 and remote CI acceptance remain
unverified, as does live Codex desktop interaction.
