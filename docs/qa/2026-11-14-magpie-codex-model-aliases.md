# Magpie Codex subscription selectors after takeover

## Finding

The reported HTTP 404 at `/intent/v1/responses` contains VaryAI's
`routing_model_unavailable` message. Authentication and endpoint dispatch have
already succeeded at that point; the saved model selector is absent from the
authenticated menu.

A read-only comparison found that the previous selection used Magpie's
`codex/<model>` namespace, while VaryAI published the same exact model under a
subscription provider ID. There was one available subscription candidate for
that selection. Its selected account workspace was also present in Magpie's
Codex logins. Only fixed classifications and counts were retained; credentials,
configuration values, account IDs, model names and conversation contents were
not included in diagnostics.

Magpie's public reference at commit
[`015860f2da6c78e9f9d2cf1fde9a240681776486`](https://github.com/yetone/magpie/blob/015860f2da6c78e9f9d2cf1fde9a240681776486/internal/agent/codex.go)
describes this namespace in `codexOwnViaMagpie` and `codexViaMagpieFor`.
VaryAI migrates conversation provider records independently of saved models.
Consequently, takeover could succeed while an existing conversation continued
to send the old subscription selector and received a local 404.

## Change and boundaries

Codex catalog projection now records a hidden `codex/<model>` alias for a unique,
published direct Codex subscription route. The existing canonical route retains
its account pool, authentication, model policy and forwarding behavior. The
picker continues to show its current provider-qualified entry. Historical
selectors inherit that entry's capabilities.

The compatibility mapping is stored with the existing connection receipt and
retained identity. Refreshing, renaming, reordering, or reconnecting cannot
reassign an established selector to another canonical route. A missing original
route remains unavailable, even if another subscription or API provider offers
the same model. Initial ambiguous subscriptions, routing groups, hidden models,
bare model names and unknown models do not acquire this alias. Explicit
provider identities take precedence when first allocating compatibility aliases.

When no supported mapping exists, the error keeps HTTP 404 and the
`routing_model_unavailable` code, with explicit guidance to select a model in
the current conversation and retry. It no longer suggests creating a new
conversation as the default recovery. Authentication still runs first, and the
message does not disclose the saved selector, account or request contents.

The mapping is prepared during catalog publication, outside the request hot
path. Request forwarding uses the existing immutable model map and model-only
JSON edit. No live client configuration, database or conversation history was
manually modified during diagnosis. The implementation is shared by macOS and
Windows.

## Verification

Before the implementation change, two new synthetic regressions failed because
the historical selector had no retained owner. Ambiguity and explicit-provider
collision controls passed. The error-guidance regression also failed against
the previous message because it did not explicitly direct recovery inside the
current conversation. The HTTP regression uses temporary Codex homes,
temporary/in-memory databases, synthetic credentials and dynamic loopback ports.
It covers legacy and paginated takeover, persisted projection reload, exact
subscription forwarding, numeric metadata preservation, authentication rejection,
source removal and restoration. Existing HTTP rejection tests also require
explicit guidance for the current conversation and retrying.

Base revision: `015521e022f3b850fdb56a2177bbdb419c68575f` plus the cumulative
working-tree changes from the earlier compatibility and latency fixes.
Host: macOS 15.3.1 arm64; Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3.
This follow-up changes the Codex alias projection, the model-unavailable error
message, their regressions, and documentation. Logs and sanitized findings are
retained under
`/tmp/varyai-magpie-alias-20261114/` and the adjacent red/green regression logs.

The final source inventory is recorded in `final/source-before.json` and
`final/source-after.json`. Its canonical SHA-256 is
`3219017bbd085b3d56beb45f674a0a942f79c5ec190860eabea14fb80f87e310`.
The earlier check run was superseded when the error guidance was tightened;
the queued full-test command was stopped while waiting for the build lock.
The final checks include both changes.

Final local checks passed against an unchanged backend source inventory:

- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7,586 passed,
  21 existing ignored tests.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 7,586 library tests and
  165 integration tests passed; the same 21 library tests remain ignored.
- `git diff --check`

Previously passed renderer checks (4,182 tests), helper checks and the
production dependency audit still apply because their inputs are unchanged.
`pnpm format:check` was also rerun successfully during this follow-up. See the
[instruction-role report](2026-11-14-codex-chat-instruction-roles.md) for those
earlier results. Rust test builds retain the existing test-only
`quota_panel::expiring` and macOS linker unwind-table warnings.

Live historical-chat success and remote Linux, Windows, macOS and Windows-to-WSL2
CI acceptance remain unverified. No installed client was launched or replaced
for verification; no commit or push is included. The updated backend must publish
its catalog before an already connected client can use the new compatibility
entry.
