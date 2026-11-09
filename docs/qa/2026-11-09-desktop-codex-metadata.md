# Claude Desktop to official Codex: rejected metadata

## Diagnosis

Claude Desktop repeatedly retried a request that reached the official Codex
endpoint and received HTTP 400. The routing history recorded upstream rejection
followed by the local `502 routing_upstream_rejected`. The local gateway listener
and `/health` were healthy.

Controlled requests used synthetic greetings and metadata with existing
credentials held only in process memory. No credentials, account identifiers,
private configuration, user prompts, or upstream error bodies were printed or
saved in this record.

| Path | Synthetic variation | Result before the fix |
| --- | --- | --- |
| Direct official Codex | Greeting without metadata | HTTP 200 and `response.completed` |
| Direct official Codex | Same greeting with `metadata.user_id` | HTTP 400 |
| Direct official Codex | Greeting with maximum reasoning effort | HTTP 200 and `response.completed` |
| Existing local Desktop connection | Greeting without metadata | HTTP 200, text deltas and `message_stop` |
| Existing local Desktop connection | Same greeting with `metadata.user_id` | HTTP 502 and `routing_upstream_rejected` |

This isolates a conversion defect without changing the selected model,
credentials, client configuration, history, or security settings. It does not
prove that every field in a full Desktop request is accepted.

## Magpie comparison and correction

Reference: `yetone/magpie` revision
[`8c733bb36232e04d74a47ff2b3e05f71c3c6fa7b`](https://github.com/yetone/magpie/tree/8c733bb36232e04d74a47ff2b3e05f71c3c6fa7b).
Its `internal/gateway/ir.go` explicitly scopes `Metadata` to Anthropic upstreams;
`anthropic.go` restores that field, while `responses.go` does not project it.
Responses `client_metadata` is a separate field.

Vary AI's shared Responses builder preserves metadata for general API
compatibility, but its official Codex conversion previously passed that field to
an endpoint that rejects it. The correction removes `metadata` only in the
official conversion adapter, alongside that adapter's existing unsupported
sampling/output fields. Native Anthropic forwarding and other Responses
providers retain their existing behavior. The change is platform independent
and applies to macOS and Windows through the same forwarding path.

This is a confirmed gap in Desktop inference compatibility. Previous browser
sign-in alignment does not establish complete Magpie parity or successful
Desktop inference.

## Regression and verification

The regression uses an in-memory database and dynamic local ports. Its upstream
rejects metadata, matching the observed official contract. Both streaming and
non-streaming Desktop requests include synthetic metadata, a function schema,
instructions, and explicit reasoning effort. Assertions require a complete
answer, the correct completion marker, and retained tool schema, instructions,
and effort.

Before the implementation change, the regression failed with HTTP 502 instead of
200. The test compiled successfully; this was an assertion failure, not a build
failure. A linker unwind-table size warning was also emitted.

Verification context:

- Date: November 9, 2026.
- Checkout: `/Users/guitar/projects/varyai-open-source`.
- Base revision: `0cb8e387fd972ac3f04c0956f8c4b6df5fd64af9`, with pre-existing
  concurrent working-tree changes retained.
- Host: macOS Darwin 24.3.0, Apple Silicon.
- Toolchain: rustc/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3.
- Check logs: `/private/tmp/vary-desktop-metadata-checks/`.
- Before-fix test log: `/private/tmp/vary-desktop-metadata-red.log`.
- Focused Rust source fingerprints:
  `/private/tmp/vary-desktop-metadata-checks/checked-source-hashes.json`.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 281 files, 3577 tests passed |
| `pnpm format:check` | Passed |
| `pnpm test:dev`, `pnpm test:trae-bridge`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec` | All passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed on the final implementation |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml workbuddy_official_tests` | 12 passed, including both Desktop response modes and tool continuations |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6179 passed, 18 explicitly ignored |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 16 suites, 6342 passed, 18 explicitly ignored |

The two focused Rust source fingerprints remained unchanged through these
checks. Results cover this local run with the recorded concurrent working tree;
they do not establish cross-platform CI acceptance.

The successful live requests above diagnose the running pre-fix backend; they
are not post-fix acceptance of the original Desktop conversation. No application
was manually launched, installed, replaced, or restarted. No commit, push,
remote CI, Windows/Linux execution, signing, or installer acceptance is claimed.

During later checks, the existing development runner had restarted its backend,
and the active `claude-desktop.json` connection receipt was absent. The planned
post-fix live probe stopped before sending a request. No connection was recreated
or client configuration rewritten; live Desktop acceptance remains incomplete.
