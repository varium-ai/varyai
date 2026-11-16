# Codex instruction roles through Chat gateways

## Finding

Identical provider settings do not imply identical upstream requests. A read-only
comparison confirmed that the selected provider's endpoint, credential and Chat
protocol matched its Magpie configuration. No private values were recorded.
Recent VaryAI failures were converted requests rejected with HTTP 400 and the
fixed diagnostic hints `invalid`, `unsupported`, and `messages`.

Small synthetic requests to that same configured upstream isolated the role
restriction. Each request used the same model, credential and endpoint, disabled
streaming, and limited output to 32 tokens. Responses and credentials were never
written to diagnostic output; only status and fixed classifications were retained.

| Synthetic message shape | HTTP result |
| --- | --- |
| Initial system text and a user message | 200 |
| Initial system text as a content-part array | 200 |
| Initial system, developer instruction, then user | 400; message-role rejection |
| Replace that developer role with system | 200 |
| System update after an assistant reply | 400; message-role rejection |
| Keep the update in place as a labeled user reminder | 200; terminal reply present |

These comparisons reproduce the compatibility defect and agree with the recent
sanitized logs. They do not constitute a capture or replay of the user's failed
desktop conversation. An earlier `service_tier` refusal is a separate issue;
matching the generic client error alone cannot establish the failing field.

## Reference and change

The public reference is Magpie commit
[`015860f2da6c78e9f9d2cf1fde9a240681776486`](https://github.com/yetone/magpie/tree/015860f2da6c78e9f9d2cf1fde9a240681776486),
specifically `internal/gateway/responses.go::responseInstructions` and
`internal/gateway/chat.go::buildChat`. Magpie folds initial system/developer
instructions into its system prompt and preserves later updates as labeled
`<system-reminder>` user context.

VaryAI's shared IR Chat builder previously forwarded these roles directly. It
now applies the compatible instruction mapping at Chat serialization. Initial
instructions remain in order in the leading system message. Later updates stay
after the corresponding assistant/tool turn, including any media. Ordinary user
content and tool results are not promoted to instructions. The shared IR remains
unchanged, so native Responses retains its instruction roles. Native forwarding
does not use the Chat builder. The implementation is shared by macOS and Windows.

Two new synthetic regressions failed against the implementation before the fix:
one covers initial and later instructions, and one covers tool history and image
preservation. They also verify that Responses serialization is unchanged. The
existing twelve-protocol-edge HTTP fixture now includes initial and later Codex
developer messages and verifies the actual outbound Chat roles and ordering.

## Verification scope

Checked on macOS 15.3.1, arm64, using Rust 1.98.1, Cargo 1.98.1, Node 24.11.0 and
pnpm 10.12.3. The base revision is
`015521e022f3b850fdb56a2177bbdb419c68575f` plus the working-tree changes. Existing
uncommitted service-tier fallback, Grok diagnostics and associated tests/docs were
preserved and included in the checked tree. The role compatibility change is
limited to the IR builder, its integration fixture and documentation. The
subsequent disconnect UI and thread-batching changes are recorded in
[Codex connection latency](2026-11-14-codex-connection-latency.md).

Final Rust source SHA-256:

- `src-tauri/src/proxy/protocol_bridge/ir/build.rs`:
  `88955fa60e8b996e4c7061453a2b0a1fa0403c27a57a57739bbc48521559198a`
- `src-tauri/src/proxy/protocol_bridge/ir/integration_tests.rs`:
  `edbdd9f7a0c6d4f6b52d2272aacf370d1b2bf6a92dbc6e36f20987ef2ff4d19a`

Completed checks:

- `cargo test --lib --manifest-path src-tauri/Cargo.toml proxy::protocol_bridge::ir::`:
  73 passed, including the twelve-edge HTTP fixture and both new regressions.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm test:unit`: 312 files and 4,182 tests passed on the final renderer changes.
- `pnpm audit --prod`: no known vulnerabilities.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`,
  `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`,
  `pnpm test:qoder-codec`: passed. The Qoder suite retains one skipped installed
  client acceptance test; its two synthetic tests passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7,578 passed,
  21 ignored on the final backend changes.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 7,578 library tests and
  165 integration tests passed; 21 existing library tests ignored.

The full integration-test result is recorded in the connection-latency report.
An earlier full build ran out of disk space before tests could run. Inspected
obsolete incremental snapshots, compiler object intermediates and old test
artifacts were removed before the final rerun; source files, application data
and installed binaries were preserved.

This review does not establish acceptance of the user's Codex conversation,
packaged applications, signing, or notarization. No commit or push is included.
Linux, Windows and remote CI acceptance remain unverified.
