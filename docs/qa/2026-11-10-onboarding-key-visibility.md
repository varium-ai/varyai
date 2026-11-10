# Onboarding API key visibility

Date: 2026-11-10 (Asia/Shanghai).
Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing unrelated working-tree changes preserved.
Host: macOS, Darwin arm64; Node v24.11.0, pnpm 10.12.3, Rust 1.98.1 and Cargo 1.98.1.

## Behavior

The API key field in onboarding now has a small eye button inside its right edge.
Keys remain hidden by default. Clicking toggles between password and text input
without changing the value, fetching models or submitting the form. Returning to
the previous step resets visibility, so entering the key step again hides the
retained value. The button is disabled with the rest of the form while connecting.

The button uses a native button element with `type="button"`, a localized title
and accessible name, an `aria-controls` reference, and its current pressed state.
The existing `apiKeyInput.show` and `apiKeyInput.hide` translations cover English,
Simplified Chinese, Traditional Chinese and Japanese. Icons are decorative.
The field reserves space for the control so the key text does not overlap it.

The implementation is shared by macOS and Windows. Existing credential handling,
validation and saving behavior are unchanged; revealing the field adds no storage
or network operation. Tests use synthetic credentials only.

## Regression coverage

Before implementation, the new visibility and return-navigation tests and the
extended connection-lock test failed because the control was absent. After the
change, both onboarding suites pass (47 tests), including the earlier copy-feedback
regressions.

```bash
pnpm exec vitest run tests/components/OnboardingWizard.test.tsx tests/components/FirstRunOnboarding.test.tsx
```

## Verification

| Check                    | Result                            |
| ------------------------ | --------------------------------- |
| Focused onboarding tests | Passed: 47 tests across two files |
| `pnpm typecheck`         | Passed                            |
| `pnpm format:check`      | Passed                            |
| `pnpm build:renderer`    | Passed                            |

`pnpm test:unit`: passed, 3,702 tests across 287 files.
`git diff --check` and Prettier checks of the changed files also passed.

Logs: `/tmp/vary-onboarding-key-visibility-checks/`,
`/tmp/vary-onboarding-key-visibility-red.log`, and
`/tmp/vary-onboarding-key-visibility-focused.log`.

The earlier pre-push checks in the same checkout completed successfully:
`cargo check`, `cargo test --lib` (6,281 passed, 19 ignored), `cargo fmt --check`,
`cargo clippy -- -D warnings`, and full `cargo test` (6,444 passed, 19 ignored),
all with `--manifest-path src-tauri/Cargo.toml`; `pnpm audit --prod` found no known
vulnerabilities. The documented helper checks also passed, with one installed-Qoder
round-trip test skipped. These results are under `/tmp/vary-prepush-20261110/`.
No backend, helper or dependency files changed for this UI-only follow-up. The
renderer checks are rerun against the updated onboarding component and CSS.

SHA-256 of the checked files:

- `src/components/onboarding/OnboardingWizard.tsx`:
  `675816f09314da2bb9506c0afd82d3bcd7f6897d1d50002eed53ed4ecb6e923b`.
- `src/components/onboarding/onboarding.css`:
  `115b8277cff3cffbff477193ce05bbbebe7e7581e2139dd216277bc4d4aaef5d`.
- `tests/components/OnboardingWizard.test.tsx`:
  `aa7e884ced5a20b84f11f2c5c5a88f7f7b77e5c5a6ab6a2b73050f60c8173a12`.

The user deferred committing and pushing. No commit, push, application launch,
installation or remote CI run was performed. Windows/Linux runtime behavior and
release acceptance were not verified locally.
