# Agent connection reload and desktop restart parity

This follow-up addresses the connection-lifecycle findings in the supplied screenshot. It does not claim parity with every Magpie integration or feature.

## Reference and scope

- Reference: [yetone/magpie at 6ddfc15843a7f99c81556e36762fed2cbcd327e9](https://github.com/yetone/magpie/tree/6ddfc15843a7f99c81556e36762fed2cbcd327e9), fetched from the user-provided repository on November 6, 2026.
- Compared lifecycle sources: `internal/agent/apprestart.go`, `apprestart_windows.go`, `codex_app.go`, their tests, and `internal/provider/codex_app_server.go` and its tests.
- VaryAI base: `417def030f4c23ec6a4577f3492ab8409468ecde`, including the pre-existing Codex watcher, Windows adapter and shared-model-picker changes. Those changes were preserved. No commit or push was requested.
- Host: macOS / Darwin arm64; Rust/Cargo 1.98.1, Node 22.22.2 and pnpm 10.12.3. Node differs from the repository's exact 22.12.0 pin.

## Resulting contract

Claude Desktop and WorkBuddy expose a confirmed restart only when a verified running installation predates VaryAI's configuration change. macOS requests a normal quit and refuses to force an application that stays open. Windows asks windows to close and can terminate the verified remaining process tree before reopening the same package or installation. WorkBuddy's managed directory selects its build; an ambiguous directory does not authorize quitting a different build. Windows WorkBuddy AI remains unsupported, matching the reference's unverified-build boundary.

Disconnecting or suspending a desktop connection preserves a token-free restoration timestamp and file fingerprints. A running copy still on the previous settings can be restarted after restoration. External file changes, an active replacement receipt, process replacement and unknown identity invalidate the operation. The UI separates configuration repair from restarting a stale running application.

Codex route changes settle before its old desktop-owned stdio app-server is reloaded. In-flight gateway requests retain activity until their response body ends or is cancelled, including streaming responses, and impose a ten-second quiet interval. Native session activity is checked separately. The wrapper preserves HTTP frames, trailers, status and headers. Configuration reads, activity reads and process identities are rechecked before termination.

Catalog-only changes, catalog-pointer changes, provider display names, inactive providers, selected models and access-token rotation do not cause automatic reloads. An account-independent custom provider ignores unrelated ChatGPT login changes. A stale catalog exposes a confirmed manual reload through the same idle-aware worker. Progress distinguishes waiting, reloading, failure and stopped processes. Replacement observation proves process replacement only, not authentication or inference.

If an ended Codex app-server has no replacement, Windows can close and reopen the verified Store Codex application once, with the same configuration and idle rechecks. An application already quit is not opened again. This recovery can lose unsent drafts. macOS leaves a failed whole-application recovery to the user. Native activity abandoned for more than thirty minutes can expire only from known rollout/database/WAL inactivity; recent gateway activity still defers reload.

TRAE remains an acknowledged bridge-delivery and cleanup flow. Apply again retries its outbox; pending delivery is not treated as proof that a generic process restart can repair it.

The Codex worker currently supports macOS and Windows. Linux requires a manual client restart. The reference's broader quota-driven account-switching, crash-page detection and other integration lifecycles are outside this targeted change. Unknown custom installations are refused rather than assigned a guessed process identity.

## Verification

Final local backend checks passed for the completed working tree:

| Check                                              | Result                                              |
| -------------------------------------------------- | --------------------------------------------------- |
| `cargo fmt --check`                                | Passed                                              |
| `cargo check`                                      | Passed                                              |
| `cargo clippy -- -D warnings`                      | Passed                                              |
| `cargo test --lib`                                 | 6,085 passed, 18 existing ignored                   |
| `cargo test --no-fail-fast`                        | 16 test binaries, 6,248 passed, 18 existing ignored |
| Changed-document formatting and `git diff --check` | Passed                                              |

Cargo commands used `--manifest-path src-tauri/Cargo.toml`, offline cached dependencies, `/tmp/vary-magpie-parity-target`, incremental compilation disabled and package debug info disabled. Test commands used the process-local loopback proxy exclusions described below. No failing test was skipped. The nonfatal macOS debug linker compact-unwind warning remained.

The canonical sorted source/configuration inventory SHA-256 is `0d41a9ccd3b87e0e075ac4269c3cf2d4b999a3a3950be891e073797cfa24990a`. The before/after inventories match, including both successful full test commands. Formatting, Cargo check and Clippy results were retained only after proving that the final source inventory was byte-identical to the one they checked. Exact commands, results and source inventories are retained in `/tmp/vary-agent-lifecycle-verification/backend-results-accepted.json` and the associated logs. This report was completed afterward without application-source changes. Earlier historical watcher reports do not validate this changed tree.

The first full library run reproduced two failures. The Qoder listener fixture was unchanged from the base revision and failed its default native-TLS handshake with `CSSM_ModuleLoad(): CSSMERR_DL_MDS_ERROR` / error -9808. Its test now selects rustls with only the explicit temporary root, and additionally requires unknown-root and wrong-hostname requests to fail for certificate-validation reasons. The real listener, certificate generation, uninstalled-system-trust check, successful forwarding, shutdown and port-release assertions remain. This isolates the local TLS fixture from the host native trust engine without disabling certificate or hostname validation. The focused test passes; native trust installation and native-TLS compatibility remain separate, unverified integration checks. No production Qoder code changed.

The profile test failed on its first DeepSeek Harness install, before profile recovery or desktop freshness logic ran. Its inherited `DSH_HOME` bypassed `CC_SWITCH_TEST_HOME`; clearing only `DSH_HOME` made the same compiled test pass, including after the host recovered from a separate disk-space incident. Harness path selection now gives a nonempty explicit test home priority over the inherited application home on every platform, with synthetic-directory regressions and ordinary custom-home behavior retained. Temporary manager diagnostics used to locate this failure were removed. No freshness-operation errors were suppressed to make the profile test pass.

A subsequent library run passed those two tests but reported 46 existing reversible-security transport fixtures returning HTTP 502 before reaching their local upstreams. The first case also failed alone; source comparison showed no security-code changes. A read-only system query confirmed native HTTP/HTTPS/SOCKS proxies were enabled despite absent proxy environment variables. The same binary passed after adding only loopback hosts to `NO_PROXY` and `no_proxy`. Final Rust test commands append `127.0.0.1,localhost,::1` to those process-local variables; application and system proxy settings are unchanged. A temporary error-code-only diagnostic was removed, and the security test file again exactly matches the base revision. No security assertion, verifier or production forwarding rule was disabled or relaxed.

Renderer checks on the completed renderer changes passed: typecheck, formatting, production build, 279 unit-test files / 3,483 tests, production dependency audit (zero known advisories), 94 Node helper tests with one existing skip, and five storage-recovery helper tests. Logs are retained locally under `/tmp/vary-agent-lifecycle-verification/`.

Windows cross-checks passed for the actual desktop native module and the complete actual Codex worker module, including native helpers and test code, targeting `x86_64-pc-windows-msvc`. The Codex harness retained project-locked Rust dependencies and genuine Tokio task bounds. It stubbed unrelated application boundaries and used generated SQLite Rust bindings without compiling or linking Windows SQLite C. These checks establish conditional-code and native API type compatibility, not a full Windows Tauri build or runtime acceptance. A separate actual-source production build of the activity middleware also passed Clippy with `-D warnings` for `x86_64-unknown-linux-gnu`, using project-locked dependencies and no source stubs or lint allowances; this does not validate the full Linux application. Evidence is retained under `/tmp/vary-desktop-windows-check-tPQShM/` and `/tmp/vary-codex-whole-module-windows-20261106/`; the corresponding module hashes remained unchanged during checks.

No installed application was launched, replaced, signalled or quit for verification. Native operation tests use fake runtimes and synthetic process/configuration fixtures. Linux/Windows/macOS remote CI, real-client recovery, authentication, inference, signing, installation and release acceptance remain separate from local checks.
