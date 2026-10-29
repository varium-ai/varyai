# xAI JSON response wire-buffer accounting: execution specification

## Approval and decision

Audience: human contributors and an implementing agent. This is the concrete five-path extension approved by the user on 2026-10-29 after the previous approval checkpoint timed out. The user subsequently explicitly authorized implementation by child agents. One implementation agent exclusively owns the three code/test paths below; the parent owns these two documents and performs read-only investigation of other blockers. No overlapping writes or parallel Cargo compilation.

Goal: charge the independent serialized JSON wire buffer created by the xAI native Responses rewrite handler, retain that reservation for the buffer's lifetime, and fail closed at that boundary when the shared payload budget cannot admit it. This is a limited resource repair, not whole-resource or submission acceptance.

## Scope and source

HEAD: `4dbfc90d1ff495c04b95bb04962bde57df799d82`. Starting identity: `/tmp/vary-dlp-remaining-final-identity.json`, SHA-256 `0cda90d5b3195e07fc8c85d09d0ce1e0e66176bf4be0ea550a7337070b342d6e`. Preflight confirmed all 71 pending paths, protected/context hashes, HEAD and empty index unchanged. Existing dirty work must be preserved.

Only these five paths may change:

| Path | Responsibility |
| --- | --- |
| `src-tauri/src/proxy/handlers.rs` | Replace xAI nonstreaming rewrite serialization for active leases; narrowly scoped test-only RAII boundary seam if needed. |
| `src-tauri/src/proxy/security/http_restoration.rs` | Expose active-lease response encoding using the existing fallible budget-owned encoder; no authority expansion. |
| `src-tauri/src/proxy/security/reversible_integration_tests.rs` | Actual loopback HTTP boundary regressions and success/lifetime/legacy controls. |
| `docs/design/security-dlp-response-wire-execution-spec.md` | This executable specification. |
| `docs/design/security-dlp-response-wire-closure.md` | New final evidence and separate acceptance decisions. |

Current behavior: `handlers.rs::handle_codex_xai_native_responses_rewrite` verifies original JSON completion, restores native namespace names, applies existing xAI whole-float argument normalization, then uses uncharged `serde_json::to_vec` at the nonstreaming output boundary (initial lines 1601-1606). The active attempt is already available. `http_restoration.rs::DlpPreparedAttempt::encode_outbound_body` and `encode_outbound_with_budget` (initial lines 210-264) demonstrate two-pass encoding with a `PayloadBuffer` reservation carried by `Bytes::from_owner`. The final restoration wrapper runs afterward; its checks must not be mistaken for this earlier serialization boundary.

## Design and invariants

Add a narrowly named response-encoding entry point on the attempt, returning no new owner for an empty lease and budget-owned bytes for a nonempty lease. Reuse the existing encoder rather than add another serializer or guessed amplification factor. Keep the original upstream bytes, parsed tree, and serialized wire buffer conceptually distinct: only the last allocation is closed in this extension. Preserve independent owner lifetimes while buffers coexist; cloning bytes shares one owner, whereas constructing a new buffer requires another reservation.

In the xAI JSON rewrite branch, use that entry point only for active restoration leases. On capacity/serialization failure, return the fixed Codex response-stage failure (HTTP 502, `dlp_restore_failed`) before tool observation/publication/client tool delivery; do not return the original body and do not retry. The upstream request may already have completed once. Keep ordinary no-lease serialization and error behavior unchanged. Do not modify the Claude/non-Codex serialization branch.

Preserve original completion evidence, single final restoration boundary, native namespaces, xAI signed-zero/numeric rules, entity-header clearing, protocol keys, loopback binding, security-redaction failures, shared-before-extension DB order, asynchronous audit/usage queues, and generic `sk-` audit/allow-only restrictions. Ordinary legacy `redact` acquires no token mapping, namespace/table, or restoration buffer. No production proxy `unwrap()` or `expect()`. No dependency/schema/config changes, registry/codec-source access, guessed allocation bounds, global formatting, application startup, model calls, generated-command/tool execution, staging, commit, push, PR or shipping. The four protected documents and prior closure documents remain unchanged.

## TDD sequence and evidence

Use synthetic fixtures, in-memory databases, temporary files, and actual listeners on `127.0.0.1:0` only. Test names use `reversible_wire_xai_json_`. Every production change follows a compiled, executed intended-boundary runtime RED. Compilation, fixtures, instrumentation-only assertions, wrong-boundary failures, timeouts, and zero matching tests do not qualify.

1. In the integration test path, add separate success and serialization-budget failure cases through the real handler/forwarder and loopback upstream. Reuse the final nonexecuting recorder or equivalent JSON item recorder; never execute tools.
2. If necessary, add only a test-only RAII-restored boundary seam in `handlers.rs`. It temporarily occupies the real shared budget immediately before the target serialization, after upstream send, original JSON verification and preceding admission. Release this injected occupancy before the final restoration wrapper. It must not replace production encoding or fabricate failure. Callback diagnostics must contain no payload or reference.
3. Run the failure test individually against unchanged production serialization. Expected contract: fixed 502, zero client tool submissions/completion and no reusable completed context. The old code must actually return a successful response at this target boundary, producing the intended runtime assertion failure. Confirm the final wrapper is not the rejecting boundary. If this cannot be isolated, stop and report the missing RED; do not implement speculatively.
4. Implement only the response encoder entry point and the active-lease handler branch. Keep the test-only seam independent of the fix.
5. Run every new named test separately, then the prefix. Success must preserve tokenized upstream, namespace/whole-float compatibility, exactly one restored complete client tool where applicable, and authorized context reuse. Failure must expose no secret/reference diagnostics, no client tool/completion/context, and no additional send on refused continuation after budget recovery. Use unique response IDs.
6. Add GREEN controls for no active tokens/ordinary `redact` acquiring no response-buffer owner, normal bytes ownership, shared clones retaining one charge, final-drop release and cancellation/drop release where applicable. An already-green control is not retrospective RED. Do not claim physical OOM or parser/converter allocation proof.

Individual test command, from repository root (120-second bound; body waits at most 10 seconds):

```bash
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib \
  --manifest-path src-tauri/Cargo.toml TEST_NAME -- --test-threads=1 --nocapture
```

After each named test, run these filters separately with the same prefix and no `--nocapture` unless needed: `reversible_wire_xai_json_`, `reversible_http_xai`, `p1_acceptance_xai_raw_completion`, `proxy::security::`. Require nonzero matching counts and zero failed/ignored for the claimed filtered coverage. Do not print sensitive fixtures on failures. Retain exact commands, test state, exit, counts, hashes and valid/nonqualifying failure classification under `/tmp/vary-dlp-response-wire-*`.

Final checks are sequential, with `CARGO_INCREMENTAL=0` for Rust and `--offline --locked` where supported:

```bash
git diff --check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
CARGO_INCREMENTAL=0 cargo check --offline --locked --manifest-path src-tauri/Cargo.toml
CARGO_INCREMENTAL=0 cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1
CARGO_INCREMENTAL=0 cargo test --offline --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

Full checks may use a bounded subprocess timeout of 600 seconds. A timeout is incomplete, never RED/GREEN. Do not globally format; format only the three owned Rust files with `rustfmt --edition 2021 --config skip_children=true` if required. Frontend inputs and environment are unchanged: reuse the previous verified typecheck/build/format and 201-file/2095-test evidence only after confirming their relevant source hashes. Rerun if any relevant input changes. Reassess supplemental strict panic diagnostics separately from standard Clippy; zero proxy primary spans is not macro-expansion or whole-library proof.

## Acceptance and handoff

Limited PASS requires a valid target-boundary runtime RED, current GREEN named/focused tests, owner lifetime/release controls, bounded read-only review, exact allowed-path comparison, unchanged protected/context hashes and an empty index. The final evidence document must give commands/counts/log hashes, source identity, retained RED versus nonqualifying failures, and explicit limitations. It uses the same fresh-reader clarity and execution-readiness quality gates as this document.

Whole-resource/specification/submission acceptance remains FAIL: parser/converter workspaces, request sanitizer/model/TOML, persistent Chat metadata, headers/transport and codec internals are not closed here. Compact SSE, real side-consumer wiring, full route/retry/policy matrix and unexplained historical 607/1 remain separate blockers. Read-only investigation may identify a concrete next design but grants no further production-write authorization. Protocol completion does not prove task correctness or authorize execution.
