# Codex Responses replay and Magpie refusal recovery

## Scope and reference

A user reported HTTP 409 `routing_unknown_continuation` after Vary AI took over
Codex, including the first message of a newly created conversation. The screenshot
establishes the local error code, not the exact request body. Synthetic fixtures
reproduce the incompatible request shapes without inspecting private prompts,
credentials, or conversation files.

Reference revision: Magpie
[`c7ebb6f8d91693b80f863f9667879060d18820f4`](https://github.com/yetone/magpie/tree/c7ebb6f8d91693b80f863f9667879060d18820f4).
Relevant source:

- [Affinity and encrypted-state cleanup](https://github.com/yetone/magpie/blob/c7ebb6f8d91693b80f863f9667879060d18820f4/internal/gateway/affinity.go):
  `foreignReasoning`, `withoutSeals`, and `withoutCompaction`.
- [Gateway recovery](https://github.com/yetone/magpie/blob/c7ebb6f8d91693b80f863f9667879060d18820f4/internal/gateway/gateway.go#L4191-L4222):
  bounded recovery before output, retrying the same candidate, with native
  compaction and checkpoint protection.
- [Codex backend](https://github.com/yetone/magpie/blob/c7ebb6f8d91693b80f863f9667879060d18820f4/internal/gateway/codex_backend.go):
  backend-specific request shaping and refused-item recovery are separate from
  initial request admission.

## Behavior

Codex Responses previously shared the router's strict local continuation
allowlist. That made missing observations an admission failure, including null
optional fields on a message and reasoning with an empty summary. A new named
conversation did not bypass the later binding and native opaque-state checks.

Codex Responses now uses observed ownership as affinity. Unknown response IDs,
item IDs, tool calls, and opaque state are initially forwarded intact so the vendor
can validate them. A named conversation retains its own identity; differently
named conversations do not import each other's ownership. Unnamed conversations
can find their observed owner from response, call, item, or ciphertext references,
including ordinary sealed message items. Conflicting known owners and collision
tombstones remain errors. Other clients retain strict continuation behavior.

Recovery snapshots remap still-available exact destinations when unrelated group
members change. A missing destination or stale invalid snapshot cannot veto a new
Codex request. Authentication, native account/workspace verification, expiring
leases, revocation, and native checkpoint validation remain enforced.

Direct Responses forwarding now recognizes Magpie's encrypted-state refusal
vocabulary and retries the same destination before releasing any response:

1. Remove reasoning items and nonempty encrypted copies of ordinary items, keeping
   visible messages, tool pairs, and compaction state.
2. If the refusal persists and the request is not native or checkpoint-bound,
   remove sealed compaction history. This loses the earlier compacted context.

The mutation budget is shared across the request and bounded to two stages.
Ordinary conflicts, unrelated errors, and ambiguous network failures do not
trigger this cleanup. Successful HTTP responses and partial successful SSE output
are never replayed by this recovery path. Native compaction/checkpoints retain
their compacted history. Untouched values preserve their original JSON numeric
lexemes through both stages. Upstream refusal bodies and sealed content are not
logged or returned as diagnostics.

This change aligns this admission and direct HTTP refusal path. It does not claim
all of Magpie's provider-specific shaping, refusal caching, named-item recovery,
or recovery from errors embedded in an HTTP 200 SSE stream.

## Regression coverage

- New named and unnamed `/intent/v1/responses` requests with null encryption
  fields, empty reasoning, unknown response/tool/item IDs, and unobserved seals.
- Native `/backend-api/codex/responses` first-message forwarding and exact optional
  field preservation, including explicit `previous_response_id: null`.
- Invalid bearer, wrong workspace, revoked lease, and unchanged upstream-call
  counts on rejected authentication.
- Known owner affinity, conflicting known owners, ordinary sealed-message
  discovery, and destination remapping after membership changes.
- Real synthetic HTTP refusal followed by two retries at the same model and
  endpoint; unrelated conflicts receive no recovery retry.
- Exact refusal vocabulary, two-stage bounds, native compaction protection, tool
  pair preservation, and numeric lexeme preservation.

All fixtures use temporary directories, in-memory databases, dynamic loopback
ports, and synthetic inputs and credentials.

## Verification

Final local verification passed on macOS Darwin 24.3.0 arm64, rustc/Cargo 1.98.1,
Node 24.11.0, and pnpm 10.12.3, on base revision
`0cb8e387fd972ac3f04c0956f8c4b6df5fd64af9` plus working-tree changes.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 281 files, 3,577 tests passed |
| `pnpm format:check` | Passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,363 passed across 16 suites; 19 ignored; no failures |
| Library suite within that full Cargo run | 6,200 passed; 19 ignored; no failures |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `git diff --check` | Passed |

All final Cargo commands used
`--config build.incremental=false --config profile.dev.package.varyai.debug=0`.
The full test link emitted a macOS compact-unwind table size warning; tests passed.
The ignored tests retain their explicit environment/live prerequisites. Frontend
checks emitted existing React test warnings and an unset npm-token configuration
warning, with successful exit statuses.

Final backend logs are under `/tmp/varyai-codex-replay-20261109/`:
`full-tests.log`, `check-final.log`, `clippy-final.log`, and `format-final.log`.
The eight source/test files in this change matched
`accepted-source-manifest.json` after all final checks; manifest SHA-256:
`87ec1e65d129c45ec9373b5015c6ccc884d551749527f5eda1170bb2d853ac5b`.
The later documentation update changes no tested source. Existing unrelated
login, subscription, onboarding, and styling work remains in the checkout.

The initial Rust test build failed because the disk was full; the first frontend
attempt also failed with a disk I/O error. Four explicitly inspected stale Rust
incremental cache directories were removed, preserving source, application data,
and installed applications. Initial intent/library runs exposed superseded Codex
409 assertions and an independently uninitialized capacity-fixture security
runtime. Those assertions and fixtures were corrected before the successful full
run. The final run includes the later ordinary-sealed-message and memory-only
ownership regressions; earlier failed runs are not acceptance evidence.

No application launch, installation, live-account request, commit, push, or remote
CI acceptance is part of this change. Backend logic is shared by macOS and Windows;
local macOS checks do not establish Windows or Linux acceptance. This validates
synthetic new-conversation forwarding and recovery, not the user's installed
application or real-account response.
