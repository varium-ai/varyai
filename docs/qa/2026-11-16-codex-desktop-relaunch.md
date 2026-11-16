# Codex desktop relaunch — November 16, 2026

## Change

The reported symptom was that confirming Restart quit Codex without reopening
it. Two code paths could fail to establish a whole-application restart:

- macOS treated a successful `open` command as restart success and could reopen
  while the previous process was still cleaning up after AppKit marked it
  terminated.
- A confirmed stale-catalog refresh terminated internal app-servers and relied
  on the desktop application to replace them.

macOS now waits for the original kernel process identity to disappear, requests
a fresh launch of the same verified bundle with `open -n`, and waits up to 20
seconds for a new verified instance. Missing replacements, unknown identities,
different bundles and launch errors do not report success. Normal quit refusal
and configuration-change protection remain enforced.

Confirmed catalog refreshes now select the whole-application restart on both
macOS and Windows after the existing idle and configuration checks. The worker
also rechecks its observed app-server identities before entering desktop
restart. Expected server exits do not invalidate the separate configuration
guard used before reopening. A failed manual desktop operation stops that queued
attempt. Automatic routing changes retain internal app-server reloads. Windows
retains its native launch-and-wait contract, with an additional portable failure
regression.

## Verification

Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`, with unrelated working
changes already present. This task changes four Rust lifecycle/test files and
adds troubleshooting guidance and this report. No commit or push was requested.

Host: macOS 15.3.1 arm64; Rust/Cargo 1.98.1; Node.js 24.11.0; pnpm 10.12.3.
The host Node version differs from the repository's 22.12.0 pin. Rust commands
use cached dependencies with `--offline`, an isolated cloned target directory,
and `CARGO_INCREMENTAL=0`. The full backend runner uses a temporary
`CC_SWITCH_TEST_HOME` and process-local loopback proxy exclusions. Native
application control is disabled in tests; no installed application was quit,
launched, installed or replaced for verification.

The test-first regression
`successful_open_without_a_replacement_is_a_restart_failure` failed before
wiring replacement confirmation: the previous implementation returned `Ok(())`
instead of `agentConnections.restartFailed`.

| Check                                                                                                | Result                                      |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| Desktop lifecycle tests, including portable Windows cases                                            | 26 passed                                   |
| Codex app-server tests                                                                               | 47 passed                                   |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                             | Passed                                      |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml`                                         | Passed                                      |
| `cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings`                         | Passed                                      |
| `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`                                         | Passed on rerun                             |
| `pnpm audit --prod`                                                                                  | Passed                                      |
| Development, DMG, client bridge, model-default, credential, check-in, rewards and codec helper tests | Passed                                      |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                                | Passed                                      |
| `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml`                                    | 7,607 passed; 21 ignored                    |
| `cargo test --offline --no-fail-fast --manifest-path src-tauri/Cargo.toml`                           | 7,772 passed; 21 ignored across 16 binaries |
| `pnpm test:unit`                                                                                     | 4,225 passed in 317 files                   |

The first frontend run encountered unrelated pagination-test TypeScript errors
in concurrent usage-table work and two notarization-fixture timeouts. TypeScript
subsequently passed, and all 21 notarization tests passed in a focused rerun.
The initial full frontend run had 4,221 passing and two failing tests. A complete
rerun subsequently passed all 4,225 tests across 317 files.

Command results and logs are retained under
`/tmp/vary-codex-restart-verification/`. SHA-256 hashes of the four task-owned
Rust files are recorded in `source-hashes.json` and matched after all backend checks.
Linux/Windows/macOS remote CI and real installed-client restart acceptance
remain unverified. Local lifecycle tests establish orchestration and error
handling, not a successful real-client restart, authentication or inference.

The task-owned Rust file hashes matched after all backend checks. The isolated
Cargo cache was removed after verification; the command logs and result JSON
remain available. `git diff --check` and formatting of the changed documentation
passed. Unrelated working changes were preserved and no generated artifacts were
added to Git. The backend test linker emitted its existing nonfatal compact-unwind
warning; the test-only quota helper also emitted a dead-code warning.
