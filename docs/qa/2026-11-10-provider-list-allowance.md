# Provider list allowance verification

## Scope and checked tree

Subsequent form-alignment request: the separate API allowance row has been removed
from the provider editor to match Magpie's upper form. Provider-list allowance
and subscription-account meters remain available. The editor screenshots below
record the earlier implementation, before that follow-up.

The November 10, 2026 change displays subscription allowance beneath provider
names while retaining the account editor's meters, reset times and refresh
controls. Custom API providers and relay presets can also discover sub2api and
new-api key allowances; their form retains the same observation and a manual
refresh control. No application was installed, launched or replaced for testing.

The checked working tree is based on `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`.
It includes unrelated pre-existing catalog, import and provider-form changes;
these were preserved. Source fingerprints for the final checks are recorded in
`/tmp/vary-list-quota-final-source-manifest.json`. The source changes for this
request comprise the new provider quota service and command, API binding,
compact allowance components, list and editor integration, three aligned labels
in each runtime locale, focused tests and subscription-provider documentation.
No dependency or persistent storage migration is required.

Host: macOS arm64. Toolchain: Node 24.11.0, pnpm 10.12.3, rustc 1.98.1
(`48a229cea`, September 1, 2026), Cargo 1.98.1. CI specifies Node 22.12.0;
local frontend results use the installed Node version.

## Behavior and test evidence

Test-first runs failed before the new UI components and relay parser existed.
Focused frontend checks cover selected subscription accounts, window percentages,
cache sharing with account identity retained, safe error states, Codex credits and
restrictions, exact relay identity/revision, manual refresh, stale readings,
explicit key rejection, unsupported queries and unsaved/missing-key providers.
The provider-list integration test verifies that allowance is visible without
opening the editor and that the subscription plan label remains visible.

The five new Rust tests cover sub2api wallet versus window-only responses,
daily/weekly ordering and resets, unknown or rejected observations, new-api quota
unit conversion, safe URL origins, and an authenticated dynamic-port server that
verifies fallback from sub2api to new-api. Tests use synthetic credentials and no
real provider account. The HTTP reader reuses the existing no-redirect client,
bounds each response to one MiB and limits each endpoint request to eight seconds.

Magpie reference: `a79d16c6843051ee75088949d6f8f611ea868dd8`,
`internal/provider/balance_probe.go`, `balance.go` and `sub2api_usage.go`.
The implementation follows the key-query contracts, without importing Magpie's
arbitrary balance expressions or panel-token configuration.

## Visual inspection

A renderer-only preview uses synthetic providers and query responses. The English
1280 by 720 previews show ChatGPT windows directly below its name, sub2api's daily
remaining percentage and wallet balance, and an unsupported relay retaining its
endpoint/model subtitle. The editor preview retains the balance, remaining
percentage, meter, reset date and manual refresh control.

Local screenshots:

- `/tmp/vary-list-quota-preview/list.png`
- `/tmp/vary-list-quota-preview/form.png`

These are fixture previews, not real-account or packaged-application acceptance.

## Final check results

Final command results and logs are recorded under
`/tmp/vary-list-quota-final-front-results.json` and
`/tmp/vary-list-quota-final-back-results.json`.

All final local checks passed against the unchanged source fingerprints above.

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
| `pnpm test:qoder-codec`                                            | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed |
| `pnpm test:agent-rewards`                                          | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed |

The frontend suite passed **3,669 tests in 285 files**. The explicit Rust library
run passed **6,263 tests**, with **19 ignored**. The complete Rust command passed
**6426 tests across 16 test suites**, with **19 ignored**.

The first final complete Rust run failed while writing its static archive because
of insufficient disk space. After removing this session's generated library
incremental caches (about eight GiB), the identical command passed; the final log
is `/tmp/vary-list-quota-final-back-4-retry.log`. No source changes were made for
the retry. macOS test linking emitted a compact-unwind size warning; it did not
fail the checks. Final source fingerprints still match the checked tree.

Windows, Linux and Windows-to-WSL2 CI were not run from this local task. No commit,
push, packaged-app replacement, installer/signing test or live-account quota
acceptance is claimed. Local macOS checks do not establish cross-platform CI
acceptance. The preview server was stopped after visual inspection.
