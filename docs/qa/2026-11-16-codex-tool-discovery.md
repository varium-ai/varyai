# Codex tool discovery and incomplete greetings

## Observations

The user reproduced `Incomplete response returned, reason: max_output_tokens`
after VaryAI takeover with a new Codex conversation containing only a greeting.
A bounded loopback diagnostic retained no packet files or message bodies. It
reported only allowlisted parameter counts, numeric limits and terminal events:

- The failing main request contained 437,103 bytes, 259 function definitions
  across 28 top-level tool declarations, and high reasoning effort.
- The client supplied no output-token cap. The response ended with
  `response.incomplete`, `max_output_tokens`, no text, and zero-valued accounting.
  Missing accounting does not establish actual upstream token consumption.
- The same selected model's VaryAI catalog declared `supports_search_tool: false`;
  its Magpie catalog declared `true`.
- Smaller synthetic requests succeeded. A synthetic request with 259 short tool
  definitions also succeeded, so this evidence does not establish a hard limit
  of 259 tools or independently prove numerical token exhaustion.

The diagnostic has stopped. Credentials, private configuration, tool inputs and
conversation content were not written to the report or synthetic fixtures.

## Correction

The routing catalog inherited a conservative native Responses template even for
routes with client tool-search conversion. Codex consequently included every
connected tool definition on each turn. The shared and temporary catalogs now
advertise discovery when all eligible targets use supported Chat, Anthropic or
Gemini conversion, or official Codex execution. Unknown native Responses and
subscription routes retain the conservative setting.

Both the shared IR and the legacy Codex conversion now preserve the client's
tool-search description and parameter schema. The previous implementation
replaced those with a generic description and schema, losing the tool index and
client constraints. Omitted definitions retain the existing fallback. Ordinary
output limits and incomplete-response semantics are unchanged.

Reference: Magpie revision `58f24cc23527776b5f1958cfd4a8092a659bdfb5`,
`internal/codexcat/codexcat.go`, `internal/gateway/responses.go` and
`internal/gateway/toolsearch.go`.

## Verification

The three new regressions first failed on the disabled capability and replaced
discovery definitions. The existing HTTP tool-search and loaded-tool round trip
passed in that same run. Tests use synthetic definitions, temporary catalogs and
existing isolated forwarding fixtures.

The host is macOS on Apple Silicon, with
Rust/Cargo 1.98.1, Node 24.11.0 and pnpm 10.12.3. The base revision is
`f9bbcb16b3f7d1e2d4b6ca8ce48840e245ca7e4f`; pre-existing WorkBuddy changes remain
in the working tree. Check records are in
`/tmp/varyai-codex-tool-search-checks`.

The SHA-256 of sorted changed Rust paths and contents, separated by NUL bytes, is
`5255b82669fccb8102a8f0626fe79e24645ff314ab986e9459fb878861732e1d`.
The pre-existing WorkBuddy source digest remains
`fac97411a2a5eeaf3a1aa5efb168e5dcbe0e81976bc871d058a06e1d2e9969e6`.

| Check                                                                              | Result                                                     |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| Focused discovery regressions and HTTP round trip                                  | 4 passed                                                   |
| `pnpm typecheck`                                                                   | Passed                                                     |
| `pnpm build:renderer`                                                              | Passed                                                     |
| `pnpm test:unit`                                                                   | 317 files; 4,225 tests passed                              |
| `pnpm format:check`                                                                | Passed                                                     |
| `pnpm audit --prod`                                                                | No known vulnerabilities                                   |
| Development, DMG, TRAE, model-default, WorkBuddy, Doubao, reward and Qoder helpers | Passed; one existing Qoder installed-client skip           |
| Shared-v23 recovery helper                                                         | 5 passed                                                   |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                 | Passed                                                     |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                            | 7,611 passed; 21 ignored                                   |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                           | Passed                                                     |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                 | Passed                                                     |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                  | 7,611 library and 165 integration tests passed; 21 ignored |
| Changed-document formatting and `git diff --check`                                 | Passed                                                     |

Initial final-build attempts exhausted disk space while writing Rust output.
Regenerable library incremental directories and obsolete object files from the
previous test executable were removed. Accepted backend checks disable
incremental compilation with `CARGO_INCREMENTAL=0`. The full-test retry is
recorded in `backend-5-retry.log`; the earlier disk-space failure in
`backend-5.log` is not accepted as a pass. No installed bundle or user data was
removed. The accepted checks use unchanged Rust sources.

No remote Linux, Windows or macOS CI, installer, signing, or release acceptance
is claimed.

## Live acceptance

The development instance interrupted by the disk-space failure was recovered
with `CARGO_INCREMENTAL=0 pnpm dev`. It used the existing VaryAI data directory,
returned HTTP 200 from its health endpoint and published the selected model with
`supports_search_tool: true`.

The user then created a new Codex conversation with the same selected model and
confirmed that the greeting received a normal reply. The bounded diagnostic
observed two completed main requests after the correction:

| Measurement                  | Before                   | First successful request after correction |
| ---------------------------- | ------------------------ | ----------------------------------------- |
| Request bytes                | 437,103                  | 75,158                                    |
| Expanded tool declarations   | 259                      | 14, including client tool search          |
| Client output cap            | Absent                   | Absent                                    |
| Reasoning effort             | High                     | High                                      |
| Terminal event               | `response.incomplete`    | `response.completed`                      |
| Response text characters     | 0                        | 14                                        |
| Reported input/output tokens | Unusable zero accounting | 15,746 / 13                               |

The second main request contained 75,182 bytes and the same 14 tool declarations;
it also completed with text. This establishes live acceptance for the reported
model and greeting flow. It does not establish that every upstream supports
every native tool type. The diagnostic was explicitly stopped after acceptance.
