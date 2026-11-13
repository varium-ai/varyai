# Codex model cache after gateway takeover

## Diagnosis and scope

The reported screenshot came from another computer. Its configuration and
running Codex were not available for inspection, so this change does not claim
to establish that computer's exact state or live acceptance.

The reproducible gap is that VaryAI replaced the provider, endpoint and generated
catalog without expiring Codex's `models_cache.json`. A still-fresh list from a
previous gateway can therefore survive the configuration change. The Magpie
reference checkout at `c5b9e448a8bc9bc2a1bf6704e5d8bb3384d084a6` has an explicit
`codexStaleCache` step in `internal/agent/codex.go`; VaryAI had no equivalent.
The displayed QA group names do not appear as defaults in this repository.

Successful connection, reapplication and catalog changes now expire only the
cache timestamp. An unchanged background refresh preserves a newly fetched
cache. Owned disconnect and receiptless recognized disconnect expire it too;
recovery after foreign takeover leaves the foreign cache alone. Selected profile
overrides prevent expiring a cache for a route VaryAI does not control.

Cache values retain their raw JSON representation, including unknown fields,
model metadata, ETags and client versions. Writes use the existing guarded
replacement helper. Missing or malformed caches are preserved; inaccessible
caches produce only a fixed diagnostic and never block a connection change.
The implementation is shared by macOS and Windows and performs no filesystem
work in request forwarding. It does not delete another manager's routing groups.

## Reproduction and verification

Checked revision: `eaf9b62d57f5e6f7fdb16a6e359292bd98ce2366`, plus the changes
to the client manager, new model-cache module and regression tests, and the
Codex model-catalog guide. The concurrent model-picker branding work was
preserved and committed separately while these checks ran.

Host: Darwin 24.3.0 arm64; rustc 1.98.1, Node 24.11.0, pnpm 10.12.3.
Node differs from the repository's exact `22.12.0` pin.
Logs, command results and source fingerprints are under
`/private/tmp/vary-model-cache-checks/`; the initial failing run is
`/private/tmp/vary-model-cache-red.log`.

Before implementation, three of five new regressions failed because the old
cache remained fresh. After implementation all five pass. They cover standalone
and native takeover, preservation of cache metadata, unchanged refresh,
catalog changes, explicit reapplication, owned and foreign disconnects,
malformed/inaccessible caches and rejected takeover. Fixtures use temporary
directories and synthetic configurations without launching installed clients.

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm test:unit`: passed, 306 files and 4,065 tests.
- All development helper suites and the storage recovery Python tests: passed.
- `pnpm audit --prod`: passed, no known vulnerabilities.
- Rust formatting, compilation and Clippy with warnings denied: passed.
- `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml`: passed,
  7,480 tests passed and 21 ignored.
- `cargo test --offline --manifest-path src-tauri/Cargo.toml`: passed on the final
  retry, 16 test targets, 7,645 tests passed and 21 ignored. Checked source
  fingerprints were unchanged throughout final backend verification.

The development-helper test initially could not bind dynamic loopback ports;
its retry passed with the required access. The audit initially could not reach
the registry and passed with network access. Cargo waited for another build in
the same checkout. Two full-suite compilation attempts exhausted disk while
creating crate archives, before tests could run. Completed incremental caches
were removed under generation/build locks, and obsolete archives, object files
and metadata were removed from an idle November 10 temporary target under its
build lock. Source files and cached test executables were retained. The test
linker emitted a compact-unwind size warning.

No application was launched, installed, replaced or signalled. No live client
configuration was edited. At local verification completion, remote CI and
Linux/Windows verification were pending; installer, signing and release
acceptance were not evaluated. The colleague must verify
the updated version using **Apply again** and a Codex reload; a running client
can still retain its in-memory menu until it reloads.
