# ChatGPT browser sign-in alignment with Magpie

The validation below describes the initial callback-worker tree. Remaining
identity, account, proxy, and error-reporting differences are addressed in
[the follow-up QA record](2026-11-09-codex-login-full-magpie-parity.md).

## Reference and scope

Reference: [Magpie `8a7f77cb716da80d48767b1dcd03bbe8629eec1d`,
`internal/provider/signin.go`](https://github.com/yetone/magpie/blob/8a7f77cb716da80d48767b1dcd03bbe8629eec1d/internal/provider/signin.go),
including its browser callback, pasted callback, stale-listener recovery, and
token exchange. Token identity was compared with `codex_request.go` at the same
revision. The reference was fetched read-only on November 9, 2026.

The reported symptom was waiting after browser consent. The original login
attempt was no longer active when inspected: no callback listener, active login
panel, or retained OpenAI authorization/callback tab was present. Existing logs
could not establish its exact failure stage.

The implementation now:

- Starts token exchange and account storage in a backend task when a valid
  automatic or pasted callback arrives. Renderer polling only reads status.
- Returns browser success after account persistence, with a safe failure message
  if exchange fails.
- Uses the same OAuth client, PKCE parameters, fixed callback port, ten-minute
  browser window, and thirty-second token request deadline as the reference.
- Asks an abandoned Codex callback server to release its port through `/cancel`,
  then retries at bounded intervals. An unrelated listener remains owned by its
  original process; manual callback submission remains available.
- Accepts the exact loopback aliases `localhost`, `127.0.0.1`, and `::1`, retaining
  strict port, path, state, credential, fragment, and duplicate-query validation.
- Makes a matching authorization denial terminal without exposing vendor text.
- Sends `originator: codex_cli_rs`, a versioned Codex User-Agent, and JSON Accept
  headers. Token response bodies are bounded to one MiB and require nonempty
  credentials.

Vary AI's existing account persistence, cancellation generation, duplicate
identity, and proxy-switch commit guards remain in use. The common Rust
implementation serves macOS and Windows. Diagnostics omit callback URLs,
authorization codes, PKCE verifiers, credentials, vendor bodies, and identity
data. This does not claim byte-for-byte parity of all account storage or
inference proxy behavior.

## Regression evidence

The initial timeout regression reproduced a shared client's inherited
600-second timeout: a synthetic token endpoint returned headers and an
incomplete body, and exchange exceeded a 32-second external bound. The explicit
OAuth deadline ended that request safely.

Final focused coverage checks automatic completion without renderer polling,
post-persistence browser success, PKCE and Codex request headers, reload of the
stored account, manual alias callbacks, terminal denial and redaction,
malformed/oversized token bodies, stale and unrelated port holders, duplicate
callback rejection, and cancellation during exchange.

All new fixtures use temporary directories and dynamic ports. No real
authorization was replayed. No application installation or manual restart was
performed. An existing development session was running concurrently and its
application PID changed during this work; actual account login remains a
separate acceptance step.

## Environment and verification

- Base revision: `0cb8e387fd972ac3f04c0956f8c4b6df5fd64af9`, plus the current
  working changes. Other pre-existing/concurrent changes were preserved.
- Host: macOS, Darwin 24.3.0, Apple Silicon.
- Rust/Cargo: 1.98.1. Node: v24.11.0. pnpm: 10.12.3.
- Implementation: `src-tauri/src/proxy/providers/codex_oauth_auth/browser_login.rs`.
- Command integration: `src-tauri/src/commands/codex_oauth.rs`.
- Guidance: `docs/troubleshooting.md`.
- Final check logs: `/tmp/varyai-magpie-login-final-checks-20261109/`.

An intermediate test build exhausted disk space. Only unlocked generated
compiler incremental-cache sessions were removed; no source, account data, or
installed application was removed. Subsequent Rust verification disables
incremental compilation and package debug information to limit generated files:
`--config build.incremental=false --config profile.dev.package.varyai.debug=0`.

Final validation against the Magpie-aligned working tree completed on November
9, 2026:

| Command                                                            | Result                                                           |
| ------------------------------------------------------------------ | ---------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                           |
| `pnpm build:renderer`                                              | Passed                                                           |
| `pnpm test:unit`                                                   | Passed: 280 files, 3,566 tests                                   |
| `pnpm format:check`                                                | Passed                                                           |
| `pnpm audit --prod`                                                | Passed                                                           |
| `pnpm test:dev`                                                    | Passed                                                           |
| `pnpm test:trae-bridge`                                            | Passed                                                           |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                                           |
| `pnpm test:doubao-checkin`                                         | Passed                                                           |
| `pnpm test:qoder-codec`                                            | Passed                                                           |
| `pnpm test:agent-rewards`                                          | Passed                                                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                           |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                           |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed on retry after the disk-space failure                     |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 6,172 tests, 18 ignored                                  |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 6,335 tests across unit/integration binaries, 18 ignored |

The Rust check, Clippy, and test commands used the compilation overrides above.
The full test total includes the library tests; it is not additional to the
library-only total. A nonfatal macOS linker compact-unwind warning appeared in
the test build. All eight browser-login regression tests passed.

The 18 ignored library tests were the repository's explicitly gated tests:
publisher download acceptance; three database performance diagnostics; installed
CLI gateway acceptance; live desktop acceptance; TRAE custom desktop gateway;
signed-in Kimi acceptance; TRAE slow-header acceptance; installed Pi SDK;
installed Gemini CLI; live TRAE native bridge; native Doubao check-in; live
Doubao messages; two live S3 tests; real Codex corpus replay; and native
WorkBuddy check-in. They were not enabled for this local run.

Checked source SHA-256 fingerprints:

| File                                                              | SHA-256                                                            |
| ----------------------------------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/proxy/providers/codex_oauth_auth/browser_login.rs` | `4cf074098c7592c19e0828b4e90aaf7f0a37a8545173b7f7a0a9ae6600db949a` |
| `src-tauri/src/commands/codex_oauth.rs`                           | `fc68f769a1d1ab8f47253469c7c3e69dc60438e0536320e5c3e26ba6ee4f2638` |
| `docs/troubleshooting.md`                                         | `1bbc40cbfd0dc121f634b0aada1b20b97a6f800a3ca31e3624b2b880ea7f193f` |

The earlier timeout-only revision's results are not used for final acceptance.
These results establish local macOS checks only. Linux/Windows CI, including
any applicable Windows-to-WSL2 contract, has not been run for this working tree.

No commit, push, remote CI acceptance, signing/installer validation, or real
account login acceptance is claimed.
