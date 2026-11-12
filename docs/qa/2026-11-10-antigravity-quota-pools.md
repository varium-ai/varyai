# Antigravity quota pool repair

## Scope and reference

The November 10, 2026 report compared Magpie's Antigravity allowance panel with
Vary AI's provider list. The list contained shared Gemini and Claude/GPT windows
but omitted independent Claude, Gemini 4 Argon and GPT-OSS readings.

The quota reader returned immediately when `retrieveUserQuotaSummary` contained
any windows, discarding `fetchAvailableModels` model readings already fetched.
The generic model parser also omitted explicit quota objects whose remaining
fraction was absent, which Antigravity uses for exhausted allowances. Upstream
bucket display names produced unnecessarily long labels, and integer rounding
changed a reported 96.8% remainder to 97%.

Reference: Magpie revision `a45dd9c6573ab19cf19afd621501c9aef2e3f462`,
`internal/provider/google.go` (`quota`, `pools`, `sameReading`, `PooledWindows`)
and `internal/provider/google_test.go`.

## Final behavior

A pure parser merges model observations and shared windows. Model-specific groups
are considered before broad family groups. Only a shared window for the same
period and an equivalent reading can replace the corresponding model reading;
weekly-only groups retain separate model short windows. A model reading that
differs from the family stays separate, including its longer reset window.
Equivalent readings use Magpie's tolerance of half a percentage point and less
than a minute between known resets. The parser never changes its input.

Explicit Antigravity quota objects without a remaining fraction mean exhausted.
Missing quota objects and malformed numeric values remain unknown; disabled models,
internal completion models and disabled windows are excluded. Gemini CLI keeps
its separate quota parser and account semantics.

The provider list and account editor share short pool/window labels and retain
one decimal place in remaining percentages. The shared fixture reproduces all
six observations from the report: Gemini short/weekly, Claude, Claude/GPT weekly,
Gemini 4 Argon weekly and GPT-OSS. This is dynamic pool reconciliation, rather than
a fixed list of six models.

## Checked tree and environment

The working tree is based on `23a4b2be4b582f892691f5c7feb5fe88e79e6b42` and
includes the preceding provider-list allowance work plus unrelated pre-existing
catalog/import/provider-form edits. Those edits were preserved. Source hashes for
the final checks are in `/tmp/vary-antigravity-final-sources.json`.

Host: macOS arm64. Node 24.11.0, pnpm 10.12.3, rustc 1.98.1, Cargo 1.98.1.
CI specifies Node 22.12.0; the local frontend checks use the installed version.
The implementation uses platform-neutral Rust and React for macOS and Windows.

## Test-first and visual evidence

The new frontend tests failed before correcting labels and decimal rounding. The
new Rust tests failed before adding the merge parser. Five focused Rust tests
then passed, covering missing/exhausted observations, the full six-pool fixture,
model-specific group priority, differing model readings and malformed/disabled
summary windows. Three new UI cases verify full and compact displays and shared
cache consistency; the focused frontend run passed 21 tests.

The fixture is `tests/fixtures/antigravity-quota-pools.json`, with a fixed clock
and synthetic model/summary observations. No real account credentials or provider
requests were used. An English renderer preview at 1280 by 720 verifies that both
the list and actual account editor display all six readings and 96.8% accurately:

- `/tmp/vary-antigravity-preview/list.png`
- `/tmp/vary-antigravity-preview/form.png`

The renderer preview server was stopped after inspection. This does not establish
live-account availability, installation, signing or release acceptance.

## Required local checks

Final commands and results are recorded in
`/tmp/vary-antigravity-final-front-results.json` and
`/tmp/vary-antigravity-final-back-results.json`.

The final source fingerprints match all 59 changed source/test files in the
checked working tree. The focused request changes are the new
`native_subscription/antigravity_quota.rs` parser, its integration in
`native_subscription.rs`, quota labels/decimal formatting in
`SubscriptionAccountQuota.tsx` and `SubscriptionQuotaSummary.tsx`, the shared
fixture/UI tests, and the applicable documentation.

| Command                                                            | Result |
| ------------------------------------------------------------------ | ------ |
| `pnpm typecheck`                                                   | Passed |
| `pnpm build:renderer`                                              | Passed |
| `pnpm test:unit`                                                   | Passed |
| `pnpm format:check`                                                | Passed |
| `pnpm audit --prod`                                                | Passed |
| `pnpm test:dev`                                                    | Passed |
| `pnpm test:macos-dmg`                                              | Passed |
| `pnpm test:trae-bridge`                                            | Passed |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed |
| `pnpm test:doubao-checkin`                                         | Passed |
| `pnpm test:agent-rewards`                                          | Passed |
| `pnpm test:qoder-codec`                                            | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed |

The frontend suite passed **3,672 tests in 286 files**. The explicit Rust library
run passed **6,268 tests**, with **19 ignored**. The complete Rust command passed
**6431 tests across 16 suites**, with **19 ignored**. macOS test linking
emitted the existing compact-unwind size warning without failing the command.
Obsolete generated library caches were cleared between build stages to maintain
local disk space; no source change or weakened assertion was needed.

Windows, Linux and Windows-to-WSL2 CI were not run. No commit, push, production
credential request, installed-app launch, installer/signing or live-account
acceptance is claimed. These local checks do not establish cross-platform CI
acceptance.
