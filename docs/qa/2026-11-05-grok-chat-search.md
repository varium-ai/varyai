# Grok Chat search retirement regression

## Scope and reference

- Base: `c7f0ce6ac8639c3ad969cd85f3e9bc22ad562ce9`, with the pre-existing staged
  and unstaged changes retained. This report does not validate another checkout.
- Host: macOS/Darwin 25.6.0 arm64; Rust/Cargo 1.98.1; Node 20.20.2;
  pnpm 10.12.3.
- Reference: yetone/magpie `1b0340fdbca37d4d202f0cecbb7b9d463942ae2b`,
  `internal/gateway/chat.go` (`buildChat`) and `gateway.go` (search dispatch).
  Magpie does not unconditionally put native search options into ordinary Chat
  requests. Its separate search executor was not ported or claimed here.

## Reproduction and repair

An ephemeral Codex CLI conversation selecting the existing Grok catalog entry
failed with HTTP 410 and `routing_upstream_rejected`. Basic direct Chat requests,
including a simple function tool, returned 200 but did not reproduce Codex's
complete tool declarations.

A second CLI run used an isolated temporary `CODEX_HOME`, a dynamic loopback
capture port, and a synthetic prompt, with no real workspace context or MCP
configuration. Its Responses request included native `web_search`. The exact
request parser, Chat builder and Grok schema normalizer were compiled into a
standalone diagnostic harness. The resulting Chat request returned 410. The same
prompt and function tools returned 200 with only `web_search_options` removed;
shortening the prompt while retaining that field still returned 410.

The Chat builder now omits optional native search for Grok models, including
provider-prefixed model names. Function tools and schemas, named/required tool
choices with remaining functions, reasoning and streaming remain unchanged.
Search-only `required` fails locally; optional tool choices and parallel flags are
omitted when no tools remain. Other models and native Responses search remain
unchanged. Native Grok search requires a Responses-capable upstream, not this
retired Chat field.

The regression tests failed before the implementation. The new loopback routing
fixture explicitly returns 410 for `web_search_options` and now passes for JSON
and real upstream SSE, including strict downstream sequence numbers and terminal
completion. The pre-existing schema tests and assertions were not weakened.

## Verification

Passed against the final source changes:

- `pnpm typecheck`
- `pnpm build:renderer`
- `pnpm format:check`
- `pnpm test:unit`: 279 files, 3,475 tests passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `pnpm test:dev`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:qoder-codec`, and `pnpm test:agent-rewards`.
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- Focused Grok tests: the two new builder tests, the new loopback regression, and
  the pre-existing sanitized-rejection test passed. The separate existing schema
  normalization test still fails on its unchanged local `$ref` assertion.
- Formatting of the changed documentation and `git diff --check`.

A fresh real Codex CLI conversation also completed through a dynamic loopback
harness built from the changed request builder, original request parser/schema
normalizer, and original Chat-to-Responses stream modules. It sent the full CLI
function declarations to the configured Grok upstream, received HTTP 200 and
`response.completed`, returned `OK`, and exited 0. The harness asserts if a tool
is called; it validates a text-only turn, not multi-turn tool execution. Its
minimal context shim is not the full app's authentication, DLP, history or
routing layer. The in-repository loopback regression covers the full local
routing/conversion path with synthetic credentials and responses instead.

Incomplete or failing checks:

- The initial `cargo test --lib --manifest-path src-tauri/Cargo.toml` run had
  6,035 passed, 3 failed, and 18 ignored before the independent new loopback
  test was added. The three failures are in unchanged schema-reference/notes
  behavior: `provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration`,
  `codex_grok_chat_normalizes_tool_schema_patterns`, and
  `grok_local_schema_refs_expand_without_losing_constraints_or_data`.
- Final run of the compiled library-test binary: 6,036 passed, the same 3 failed,
  18 ignored. This includes the new independent JSON/SSE loopback test. No
  failing assertion was removed or relaxed.
- `cargo test --manifest-path src-tauri/Cargo.toml` could not finish building the
  additional library archive: `No space left on device`. A follow-up
  `cargo test --manifest-path src-tauri/Cargo.toml --test '*'` also failed to write
  its incremental query cache. Only the failed incremental directory newly
  created by those attempts was removed; application databases, installed apps,
  older caches and user files were not touched.
- Rust test linking reports the existing oversized `__eh_frame` warning.
- No Linux, Windows, WSL2, signing, installer, remote CI or desktop acceptance is
  claimed. No commit or push was made.

## Running application boundary

The running VaryAI app was not replaced or restarted, and live model/credential
configuration was not modified. The source fix and isolated CLI success therefore
do not establish that an already running desktop process contains the fix. A
corrected backend build and explicit restart are needed before repeating the
user's desktop conversation.

## Final source fingerprints

| Path                                              | SHA-256                                                            |
| ------------------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/proxy/protocol_bridge/ir/build.rs` | `9f55c2c6487bec7b4a7350abec182b252236f097afed295d89e6ad76bd2b94d8` |
| `src-tauri/src/proxy/intent/global_tests.rs`      | `70a876f9160c09d2f21629a72c0a6b2f68a8123b19ccf9b651e9e233924a144f` |
| `docs/architecture/protocol-conversion.md`        | `ea520285fab8e87053c7d43440e64932f1f387d876f749b8540742d5dc7b3e41` |
| `docs/troubleshooting.md`                         | `2fecbec6576408d8fc26aa62cb3ee550c1302f034b11895202f3a88ac73b4aaf` |
| `third_party/routing-reference/NOTICE.md`         | `121839eac7e1e49b56e6b366e90fa9984704c5537790b426aeffd38f2cadb9d1` |
