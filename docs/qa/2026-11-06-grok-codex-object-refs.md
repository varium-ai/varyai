# Grok Codex object-reference rejection

## Scope

- Base revision: `c7f0ce6ac8639c3ad969cd85f3e9bc22ad562ce9`, retaining the
  pre-existing staged and unstaged work.
- This change updates `src-tauri/src/proxy/protocol_bridge/grok.rs`, the Grok
  sections of the protocol-conversion and troubleshooting documentation, and this
  report. It does not commit or stage unrelated work.
- Host: macOS / Darwin 25.6.0 arm64; Rust and Cargo 1.98.1; Node 20.20.2;
  pnpm 10.12.3.

## Reproduction

The remaining rejection was HTTP 400, separate from the earlier HTTP 410 caused
by retired Chat search options. The configured gateway returned only a generic
`invalid_request_error`, with no schema path or detailed validation reason.

The previous temporary capture script accidentally replaced schema objects for
properties named `description` and `title` while sanitizing annotation text.
Its affected schema-isolation results are invalid. The corrected capture replaces
only string annotations, preserves the complete 393 function declarations, and
replaces prompts and annotations with synthetic content before any upstream probe.

With the corrected fixture, four individually isolated object-reference tools
returned 400. Inlining their local references without removing constraints changed
each result to 200. The complete 393-tool request also changed from 400 to 200.
Primitive-reference tools that already returned 200 were not evidence of general
object-reference support.

## Repair

Grok normalization expands local JSON Pointer references within each tool schema,
including object references inside nullable properties and array items. Sibling
constraints are intersected with the target through `allOf`. Expansion has depth
and node budgets and preserves the original schema when references are recursive,
unresolved, external or scoped. Schema traversal does not modify example/default
data or confuse property names with annotation keywords. Other model schemas are
unchanged. Existing regex and null-required normalization remains active.

The new nullable-property/array-item regression failed before implementation.
Existing tests for reference siblings, escaped pointers, boolean schemas and
preserved unresolved references passed after implementation, without weakening
their assertions. An additional test covers bounded expansion.

## Live verification boundary

A real ephemeral Codex CLI conversation, using its configured tools, completed
through a dynamic loopback diagnostic bridge compiled from the updated normalizer
and existing request/stream conversion modules. It sent all 393 function tools,
received HTTP 200 and `response.completed`, returned `OK`, emitted
`turn.completed`, and exited 0.

The bridge substitutes synthetic prompts and annotation text before forwarding.
It does not execute tools and does not validate the complete application's
authentication, DLP or multi-turn tool-history handling. Repository loopback tests
cover the application's routing/conversion path separately. The running desktop
process was not replaced by this diagnostic, so this result alone is not desktop
acceptance.

At the user's request, a separate visible desktop chat was created with the Grok
catalog model and a greeting. That chat failed with HTTP 409
`routing_unknown_continuation` before reaching Grok. Its initial transcript
contains a `create_thread` function output without a `call_id`; the existing
continuation ownership check rejects that shape. This tool-created chat is not
equivalent to manually opening a blank chat. No continuation check was weakened.
Native computer control of the Codex app is unavailable under the tool's safety
restriction, so a manual blank-chat test remains necessary.

## Repository verification

Passed on the changed working tree:

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`.
- `pnpm test:unit`: 279 files, 3,475 tests passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `pnpm test:dev`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec` and
  `python3 -B scripts/tests/recover-shared-v23.test.py`.
- `cargo check --manifest-path src-tauri/Cargo.toml`.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml grok`:
  110 tests passed, including the application loopback regression.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check`.

`cargo test --lib --manifest-path src-tauri/Cargo.toml` completed with 6,040
passed, 1 failed and 18 ignored. The remaining failure is the pre-existing
`provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration` test;
its assertion was not changed. Both previously failing Grok reference tests now
pass. This is not a passing full backend suite.

`cargo test --manifest-path src-tauri/Cargo.toml` reached the same library-test
failure (6,040 passed, 1 failed, 18 ignored), so it did not execute the integration
suite. A separate integration attempt and the first executable build exhausted
disk space. Only this task's completed or failed crate incremental caches were
removed. Subsequent build checks disable incremental compilation to avoid
recreating those caches; source, configuration, credentials and the running
application are retained.

`CARGO_INCREMENTAL=0 cargo build --manifest-path src-tauri/Cargo.toml --bin varyai`
passed. A separate development bundle was prepared at
`/private/tmp/vary-grok-20261106/Vary AI Grok Fix.app` with an ad-hoc signature and
passed `codesign --verify --deep --strict`. This is not Developer ID signing or
notarization. Its executable SHA-256 is
`e97d708cea1bd18dd9bbd8771502d31c214962aefcb22ee23ea686fc4c88963f`.

The user explicitly chose not to restart the running application. The prepared
bundle was not launched or installed, and the existing backend is unchanged.
Desktop acceptance remains incomplete.

The separate integration retry with `CARGO_INCREMENTAL=0` also did not reach test
execution: Cargo reported that it could not create its fingerprint output file
and could not parse the failed compiler process output. The log does not establish
the cause of that final filesystem failure. Integration verification remains
incomplete; no passing integration result is claimed.
Remote Linux, Windows and WSL2 CI, installer signing and release acceptance are
outside this local run. No commit or push is requested.

## Source fingerprints

| Path                                          | SHA-256                                                            |
| --------------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/proxy/protocol_bridge/grok.rs` | `377c08ae4801bdf84521cc5a1416f7c81cd832ed82f905b7e00d3bdd228a1a6e` |
| `docs/architecture/protocol-conversion.md`    | `464418a5126b41f0fc29b31be5527b5fa8473944d23295b8bcdc680374c13a30` |
| `docs/troubleshooting.md`                     | `75d3ec3a4955546d37b2faefaa3dfe47b7f165521671c2028aa0408b1a8e2711` |
