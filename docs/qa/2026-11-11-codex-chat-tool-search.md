# Codex client tool discovery on Chat upstreams — 2026-11-11

## Investigation and scope

The reported desktop conversation returned `routing_conversion_unsupported` for
a custom model but worked after selecting a native Codex model in the same chat.
Read-only routing history identifies the failed target's configured protocol as
Chat Completions. The Responses-to-Chat route returns this fixed error for local
conversion failures. No credentials, prompts, raw tool inputs or private source
configuration are included in this report.

The current base is `7a00b1869bce94b0c341f0f199f9a8c6993923aa`, with preceding
route-recovery and desktop-restart changes. Those changes did not alter the
protocol converter. Unrelated daily-check-in work is preserved.

Git traces the IR's explicit rejection of `tool_search_call` and
`tool_search_output` to `554ff7b33781be2b915b3ca24b3ec1379446aded` on November 4,
2026, titled `feat: unify protocol conversion and improve agent integrations`.
The Git author field is `baomi`; this does not identify which person or coding
agent performed the edit. The existing specialized Codex converter already
handles client-side discovery and loaded namespaces. The shared IR lacks that
compatibility. This code gap predates the current restart/reconnection changes.

Historical routing records include a successful request to the same upstream and
model on November 5. Success is conditional on the request's protocol features;
it does not prove that every request shape was supported. Available failed-request
history does not retain the original wire body or the exact internal rejection.
The synthetic regressions establish a reproducible compatibility gap, rather
than claiming that the private original payload has been replayed.

## Verification

Host: macOS 26.6.2 arm64, Rust/Cargo 1.98.1, Node.js 22.12.0 and pnpm 10.12.3.
Tests use in-memory storage, temporary client homes, synthetic credentials and
dynamic loopback ports. Logs are in `/tmp/vary-tool-search-verification/`.

The initial unit regression fails with `Invalid tool choice` for an explicit
client-side tool-search choice. An initial HTTP fixture run rejected its
unpublished local menu with 401; this setup failure is not counted as a bug
reproduction. After publishing its menu, the HTTP regression reaches the actual
`/intent/v1/responses` ingress and reproduces HTTP 400 on the first turn, before
the synthetic Chat upstream can answer (`http-red-admitted.log`).

## Implementation

The shared IR retains native tool-search calls and results, including call IDs,
execution metadata and loaded declarations. Portable builders expose the same
query/limit function schema as the existing specialized converter; they do not
use another vendor's request format as an intermediate. Returned namespace
declarations are lifted for following turns without adding phantom messages.
The existing response context restores client tool-search calls and namespaced
calls in JSON and SSE replies. Native Responses execution remains unchanged.
Server-executed discovery is refused across protocols instead of being relabeled
as client execution. These platform-independent changes apply to macOS and Windows.

The HTTP regression covers a fresh request, a tool-search result carrying a
namespace, a returned namespaced call, and the next tool-result turn. Each runs
with buffered and streamed Responses replies. It checks outbound model and
authentication, preserved call IDs and security inspection before conversion.
Unit regressions also cover Anthropic/Gemini targets, explicit discovery choice,
native-item preservation and refusal of server-side discovery conversion.

The first broad bridge run passed the new HTTP round trip but exposed a unit
fixture that omitted Gemini's URL-selected model and a classic HTTP fixture that
did not install the application's explicit Rustls ring provider. Both fixture
setups were corrected; assertions were retained. The latter mirrors production
startup in `lib.rs` when dependencies expose multiple crypto providers.

## Final checks

| Command or contract                                                                                                            | Result                                   |
| ------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------- |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                                                       | Passed                                   |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml`                                                                   | Passed                                   |
| `cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings`                                                   | Passed                                   |
| `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml proxy::protocol_bridge:: -- --test-threads=1`                 | 80 passed                                |
| `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml proxy::providers::transform_codex_chat:: -- --test-threads=1` | 120 passed                               |
| `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`                                                                   | Passed                                   |
| `pnpm test:unit`                                                                                                               | 3 failed, 3,848 passed; 292 files passed |
| `pnpm audit --prod`                                                                                                            | Passed                                   |

The renderer failures remain the recorded authorized-model-save cases for
WorkBuddy, WorkBuddy AI and Qoder. No restart interface or localization changes
were made for this compatibility fix. Native Windows/Linux/WSL2 CI, the original
private conversation and installed-application acceptance remain unverified.
No commit, push, installation or live desktop restart was performed by this task.

The required `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml`
again aborts in the previously recorded
`services::proxy::tests::trae_reconnection_delivers_latest_offline_selection_without_enabling_audit`
fixture. Its fake bridge rejects an operation and then panics during destructor
cleanup. The fixture and its assertions were not modified or disabled by this
fix. This remains a verification gap, not a passing full library check.

`cargo test --offline --no-fail-fast --manifest-path src-tauri/Cargo.toml`
also reports the same library abort and the previously recorded
`profile_bundle::trae_profiles_deliver_when_connected_without_binding_preview_to_runtime`
fake-bridge abort. All other test programs pass. Final command outcomes are in
`verified-results.json`; `verified-before.json` and `verified-after.json` confirm
that the checked backend sources remained identical throughout this final run.
`git diff --check` and changed-document Prettier checks pass. Remote CI and live
inference acceptance remain incomplete.
