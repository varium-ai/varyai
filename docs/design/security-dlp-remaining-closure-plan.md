# Remaining reversible DLP closure: execution plan

## Decision and approval

Audience: contributors and an implementing agent (human + LLM). The user approved the concrete 23-path expansion on 2026-10-29. This plan records that approved design; it does not grant additional scope. Goal: repair the nine remaining functional findings using valid runtime RED/GREEN evidence, complete synthetic acceptance where possible, and separately assess resource/specification acceptance and code-submission readiness.

**Whole-feature acceptance remains FAIL.** Tests of completion/restoration do not prove task correctness or authorize tool execution. Codec allocation evidence remains unavailable; no claim of merge readiness is permitted while required functional/resource/specification blockers remain.

Authoritative contract: [reversible tokenization specification](security-dlp-reversible-tokenization-spec.md). Preserve the specification, `docs/architecture/security-audit.md`, the original completed-context execution specification, and the existing P1 closure record unchanged. This approved supplement explicitly refines the exact-binding design without silently changing the original document.

## Execution context and baseline

Repository root: `/Users/chikham/workspace/varyai`. HEAD: `4dbfc90d1ff495c04b95bb04962bde57df799d82`. The dirty tree has 55 tracked modified and 12 untracked paths, with no staged changes. The parent verified pending, owned, context and protected hashes and exact HEAD/status against the previous final identity. New baseline: `/tmp/vary-dlp-remaining-start-identity.json`, SHA-256 `cff2ebc73a90846de3d1a2423f9ffb984d3f4f5210d3abbefc383c644dc6acf9`.

Use synthetic fixtures, in-memory databases, temporary files and actual HTTP listeners on `127.0.0.1:0`. Do not start the application, package, use live models, execute generated commands/tools, stage, commit, push or create a PR. Parent owns all writes; agents perform bounded read-only reviews. No global formatting. Rust commands run from the repository root with `CARGO_INCREMENTAL=0`, `--offline --locked`; use bounded test timeouts. Current environment: macOS arm64, Rust/Cargo 1.98.1, Node v25.2.1, pnpm 10.12.3; free disk approximately 5.7 GiB at admission.

## Architecture and invariants

Retain the private request/attempt/lease/completion pipeline and single final post-conversion restoration boundary. Exact actual-send authorization and explicit cross-operation lineage are different private facts. Original-protocol observations must precede lossy conversion; validated raw field consistency must precede restoration, tool replay and context publication. Use existing charged payload owners for new retained copies and reserve before growth, without claiming that this proves dependency-internal allocations.

Keep loopback-only binding; protocol keys `openai_chat`, `anthropic`, `openai_responses`; legacy placeholders; generic `sk-` audit/allow-only restrictions; security-redaction failures; shared-before-extension database lock order and asynchronous audit/usage queues. No production proxy `unwrap()`/`expect()`, including preexisting code. Ordinary legacy `redact` must acquire no token mapping, restoration namespace/table or restoration buffering; preserve native namespace compatibility. No plaintext fallback, retry bypass, token-store key reuse for audit, secret diagnostics or broadened writable carriers.

Technology: Rust/Tokio/Axum HTTP fixtures, serde JSON with strict duplicate-key checks, existing payload/store ownership primitives, Cargo tests, and existing React/TypeScript/Vitest checks. No new dependencies or database schema changes.

## Approved files and responsibilities

All Rust paths below are under `src-tauri/src/proxy/`:

| Path | Authorized responsibility |
| --- | --- |
| `forwarder.rs` | Exact actual-send binding, private operation descriptor, reconstruction/cache provenance handoff |
| `handlers.rs` | Original completion witness and final response boundary wiring |
| `providers/codex.rs` | Explicit configured route-family/operation resolution only |
| `providers/codex_chat_history.rs` | Distinguish original self-contained history from cache contributions |
| `security/chat_enrichment.rs` | Private enrichment provenance, not authority |
| `security/http_restoration.rs` | Admission/history classification, actual attempts, witnesses and entity headers |
| `security/completed_context.rs` | Keep exact-send proof; separate private lineage/operation identities |
| `security/tokenization.rs` | Explicit authorized operation transitions and strict context lookup |
| `security/request_preparation.rs` | Preserve transactional inspection for fresh readable history |
| `security/restoration_audit.rs` | Narrow optional auxiliary fingerprint degradation |
| `security/identity.rs` | Audit-only test fault seam, never global/core entropy failure |
| `security/response_restoration.rs` | Unified raw field/snapshot consistency and semantic argument equality |
| `security/completion_json.rs` | Original JSON carrier closure and completion checks |
| `security/completion_tools.rs` | Original Chat tool/function sibling closure |
| `security/completion_evidence.rs` | Genuine JSON/SSE evidence distinction and existing normalization attestations |
| `security/reversible_integration_tests.rs` | Retained actual HTTP regressions, nonexecuting client recorder and side-effect matrix |
| `security/reversible_context_tests.rs` | Exact binding, lineage and negative authorization controls |
| `security/reversible_preparation_tests.rs` | Fresh-history inspection/rollback controls |
| `security/reversible_response_tests.rs` | Snapshot, barrier, equality and replay controls |
| `security/reversible_completion_tests.rs` | Strict original evidence and unsupported carriers |
| `security/restoration_audit_tests.rs` | Auxiliary degradation and core failure controls |

Documentation scope: this plan and `docs/design/security-dlp-remaining-closure.md` (new final evidence record). No other production repair is authorized. Stop for approval if a required repair needs another path or a different design. Resource review is read-only; no generic resource rewrite is included.

## Repair decisions

### RESP-2 / RESP-4 / RESP-5: one raw snapshot model

Track response/item/part/field identity and every supported raw added/done/item/completed snapshot, before restoration or replay. Every present nonempty added value is an independent complete snapshot, not a prefix to append to later deltas; empty added values alone are placeholders. Missing added fields make no content claim. Every present done/item-done/completed value, including the empty string, is authoritative and must compare equal. The fully validated completed envelope supplies the final raw value; with deltas compare their concatenation to it, without deltas compare each supplied non-placeholder snapshot directly to it. No observed snapshot may disappear by replay or by omitting its final item/part. Contradictions fail even if a later value could overwrite them. Restore supported added values from that same validated final value without guessing partial JSON.

Function arguments use duplicate-rejecting parsed JSON semantic equality: object order, whitespace and equivalent string escapes may differ; array order and numeric types must not change. Only existing attested Chat canonicalization and xAI whole-float normalization may relax their documented boundary. Custom tool input remains exact free text. Generate one restored final value and preserve replay event count/order/type/IDs/sequence numbers; argument delta concatenation must equal the final restored argument. No invented events.

### RESP-3: close original Chat carriers

Inspect unknown sibling values in original Chat tool/function objects before conversion can discard them. Authorized references there produce fixed unsupported failure, no tool release or completed context. Do not restore unknown fields or expand request inspection.

### REQ-2: exact send plus private lineage

Keep full actual URL, provider ID, protocol and resolved credential/account identity bound to every attempt. Separately issue a private forwarder-controlled route-family/operation descriptor. The family preserves provider, protocol, resolved base prefix, full query, configured route mode and authentication. Never sort/drop queries or strip `/compact` from an arbitrary URL to infer authority. Explicit existing aliases identify logical operations; only deterministic configured Responses/compact endpoint pairs authorize cross-operation transfer. Ambiguous full-URL configurations refuse. Context lookup still verifies store/session/generation/policy/snapshot/dependencies/opaque provenance and an explicit allowed operation transition. Same-operation attempts retain exact URL compatibility; unrelated endpoints are not equivalent.

### REQ-3: reconstruct from immutable client input

Classify original self-contained readable history before tokenization or cache enrichment. Require no prior token, cursor or opaque dependency; do not silently drop stale cursors. Supported readable messages/reasoning and complete function/custom call plus matching result are reinspected as new input; call IDs are unique and each result follows its call. Do not inherit historical completed-context authority. Do not let cache enrichment fill incomplete reconstruction candidates. Old references, result-only history, cached dependencies and opaque history still require strict resume authorization. Allocation provenance is not authorization and must not reopen REQ-1.

### REQ-4: isolate auxiliary fingerprint failure

When an auxiliary audit fingerprint is unavailable, skip only the affected finding/summary and dedup work with fixed nonsensitive degradation diagnostics. Do not catch all audit-capture errors. Do not substitute plaintext, an unkeyed hash, a constant fingerprint or token-store keys. Core entropy, lease, collisions, policy, reinspection, capacity and security-redaction failures remain fail-closed. Test injection is audit-specific and isolated from core randomness.

### RESP-6 / RESP-7

Remove stale `Content-Digest` and `Repr-Digest` at the final mutated-entity boundary along with applicable existing entity headers; do not alter ordinary unmodified traffic. Carry a genuine fully read, strict Anthropic JSON completion witness through synthetic SSE conversion. Do not fabricate original SSE frames/EOF facts or permit synthetic terminals alone to authorize success.

## Ordered tasks and proving checks

For every production change: write a retained functional test first, compile successfully, execute it against the pre-repair source, and record the intended-boundary runtime assertion failure. Compilation, fixture, instrumentation-only, wrong-boundary, timeout and zero-test failures do not qualify as functional RED. Run each new named test separately before aggregating so one failure cannot hide another scenario. Post-repair controls are GREEN coverage, not retroactive RED.

Use this command prefix for each filter `FILTER` below:

```bash
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib \
  --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
```

- [ ] Before any shared-model production edit, write and separately run all `reversible_closure_resp2_`, `reversible_closure_resp4_`, and `reversible_closure_resp5_` cases against the unchanged production source. Retain each valid runtime RED and a successful consistent control; this front-loaded evidence is required before repairing their shared model.
- [ ] In `reversible_integration_tests.rs`, retain `reversible_closure_resp2_` actual HTTP regressions for contradictory message/reasoning/part snapshots before and after the tool barrier. Repair `response_restoration.rs`; require fixed failure, zero buffered tool submissions and no context publication. Run the new filters and `reversible_response_tests`.
- [ ] In `reversible_integration_tests.rs`, add `reversible_closure_resp3_` JSON/SSE tool/function unknown-sibling regressions. Repair `completion_json.rs` / `completion_tools.rs`, with `completion_evidence.rs` / `handlers.rs` only if needed for original observation wiring. Require unsupported failure, zero buffered submissions and zero continuation sends. Run these filters and `reversible_completion_tests`.
- [ ] In `reversible_integration_tests.rs` and `reversible_response_tests.rs`, add `reversible_closure_resp4_` supported nonempty-added and `reversible_closure_resp5_` semantic-argument regressions. Repair the same raw snapshot model; check event identity and byte-identical restored replay, plus duplicate/type/array-order negative controls. Run each named filter, `reversible_response_tests`, `p1_` and the Chat/xAI HTTP filters.
- [ ] In `reversible_integration_tests.rs` / `reversible_context_tests.rs`, add `reversible_closure_req2_`: one session/provider/auth Responses -> compact -> Responses with actual paths, cumulative sends 1/2/3, tokenized upstream, inherited authorized lineage and restored final output. Repair `forwarder.rs`, `providers/codex.rs`, `http_restoration.rs`, `completed_context.rs`, `tokenization.rs`; check changed query/prefix/provider/auth/session/generation and unknown opaque refusal. Run the new filter, `reversible_context_tests` and `compact_json`.
- [ ] In `reversible_integration_tests.rs` / `reversible_preparation_tests.rs`, add `reversible_closure_req3_`: empty store/new generation, complete original call/result and no old dependency must send safely; orphan/cache/opaque/old token/stale cursor/duplicate/out-of-order controls refuse with zero sends. Repair the approved request/provenance files. Run the new filter, `reversible_preparation_tests` and `p1_`.
- [ ] In `reversible_integration_tests.rs` / `restoration_audit_tests.rs`, add `reversible_closure_req4_` audit-only fault regression. Repair `identity.rs`, `restoration_audit.rs` and narrow handoff only if needed. Require one safe tokenized send, safe degraded audit behavior, unchanged core-failure zero-send controls. Run the new filter and `restoration_audit_tests`.
- [ ] In `reversible_integration_tests.rs`, add `reversible_closure_resp6_` identity JSON/SSE digest-header regressions; repair `http_restoration.rs`. Run the filter and JSON/SSE compatibility controls.
- [ ] In `reversible_integration_tests.rs` / `reversible_completion_tests.rs`, add `reversible_closure_resp7_` explicit Anthropic JSON -> synthetic SSE regression. Repair `handlers.rs`, `http_restoration.rs`, `completion_evidence.rs` as needed. Require genuine JSON witness, restoration and publication only on full valid body; failure/cancel/truncation remain closed. Run the filter and Anthropic HTTP/completion tests.
- [ ] In `reversible_integration_tests.rs` / response/completion tests, add a nonexecuting final-client recorder. Successful complete restored tool is recorded exactly once, all buffered calls on error/cancel/truncation/budget failure zero. Add shell quote, nested JSON, patch, partial/encoded/missing/wrong-valid-token examples and inspect synthetic cache/audit/usage/error/observation side effects. Run `reversible_closure_` and the entire security test group; inspect every matching count.
- [ ] Read-only resource/acceptance inventory: inspect approved source boundaries plus existing `upstream_guard.rs`, payload primitives, converters and transport. Record compact SSE status and historical 607/1 uncertainty in the new closure record. Do not repair these outside the approved design; do not convert missing proof into PASS.
- [ ] Run current-state complete verification and bounded independent read-only reviews, then write `docs/design/security-dlp-remaining-closure.md` with exact evidence/source identity and separate functional/specification/submission decisions.

## Final verification and evidence

Run required checks from the repository root; Rust uses the environment/flags above. `pnpm test:unit --exclude '**/.voidx/**'` deliberately excludes ignored tool caches; disclose any other exclusions and warnings.

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit --exclude '**/.voidx/**'
pnpm format:check
CARGO_INCREMENTAL=0 cargo check --offline --locked --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
CARGO_INCREMENTAL=0 cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml
CARGO_INCREMENTAL=0 cargo test --offline --locked --manifest-path src-tauri/Cargo.toml
git diff --check
```

Retain per-run command, execution context, counts, exit, log hash and relevant source identity. Reuse prior evidence only if relevant inputs/environment are unchanged; tests before integration do not establish current-state success. Supplement proxy panic source inspection with the prior strict non-test Clippy procedure; the known 41 nonproxy diagnostics are not a whole-library PASS. Never print tokens, raw sensitive fixtures/configuration or parser excerpts in test reports.

## Explicit remaining risks and acceptance gates

Seven resource boundaries remain separate: request sanitizer/provider-model/TOML allocations; response copies/parsers/serializers/converters; persistent Chat metadata/index growth; general workspace; codec constructors/internal buffers; headers/transport; and the already limited-pass response namespace table. Finite admission (`64 KiB + 4096 * cumulative input`) is not an allocation proof. No guessed HashMap/container/codec multipliers, alternate access path to denied registry source, RSS cap, physical OOM guarantee or zeroization claim.

Compact SSE remains unsupported by the current restoration entry point and cannot be credited from JSON controls. The full route/conversion/history/retry/policy/side-effect matrix and historical security 607/1 need explicit evidence, not later GREEN as an explanation. Real Codex client acceptance and real-model behavioral evaluation remain separate, unapproved tiers. If required proof or functionality cannot be obtained in this scope, report FAIL/NOT-PROVEN and request a concrete further design rather than claiming code is ready to merge.

Quality gate: reread as a fresh contributor for clear decisions and limits; verify every source path and command and each task's source/test/filter/invariant as an execution-readiness check. Record the result before leaving the design workflow.

### Exact targeted selectors and timeout procedure

In the prefix above, replace `FILTER` with each exact selector separately: `reversible_response_tests`, `reversible_completion_tests`, `reversible_context_tests`, `reversible_preparation_tests`, `restoration_audit_tests`, `p1_`, `reversible_http_chat`, `reversible_http_xai`, `reversible_http_anthropic`, `reversible_http_native_json`, `reversible_http_native_sse`, `compact_json`, and `proxy::security`. Each must run at least one test and finish with zero failed/ignored for its claimed coverage. A selector with no matches is not evidence. JSON/SSE compatibility means the native, Chat, Anthropic and xAI selectors above; it does not mean compact SSE success.

Parent invokes individual Cargo checks using Bash tool timeout 120 seconds. A timeout is recorded as incomplete, never RED/GREEN. For a longer full suite, use Python `subprocess.run(command, timeout=600, env={**os.environ, 'CARGO_INCREMENTAL':'0'}, stdout=log, stderr=subprocess.STDOUT)` through a bounded Bash invocation (timeout 660 seconds); record a `TimeoutExpired` as incomplete and stop before claiming completion. New individual functional tests also use a 10-second runtime bound on body collection; reaching that bound is not functional RED.

Strict supplemental panic check, not a substitute for normal Clippy:

```bash
CARGO_INCREMENTAL=0 cargo clippy --offline --locked --lib \
  --manifest-path src-tauri/Cargo.toml --message-format=json -- \
  -D clippy::unwrap_used -D clippy::expect_used
```

Capture JSON diagnostics and exit without printing sensitive payloads. Classify primary-span proxy versus nonproxy diagnostics; no primary proxy diagnostics alone is not macro-expansion proof or a whole-library PASS. Source inspection still excludes test-only blocks explicitly. The prior 41 nonproxy diagnostics remain a limitation unless current evidence establishes otherwise.
