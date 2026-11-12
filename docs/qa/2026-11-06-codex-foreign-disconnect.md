# Codex disconnect after an external takeover — 2026-11-06

## Report and cause

The reported Tools dialog said that the Codex recovery record was missing, then refused **Disconnect** with the generic configuration-conflict error. A synthetic regression fixture reproduces an external provider selection, a retained Vary provider definition rewritten to a local `/v1` endpoint, and a native Codex URL at the same origin. The fixture and descriptions below use synthetic data and public reference behavior; live configuration contents, model names and credentials are not included. This task did not directly write live Codex configuration or login files. Live client state can change during an investigation, so actions must use refreshed status rather than a stale dialog.

Magpie deliberately retains provider definitions used by historical conversations and rewrites their URLs to its gateway. See [`ccSwitchTables` and `takeTables`](https://github.com/yetone/magpie/blob/465928bccea4fec3537edd6280f7cabb9a0c5bad/internal/agent/codex.go#L1037) at revision `465928bccea4fec3537edd6280f7cabb9a0c5bad`. The retained Vary label and flags therefore do not establish current Vary ownership. The inspected Vary intent-menu writers use `/intent/v1`; there is no basis for accepting every local `/v1` endpoint as an owned menu route.

Two conditions caused the failure: disconnect inferred native ownership from matching origins before validating the retained provider, and status treated any retained Vary provider definition as an orphan. A third UI condition hid the disconnect action once only a saved Vary connection request remained.

## Change

A shared classifier recognizes only the demonstrated external-takeover shape: no managed Vary model/provider/catalog references anywhere in the configuration; a defined effective external provider; the preserved Vary provider signature with an exact `/v1` URL; and matching explicit, nonzero-port loopback HTTP origins for both `/v1` definitions and the valid native Codex URL. Credentials in URLs, query strings, fragments, unknown signatures, ambiguous references, missing definitions and other endpoints remain outside this exception. The owned orphan validator still requires `/intent/v1`.

For this confirmed external shape, receiptless disconnect leaves configuration, native URL, provider definitions, model selection and authentication byte-for-byte unchanged. Vary can revoke its own runtime and clear its saved connection request. Status no longer labels the retained definition as an orphan. Existing recovery receipts still go through their normal restore path; an external baseline restored after explicit reconnection must also remain usable. Unreadable recovery evidence still blocks changes.

The main and compact Tools views keep **Disconnect** available for the specific disabled-Codex `connection_inactive` intent state. A new four-language confirmation describes clearing Vary's saved request while preserving the current external provider. Configuration-change protection and the owned stopped-gateway flow remain in place.

## Verification

- Baseline: `417def030f4c23ec6a4577f3492ab8409468ecde`, including all pre-existing local changes.
- Host: macOS 15.3.1, ARM64; Rust/Cargo 1.98.1, Node.js 24.11.0, pnpm 10.12.3.
- Evidence directory: `/var/folders/q7/3zkkhzjs2vdb1xb_y2czj_hh0000gn/T/vary-codex-foreign-disconnect-r8gpi6lk`.
- Backend RED: the isolated foreign-takeover fixture reproduced the erroneous orphan status before the change.
- Frontend RED: both main/compact lifecycle tests reproduced the disabled picker before the change. The focused page suite then passed 138 tests.
- An initial broad `foreign` filter passed the four new backend regressions but also selected an existing skill-storage test whose process lacked a temporary application home. Its attempt to write real settings was denied. Subsequent backend commands explicitly set a temporary `CC_SWITCH_TEST_HOME`; the denial was not retried with wider filesystem access and no assertion was disabled.

The final focused backend runs passed 105 client/configuration tests and six connection-status tests, including the external baseline after explicit reconnection. They used an isolated application home and an unchanged source inventory. Read-only independent review found no blocking correctness, privacy or ownership issue in the frozen changes. All ten final verification commands passed against an unchanged source inventory, recorded in `all-results.json` and `all-summary.json`. The combined source inventory SHA-256 is `065df8f85781d14216346104c2bddab45b44d1b6a7782cd3545e61272c43970b`. Rust checks use cached offline dependencies, `/private/tmp/vary-magpie-parity-target`, incremental compilation disabled and package debug info disabled. Loopback exclusions are appended only to the verification process's proxy environment. Test fixtures use temporary directories and dynamic listener ports.

| Final check                                        | Result                                         |
| -------------------------------------------------- | ---------------------------------------------- |
| `cargo fmt --check`                                | Passed                                         |
| `cargo check`                                      | Passed                                         |
| `cargo clippy -- -D warnings`                      | Passed                                         |
| `pnpm typecheck`                                   | Passed                                         |
| `pnpm format:check`                                | Passed                                         |
| `pnpm build:renderer`                              | Passed                                         |
| `pnpm test:unit --maxWorkers=1`                    | 280 files; 3,506 tests passed                  |
| `cargo test --lib`                                 | 6,107 passed; 18 existing ignored              |
| `cargo test --no-fail-fast`                        | 16 binaries; 6,270 passed; 18 existing ignored |
| `pnpm audit --prod`                                | Passed; no known vulnerabilities               |
| Changed-document formatting and `git diff --check` | Passed                                         |

The nonfatal macOS compact-unwind linker warning remains. No new regression was skipped. A baseline patch comparison confirms that pre-existing tracked changes outside the ten touched existing files remain byte-equivalent; the only new file for this repair is this verification document. The earlier diagnostic feature and cache-ignore work were preserved.

## Acceptance boundaries

The logic is shared across macOS and Windows and the frontend preserves all four runtime languages. Local macOS fixtures do not establish Windows/Linux CI acceptance or a successful real Codex conversation. Verification commands did not launch, install, replace, or explicitly ask Vary to take over a live client. The running development application or other client activity may change configuration independently; refresh Tools and inspect the current receipt before choosing a live action. Loading the updated native backend is required to use the fix. No commit or push was created by this task.
