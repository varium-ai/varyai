# Magpie basic API-provider import

Scope: import usable API connections even when Magpie has advanced settings.
Endpoints, credentials, exact model IDs, supported headers and backup keys retain
their existing mapping. Spending, routing, proxy, TLS overrides and other advanced
source settings are omitted. The import dialog explains this in all four runtime
languages. Existing transport validation and local-gateway exclusions remain.

The backend regression uses 25 synthetic advanced-setting fixtures and checks
preview eligibility, transactional import, executable provider resolution,
model/header preservation, secret-free previews and unchanged source files.
Six invalid transport/credential fixtures remain ineligible. The dialog regression
checks the scope notice and selectable imports. No live provider request, installed
application replacement or release acceptance is claimed.

Verification host: macOS 15.3.1 arm64, Node 24.11.0, pnpm 10.12.3,
Rust 1.98.1 (stable).
Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with working-tree
changes in the importer, its dialog, four locale files and associated tests/docs.
Concurrent work edited provider-key and agent-connection UI files in the same
checkout, then added image-model handling and tests; those changes were preserved.
The passing rows below describe the snapshots observed by each command. Continued
concurrent edits mean they do not establish final whole-tree acceptance.

The task-owned files, in digest order, are
`src-tauri/src/services/provider_import/readers.rs`,
`src-tauri/src/services/provider_import/tests.rs`,
`src/components/providers/ProviderImportDialog.tsx` and
`tests/components/ProviderImportDialog.test.tsx`.
SHA-256 of their concatenated `path + NUL + contents + NUL` is
`b1b46cd06c69108ecf0d19ad31658f8922148e3e8e5f27747b6f5d7b4a9ef9fb`.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm exec vitest run tests/components/ProviderImportDialog.test.tsx` | 9 passed |
| `pnpm test:unit` | Latest full run: 3,590 passed, 1 failed in the concurrently edited AgentConnectionsPage recovery-status assertion. The importer tests passed. An earlier run failed 2 provider-key tests; both passed on rerun. |
| `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | 5 passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| Built lib-test executable, filter `services::provider_import::tests` | 34 passed, including both new backend regressions |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed after disk cleanup |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,241 passed, 19 ignored after disk cleanup |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,241 passed, 1 failed, 19 ignored. The parallel image-model change added `services::routing_upstreams::tests::image_models_stay_in_the_editor_catalog_but_leave_agent_chat_lists`, which returned `routingUpstreams.invalid`. All 34 provider-import tests passed in this run. The library failure stopped subsequent integration execution. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test '*'` | All 14 integration test binaries passed: 163 tests, 0 failed, 0 ignored |

The backend and dialog regressions were written before the implementation change.
The new dialog test failed on the original UI, then passed with the scope notice.
The initial backend regression run could not finish compilation because the disk
filled; subsequent standard Rust test and Clippy attempts also encountered disk
exhaustion. Abandoned compiler sessions and obsolete single-link compiler objects
were removed while holding the build lock; source files, installed applications
and valid caches were retained. Rust verification was retried after restoring
approximately 12 GiB of free space.

Integration targets passed separately after the full-command failure. The
task-owned implementation/test digest remained unchanged after verification.
Linux/Windows CI, live credentials and installed-application acceptance have not
been run. Nothing was committed or pushed, and full-tree and cross-platform
verification remain incomplete.
