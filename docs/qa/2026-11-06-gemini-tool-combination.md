# Gemini search and function combinations from Codex

## Evidence and correction

The new failing Codex conversation used Gemini on its first turn and received an
upstream HTTP 400. It contained no preceding tool or reasoning history, unlike
the earlier `409 routing_unknown_continuation` report. Only request shapes and
sanitized error categories were inspected; private prompts and credentials are
not included in this record.

A synthetic greeting sent directly to the configured Gemini-native upstream
succeeded without tools. The same prompt with Codex's full tool declarations
failed with HTTP 400 because the upstream required
`tool_config.include_server_side_tool_invocations` to combine built-in search
and function calling. The captured CLI shape flattened to 396 functions plus
Google Search. Tool descriptions and user content were replaced with synthetic
fixtures before forwarding or retaining diagnostic requests.

The Gemini builder now enables `toolConfig.includeServerSideToolInvocations`
for mixed tools and uses `VALIDATED` for automatic or unspecified function choice.
Disabled, required and named function choices preserve their constraints; schemas
and both tool categories remain present. Function-only and search-only behavior
is unchanged. Google's [tool combination guide](https://ai.google.dev/gemini-api/docs/tool-combination)
and [GenerateContent reference](https://ai.google.dev/api/generate-content)
describe the server-tool context and response parts.

Live search also exposed `toolCall` and `toolResponse` parts that the previous
reply reader rejected. The adapter now recognizes Google web/image search as
server work already completed, renders completed search items to Responses, and
keeps the native parts/signatures in provider/session-scoped shadow history.
Restoration requires exact, unambiguous visible history; another provider,
another session or changed history cannot obtain the saved native state.
Unsupported server tool kinds still fail rather than becoming executable client
functions. Native state is not exposed as client-visible metadata.

## Regressions and live checks

All four new regressions failed before implementation. They cover mixed tool
configuration and explicit choices, native response round-trip, Responses stream
completion, and scoped native-history restoration. Synthetic fixtures include
signature sentinels to verify that Responses output does not leak them. The
focused protocol suite then passed these regressions; its separate existing HTTP
integration fixture failed at process-level rustls CryptoProvider initialization.
Complete-suite results are recorded below rather than treating the filtered run
as acceptance.

A temporary dynamic-loopback bridge compiled the actual updated request/reply
and streaming IR modules, accepted a real Codex CLI request, and forwarded its
sanitized shape to the user's configured Gemini-native upstream. The bridge
retained all 396 flattened function schemas and native search, used the actual
streaming endpoint, and transcoded the upstream stream back to Responses. The
upstream returned HTTP 200, Responses emitted `response.completed`, the CLI
returned `OK`, emitted `turn.completed`, and exited 0. A second real CLI run
requested native Google Search with the same full tool set. It returned HTTP 200, completed search items, an official documentation URL,
`turn.completed`, and exit 0, without a client function call. A custom-function
probe returned a signed native function call. These probes do not establish live execution of every function.

The temporary bridge used a dynamic port and synthetic input; it did not replace
the running gateway, edit account configuration, or restart the Codex daemon.
Live probes establish this configured upstream's behavior, not acceptance by
every Gemini model or relay.

## Working-tree verification

Base revision: `c7f0ce6ac8639c3ad969cd85f3e9bc22ad562ce9`, with existing staged and
unstaged work preserved. Host: macOS Darwin 25.6.0 arm64; Rust 1.98.1, Node 20.20.2,
pnpm 10.12.3. Cargo uses `CARGO_INCREMENTAL=0`.

| Command                                                               | Result                                                                   |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `pnpm typecheck`                                                      | Passed                                                                   |
| `pnpm build:renderer`                                                 | Passed                                                                   |
| `pnpm test:unit`                                                      | 3,475 passed in 279 files                                                |
| `pnpm format:check`                                                   | Passed                                                                   |
| Six Node helper suites in `docs/development.md`                       | 95 passed                                                                |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                 | 5 passed                                                                 |
| `pnpm audit --prod`                                                   | No known vulnerabilities                                                 |
| `cargo check --manifest-path src-tauri/Cargo.toml`                    | Passed                                                                   |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`              | Passed                                                                   |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`    | Passed                                                                   |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`               | 6,052 passed, 1 existing failure, 18 ignored                             |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast -j 1` | Incomplete acceptance: artifacts disappeared during execution; see below |
| `cargo build --manifest-path src-tauri/Cargo.toml --bin varyai -j 1`  | Passed on retry                                                          |
| `git diff --check`                                                    | Passed                                                                   |

The library failure remains
`database::routing_upstreams_compat_tests::provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration`
with `routingUpstreams.invalid`, as recorded before this Gemini change. The
filtered protocol HTTP fixture passed in the full library run; its isolated
CryptoProvider initialization failure is not a Gemini assertion failure.

The first full Rust test/build attempt exhausted local disk space during linking
(`errno=28`). Regenerable incremental cache and its obsolete object files were
removed; source, account databases and the running application bundle were left
intact. The full test retry used `-j 1` to bound concurrent linking. During execution,
already-built test executables disappeared: integration targets could not start
with `No such file or directory`. That run reported 6,009 library passes, 44
failures and 18 ignored; additional failures reported missing files or unavailable
runtime state, while all four Gemini regressions still passed. The exact removal
mechanism was not established. This run does not establish integration-test
acceptance. The earlier completed library run remains recorded separately above.
The binary build subsequently passed. No assertions or checks were disabled.

SHA-256 fingerprints of the checked source files:

```json
{
  "src-tauri/src/proxy/protocol_bridge/ir/build.rs": "945f80dff990bfa27f49ce7c75724020b7157c6699d66ea7770889bcb35698b9",
  "src-tauri/src/proxy/protocol_bridge/ir/continuity.rs": "c52b9adbf838e1beca9e411594bfa6517ad5a8261962d9ba8f5ce9aa23981666",
  "src-tauri/src/proxy/protocol_bridge/ir/mod.rs": "76acbed010940b744b8fd4f9b1716c1177507d5749cb7a2efd63e3e5f10daf88",
  "src-tauri/src/proxy/protocol_bridge/ir/parse.rs": "8d3822bc5e008152e64b06410352232c9a1c9f38501ed6001bd6a370ef2f22ee",
  "src-tauri/src/proxy/protocol_bridge/ir/reply.rs": "efc504eaef193506adb451d301870258575e696275826e0527307a494e84bfc9",
  "src-tauri/src/proxy/protocol_bridge/ir/stream.rs": "30f436b8ec2b655b8edafe4c5712cca74346093a2f5d9b437b7032330b33dd95"
}
```

## Runtime boundary

The user first restarted development with `pnpm dev --no-watch` before this
change was finished. Source edits and building do not reload that process. The
user then manually relaunched VaryAI and confirmed that the model test now works.
The active application process started at 14:42 on November 6 (Asia/Shanghai), and
its executable contains the new combination field. The agent did not restart or
replace the active application.

A later interactive CLI request at 14:47:35 still received an upstream 400. Its
request carried 408 functions, compared with 397 in fresh ordinary CLI checks.
The initial successful `exec` probes therefore did not cover that interactive
shape. Subsequent capture with the native OpenAI provider, low reasoning effort,
and all 408 functions completed through the running gateway with HTTP 200 and
`response.completed`. The independently converted full streaming request also
returned HTTP 200. Replaying the failed first turn's message structure in memory
with the captured tool set succeeded through both paths. Original conversation
text was not written to diagnostic fixtures or reports.

The user then retried successfully without another code change, configuration
change or application restart during this follow-up. This establishes current
interactive availability, not elimination of intermittent failures. The earlier
400 retained only a generic rejection code, so its exact upstream cause remains
unconfirmed; tool count alone is not established as the cause.

## Commit verification boundary

The user explicitly requested committing the staged changes and pulling the
tracked remote branch without rerunning tests. No additional test or build run
is part of that operation. The results above apply to the recorded pre-merge
working tree and do not validate subsequent merged changes. Complete backend
verification remains incomplete for the reasons above. No installer verification,
push or cross-platform CI run was performed for this record.
