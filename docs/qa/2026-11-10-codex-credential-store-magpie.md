# Codex allowance credential-store selection

## Reference and reproduced failure

Reference: [Magpie `0f94acc994a9fa9af0a5f8bbafa6bb74e7692b96`](https://github.com/yetone/magpie/tree/0f94acc994a9fa9af0a5f8bbafa6bb74e7692b96), read on November 10, 2026. Relevant implementations are `internal/provider/codex_store.go` (`readCodexLogin`, `readCodexStoreSetting`, `codexKeychainAccount`) and `internal/provider/account.go` (`codexAccount`).

The native account row and its model discovery read the file login selected by Codex, while allowance queries previously tried a macOS keychain item first, without selecting its account. A legacy keychain item could therefore override a newly authorized file login. The allowance parser also required an explicit `auth_mode: "chatgpt"`, rejecting older token records without that marker. This produced a credentials-unavailable / re-login message even while the native login file contained a complete login.

A read-only structural inspection confirmed the file-store / legacy-keychain mismatch on the reporting host. No credential values were printed or copied into fixtures. No live vendor request was made to establish allowance or generation success.

## Change

Allowance queries follow Magpie's store selection:

- The default and explicit `file` mode read `auth.json` without asking the keychain.
- `auto` tries the supported keychain item, falling back to the file when that item cannot be read.
- `keyring` does not fall back to an unrelated file, and `ephemeral` is unavailable to an external reader.
- Encrypted storage follows `features.secret_auth_storage`, defaulting on for Windows. Magpie does not read that encrypted store: `auto` uses the file fallback; `keyring` remains unavailable.
- An unreadable or unrecognized store setting uses Magpie's file fallback.
- A macOS keychain read selects `Codex Auth` and the `cli|` account derived from the canonical credential home. A service-only lookup cannot select another home's login.
- OAuth token records do not require an explicit mode marker; explicit API-key mode remains excluded, as in Magpie's `codexAccount`.

Credential reading runs outside the asynchronous executor. No new login UI, subscription-tier gate, account switching, automatic login, credential deletion, or native-client write is introduced. This change fixes allowance credential selection and legacy token recognition. It is not evidence of full Magpie parity for every subscription transport, renewal policy, quota diagnostic, or model-discovery path.

## Regression verification

The default/file regression failed before the store-selection fix: the synthetic keychain reader was invoked despite a valid file login. The legacy-mode regression was also run against the old parser before changing that parser.

Fixtures use synthetic tokens and temporary directories. Store-selection tests simulate macOS and Windows behavior without reading a real keychain. They cover file precedence, auto preference/fallback, keyring isolation, ephemeral storage, encrypted-store defaults and overrides, invalid settings, canonical-home keychain identity, and marker-free token records.

## Local verification

Base revision: `72ce390fa473087505934f91ed77728fac51ef01`, plus this change and unrelated concurrent working-tree changes. Host: macOS 15.3.1 arm64; Rust/Cargo 1.98.1 stable; Node 24.11.0; pnpm 10.12.3. The repository pins Node 22.12.0; this host used its existing Node installation.

The final subscription implementation/test file has SHA-256 `48c350c989ce7e902b0eaa6b0199c7cdbae8d15ff209fdb4a00fc830a900f691`. It stayed unchanged during the final backend checks. Logs and machine-readable receipts are under `/tmp/varyai-codex-store-checks-20261110/`; the two expected regression failures are in `/tmp/varyai-codex-store-red-20261110.log` and `/tmp/varyai-codex-store-parser-red-20261110.log`.

| Check                                                                                 | Result                                                                                                                                  |
| ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                      | Passed, including the final retry                                                                                                       |
| `pnpm build:renderer`                                                                 | Passed, including the final retry                                                                                                       |
| `pnpm format:check`                                                                   | Passed, including the final retry                                                                                                       |
| `pnpm test:unit` final retry                                                          | 3,766 passed; 4 failed in `WorkspaceNavigation.test.tsx`, whose expected paired navigation labels disagree with existing locale changes |
| Development, macOS DMG, TRAE, WorkBuddy, Doubao, agent rewards and Qoder helper tests | Passed                                                                                                                                  |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                 | Passed                                                                                                                                  |
| `pnpm audit --prod`                                                                   | Passed; no known vulnerabilities                                                                                                        |
| `cargo check` final retry                                                             | Passed                                                                                                                                  |
| `cargo clippy -- -D warnings` final retry                                             | Passed                                                                                                                                  |
| `cargo test --lib` final retry                                                        | 6,304 passed; 19 ignored; no failures                                                                                                   |
| Complete `cargo test` final retry                                                     | 6,467 passed; 19 ignored; no failures across 16 suites                                                                                  |
| `cargo fmt --check` final retry                                                       | Failed on formatting in concurrent changes outside `subscription.rs`; this file independently passed `rustfmt --check --edition 2021`   |
| Codex allowance regressions within the complete Rust run                              | All 8 related tests passed, including both previously failing regressions                                                               |

Cargo commands use `--manifest-path src-tauri/Cargo.toml`, `CARGO_INCREMENTAL=0`, `--config profile.test.package.varyai.debug=0` and `--config profile.dev.package.varyai.debug=0`. These resource settings remove package debug information without disabling assertions. The 19 ignored tests retain their existing environment/live-client prerequisites.

Earlier backend attempts encountered in-progress concurrent projection edits (`E0423`, `E0425` and `E0596`). A one-line `let mut config` correction in the Qoder branch of `src-tauri/src/services/agent_connections/projections.rs` allowed its existing mutation to compile; no projection behavior was otherwise changed by this task. Subsequent check, Clippy and library retries passed. The rest of the concurrent projection, routing, onboarding and locale changes were preserved. Existing navigation assertions were not weakened.

The final complete run used the source inventory in `backend-full-final-source-start.json`. Its receipt (`backend-full-final-results.json`) records a concurrent change to `src-tauri/src/services/deepseek_harness.rs` during the run. The subscription source remained unchanged. Results validate the command snapshots and this unchanged subscription fix; they do not establish acceptance of the later concurrent edit.

The frontend failures, workspace formatting failures and concurrent source changes prevent whole-checkout acceptance. No commit, push, remote CI, Linux/Windows/WSL2 runtime acceptance, installed-app replacement, or live account success is claimed.
