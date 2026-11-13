# Reopening onboarding without changing suppliers

Base revision: `9804cdb78f509a7b47ce2db90da1ce18adc496c8`, with the preceding local bug fixes and discovery-control retirement retained. No commit, push, installation or running-application replacement was performed.

## Withdrawn at the user's request

The user subsequently reported that one Antigravity account returned models while another failed both in VaryAI and in the official client. This did not establish a VaryAI-specific model-fetch defect; no production model-fetch change was made during that investigation. The user then requested removal of the temporary onboarding entry.

The settings entry, test-only guide behavior, its eight localization keys and feature-specific tests have been removed. Normal first-run onboarding remains. The complete source inventory again matches the previously verified [49-file discovery-retirement snapshot](2026-11-13-discovery-retirement-sources.json), including all unrelated local fixes. The sections below describe the historical feature checkpoint, not the current UI. Follow-up affected-check results are recorded below. This feedback does not establish sign-in acceptance for every provider or identify the exact account-side failure.

## Requested behavior

The user requested a reusable onboarding entry without clearing existing configuration, then explicitly chose to test login and model discovery without saving suppliers. Settings > General now includes **Connection guide > Reopen connection guide**. The guide opens in test mode regardless of first-run acknowledgement or existing suppliers.

Opening, dismissing and reopening use local component state. They do not save settings, reset acknowledgement, delete configuration or change Agent selection. Each opening starts with an empty guide form. Successful API tests clear the entered key and display model IDs and their count. Account tests fetch the selected account's model list without supplier creation, supplier editing, model-policy changes or saving provider defaults. Empty or failed discovery stays retryable. Returning to the first step, changing provider/connection fields or protocol clears prior results. Import is disabled in test mode so it cannot modify suppliers.

Account sign-in still uses real authorization and retains account login information in account management; the UI states this explicitly. This feature is not a credential rollback mechanism. Ordinary automatic first-run onboarding retains its existing saving and completion behavior. Four runtime languages are updated. No backend API, database schema or platform-specific implementation changed.

## Verification

Host: macOS 26.6.2 (25G83), Apple Silicon. Node 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1, unchanged from the preceding checkpoint.

Test-first evidence: the settings-entry regression failed before the entry existed. Nine test-mode regressions failed on the normal saving behavior before the test-only implementation. The final affected suite passed 85 tests in four files. It covers reopening with both acknowledgement values, repeated dismissal, clean local state, unchanged settings, API tests, all seven subscription identities, untouched existing supplier policies, import refusal, empty-model retry and unchanged normal first-run completion. Login/model APIs use synthetic mocks; these results do not establish real vendor authorization acceptance.

The final complete frontend run passed 4,067 tests in 304 files. All commands below passed. A follow-up formatting-only change to a test file was followed by another passing 85-test affected run, typecheck and explicit formatting check. Production implementation files were unchanged after the full suite. Earlier test-only typecheck errors were corrected before the successful final checks.

Backend file hashes still match the fully verified restoration checkpoint (7,478 library tests and 7,643 complete Cargo tests, 21 existing ignored tests). Rust commands were not repeated for this frontend-only change. This batch has no new cross-platform CI, packaged-installer or live-client acceptance.

The current [source manifest](2026-11-13-onboarding-reopen-sources.json) contains 54 changed source/test/guide files. Its sorted-mapping SHA-256 is `f52d7cdecfe1eb802bb1c092b011437ba4f0678f79d65a4171dbd5e1f8712b46`. QA reports are excluded. Only the nine files required for this feature changed from the preceding working tree; unrelated local work remains intact.

| Command                                                                                                                                                                                           | Result                         |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| `pnpm typecheck`                                                                                                                                                                                  | Passed                         |
| `pnpm build:renderer`                                                                                                                                                                             | Passed                         |
| `pnpm format:check`                                                                                                                                                                               | Passed                         |
| `pnpm audit --prod`                                                                                                                                                                               | Passed                         |
| `pnpm test:unit`                                                                                                                                                                                  | Passed: 304 files, 4,067 tests |
| `pnpm test:dev`                                                                                                                                                                                   | Passed                         |
| `pnpm test:macos-dmg`                                                                                                                                                                             | Passed                         |
| `pnpm test:trae-bridge`                                                                                                                                                                           | Passed                         |
| `pnpm test:model-default`                                                                                                                                                                         | Passed                         |
| `pnpm test:workbuddy-checkin-auth`                                                                                                                                                                | Passed                         |
| `pnpm test:doubao-checkin`                                                                                                                                                                        | Passed                         |
| `pnpm test:agent-rewards`                                                                                                                                                                         | Passed                         |
| `pnpm test:qoder-codec`                                                                                                                                                                           | Passed                         |
| `pnpm exec vitest run tests/components/OnboardingWizard.test.tsx tests/components/FirstRunOnboarding.test.tsx tests/components/SettingsDialog.test.tsx tests/integration/SettingsDialog.test.tsx` | Passed: 85 tests               |
| Explicit Prettier check for both components, both test files and the guide                                                                                                                        | Passed                         |

## Entry-removal verification

On the same host/toolchain, the four affected onboarding/settings test files passed all 74 tests after removal. `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` and `git diff --check` passed. The final source hashes match the earlier discovery-retirement snapshot exactly. The complete frontend and Rust suites were not rerun for this restoration to that identical verified source tree; its previously recorded 4,056 frontend tests and 7,643 Cargo tests remain checkpoint evidence, not newly executed results. No real application, user configuration or supplier record was modified. No commit or push was made.
