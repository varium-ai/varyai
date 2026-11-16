# Codex source labels and stable request identities

## Scope and behavior

The Codex publication path previously appended a provider identity only when
individual model labels collided. A subscription source could therefore expose
its internal UUID, and two models from the same API source could have different
suffix treatment. The fix qualifies direct models with readable source names
before checking label collisions. Source numbering is allocated once per visible
source in stable identity order, not independently for each model.

Automatically generated Codex account labels publish only the service name.
Custom source/model names remain readable, hidden sources do not affect visible
numbering, and literal names are reserved before generating numeric hints.
Routing groups do not inherit their first member's source label. Display hints
never fall back to internal source IDs or complete request aliases.

The request slug construction, canonical alias map, retained hidden aliases,
model capabilities and account bindings are unchanged. Native account catalog
merging is unchanged: official models already listed by the backend are not
appended again and the backend retains its own names. The implementation is
shared by macOS and Windows, with no platform-specific behavior added.

## Verification context

- Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b`.
- Checked changes: `src-tauri/src/services/intent_routing/client/aliases.rs`
  the two associated Rust publication test assertions and the catalog guide;
  this QA record contains results only.
- Host: macOS 15.3.1, Apple Silicon.
- Toolchain: Node 24.11.0, pnpm 10.12.3, rustc 1.98.1, cargo 1.98.1.
- Source SHA-256 and host metadata: `/tmp/vary-codex-label-checks/source.json`.
- Logs: `/tmp/vary-codex-label-checks/`.
- Test-first run: three new source-label regressions failed against the original
  implementation before the behavior change. Final tests additionally cover
  consistent suffixes for noncolliding models, hidden sources, ordering,
  literal-name collisions, routing groups and synthetic account-name privacy.
- Fixtures use synthetic identities and credentials, with no running application
  launch, installation, restart or production configuration changes.

## Results

| Check | Result |
| --- | --- |
| Compiled alias regressions | 13 passed |
| Existing native-account catalog deduplication regression | 1 passed |
| `pnpm typecheck` | Passed initially and on the latest rerun |
| `pnpm build:renderer` | Passed, including the later rerun |
| `pnpm test:unit` | Initial run: 306 files / 4,065 tests passed; later run: 306 files / 4,086 tests passed |
| `pnpm format:check` | Passed, including the later rerun |
| Development helper tests | 8 passed on the loopback-enabled rerun |
| DMG, Trae bridge, model-default, WorkBuddy, Doubao and rewards helper tests | Passed |
| Qoder codec helper tests | 2 passed; installed-codec test skipped by its existing availability gate |
| Shared-v23 recovery helper tests | 5 passed |
| `pnpm audit --prod` | Passed; no known vulnerabilities |
| `CARGO_INCREMENTAL=0 cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `CARGO_INCREMENTAL=0 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `CARGO_INCREMENTAL=0 cargo test --lib --manifest-path src-tauri/Cargo.toml` | 7,486 passed, 2 failed, 21 ignored; see final-tree gap below |
| `CARGO_INCREMENTAL=0 cargo test --manifest-path src-tauri/Cargo.toml` | Stopped while waiting for the build lock because disk space was low; incomplete |
| `git diff --check` | Passed |

The two full-library failures were publication tests that still asserted bare
model names: `shared_menu_catalog_matches_disk_projection_rich_metadata_and_capabilities`
and `published_catalog_distinguishes_direct_models_from_single_member_groups`.
Their assertions were updated to require the readable source suffix, without
removing any account, routing or capability assertions. Those final test-only
edits have **not been recompiled or rerun**. Disk availability fell below 1 GiB
while other builds were active, so this task stopped its own remaining full
build rather than risk filling the system volume. The passing focused tests,
check and Clippy results cover the unchanged production implementation; they do
not establish a passing final full Rust suite. The test linker also emitted an
existing-size warning about the large DWARF unwind table.

Independent renderer, localization and routing changes appeared in the shared
checkout during verification and were preserved. A transient typecheck failure
in an independently edited test (`TS6133`, unused `notify`) was absent on the
latest successful rerun. Frontend results are timed snapshots, not acceptance
for subsequent independent edits.

The first development-helper test run could not bind its synthetic loopback
listeners under the sandbox; its unsandboxed rerun passed all eight tests.
The initial production audit could not resolve the registry in the sandbox;
the network-enabled rerun reported no known vulnerabilities. The first full
Rust sandbox run was stopped after loopback-dependent tests failed, and must
not be treated as a passing verification run.

No commit, push or remote CI run was requested. Linux, Windows, Windows-to-WSL2,
installer, signing, notarization and live installed-app acceptance remain
unverified. A build does not update the running app or its cached Codex menu.
