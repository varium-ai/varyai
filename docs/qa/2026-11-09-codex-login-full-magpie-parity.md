# Codex sign-in: remaining Magpie behavior alignment

## Scope and reference

This follows the first callback-worker change documented in
[the earlier QA record](2026-11-09-codex-browser-login-magpie.md).
The later real-account attempt reached Vary AI's callback and started token
exchange, then failed about one second later. The earlier diagnostics combined
HTTP rejection, parsing, identity validation, and persistence errors, so that
attempt's exact cause cannot be reconstructed.

The reference remains Magpie revision
[`8a7f77cb716da80d48767b1dcd03bbe8629eec1d`](https://github.com/yetone/magpie/tree/8a7f77cb716da80d48767b1dcd03bbe8629eec1d),
specifically `signin.go`, `codex_request.go`, `logins.go`, and `account.go`.
Comparison covered the authorization parameters, code exchange, identity
parsing, account matching, proxy selection and retention, callback completion,
and failure reporting.

## Resulting behavior

- Codex identity includes version, OS version, architecture, and terminal label.
  OS detection is bounded and cached outside request forwarding, with common
  implementations for macOS, Windows, and Linux.
- Browser and device exchange accept the three nonempty credentials and an ID
  token carrying an email. Payload parsing accepts padded base64url and does
  not require a JWT header or subject. Workspace is optional; an absent
  workspace remains empty, never a local account ID or an invented workspace.
  Unused or malformed `expires_in` metadata does not reject otherwise valid
  credentials.
- Matching email/workspace credentials update the existing local record.
  Separate workspace seats stay separate, targeted login cannot replace a
  different account, and existing provider bindings and default selection remain.
  Login cancellation and generation checks still guard persistence.
- The selected proxy is retained in the existing private account store and
  restored after restart. Targeted sign-in inherits it, while an explicit global
  selection removes the override. Refresh, quota, model discovery, probes, and
  forwarding use the account's transport without internal redirects.
  The account editor distinguishes inherited, global, direct, and custom proxy
  choices; both browser and device command bindings preserve explicit global
  selection. Inherited global forwarding retains the shared connection pool.
- Browser credentials retain their access-token cache after restart. A newer
  native refresh-token generation invalidates the retained old access token.
  Existing imported-account validation and quarantine remain unchanged.
- Native ownership markers can identify email-based credentials without a
  workspace, while still requiring the exact local record and user identity.
- Browser and pasted callbacks finish in the backend and report success after
  persistence. Pasted callback submission waits for completion.
- Failure pages and status include bounded diagnostics: a fixed stage, numeric
  HTTP status when available, and an allowlisted OAuth code. The renderer maps
  only recognized error keys and translates them in all four runtime languages.
  A failed attempt offers a fresh authorization flow rather than requiring the
  user to reopen an already consumed authorization URL.

## Deliberate security boundaries

Magpie exposes arbitrary upstream error descriptions. Vary AI instead exposes
fixed failure categories, HTTP status, and known error codes, to honor the
repository's credential-redaction requirements. Callback state, expected
loopback address/port/path, duplicate-query validation, cancellation,
atomic storage, and ownership checks remain enforced. Legacy imports are not
silently reclassified as successful browser authorizations.

## Regression and verification

Test-first regression reproduced failure for an ID-token payload with email but
no subject or workspace, and exercised the renderer's error allowlist.
Additional fixtures cover repeat-login replacement, restart and access-cache
retention, custom proxy routing to reserved invalid hostnames, inherited and
reset proxy choices, distinct workspace seats, mismatched targeted login, HTTP
status preservation, vendor-text redaction, native marker ownership, full client
identity, bounded responses, cancellation, and the stalled-body deadline.
Fixtures use temporary directories, dynamic loopback ports, and synthetic
credentials.

The following local checks passed for this change. The earlier QA record's
results apply only to its recorded source fingerprints.

| Command                                                            | Result                                                 |
| ------------------------------------------------------------------ | ------------------------------------------------------ |
| `pnpm typecheck`                                                   | Passed                                                 |
| `pnpm build:renderer`                                              | Passed                                                 |
| `pnpm test:unit`                                                   | 281 files, 3,577 tests passed                          |
| `pnpm format:check`                                                | Passed                                                 |
| `pnpm audit --prod`                                                | Passed; no known vulnerabilities                       |
| `pnpm test:dev`                                                    | Passed                                                 |
| `pnpm test:trae-bridge`                                            | Passed                                                 |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                                 |
| `pnpm test:doubao-checkin`                                         | Passed                                                 |
| `pnpm test:qoder-codec`                                            | Passed                                                 |
| `pnpm test:agent-rewards`                                          | Passed                                                 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                 |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                 |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,178 passed, 18 ignored, no failures                  |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 6,341 passed across 16 suites, 18 ignored, no failures |

The ignored Rust tests retain their existing explicit live-client, publisher
download, performance, or environment prerequisites. They were not run as part
of this change.

- Base revision: `0cb8e387fd972ac3f04c0956f8c4b6df5fd64af9`, plus working changes.
- Host: macOS Darwin 24.3.0 arm64; rustc/Cargo 1.98.1; Node 24.11.0; pnpm 10.12.3.
- Backend, audit, and helper logs:
  `/tmp/varyai-magpie-parity-20261109/final-v2/`.
- Final renderer logs:
  `/tmp/varyai-magpie-parity-20261109/frontend-final/`. These supersede the
  earlier renderer run and its formatting failure, after the proxy-inheritance
  and fresh-attempt retry changes.
- Source manifests were captured before checks. All 26 source/test files in
  this login change remained unchanged against their applicable backend or
  final renderer snapshot. Their manifest is
  `/tmp/varyai-magpie-parity-20261109/login-source-manifest.json`, SHA-256
  `214676efed7a1c46c07eb1082ea23b117db99bc2885a0be23ab70c3aed5b6a0c`.
- Rust builds disable incremental compilation and package debug information:
  `--config build.incremental=false --config profile.dev.package.varyai.debug=0`.
- Pre-existing and concurrent onboarding, styling, native subscription, and
  provider changes remain in the checkout.
- A separate concurrent change to
  `src-tauri/src/proxy/intent/workbuddy_official_tests.rs` appeared after the
  backend snapshot. The backend counts above validate the captured snapshot
  and this unchanged login change, not that later unrelated test addition.

No real authorization code was replayed or logged. No manual app launch,
installation, or restart was performed; an existing development runner can
rebuild and restart its own application while source files change. No commit,
push, remote CI, Linux/Windows acceptance, installer acceptance, or successful
real-account sign-in is claimed.
