# Update release notes verification

Date: 2026-11-12

## Checked source and environment

Working-tree changes on `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`,
including pre-existing provider and routing edits. This change touches only
release-note selection, the update dialog, its stylesheet, focused tests, and
release documentation. Existing unrelated changes were preserved.

macOS 15.3.1 (Darwin 24.3.0, arm64), Node.js 24.11.0, pnpm 10.12.3,
Rust 1.98.1, Cargo 1.98.1.

## Behavior

- Select the interface language from release descriptions separated by language
  comments. Use exact locale, base language, then English as fallbacks.
- Apply selection to manifest notes and exact-version GitHub fallback notes.
- Update displayed text when the interface language changes.
- Append the target release version to each item and reduce list and section
  spacing. This remains a target-release view, not cumulative update history.
- Preserve Markdown formatting, safe external links, HTML/image suppression,
  portable download behavior, and installation and dismissal actions.

Focused tests were written first and failed for missing language selection and
version labels. After implementation, both test files passed (22 tests).
An isolated Vite fixture rendered the actual dialog in the in-app browser at
1280 by 720. Chinese and English each showed only the selected language, with
compact spacing and inline version labels. No installed application was started
or replaced; the preview mocked update installation and external navigation.

## Release attribution audit

The public `v0.3.3` tag resolves to
`e6c7da67e1f75f6836b2e39a38a831dc96ea582c`. Its description contains the ten
items shown in the reported screenshot. Git ancestry confirms all six source
commits below are absent from `v0.3.1` and `v0.3.2` and included in `v0.3.3`:

| Source commit | Changes |
| --- | --- |
| `345dd331` | Independent Agent updates and local check-in dates |
| `0526276e` | Cursor and T3 Code agents |
| `2b748a9c` | TRAE bridge recovery and native model synchronization |
| `55a07d45` | Agent recovery, Codex conversion, and check-in rewards |
| `7a00b186` | Native black-and-white TRAE International icon |
| `a17b92de` | Model selection separated from desktop restart confirmation |

The `v0.3.2` description separately lists update button placement and installation
diagnostics (`63ee9c14`). It is not included in the target-only `v0.3.3` view.
Release descriptions and tags were read for verification; no remote release
metadata was edited.

## Local checks

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed: 299 files, 3,948 tests |
| `pnpm format:check` | Passed |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Failed: 6,337 passed, 2 failed, 19 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Failed on the same two library tests; stopped before integration tests |
| `cargo test --manifest-path src-tauri/Cargo.toml --test '*' --no-fail-fast` | Passed: 164 integration tests |

The backend failures concern model catalog projection and provider import,
outside the release-note files changed here:

- `commands::agent_connections::tests::reasoning_catalog_projects_into_native_agent_files_and_restores_them`: model display label assertion mismatch.
- `services::provider_import::tests::provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs`: missing imported model.

The current tree includes pre-existing routing and model-catalog edits. These
failures were recorded without modifying that separate work. Full verification
remains incomplete; no claim is made that this working tree passes all checks.

## Limits

No commit or push was requested. Remote CI, Linux, Windows, and Windows-to-WSL2
checks were not run. Installer behavior, signing, and notarization were not
validated. Local backend failures do not establish cross-platform acceptance.

All file hashes below were rechecked after the checks completed.

## Checked file hashes (SHA-256)

```text
b4913cde9dc599f938842ba26ef9e6818fb082b00a64284e570eab5a8eb6657e  src/lib/releaseNotes.ts
f003a903819ec9ea5a6b82fbfe19c69efbcef980699ac7c34d0c0bafd4f9f594  src/components/AppUpdateDialog.tsx
eb13a990de2b24aa2f4240bcd91aacb363c96e266159d6cf3d5d2be4635629d0  src/components/app-update-dialog.css
c3c2d6fa8f38c5bed7cfa1b4a9d653e6b0148c4e647ab46e882bf21ce2927581  tests/lib/releaseNotes.test.ts
721aac14401ece66e76d2c0abb5dc40c6b6400afe4f7a943cdc923d05708248f  tests/components/AppUpdateDialog.test.tsx
a5ccf46e5a38faa32000577b4ee02137f6f37fbd890522ac20ab4832f9c0e595  docs/guides/release-distribution.md
```
