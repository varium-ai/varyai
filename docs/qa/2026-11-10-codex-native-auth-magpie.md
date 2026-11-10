# Codex native authentication follows Magpie

For acceptance of the combined submission, see the
[consolidated pre-submit record](2026-11-10-pre-submit-verification.md).
The results below describe this task's intermediate working trees.

## Reference and scope

Reference: Magpie main at
[`4869d0c9665e1a815fee3a1a4d67a575e0baab93`](https://github.com/yetone/magpie/tree/4869d0c9665e1a815fee3a1a4d67a575e0baab93),
resolved and read on November 10, 2026. Reviewed implementations:

- `internal/gateway/codex_backend.go`: `codexUpstreamOn`, `codexAccountOut`,
  `codexBackendList`, `askCodexList`, and `codexModels`.
- `internal/provider/signin_revoked.go`: `SignInRevoked` and its exact refusal
  vocabulary.
- `internal/agent/codex.go`: `chatGPT` and `codexChatGPTKnown`, especially
  incomplete authentication-file reads during token rotation.

The requested change concerns the native Codex takeover authentication and
HTTP 401 path. It does not claim that VaryAI implements every Magpie endpoint,
WebSocket transport, account-store adapter, or configuration format.

## Behavior

Removed the native five-second authentication lease, two-second background
reconciliation, token HMAC proof, mandatory local binding, pinned-workspace
authentication check, and client-version allowlist. Native requests use their
own authorization and workspace headers. Missing configuration receipts or
an unavailable native menu do not block forwarding built-in models. An upstream
generation 401 retains its status, headers and body, allowing Codex to manage
its native sign-in. Managed-account removal/logout does not independently
revoke Codex's native connection.

Model discovery uses Magpie's one-minute account cache, one background request
per account and query, three-second cold-call deadline, thirty-second fetch
deadline, and immediate stale responses. Ordinary 401/403 responses reach
Codex. A model-list response explicitly reporting a revoked sign-in falls back
to the available catalog without retaining a permanent denial; a new sign-in
can fetch its catalog immediately. Discovery preserves an established native
route when its authentication file is temporarily unreadable or incomplete.

The existing security-inspection pipeline, gateway policies and checkpoint
validation remain in place. The request workspace, falling back to its bearer
header, supplies the native conversation namespace; it is not an extra
authentication gate. Shared Rust forwarding changes apply to macOS and Windows.
Status descriptions were updated in all four runtime languages.

## Regression coverage

Synthetic fixtures use dynamic loopback ports, temporary directories and
in-memory databases. No fixture reads a real credential or calls an official
account endpoint.

- Forwarding works without installing a local credential binding or menu.
- Rotated tokens and different request workspaces reach the upstream unchanged.
- Missing native authorization reaches the upstream and preserves its 401.
- Ordinary model-list authentication failures reach the client.
- Revoked model discovery keeps managed models visible and retries a new sign-in.
- Concurrent clients with rotated tokens share the same workspace catalog fetch.
- A stream completes after the former five-second lease without a worker.
- A cold model-list timeout leaves the background request running; a completed
  cached list and subsequent stale list are available immediately.
- The revoked-sign-in vocabulary distinguishes permanent revocation from
  refreshable expiry and ordinary authorization errors.
- Authentication-file rewrites and rotation preserve the configured native URL.
- Connecting beside a native sign-in preserves Codex's current model while
  configuring the local native URL, matching Magpie's join behavior.
- Existing request-body limits, DLP redaction/blocking, exact transport fields,
  compaction, account-scoped continuation recovery, configuration restoration,
  and gateway-policy regressions remain covered.

## Verification record

Base revision: `82204c3fca2ed229f40fd5bfda5c4daf33967d4f`, plus concurrent
working-tree changes. Existing concurrency, Desktop tool handling, model-picker,
and packaging edits were preserved. Host: macOS arm64. Rust/Cargo 1.98.1,
Node.js 24.11.0, pnpm 10.12.3. Node differs from the repository's 22.12.0 pin.

Source inventory: `/tmp/varyai-native-magpie-final-source-manifest-20261110-v3.json`.
It records 1,573 source/configuration files with SHA-256 inventory digest
`a88ed468419cf6ff97ea5f1ca692df03d2735b8d5c076f7a0c4d4aaca064d27d`.
Check logs use `/tmp/varyai-native-magpie-*-20261110.log`.

Initial Rust test compilation and one later complete-check attempt failed
because the host filesystem ran out of space. An affected frontend run also
failed while creating temporary fixture files. Those attempts are not passes.
Only unused generated build artifacts were removed. Rust tests are retried with
`CARGO_INCREMENTAL=0` and `--config 'profile.test.package.varyai.debug=0'` to
reduce build storage while preserving the tests and assertions.

Final renderer and helper checks passed: `pnpm typecheck`,
`pnpm build:renderer`, `pnpm format:check`, `pnpm test:unit` (281 files,
3,588 tests), `pnpm audit --prod`, all eight documented helper suites,
including the five Python recovery tests. The Qoder helper retains its
existing skipped test. `cargo check`, Clippy with `-D warnings`,
`cargo fmt --check`, and `git diff --check` also passed on the final source.

Final Rust tests passed with the storage-reduction options above:

- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,240 passed,
  zero failed, 19 existing ignored tests.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 16 test suites,
  6,403 passed including library and integration tests, zero failed,
  19 existing ignored tests.

An earlier complete library run exposed the old assertion that connecting
beside ChatGPT must replace the current model with a routing group. Magpie
keeps the current native model on join; the updated regression also verifies
that `openai_base_url` points to the local native endpoint. That regression
and both complete final Rust commands passed. Intermediate checks during
concurrent catalog/tool-conversion edits were superseded by these final runs.
The source inventory was compared again after completion and remained unchanged.
The macOS test linker emitted its large-unwind-table warning; it did not prevent
linking or test execution.

No commit, push, remote CI, installed-app replacement, real-account forwarding,
signing, notarization, or release acceptance is claimed. Windows and Linux CI
remain required for cross-platform acceptance.
