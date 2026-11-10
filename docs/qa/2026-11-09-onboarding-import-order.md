# Onboarding import source order

Checked working tree: `00162023a28cdcff1a950a32593d7e8ee36890b2` plus
the importer, regression test and onboarding guide changes in this task.
Host: macOS 15.3.1 arm64; Node 24.11.0, pnpm 10.12.3, Rust 1.98.1.

The embedded onboarding importer skipped the sorting used by the Models
importer. Both now use the same ordering: Magpie, CC Switch, Claude Code,
Codex and OpenCode. Onboarding still initially selects the first source with
importable connections. This shared renderer change applies to macOS and Windows.

The new regression failed before the implementation change because Magpie
appeared last. It now passes and also verifies that onboarding selects an
importable source when Magpie is empty.

## Verification

- `pnpm exec vitest run tests/components/ProviderImportDialog.test.tsx tests/components/OnboardingWizard.test.tsx tests/components/FirstRunOnboarding.test.tsx`: 24 tests passed.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm exec prettier --check tests/components/ProviderImportDialog.test.tsx docs/guides/onboarding.md`: passed.
- `pnpm test:unit`: 3,517 tests passed in 280 files.
- `pnpm test:dev`, `pnpm test:trae-bridge`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`: passed.
- `pnpm test:qoder-codec`: two passed; the optional installed-client codec test was skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,151 passed, 18 ignored.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 6,151 library and 163 integration tests passed; 18 library tests ignored.
- `git diff --check`: passed.

The macOS test linker emitted an `__eh_frame` size warning; linking and tests
completed successfully. The final documentation update was formatted and checked
after these runs; the source, test and guide fingerprints below stayed unchanged.

## Checked fingerprints

SHA-256 values for the source, regression and guide verified above:

| File                                                | SHA-256                                                            |
| --------------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/providers/ProviderImportDialog.tsx` | `74118caec2af1ed81ded3b4c7034fa2af6e1452e77f8693277059f6bf223cd2f` |
| `tests/components/ProviderImportDialog.test.tsx`    | `f95dcd07956e1c391d20115b325948a112b096f8363e79d468bb579194298a2e` |
| `docs/guides/onboarding.md`                         | `269b776a5e8c98cc15c8fccd0bf1f05a9175f7d00b259514906f1310542eba0d` |

No commit, push, remote CI, packaged-app installation or live desktop acceptance
was performed. Local checks do not establish cross-platform CI acceptance.
