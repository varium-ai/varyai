# Automatic entry after onboarding

Checked tree: `26a69df7056461c4f64a04c6739f29a76fed5042` plus this task's
onboarding, locale, test and guide changes and the existing uncommitted UI work.
Host: macOS 15.3.1 arm64; Node 24.11.0, pnpm 10.12.3, Rust 1.98.1.

API-key connections, subscription/Codex account connections and imports now
automatically persist onboarding completion and enter the application's Tools
page after their connection save succeeds. The success card and Open Tools
button were removed, including their unused styles and five strings in each
runtime language. The retry action has translations in all four languages.

Completion failures retain the saved connection and retry only the acknowledgement
step. Busy guards remain active while that step runs, including after the
embedded importer unmounts. Failed connection saves keep the guide open. Cache
refresh failures do not block completion.

## Verification

The automatic-entry regressions failed against the previous success screen
before implementation. The final focused run passed 80 tests in four files,
including locale coverage. Tests cover all three paths, Codex account saves,
waiting for provider persistence, duplicate import submission, failed connection
saves, completion retry without repeated provider writes, and navigation locks
while completion is pending. Three tests use the actual first-run wrapper and
wizard to verify that acknowledgement persists, the dialog closes and the app
navigation callback runs exactly once.

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed on the final source and locale changes.
- Prettier checks for the test and onboarding guide, plus `git diff --check`: passed.
- `pnpm test:dev`, `pnpm test:trae-bridge`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`: passed.
- `pnpm test:qoder-codec`: two passed; optional installed-client test skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`: 6,165 passed, 18 ignored.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `pnpm test:unit`: 3,546 passed in 280 files on the final source and locale changes.
- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`: 6,165 library and 163 integration tests passed; 18 library tests ignored.

The final focused command was `pnpm exec vitest run tests/components/OnboardingWizard.test.tsx tests/components/FirstRunOnboarding.test.tsx tests/components/ProviderImportDialog.test.tsx tests/config/localeCoverage.test.ts`.
Only this report changed after the completed checks; the eight fingerprints
below were verified again before delivery.

Tests use mocked Tauri operations and fixture accounts; no real provider data,
login session or installed application was changed. The macOS test linker emitted
an `__eh_frame` size warning and completed successfully. These shared renderer
changes apply to macOS and Windows. No commit, push, remote CI, native visual
acceptance or installer acceptance was performed.

## Final SHA-256 fingerprints

| File                                             | SHA-256                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------ |
| `src/components/onboarding/OnboardingWizard.tsx` | `56a39eca2009af41768276ae1df564b77566e6931dd159d2413ce9ea9aae9443` |
| `src/components/onboarding/onboarding.css`       | `2655f1ddc2638722bc7dceba8d5b134158e226acfd0923a9280a5bba88846099` |
| `tests/components/OnboardingWizard.test.tsx`     | `39f5bd6a73d9686262a29195d6b5b21e5559ee5b005aa0ca7d2b84b86753a371` |
| `docs/guides/onboarding.md`                      | `79b4396fb87b4115f86699c76462e625bbbe2da5bf1bedd68182be23bc0a692c` |
| `src/i18n/locales/en.json`                       | `596ddc5f07b9edc267a6fa72173811db4d282d9438cc0191052aa0ff1aceb914` |
| `src/i18n/locales/zh.json`                       | `5bbb633ecc50221a4d2d842bb37af25c1735e9a643c013a34c1782e917eeb383` |
| `src/i18n/locales/zh-TW.json`                    | `1313e58ae08fbf10a9501955d2ff1c48dd057ea3582287a62e44b69a1727c8fe` |
| `src/i18n/locales/ja.json`                       | `b49634938fd2536d8ab23bbb8d16cf5e724b4aaffaf9171c5e3338951be08158` |
