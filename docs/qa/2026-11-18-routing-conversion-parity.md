# Routing conversion parity

## Scope and reference

The reference is Magpie revision `f9492fd8ed2c6fd3c65f1dab00062771b551d7b5`,
`internal/gateway/responses.go` (`readResponsesFor`, allowed-tool filtering) and
`internal/gateway/gateway.go` (`translate`, request errors versus forwarding
errors). This change addresses a reproducible rejection of Responses
`tool_choice.type = allowed_tools` and separates safe conversion diagnostics.
It does not establish the cause of the remotely reported GLM conversation.
Raw Magpie error-string passthrough is not adopted: diagnostics must not expose
credentials, tool arguments, prompts, provider configuration or upstream bodies.

## Pre-implementation coverage matrix

All rows apply to macOS and Windows. Shared execution does not establish native
acceptance. Client-rendered error messages change; the VaryAI renderer and
connection toggles are unchanged. Disabled routing retains the native path.
Allowed-tool handling applies only when a Responses request is converted;
same-protocol byte-preserving forwarding must remain unchanged.

| Agent | Entry / native adapter | Runtime / UI | Verification planned |
| --- | --- | --- | --- |
| Claude Code CLI | Shared Messages ingress | Classified conversion error / client message | Shared protocol tests; native pending |
| Claude Desktop | Desktop Messages normalization | Same; native normalization unchanged | Shared protocol tests; native pending |
| Codex CLI | Responses routing | Allowed tools and classified error / client message | Parser, builder, HTTP regressions; native pending |
| Codex Desktop | Native Codex Responses ingress | Same; native checkpoint rules unchanged | Shared Responses regressions; native pending |
| Gemini CLI | Native Gemini ingress | Classified conversion error / client message | Shared protocol tests; native pending |
| Grok Build | Shared gateway | Same | Shared protocol tests; native pending |
| OpenCode | Shared connection gateway | Same; Responses restrictions when used | Shared protocol tests; native pending |
| OpenClaw | Shared connection gateway | Same | Shared protocol tests; native pending |
| Hermes CLI | Shared connection gateway | Same | Shared protocol tests; native pending |
| Pi CLI | Pi connection gateway | Same | Shared protocol tests; native pending |
| WorkBuddy CN CLI / Desktop | Shared gateway and normalization | Same; normalization unchanged | Shared protocol tests; both native clients pending |
| WorkBuddy International | Separate connection identity | Same | Shared protocol tests; native pending |
| Qoder CN | Native adapter and shared gateway | Same | Shared protocol tests; native pending |
| Doubao Work | Dedicated connection / native adapter | Shared execution only if reached; dedicated handling unchanged | Native adapter unverified |
| TRAE CN | Dedicated bridge | Shared execution only if reached; dedicated handling unchanged | Native adapter unverified |
| TRAE International | Separate dedicated bridge | Same | Native adapter unverified |
| Kimi CLI / Desktop | Native adapter and shared gateway | Classified conversion error / client message | Shared protocol tests; both native clients pending |
| Qwen CLI | Shared gateway | Same | Shared protocol tests; native pending |
| ZCode Desktop | Native adapter and shared gateway | Same | Shared protocol tests; native pending |
| DeepSeek Harness | Native adapter and shared gateway | Same | Shared protocol tests; native pending |
| T3 Code Desktop | Native adapter and shared gateway | Same | Shared protocol tests; native pending |

## Verification record

The working tree based on `8df4a222` was checked on macOS 15.3.1 arm64 with
Rust 1.98.1, Node 24.11.0 and pnpm 10.12.3. The checkout also contains unrelated
in-progress changes. The regressions use synthetic content, in-memory storage
and dynamic ports.

- The five `magpie_allowed_tools` tests passed. They cover named and namespaced
  selection, legacy Codex Chat and Messages conversion, empty and malformed
  lists, prior tool pairs, native Responses forwarding, and HTTP 400 versus 502
  with sanitized error bodies.
- The protocol bridge suite passed all 94 tests.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` and `pnpm test:unit`
  passed. The frontend test count was 4,640.
- `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings` and
  `git diff --check` passed.
- The sandboxed `cargo test --lib` attempt encountered denied local socket
  operations. The complete `cargo test` run with loopback access passed
  7,780 library tests and 165 integration tests, with 21 ignored.

Linux and Windows execution, native Agent self-tests and remote CI remain
pending. The reported GLM conversation still needs confirmation on the affected
installation.
