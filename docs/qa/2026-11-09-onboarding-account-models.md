# Account model selection during onboarding

Checked base: `0cb8e387fd972ac3f04c0956f8c4b6df5fd64af9` plus the working-tree
onboarding, locale, test and guide changes. Existing UI and subscription work
was present in this checkout. Host: macOS 15.3.1 arm64; Node 24.11.0,
pnpm 10.12.3, Rust 1.98.1.

The previous account path saved empty `models` and `catalog` arrays with the
vendor policy, then completed onboarding without discovering models. It did not
populate the selected models shown by the editor.

Onboarding now discovers models for the exact account that completed login
before saving its provider. ChatGPT / Codex uses the account-specific Codex
catalog operation; GitHub Copilot, Grok and the four native subscription types
use the same model operations as their normal editors. New account bindings
save the discovered models in picked mode, with group-only visibility disabled.
The existing editor limit of 256 selected models still applies. Subscription
catalogs retain the full response, and model capabilities and display metadata
are preserved.

An existing account's explicit policy is preserved when reconnecting that same
account. Empty vendor defaults from the previous onboarding path are initialized
when that account is reconnected. Discovery failures and empty responses keep the
guide open with a translated, sanitized error and allow retrying without a new
login. No provider write or completion callback occurs until discovery succeeds.
Successful saves still enter the app automatically.

## Verification

The new assertions were run before implementation: 11 failed and 21 passed,
reproducing the missing selection, discovery and retry behavior.

- Focused regression run: 93 passed in `OnboardingWizard`, `FirstRunOnboarding`,
  `ProviderImportDialog` and locale coverage tests. Coverage includes every login
  provider, full model selection and metadata, empty/failed discovery and retry,
  waiting for discovery before persistence, old empty defaults, existing explicit
  policies, automatic entry and completion retries without duplicate writes.
- `pnpm typecheck` and `pnpm build:renderer`: passed.
- `pnpm test:unit`: 3,560 passed in 280 files.
- `pnpm format:check`: passed after a formatting-only correction to the existing
  `SubscriptionAccountQuota.tsx` work. Its initial format check reported that file;
  no quota logic was changed by this task.
- `pnpm test:dev`, `pnpm test:trae-bridge`, `pnpm test:workbuddy-checkin-auth`,
  `pnpm test:doubao-checkin` and `pnpm test:agent-rewards`: passed.
- `pnpm test:qoder-codec`: two passed; optional installed-client test skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`,
  `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`: 6,167 passed,
  18 ignored.
- The initial `cargo test --manifest-path src-tauri/Cargo.toml --quiet` could
  not build its archive because the host ran out of disk space. Regenerable
  incremental cache and stale test codegen objects were cleared. The retry
  passed: 6,168 library and 163 integration tests, with 18 library tests ignored.
  Existing subscription work added one library test between the library-only
  run and this full run. The macOS linker emitted an `__eh_frame` size warning
  and completed successfully.
- Prettier checks for the final test and documentation files and
  `git diff --check`: passed. The seven onboarding-specific file fingerprints
  below were verified after the complete checks.

Tests use synthetic accounts and mocked model catalogs. Real login, installed
application and live provider data were not changed. The shared React flow
applies to macOS and Windows; no platform-specific implementation was added.
Remote CI, native visual acceptance and packaged application acceptance were
not performed. No commit or push was made.

## Checked file fingerprints

SHA-256 values for this task's final source, tests, locales and guide:

| File                                             | SHA-256                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------ |
| `src/components/onboarding/OnboardingWizard.tsx` | `f3ee0c4a6338d9929f1688a7ee2c5b2bf3b9cb5f987d437354eae40c27da417d` |
| `tests/components/OnboardingWizard.test.tsx`     | `5bb4e6b1517ed212e8d037e4cae3dd19f6f6bb05f85e64bc832a65bb84819274` |
| `docs/guides/onboarding.md`                      | `49fc331ce3476074743b22509995fd2b4d2524f5fe81ec7c91e997c08c8ac37c` |
| `src/i18n/locales/en.json`                       | `74ea38ce432b3432ff365e1138d63520816e15e25af09172dc4cf92b7f48d8f1` |
| `src/i18n/locales/zh.json`                       | `6f92bbaeed1c9aadbe05416a73d903c458f357fcb36982ba7f02ac81b7617d67` |
| `src/i18n/locales/zh-TW.json`                    | `88450a4d73708e9a4e53c8f12ae69ac47c34d00e213e3c0517c2365380df6736` |
| `src/i18n/locales/ja.json`                       | `083306cabcaacabd7a2def40bf41f6a9695406652c6fda57e470d2b796a608b5` |
