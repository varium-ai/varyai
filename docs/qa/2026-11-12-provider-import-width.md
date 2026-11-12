# Provider import dialog width

Date: 2026-11-12

## Change

The importer previously set a 660 px width but removed its maximum width. The
shared dialog selector had equal specificity and could override that width when
loaded later, leaving only the shared 16 px margins. The import-specific selector
now includes the shared surface class so its sizing wins in either load order.

The importer is capped at 620 px. Above the 480 px breakpoint it leaves at least
32 px per side; at or below the breakpoint it leaves 16 px per side. Source tab
horizontal padding is reduced slightly so all five sources fit at the maximum
width. Narrow windows retain horizontal tab scrolling. The shared renderer CSS
applies equally on macOS and Windows. Import parsing, selection and persistence
logic are unchanged.

## Test-first and visual verification

The new `ProviderImportDialogSizing.test.tsx` regression test failed in both CSS
load orders before the fix and passed afterward. The existing import interaction
and provider-dialog sizing tests also passed: 3 files, 14 tests total.

An isolated Vite preview rendered the actual importer with synthetic sources and
mocked preview/import APIs. No private configuration was read and the installed
application was not launched or replaced.

- At 1280 by 720, the centered dialog measured 620 px with 330 px margins. All
  five tabs fit without horizontal scrolling after the padding adjustment.
- At 420 by 760, the dialog measured 388 px with 16 px margins. The dialog had
  no horizontal content overflow; the source tabs scrolled independently.

## Checked source and environment

Working-tree changes on `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`.
The working tree already contained unrelated provider, routing and update-dialog
changes, which were preserved. This task modifies only the import-specific rules
in `providers.css`, adds the sizing regression test, and adds this record.

macOS 15.3.1 (Darwin 24.3.0, arm64), Node.js 24.11.0, pnpm 10.12.3,
Rust and Cargo 1.98.1. Logs: `/tmp/vary-import-width-checks/`.

## Checks

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed; rerun after the final tab-padding adjustment |
| `pnpm format:check` | Passed; rerun after the final stylesheet adjustment |
| `pnpm test:unit` | Passed: 300 files, 3,961 tests |
| Focused import and dialog sizing tests | Passed: 3 files, 14 tests; rerun against final stylesheet |
| Prettier check for the new test and stylesheet | Passed |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Failed: 6,337 passed, 2 failed, 19 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` | Same 2 library failures; all 164 integration tests passed |
| `git diff --check` | Passed |

The two library failures are the previously recorded model-catalog failures,
outside the CSS and test files changed by this task:

- `commands::agent_connections::tests::reasoning_catalog_projects_into_native_agent_files_and_restores_them`: model display label assertion mismatch.
- `services::provider_import::tests::provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs`: missing imported model.

Those unrelated changes were preserved. Full verification remains incomplete
while these tests fail.

## Limits

No commit or push was requested. Linux, Windows, Windows-to-WSL2 and remote CI
were not run. Installer, signing and notarization acceptance was not exercised.

File hashes were verified again after all checks completed.

## Checked file hashes (SHA-256)

```text
a2b86dea2d438c020f36f0a4ef7e6021b3959f0e402d5fe4aee71f7e58e72c76  src/components/providers/providers.css
f4115acd39495c331127542a1d8ce678bb3f9f5378c02e02e9141b98879a4495  tests/components/ProviderImportDialogSizing.test.tsx
```
