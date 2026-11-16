# Provider name validation and Agent model labels

The historical-duplicate editor rule below was subsequently corrected in
[the unchanged-name validation follow-up](2026-11-16-provider-name-edit.md).
This report retains the verification evidence for the original implementation.

## Behavior

Provider creation, copying and renaming require a unique name after trimming
outer whitespace and ignoring case. The editor shows a localized field error,
disables Save and blocks submission with Enter. A backend collision preserves
the draft, including when the conflicting provider was absent from the loaded
list. Saved subscription providers can also edit their names.

The extension database checks names in the same transaction that saves the
provider. Existing duplicate records remain usable for toggles and account
refreshes, but the editor requires a distinct name before saving them. Ordinary
imports allocate distinct names; conflicting overwrite imports roll back their
entire batch. No stored providers are automatically renamed or reassigned.

The existing shared picker and Agent projection already use saved supplier
names. Regression coverage now verifies `exact-model · Relay Alpha` and
`exact-model · Relay Beta`, then renames Alpha to Gamma and checks both Agent
models and the Codex catalog. Display labels update while request aliases remain
unchanged. Legacy duplicate names retain their readable fallback until renamed.
A client with a cached catalog needs a model-list refresh to show the new label.

## Regression evidence

The frontend and database regressions failed before implementation. Final
coverage includes creation, copying, renaming, normalization, editing a
provider's own name, stale frontend lists, legacy updates and atomic import
rollback. Older catalog and credential-copy fixtures now give unrelated
providers distinct synthetic names without weakening their original assertions.

An isolated browser fixture exercises the real provider form with synthetic
providers. All 16 cases passed across Chromium and WebKit, light and dark themes,
and English, Simplified Chinese, Traditional Chinese and Japanese. Checks cover
the field error, disabled Save, Enter submission, the hidden internal ID, a
successful corrected name with the same ID, horizontal overflow and page errors.
English light and dark screenshots were visually reviewed.

## Checked tree and environment

- Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`, plus the shared
  working-tree changes. Existing quota, provider-picker, hidden-ID and usage-source
  changes were preserved.
- Host: macOS 15.3.1 (24D70), arm64; Node 22.23.5, pnpm 10.12.3,
  Rust/Cargo 1.98.1. The local Node version differs from the 22.12.0 CI pin.
- Logs and command results: `/tmp/varyai-provider-name-verification/`.
  The final frontend logs use the `-final` suffix; backend logs use `cargo-`.
  `test-lib.log` records the successful standalone library run.
- `provider-name-scoped-hashes.json` records the 18 implementation, test, locale
  and documentation files in this change. SHA-256 of its sorted compact JSON
  mapping: `a421feaac38d7eb5e945af93d2e1d17f5cedb6431c3246839d69bc95301630af`.
- `frontend-final-hashes.json` and `backend-final-hashes.json` record the source
  snapshots used by the final checks. No recorded source changed during those
  checks. Helpers and the production dependency audit passed earlier in the
  same session; their scripts, manifests and lockfile did not change afterward.

## Verification results

| Check                                                                                                              | Result                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------- |
| `pnpm typecheck`                                                                                                   | Passed                                                            |
| `pnpm format:check`                                                                                                | Passed                                                            |
| `pnpm build:renderer`                                                                                              | Passed                                                            |
| `pnpm test:unit`                                                                                                   | Passed: 316 files, 4,218 tests                                    |
| `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`, `pnpm test:model-default`                         | Passed                                                            |
| `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec` | Passed                                                            |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                                              | Passed                                                            |
| `pnpm audit --prod`                                                                                                | Passed: no known vulnerabilities                                  |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                                 | Passed                                                            |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                                           | Passed                                                            |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                                 | Passed                                                            |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                                                            | Passed: 7,602 tests; 21 ignored                                   |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                                                  | Passed: 7,602 library tests and 165 integration tests; 21 ignored |
| Isolated Chromium/WebKit form checks                                                                               | Passed: 16 cases                                                  |
| `git diff --check`                                                                                                 | Passed                                                            |

The successful backend runs supersede the provider-name fixture failures
recorded in the earlier copied-provider allowance report. Linux, Windows, WSL2
and remote CI were not run, so cross-platform acceptance remains unverified.
The implementation uses shared React/Rust code on macOS and Windows. No commit
or push was made, and no installed application or AI client was launched,
installed, replaced or restarted.
