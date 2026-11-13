# Provider loading states

## Scope

Based on revision `98a894d84ed2774c769db22757075777f4d5ef7a`, this working-tree
change adds initial catalog row skeletons and initial API, Codex and subscription
allowance skeletons. Cached rows and readings remain visible during background
refresh. Opening an editor keeps allowance components mounted behind the dialog
instead of collapsing those rows. The implementation is shared by macOS and
Windows; no backend, storage or dependency changes are required.

Pre-existing routing UI, routing documentation and locale changes were preserved.
Other work continued in those files during this task. Validation applies to this
checkout and the provider files recorded in
`/tmp/vary-provider-loading-source-manifest.json`.

## Behavior verification

The initial test-first run failed four new assertions for missing skeletons.
Focused regression coverage checks initial loading through both Codex account
discovery and allowance fetches, API allowance resolution, subscription loading,
empty-list resolution, cached rows and allowance refreshes, and allowance
retention while an editor is open. Existing failure, unsupported-provider,
signed-out-account and credential rejection coverage remains in place.

Keeping the list mounted exposed its unknown allowance status to an existing
editor test that searched the entire page. That assertion now checks the dialog,
where the safe failure must replace the unknown status. The final focused run
passes 155 tests in four files.

A renderer-only browser preview uses synthetic providers and pending promises.
Visual inspection covers 1280 by 720 light and dark themes, a 420 by 720 narrow
window, and separate initial allowance loading. No horizontal overflow or browser
runtime errors were observed. Reduced-motion checks confirm animations are
disabled. Decorative placeholders are hidden from accessibility APIs; loading
regions retain translated status text and busy state.

Preview captures are local, temporary artifacts:

- `/tmp/vary-provider-loading-light.png`
- `/tmp/vary-provider-loading-dark.png`
- `/tmp/vary-provider-loading-narrow.png`
- `/tmp/vary-provider-loading-quota.png`

No installed application was launched, installed or replaced.

## Verification environment

Host: macOS arm64. Node 24.11.0, pnpm 10.12.3, rustc 1.98.1
(`48a229cea`, September 1, 2026), Cargo 1.98.1. The CI Node version differs from
this local environment.

Command results and individual logs are recorded under
`/tmp/vary-provider-loading-frontend-results.json`,
`/tmp/vary-provider-loading-backend-results.json` and
`/tmp/vary-provider-loading-backend-network-results.json`.

The sandbox backend runs could not bind synthetic loopback HTTP servers
(`Operation not permitted`). The backend checks were rerun with local socket
access. The development helper tests also passed after allowing loopback access;
the dependency audit passed after allowing access to the npm registry.

The initial complete frontend run passed 4,051 tests and failed the editor
assertion described above. The corrected editor file passed all 52 tests on its
own. The final complete frontend run passed 4,053 tests in 304 files.

| Check                                                              | Result                                    |
| ------------------------------------------------------------------ | ----------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                    |
| `pnpm build:renderer`                                              | Passed                                    |
| `pnpm format:check`                                                | Passed                                    |
| Focused provider tests                                             | 155 passed in four files                  |
| `pnpm test:unit`                                                   | 4,053 passed in 304 files                 |
| `pnpm test:dev`                                                    | 8 passed with loopback access             |
| `pnpm test:macos-dmg`                                              | Passed                                    |
| `pnpm test:trae-bridge`                                            | Passed                                    |
| `pnpm test:model-default`                                          | Passed                                    |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                    |
| `pnpm test:doubao-checkin`                                         | Passed                                    |
| `pnpm test:agent-rewards`                                          | Passed                                    |
| `pnpm test:qoder-codec`                                            | Passed                                    |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                    |
| `pnpm audit --prod`                                                | No known vulnerabilities                  |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                    |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 7,471 passed; 21 ignored                  |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                    |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                    |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 7,636 passed across 16 suites; 21 ignored |

Final frontend logs are `/tmp/vary-provider-loading-unit-final.log`,
`/tmp/vary-provider-loading-typecheck-final.log`,
`/tmp/vary-provider-loading-build-final.log` and
`/tmp/vary-provider-loading-format-final.log`. Loopback and network rerun logs
are `/tmp/vary-provider-loading-dev-network.log` and
`/tmp/vary-provider-loading-audit-network.log`. The final backend results are
in `/tmp/vary-provider-loading-backend-network-results.json`. All applicable
local checks passed. Provider source fingerprints still match the recorded tree.
The renderer preview server was stopped after visual inspection.

Remote Linux, Windows, macOS and Windows-to-WSL2 CI were not run. No commit, push,
installer, signing, notarization or live-account acceptance is claimed.
