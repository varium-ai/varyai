# Codex Unicode routing identity regression

Base revision: `9804cdb78f509a7b47ce2db90da1ce18adc496c8`, plus the shared uncommitted working tree. No commit, push, application installation or live configuration write was performed for this investigation.

## Report and cause

The user enabled Codex routing, restarted the client, reopened an existing conversation and selected another model. The client reported that `x-codex-routing-hint` could not be converted to a string. Its selected model identity contained non-ASCII provider text.

The catalog previously supported Unicode display labels but also published Unicode provider/model identities unchanged. Reproducing Codex's header conversion in two regression tests failed before the fix. This failure can occur before a request reaches VaryAI and does not establish whether an account token is valid.

The earlier unsupported-restoration refusal and upstream HTTP 503 report remain separate observations. This header fix does not establish their cause or live acceptance.

## Change

ASCII wire identities remain unchanged. Non-ASCII identities receive a deterministic ASCII `vary-provider/alias-<sha256>` identity. Unicode picker labels and the actual upstream model remain unchanged. Retained selections still map to the same canonical target; previous published names remain hidden compatibility entries. Existing collision refusal is preserved.

The two regressions cover header readability, old-selection resolution, retained model capabilities, uniqueness against literal escape text and stable identities after catalog reordering. The implementation is shared Rust code for macOS and Windows.

## Native ingress restoration coverage

The existing real HTTP proxy regression now also publishes a synthetic native Codex model and exercises `/backend-api/codex/responses`. Alongside shared Responses and WorkBuddy AI Chat, it covers JSON/SSE and sensitive first requests/safe follow-ups carrying earlier sensitive history: 12 request scenarios through a Chat upstream passed. The Agent matrix additionally exercises four native Codex requests against a Responses upstream with the same first-request/follow-up and JSON/SSE combinations. Both native upstream paths passed. The upstream must receive restoration tokens rather than synthetic phone/identity plaintext, and the client must receive restored output. Explicit-block and unsupported-conversion assertions remain in place.

The complete five-test HTTP matrix now covers 304 successful request scenarios and 72 unauthorized shared requests. Native Codex uses its separate caller-account admission contract; it is not included in the shared-token rejection count. These are synthetic HTTP requests, not tests performed by installed Agent clients.

The first extended test returned HTTP 503 because its synthetic native runtime had no checkpoint store. The fixture now initializes a temporary checkpoint store, as required by the existing native handler. This fixture omission does not identify the cause of the user's earlier HTTP 503. No production guard was relaxed.

## Verification

Host: macOS 26.6.2 (25G83), Apple Silicon; Node 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1. Rust checks use `CARGO_INCREMENTAL=0`. The isolated rerun uses a separate temporary `CARGO_TARGET_DIR` rather than the application's shared build directory. The [source manifest](2026-11-13-codex-unicode-routing-sources.json) records 66 changed files at the start of that rerun. The four task files (the alias implementation, both HTTP regression files and the Codex catalog guide) still match that manifest.

The shared working tree continued changing during validation. Later versions of `docs/troubleshooting.md`, `src-tauri/src/services/agent_connections.rs`, `src-tauri/src/services/agent_connections/bridge.rs`, `src-tauri/src/services/proxy.rs` and `tests/components/AgentConnectionsPage.test.tsx` differ from the manifest. Results below do not establish acceptance of those newer versions. Full current-tree verification remains incomplete.

| Check                                                                                                                                             | Result                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Unicode regression before/after the fix                                                                                                           | Two expected failures, then two passes                                                            |
| `pnpm typecheck`                                                                                                                                  | Passed; repeated after the first subsequent Agent UI update                                       |
| `pnpm build:renderer`                                                                                                                             | Passed; repeated after the first subsequent Agent UI update                                       |
| `pnpm format:check`                                                                                                                               | Passed; repeated after the first subsequent Agent UI update                                       |
| `pnpm audit --prod`                                                                                                                               | Passed                                                                                            |
| `pnpm test:unit`                                                                                                                                  | 4,062 tests / 304 files passed before later Agent UI updates                                      |
| Six affected Agent UI test files                                                                                                                  | 348 tests passed after the first UI update; the later test-file revision remains outside that run |
| Development, macOS packaging helper, TRAE bridge, default model, WorkBuddy authentication, Doubao check-in, Agent rewards and Qoder codec helpers | Passed                                                                                            |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                                                                             | Five tests passed                                                                                 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                                                                          | Passed in the isolated rerun                                                                      |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                                                                | Passed in the isolated rerun                                                                      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                                                                | Passed in the isolated rerun                                                                      |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                                                                                           | Earlier snapshot: 7,482 passed, 21 ignored                                                        |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                                                                                 | Isolated rerun: 7,483 passed, one failed, 21 ignored; all task regressions passed                 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test '*'`                                                                                      | 165 integration tests passed across 14 targets                                                    |

The remaining full-run failure is `services::agent_connections::tests::disconnect_preserves_each_native_agents_last_model_without_retaining_connection_intent`: the synthetic Claude Desktop remembered selection was absent after disconnect. The connection module is concurrently edited outside this fix. It was not changed here to force that test green.

Earlier shared-target validation lost its running test executable; subprocess-based diagnostics then failed with file-not-found errors. A supplementary build also ended with SIGKILL. Those attempts are not counted as successful checks. In the isolated rerun, the diagnostic/security failures disappeared without changing their assertions. The large-unwind linker warning remained nonfatal.

These are local proxy and synthetic-upstream results, not actual Codex/vendor acceptance. No current Windows/Linux CI, installer or live-account result is established. No tracker record was changed to accepted.

## Manual retest

1. Use a build containing this working-tree change.
2. Refresh or reapply the Codex connection so its catalog contains the ASCII wire identity.
3. Restart Codex, reopen the existing conversation and explicitly reselect the model. Restarting alone does not establish that the conversation stopped using its old saved Unicode identity.
4. Send the synthetic restoration prompt again, then a normal follow-up. Record the header failure and restoration result separately.

Current issue 6 remains awaiting live manual acceptance. Native official targets, specialized account transports and unsupported protocol conversions retain their existing restoration refusal; this change does not enable those paths.
