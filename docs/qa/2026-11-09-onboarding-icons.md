# Onboarding source icons

Checked tree: `26a69df7056461c4f64a04c6739f29a76fed5042` plus this task's
onboarding, shared import icon, regression test and guide changes, together with
the existing uncommitted import-order and native select styling changes.
Host: macOS 15.3.1 arm64; Node 24.11.0, pnpm 10.12.3, Rust 1.98.1.

The first-step account and import text lists now use existing brand assets.
Account previews show each brand once, retaining ChatGPT / Codex for OpenAI.
Import previews follow Magpie, CC Switch, Claude Code, Codex and OpenCode, and
share icon metadata and rendering with the regular importer. All icons have
accessible names and hover titles. Rows wrap as needed and remain visible on
small layouts. Monochrome logos remain legible in dark mode.

## Verification

The icon accessibility regression first failed against the text lists. The
duplicate-brand regression then failed against the repeated OpenAI icon. Both
pass, and a separate assertion verifies that login methods remain available in
the account chooser.

- Focused onboarding, first-run and importer tests: 26 passed in three files.
- `pnpm test:unit`: 3,535 passed in 280 files after the deduplication change.
- Final icon color and spacing received another focused test run and repeat `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, plus Prettier checks for the test and guide; all passed.
- `pnpm test:dev`, `pnpm test:trae-bridge`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`: passed.
- `pnpm test:qoder-codec`: two passed; optional installed-client codec test skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`: 6,165 passed, 18 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`: 6,165 library and 163 integration tests passed; 18 library tests ignored.
- `git diff --check`: passed.

Chrome previewed the actual onboarding React component with an isolated query
client and no native backend. Chinese and English layouts were inspected; final
English light/dark screenshots show six account icons and five import icons,
with exactly one ChatGPT / Codex icon. The GitHub mark uses the foreground theme
color. The preview neither loaded nor saved provider credentials.

The macOS test linker emitted its `__eh_frame` size warning; linking and tests
completed successfully. No commit, push, remote CI, packaged-app installation or
native Windows/macOS visual acceptance was performed. These shared renderer
changes apply to both platforms; local results do not establish cross-platform
CI acceptance.

## Final SHA-256 fingerprints

| File                                                    | SHA-256                                                            |
| ------------------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/onboarding/OnboardingWizard.tsx`        | `9e74119331fe58d1a217ca5678ca852468e212ef423352956193a6a8141080a5` |
| `src/components/onboarding/onboarding.css`              | `2f9ee47d071ac9734767694bd3f271cc3d30c92de4c04266dfe0921aaa6569ad` |
| `src/components/providers/ProviderImportDialog.tsx`     | `f916e853692245bf7f0e5f56345bc19fe3f816c1c667bb64cc7d2dcf5557de45` |
| `src/components/providers/ProviderImportSourceIcon.tsx` | `1c679702d8bfc0f4241d695b21deb730ce2ebd67e674b40533d2efdb93304615` |
| `src/config/providerImportSources.ts`                   | `98ac8eab2effc44b0fd13632941b81a2b3f81125638ae4f7a06a2d08afad003b` |
| `tests/components/OnboardingWizard.test.tsx`            | `716d2a24ea4adf8446fcb7c38ded983986d215fcbf97a730d8512965f821205f` |
| `docs/guides/onboarding.md`                             | `e76d1a78c1404f954b88143b3162ccc32add8944a66996e9c367d05606b3b6ad` |
