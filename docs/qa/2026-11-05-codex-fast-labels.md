# Codex Fast mode and personal-account picker labels

Date: 2026-11-05. Base revision: `a30e86d1d11641cf40dbbdaf02c11985d3e34df0` with an existing,
shared dirty working tree. This work has not been committed, pushed, installed,
or launched as a desktop application.

Host: macOS / Darwin arm64. Toolchain: rustc 1.98.1, Cargo 1.98.1,
Node.js 24.11.0, pnpm 10.12.3.

## Scope and evidence

Reviewed Magpie revision `465928bccea4fec3537edd6280f7cabb9a0c5bad`, including
`internal/codexcat/codexcat.go`, `internal/agent/codex_catalog_tiers_test.go`,
`internal/provider/modelprefs.go` (`Label`, `Labels`), and
`internal/provider/codex_models.go` (`codexApart`, `CodexTiers`).

Service tiers now survive model discovery and account-scoped caching, and are
published consistently to generated Codex catalogs and model-list entries.
The real upstream model ID controls the GPT/o-series Fast fallback; an explicit
empty tier list disables it. Group entries publish the common tiers of selectable
members because VaryAI does not yet route by service tier. Native forwarding
regression coverage includes an unchanged `service_tier: priority` request body.

Automatically named personal-account sources omit account identities from picker
labels. Explicit custom names are retained, colliding visible sources get stable
short numbers, and hidden sources do not affect that numbering. Request aliases,
account records and authentication bindings remain unchanged.

The relevant twelve Rust source files were fingerprinted before final checks.
Final verification completed with the source snapshot recorded in this report.
Their aggregate SHA-256 is `cae3ea803173ae76659406fae2469b4a170b94086e3f9c351b415c87d97a2c1c`. The paths are:

- `src-tauri/src/services/intent_routing/speed.rs`
- `src-tauri/src/services/intent_routing/menu_labels.rs`
- `src-tauri/src/services/intent_routing/mod.rs`
- `src-tauri/src/services/intent_routing/menu.rs`
- `src-tauri/src/services/intent_routing/effort.rs`
- `src-tauri/src/services/intent_routing/catalog.rs`
- `src-tauri/src/services/intent_routing/client/mod.rs`
- `src-tauri/src/services/intent_routing/client/tests.rs`
- `src-tauri/src/services/model_fetch.rs`
- `src-tauri/src/proxy/intent/mod.rs`
- `src-tauri/src/proxy/intent/native.rs`
- `src-tauri/src/codex_config.rs`

## Verification

- Initial Fast regressions were written before implementation. Two initial crate
  builds were blocked by concurrently edited, unrelated OAuth/subscription test
  code; those compile errors were subsequently corrected in the shared tree.
- Final focused runs against the newly compiled library test harness: 19 passed
  (10 speed-tier regressions, 5 source-label regressions, shared catalog/disk
  parity, personal account-pool menu publication, WorkBuddy route/name
  compatibility, and byte-preserving native service-tier forwarding).
- An initial full library run detected that custom relays with identical names
  were also being numbered. The implementation now numbers abbreviated personal
  sources only; the existing WorkBuddy test and a new regression both pass.
- Final `pnpm typecheck`: passed.
- Final `pnpm build:renderer`: passed.
- Final `pnpm test:unit`: 279 files, 3,470 tests passed.
- Final `pnpm format:check`: passed.
- `pnpm test:dev`: 8 passed.
- `pnpm test:trae-bridge`: 57 passed.
- `pnpm test:workbuddy-checkin-auth`: 5 passed.
- `pnpm test:doubao-checkin`: 13 passed.
- `pnpm test:agent-rewards`: 9 passed.
- `pnpm test:qoder-codec`: 2 passed, 1 skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: 5 passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed on the final
  tree after concurrently edited thread-migration tests were formatted.
- `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`: passed across
  16 test binaries: 6,188 passed, 18 ignored. The library portion passed 6,025 tests.
- A final `cargo check --manifest-path src-tauri/Cargo.toml` rerun passed.
- Final `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,025 passed,
  18 ignored, 0 failed. The final formatting rerun also passed.
- `git diff --check`: passed. Relevant source fingerprints remained unchanged
  throughout the final successful checks; no frontend source changed after the
  last successful renderer/typecheck/unit/format runs.

Full desktop interaction, account entitlements, actual acceleration, packaging,
signing and cross-platform CI have not been tested. Remote CI is unavailable for
these uncommitted working-tree changes. A linker warning about the macOS unwind
table size appeared in the initial focused test build; it did not fail the tests.
