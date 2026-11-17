# Claude Desktop startup probe recovery

## Problem and reference

Read-only inspection of the installed Claude Desktop's **Details** on November
17, 2026 showed HTTP 409 `routing_unknown_continuation` from VaryAI's authenticated
Messages ingress. The original screen's suggestion about provider quota/access
was not the cause of that rejection: forwarding had not started. A new regression
seeded an ordinary `.` conversation, persisted its ownership, changed the selected
destination and then sent the startup probe. It reproduced the exact HTTP 409
before the fix, with both derived and explicit conversation identities assessed.

Startup probes now use request-local continuation state without a conversation
identity or durable ownership lookup. They cannot load, overwrite or release a
saved conversation binding. Explicit continuation fields disqualify a request
from being treated as a standalone probe. Ordinary conversations still reject a
changed destination after a probe succeeds; no recovery files are deleted.

A separate comparison with Magpie also found missing thinking-required recovery:
a model requiring `thinking.type` to be enabled rejects Desktop's one-token `.`
check. The existing VaryAI probe raised the reply limit to 16 but returned this
refusal without repairing it. A synthetic native HTTP regression reproduced HTTP
502 `routing_upstream_rejected` before that compatibility implementation. This
second path was not the error observed in the installed client's Details.

Reference: [yetone/magpie at
0746140772bedf3e31548dea5cab76197f1c98ff](https://github.com/yetone/magpie/tree/0746140772bedf3e31548dea5cab76197f1c98ff),
`internal/gateway/desktop.go`, `thinking_needed.go` and `desktop_probe_test.go`.
The implementation follows its real-upstream recovery, rather than returning a
synthetic successful response. Its general model capability cache and title
request adaptation are outside this startup-probe change.

## Implementation and coverage

An internal probe marker is assigned only by the authenticated Claude Desktop
connection ingress. Native Messages execution and the converted transport share
a bounded recovery helper. It examines only structured error-message fields,
including nested relay errors and error-only SSE; it retries once with a minimum
reasoning setting and enough reply tokens after the specific thinking-required
refusal. Messages, OpenAI Chat, Responses and Gemini wire settings are adapted
separately. The same Rust code runs on macOS and Windows.

No credentials, provider names or private configuration are used to decide that
a model requires thinking. Other rejections, malformed or oversized errors,
partial streams, authentication failures and ordinary requests are preserved.
Inspection has a 64 KiB/5-second bound and retains buffered and unread response
chunks when it cannot finish. Unknown native request fields keep their raw JSON
representation. A retry retains the selected URL, model and upstream credentials;
the local connection token is never forwarded. Requests bound to a redaction or
restoration attempt keep the existing guarded handling, including safe error
failures; this helper does not replay or mutate a prepared security attempt.

The full per-Agent assessment is in [Agent feature coverage](agent-feature-coverage.md#desktop-startup-probe-compatibility).
Synthetic coverage includes four upstream protocols, a Chat error inside HTTP
200 SSE, enabled/disabled security policy, ordinary Desktop requests, other
shared Agent identities and unsupported connection endpoints. Unit cases cover
one-retry bounds, already-enabled reasoning, malformed inputs, exact response
bytes and headers, output before an SSE error and unrelated upstream failures.
Existing Electron-origin, credential, revoked-connection and classifier-isolation
regressions remain applicable.

## Verification

Base revision: `e84ae0dc90ca5e3b77821f121e41347f8f3d4ad0`, plus the working tree.
Host: macOS 15.3.1 (24D70) on Apple Silicon; Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3.
Node differs from the repository's pinned 22.12.0. Backend checks use temporary
`CC_SWITCH_TEST_HOME` directories, dynamic loopback ports and in-memory storage;
development/test debug information is disabled. The checkout contains concurrent
catalog/continuation and routing UI work, which is preserved. Results from the
whole-tree checks cover that combined tree and cannot be attributed solely to
this fix. Source hashes, command logs and results are saved under
`/tmp/vary-desktop-probe-verification/`.

| Final check | Result |
| --- | --- |
| `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 322 files, 4,282 tests passed |
| All development, CI-policy, packaging, bridge, identity, check-in, rewards and codec helpers in `docs/development.md`; shared-storage recovery helper | Passed; exact commands in `frontend-results.jsonl` |
| `pnpm audit --prod` | Passed; no known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed on the final production changes |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed on the final production changes |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml proxy::connections::` | 22 passed, including both new 409 regressions |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Passed: 7,711 library tests and 165 integration tests; 21 manual library tests ignored |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed: 7,711 passed, zero failed, 21 manual tests ignored |
| `git diff --check` | Passed |

The final executable also passed the same 22-test connection suite when invoked
directly. Its SHA-256 and the final source hashes are in the verification
directory (`test-executable.sha256`, `final-source-v2.sha256`). The fixture updates
both the exact target and its routing-group member when changing a destination;
changing only the target produced an invalid candidate plan in an intermediate
run. An earlier full-library binary was compiled before the continuation fix and
correctly failed the two new regressions (7,709 passed, two failed, 21 ignored).
The final full run above passed them both. One initial Clippy invocation failed
during build-script linking with `errno=28` (disk full); the final retry passed.

Native macOS startup-check acceptance passed in the already running Claude
Desktop 5.11492.5. Before the fix, expanding **Details** showed the 409 above.
After the existing development watcher rebuilt the local backend and it resumed
listening on its configured port, clicking **Check again** briefly showed
**Checking…**, then removed the entire gateway-error banner and its Details,
Open Setup, Check again and Dismiss controls from the accessibility tree. This is
native startup-probe acceptance, not conversation, tool execution or installer
acceptance. The existing conversation's separate connection error was not retried.

The verification did not request an installed Agent launch, quit, replacement or
installation, or alter its credentials/configuration. Native Windows acceptance
and Linux/Windows/macOS remote CI remain unverified. No commit or push is part of
this task.
