# Unified DLP core: parallel native-adapter implementation plan

> Status: execution approved by the user. M0/C1/C2/P2 foundations and the [P3/P4 scoped closure](security-dlp-unified-core-implementation-evidence.md#22-p3p4-integrated-verification-and-publication-proof-closure) are preserved. **C3/C4/C5 and all ten CORE-1 technical items are complete** on the [post-F1 frozen source and independent PASS](security-dlp-unified-core-implementation-evidence.md#50-whole-task-reviews-and-approved-f1-evidence-amendment). The user authorized baseline commit `ca7f883c742df07abe9bcaa9b0eade02aae5e119` and three isolated worktrees. [Sections 51–53](security-dlp-unified-core-implementation-evidence.md#51-approved-baseline-commit-isolated-workspaces-and-request-lane-red) retain request RED, historical findings, GE1 F1 closure and isolated checks **CH1 19/19, AN1 15/15, GE1 33/33 / security 1935/1931/1949**. [Section 10](#10-approved-chat-only-integrator-amendment-current-execution-boundary) and [evidence Section 54](security-dlp-unified-core-implementation-evidence.md#54-authorized-chat-only-shared-amendment-execution-checkpoint) record the approved **unfrozen Chat-only amendment**, C5 coordination, parent **24/24 CH2 / 1941/1941 security**, snapshot F1 repair and bounded independent closure. Typed policy and whole-amendment gates remain open. Characterizations are not RED; the baseline Full Gate does not cover these diffs. Native registrations remain **Disabled**; lane handoff, M2–M4 integration and M5 are not accepted.
>
> Audience: human maintainers and implementing/reviewing agents. Authoritative behavior: [unified core and protocol-adapter architecture](security-dlp-unified-core-protocol-adapters.md). Current shipped baseline: [DLP architecture](../architecture/security-audit.md).

## 1. Goal and delivery order

Extract and verify a stable reversible DLP core without changing existing Codex Responses behavior, then develop native Chat, Anthropic, and Gemini adapters concurrently and integrate them one at a time.

Keep native JSON/SSE and existing provider conversion outside a lossy common representation. The core owns authority, budgets, tool release, completion publication, and failure policy; adapters own native sites, indexing, observations, and protocol validation. Retain `admit_http_request` and `DlpPreparedAttempt` as compatibility facades while moving responsibilities behind the new boundary.

```text
M0 baseline -> M1 core extraction + contract freeze (CORE-1)
                                      |
                      +---------------+---------------+
                      |               |               |
                 Chat lane       Anthropic lane   Gemini lane
                      |               |               |
                      +---------------+---------------+
                                      |
                   M2 Chat integration and acceptance
                                      |
                   M3 Anthropic integration and acceptance
                                      |
                   M4 Gemini integration and acceptance
                                      |
                   M5 final integrated review and evidence
```

Lanes may finish in any order. M2-M4 are ordered integration/enablement gates, not a requirement to delay independent adapter work. Do not merge all three and debug the combined change afterward. New converted-upstream cells remain a separate Stage 5 project; preserve already-working Responses conversion cells throughout.

**Technology:** Rust, Tokio, Axum/Hyper/reqwest, serde JSON, existing budget-owned payload/store primitives and Cargo tests. Use the repository's pnpm/TypeScript/Vitest checks for final acceptance. No new dependency, database schema, UI, persisted setting, credential source, or client takeover change is planned.

**Planning baseline:** inspected HEAD `ebfca878` on 2026-11-05. The architecture document was untracked before this plan was written. This is a source inspection baseline, not a passing test result. Reconfirm HEAD, worktree state, toolchain, and baseline tests at M0; preserve unrelated user changes.

## 2. Scope and invariants

### First native enablement cells

| Client route owner                  | Client / actual upstream                  | Planned scope                                                                                                |
| ----------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `handlers::handle_chat_completions` | Chat / `openai_chat`                      | Existing direct Chat aliases registered in `server.rs`; JSON and SSE.                                        |
| `handlers::handle_messages_for_app` | Anthropic / `anthropic`                   | Existing direct Claude and Claude Desktop message aliases; distinct application namespaces; JSON and SSE.    |
| `handlers::handle_gemini`           | Gemini / `gemini_native`                  | Existing direct generation routes only: `generateContent` JSON and `streamGenerateContent?alt=sse`.          |
| Existing Codex Responses handlers   | Responses / currently supported upstreams | Preserve Responses, Compact, and already-enabled conversion behavior; no new cell inferred from these tests. |

Protocol keys remain `openai_responses`, `openai_chat`, `anthropic`, and `gemini_native`; reuse the mapping in `protocol_bridge.rs`. App attribution is not client protocol, and client protocol is not actual upstream protocol. Re-evaluate the actual cell for every provider attempt, including retries/failover.

Explicit exclusions:

- New reversible enablement for `agent_clients.rs`, `native_protocol.rs`, intent/global routing, and authenticated gateway ingress is outside M2-M4. They must retain their existing behavior, not accidentally acquire a new session namespace because a shared handler was enabled.
- `gateway/ingress.rs` directly calls the three handlers. Add a trusted, internal ingress classification for the **new** native admission gates; a client header cannot choose it. Preserve the existing Responses behavior. Gateway expansion later needs consumer-scoped authority and its own acceptance plan.
- Gemini model listing, GET requests, token-counting/non-generation methods, streaming without `alt=sse`, WebSockets, and unlisted framings are not new restoration routes. Unsupported Gemini streaming retains ordinary behavior only when no reversible action/dependency needs restoration; otherwise fail before forwarding, as required by the architecture.
- New cross-protocol cells, arbitrary native-agent integrations, dual-version signed-payload replay caches, and inbound tool-result history stores are not included.

Non-negotiable invariants from the architecture and contributor guide:

1. No relaxation of token/store/session/generation/provider/operation/actual-attempt authority, no proof from placeholder syntax, and no generated UUID as restoration identity.
2. No unredacted fallback. Only explicit `Block` is a policy rejection; optional context degradation is whitelisted at the HTTP boundary, never a global catch of `ContextUnavailable`.
3. Preserve raw bytes when unmodified, unknown native fields, JSON types, headers, event order, original failures, and genuine upstream semantics. Preserve safe upstream-error redaction and prepared-attempt redirect rejection, including zero-token attempts.
4. No privileged partial tool release or completed record after invalid completion, transport interruption, cancellation, signed-unit conflict, or budget failure.
5. Signed Anthropic thinking and Gemini signed Parts are immutable in the first adapters. Supported readable fields remain inspectable; signatures/redacted payloads/binary data remain opaque. Use the architecture's generic failure projection for mandatory patch/restoration conflicts.
6. Client tool results are untrusted input, not delivered response snapshots. Pairing grants neither execution permission nor independent mapping authority.
7. No new `unwrap()`/`expect()` in the proxy hot path, blocking database work, synchronous persistence, raw-secret diagnostics, or interruption from auxiliary audit/usage/tool-observation failures.
8. Tests use synthetic values, in-memory/temporary storage, nonexecuting tools, and dynamic loopback ports. Do not run ignored/live tests, launch/install the application, use real credentials, or bind `127.0.0.1:15722`.

## 3. Ownership and file layout

Paths in this section are relative to `src-tauri/src/proxy/`, unless explicitly prefixed with `docs/`. All files marked **new** are proposed targets, not existing APIs. Additional production files require a scope review before editing.

### Integrator-owned shared surface

| Path                                                                                                                                              | Responsibility                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `security/restoration_core.rs` **new**                                                                                                            | Core facade, private attempt lifecycle, protocol-aware authority dispatch, and delegation to existing strict owners. Do not duplicate token storage.                                |
| `security/restoration_contract.rs` **new**                                                                                                        | Internal protocol/site/edit/history/observation contracts and capability visibility boundaries. Freeze their concrete signatures at CORE-1.                                         |
| `security/protocol_adapters/mod.rs` **new**                                                                                                       | Adapter registration and explicit route/client/upstream/framing support registry. New native entries remain disabled until each gate passes.                                        |
| `security/protocol_adapters/responses.rs` **new**                                                                                                 | Responses adapter over existing request/history/response logic, including Compact.                                                                                                  |
| `security/mod.rs`                                                                                                                                 | Register core/adapters/test modules; keep existing imports/facades compatible.                                                                                                      |
| `security/http_restoration.rs`                                                                                                                    | Admission orchestration, compatibility facade, attempt handoff, narrow context degradation, and client projections.                                                                 |
| `security/tokenization.rs`, `security/completed_context.rs`                                                                                       | Preserve token authority and private store ownership; introduce protocol-separated completed-evidence variants and namespace domains without changing existing Responses semantics. |
| `security/request_preparation.rs`, `security/request_transaction.rs`, `security/output_text_matching.rs`, `security/response_restoration.rs`      | Extract reusable mechanics from Responses knowledge; preserve transactions, matcher behavior, budget ownership, and tool barriers.                                                  |
| `security/completion_evidence.rs`, `security/completion_json.rs`, `security/completion_tools.rs`                                                  | Shared strict evidence primitives and verifier integration; reuse validation rather than introducing a second completion model.                                                     |
| `handlers.rs`, `response_processor.rs`, `handler_context.rs`                                                                                      | Trusted ingress/client-protocol context, request and final response wiring, cancellation/ownership, and protocol-aware error projection.                                            |
| `forwarder.rs`, `session.rs`, `error_mapper.rs`                                                                                                   | Actual upstream/target/operation binding, retry/failover checks, strict native session parsing, and generic client error envelopes.                                                 |
| `gateway/ingress.rs`                                                                                                                              | Internal classification of gateway calls so enabling direct native handlers does not implicitly expand gateway restoration scope.                                                   |
| `security/tests/unified_core_tests.rs` **new**                                                                                                    | Baseline equivalence and shared contract/authority tests.                                                                                                                           |
| `security/tests/unified_test_support.rs` **new**                                                                                                  | Budgeted synthetic fixtures and private test-only core/adapter harness, shared by all lanes.                                                                                        |
| `security/tests/native_chat_http_tests.rs`, `security/tests/native_anthropic_http_tests.rs`, `security/tests/native_gemini_http_tests.rs` **new** | Integrator-owned native ingress-to-final-response tests.                                                                                                                            |
| `security/tests/native_route_scope_tests.rs` **new**                                                                                              | Disabled-cell, retry, indirect-ingress, and route/framing admission negatives.                                                                                                      |
| Existing security test files, `protocol_matrix_tests.rs`                                                                                          | Retain/extend regressions; never replace Responses-client tests with native-client tests.                                                                                           |
| `docs/design/security-dlp-unified-core-parallel-implementation-plan.md`                                                                           | This plan and task/gate status.                                                                                                                                                     |
| `docs/design/security-dlp-unified-core-implementation-evidence.md` **new at M0**                                                                  | Sanitized baseline, contract freeze, RED/GREEN, integration, and final acceptance records.                                                                                          |
| `docs/architecture/security-audit.md`, `docs/troubleshooting.md`                                                                                  | Update shipped behavior/limitations only after the relevant acceptance gates, in English.                                                                                           |

Existing provider converters, `providers/codex_chat_history.rs`, payload owner modules, `server.rs`, `agent_clients.rs`, `native_protocol.rs`, and `protocol_bridge.rs` are read/verification references. If a necessary production change cannot stay within the table, stop for a scoped amendment; do not quietly widen a lane's patch.

### Exclusive lane-owned paths

For each lane, create the following files under its directory. The lane owns that entire directory and nothing in the shared table.

| Lane root (**new**)                     | Files and responsibilities                                                                                                                  | Test module filter                                       |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `security/protocol_adapters/chat/`      | `mod.rs` entry; `request.rs` sites/patching; `response.rs` JSON/SSE and strict verifier; `history.rs` replay/pairing; `tests.rs` lane tests | `proxy::security::protocol_adapters::chat::tests::`      |
| `security/protocol_adapters/anthropic/` | Same file roles; block lifecycle and immutable thinking/signature handling                                                                  | `proxy::security::protocol_adapters::anthropic::tests::` |
| `security/protocol_adapters/gemini/`    | Same file roles; candidates/Parts, structured calls, framing and immutable signed Parts                                                     | `proxy::security::protocol_adapters::gemini::tests::`    |

The integrator registers all three module/test slots and a compileable disabled facade during M1. Do not implement native behavior in these scaffolds. Each lane then writes a failing behavioral test before adding its behavior. Tests import the frozen test harness rather than editing global test helpers or relying on public witness constructors.

## 4. CORE-1: the parallel-start gate

Freeze the **adapter-facing contracts and security invariants**, not every internal function. Record the exact integrated source revision/content identity in the evidence file. No lane starts implementation until all items below pass:

- [x] Responses/Compact equivalence tests and all affected existing suites pass on the extracted core; client-visible results and internal permissions are checked separately.
- [x] Concrete types and ownership are defined in `restoration_contract.rs`; methods cover architecture Section 3's request observation/atomic patches, original response observations, client JSON/SSE restoration, and finish/abort.
- [x] `TextSite` uses an opaque native location and source class; it is not an extractor's human-readable diagnostic path. Ranges refer to decoded string UTF-8 byte boundaries. Native JSON argument leaves are edited through their adapter, not serialized into a common response IR.
- [x] `editable`/`immutable` constraints are known before a patch can escape; SSE may hold an affected unit within budget while late signature association is unresolved. Core validates atomic patch sets and retains ownership until release.
- [x] Client protocol, actual upstream protocol, application/ingress namespace, stable identity, policy revision, provider target/principal, operation, and retry binding have explicit independent roles. Gemini has no inherited Responses cursor or Responses/Compact operation transfer.
- [x] Protocol evidence variants describe ordered delivered assistant/call snapshots and dependencies. Client result bodies are excluded from delivery attestation. Per-protocol verifier output is sealed; handlers/adapters cannot mint completion from a boolean or publish records directly.
- [x] Shared matcher, tool barrier, budget, transport completion, failure projection, and context-degradation APIs are exercised through a non-Responses test fixture. This fixture remains test-only and grants no production route support.
- [x] The harness can drive native request/JSON/SSE/finalization, arbitrary byte splits, failure injection, cancellation, delivery recording, and absence of publication without disclosing fixture plaintext in reports.
- [x] Disabled native route entries and gateway classification pass negative tests. Existing Responses conversion cells, cached-history handling, and no-redirect final-send boundaries remain green.
- [x] The three empty lane modules compile against the same frozen contract; independent review returns PASS for the boundary. Freeze record lists any deliberately unsupported capability and no unresolved lane-blocking API decision.

### Proposed strict native identity sources

These are plan decisions to approve, not a claim of upstream-standard conversation identifiers. Keep legacy logging/session extraction unchanged; restoration parsing returns an optional borrowed validated identity, never a generated fallback.

| Client                                | Sources in precedence order                                                                                                                         |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Responses                             | Preserve `extract_codex_restoration_session_id` exactly.                                                                                            |
| Native Chat on approved direct routes | Same source candidates and strict validation as the existing Codex restoration parser, but a distinct client-protocol namespace.                    |
| Native Anthropic                      | `x-claude-code-session-id`, `claude-code-session-id`, `metadata.session_id`, then the existing recognized `_session_` suffix in `metadata.user_id`. |
| Native Gemini                         | Explicit local `x-session-id` header only; no identity inferred from model, contents, response ID, or thought signature.                            |

For new parsers, the first present source must be a single valid string: nonempty, at most 256 UTF-8 bytes, without whitespace/control characters. Repeated headers, invalid encoding/types, or an invalid higher-precedence source disable continuity rather than falling through to another identity. Do not trim, log the value, or merge namespaces. Without a valid identity, current-request restoration remains available; historical privilege does not.

### Contract changes after freeze

A lane reports the failing fixture, missing contract operation, and proposed minimal change to the integrator. Only the integrator changes shared files, runs core/Responses and affected lane tests, obtains an independent review, and records a new contract revision. All lanes rebase onto that revision and rerun their checks. Never work around a frozen boundary with a second token store, public completion constructor, local failure-policy override, or relaxed authority.

## 5. Execution and verification commands

All commands run from the repository root of the relevant worktree. The following shell function is a command convention for the plan, not a new repository script. It verifies a nonzero test match before execution; Cargo's zero-test success is not acceptance.

```bash
check_tests() {
  local filter="$1" listing
  listing="$(cargo test --lib --manifest-path src-tauri/Cargo.toml "$filter" -- --list)" || return
  printf '%s\n' "$listing"
  printf '%s\n' "$listing" | grep -Eq ': test$' || {
    printf 'No tests matched: %s\n' "$filter" >&2
    return 1
  }
  cargo test --lib --manifest-path src-tauri/Cargo.toml "$filter" -- --test-threads=1
}
```

Every task below names its verification filter or command. Paths in task rows use the exact roots from Section 3. New filters are expected to be absent until their owning task creates and registers the tests; absence after that task is a failure.

**TDD:** write one scoped test, run `check_tests` and observe a failure for the intended behavior, implement the minimum change, rerun green, then run the stated broader filter. A syntax/import/build failure is not the intended behavioral RED. Characterization tests may initially pass for unchanged behavior; keep them green while adding a failing test for each newly introduced core boundary/capability before extraction changes. Refactor only after that contract turns green. Documentation and empty module registration add no behavior and are exempt from the RED requirement; they do not authorize a feature implementation without its failing test.

### M0: baseline and test harness admission — integrator

- [x] **B1 — Record source/environment identity.** Read `security/http_restoration.rs`, `security/response_restoration.rs`, `handlers.rs`, `response_processor.rs`, `forwarder.rs`, `session.rs`, and the architecture; create the evidence file from Section 3. Run `git rev-parse HEAD`, `git status --short`, `rustc --version`, `cargo --version`, `node --version`, `pnpm --version`, and `df -h .`. Record sanitized results, dirty paths and document hashes, not configuration/credential contents.
- [x] **B2 — Establish a passing baseline.** Run the Full Gate below, plus `check_tests proxy::security::`, `check_tests proxy::protocol_matrix_tests::`, and `check_tests proxy::session::tests::`. Record preexisting failures separately; do not reclassify them as passed or silently expand this task to unrelated repairs. The isolated matrix fixture prerequisite and its test-only repair are recorded in the evidence file.
- [x] **B3 — Add the equivalence fixture set.** Create/register `security/tests/unified_core_tests.rs` and `unified_test_support.rs` via `security/mod.rs`; reuse synthetic patterns from `request_literal_tests.rs`, `reversible_context_tests.rs`, `reversible_completion_tests.rs`, `reversible_integration_tests.rs`, and `upstream_error_tests.rs`. Run `check_tests proxy::security::unified_core_tests::` and `check_tests proxy::security::`. Cover cursor/history degradation, explicit Block, current mandatory redaction, mixed media, stale/forged references, Compact, cached-history authority, retries, original failures, redirects, and fixed error projection. Record unchanged client output separately from withheld historical privilege. The evidence file distinguishes 13 new facade characterizations from inherited regression coverage; neither is native-client acceptance.

**M0 exit:** source snapshot and nonzero baseline results recorded; no unexplained failing prerequisite. No extraction or parallel implementation until this gate is satisfied.

### M1: extract core and freeze contracts — integrator

- [x] **C1 — Introduce protocol-aware contract types and disabled registration.** Create `security/restoration_contract.rs`, `security/restoration_core.rs`, `security/protocol_adapters/mod.rs`, `responses.rs`, and the lane skeletons; register in `security/mod.rs`. Add tests in `unified_core_tests.rs` for independent client/upstream identities and immutable-site/atomic patch constraints. Verify `check_tests proxy::security::unified_core_tests::` and `cargo check --manifest-path src-tauri/Cargo.toml`. Completed as unconnected foundations: 14 C1 controls plus 13 M0 characterizations pass, the Full Gate passes, and independent C1 implementation review returns PASS. Core/Responses slots and native lanes remain empty; this is not CORE-1 approval. See evidence Sections 8-12.
- [x] **C2 — Extract request/authority ownership.** Refactor `security/http_restoration.rs`, `request_preparation.rs`, `request_transaction.rs`, `tokenization.rs`, and `completed_context.rs` behind the core/Responses facade. Add protocol-separated binding/evidence tests, no-session current-request tests, independent-reference controls, and first-result-versus-delivered-call tests in `unified_core_tests.rs`. Run its filter, `check_tests proxy::security::reversible_context_tests::`, then `check_tests proxy::security::`. Completed with ten C2 controls (37 unified total), current-source Full Gate and independent C2-only review PASS. Request lease/budget and fork/bind ownership moved to the core; Responses-specific evidence/preparation reject native scope. The Responses facade remains thin, and strict completion sealing/native evidence/non-Responses harness remain C3/C5 work. See evidence Sections 13-16; this is not CORE-1 approval.
- [x] **C3 — Extract response mechanics and strict evidence boundary.** Refactor `security/output_text_matching.rs`, `response_restoration.rs`, `completion_evidence.rs`, `completion_json.rs`, and `completion_tools.rs` through the core/Responses facade. Test all byte/delta splits, budgets, original-versus-converted evidence, terminal/snapshot contradictions, tool barriers, abort/drop and delayed publication in `unified_core_tests.rs`. Run its filter, `check_tests proxy::security::reversible_completion_tests::`, and `check_tests proxy::security::`. Closed by Iris's whole-C3 PASS, Kai's current-source preservation PASS and the post-F1 Full Gate; see evidence Section 50. Legacy compatibility entries remain; this is not universal lifecycle enforcement.
- [x] **C4 — Preserve actual-send and final-client orchestration.** Update `forwarder.rs`, `handlers.rs`, `handler_context.rs`, `response_processor.rs`, `session.rs`, `error_mapper.rs`, `gateway/ingress.rs`, and the HTTP facade only as needed for explicit protocol/ingress dispatch. Do not enable native cells. Create/register `native_route_scope_tests.rs`; verify disabled routes, gateway isolation, retries to unsupported cells, both HTTP transports' target/redirect checks, and original upstream errors. Run `check_tests proxy::security::native_route_scope_tests::`, `check_tests proxy::security::`, `check_tests proxy::protocol_matrix_tests::`, and `check_tests proxy::session::tests::`. Closed by Quill's whole-C4 PASS, Kai's current-source preservation PASS and the post-F1 Full Gate; see evidence Section 50. Existing ignored/live coverage was not executed.
- [x] **C5 — Freeze CORE-1.** Complete Section 4, test the non-Responses harness, and record actual contract signatures/visibility, exact base identity, and lane directories in the evidence file. Run the Full Gate and obtain an independent boundary review. Do not start a lane while a required capability remains a stub or needs an unresolved core redesign. Technical freeze accepted at the 518-input identity `ce68663772ada37be5a34db78c54bcdbc7e99062a657d21621a6bfac4582c9aa`: Section 44's contracts plus Section 46 amendments, Section 50's F1 controls, current Full Gate and Kai's ten-item technical PASS. This content identity is not the approved Git base required by Section 6.

**M1 technical exit: PASS.** CORE-1 items 1–10 are checked; Responses/Compact regressions pass, native admission remains Disabled, and the executable harness plus three compileable disabled lane slots share one content freeze. **Operational lane-start exit: satisfied after explicit user authorization.** The accepted source was committed as `ca7f883c742df07abe9bcaa9b0eade02aae5e119`; three initially clean isolated worktrees start at that exact base. The former no-commit/no-worktree hold is preserved as historical evidence in Section 50, not an active instruction. Section 51 records the new workspaces, independent Cargo targets and first request-task RED; no lane or later enablement/integration gate is accepted by that evidence.

### Parallel native lanes — independent owners after CORE-1

In each lane, every row is a separate RED/GREEN task. Run the lane filter after each task and `check_tests proxy::security::` at lane handoff. Lane tests exercise the real shared core via the frozen harness, not a permissive fake authority implementation.

| Task | Owned files                                                             | Requirement and expected result                                                                                                                                                                                                                                                                                   | Verification                                                         |
| ---- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| CH1  | `security/protocol_adapters/chat/request.rs`, `mod.rs`, `tests.rs`      | Native string/array messages, tool argument strings and result text; atomic native patches, opaque media and unknown-field preservation; no writes through diagnostic paths.                                                                                                                                      | `check_tests proxy::security::protocol_adapters::chat::tests::`      |
| CH2  | `security/protocol_adapters/chat/response.rs`, `tests.rs`               | JSON and SSE, independent choice/tool indexes, fragmented text/arguments, finish reasons plus `[DONE]` and entity end; original evidence is separate from client release. Legacy function fields never prove completion by their presence. Failure emits no success sentinel, partial privileged call, or record. | Same Chat filter.                                                    |
| CH3  | `security/protocol_adapters/chat/history.rs`, `tests.rs`                | Exact ordered delivered assistant/call replay; first/edited tool results remain untrusted; no-session, cross-session/provider/protocol, unknown-reference, eviction and retry negatives.                                                                                                                          | Same Chat filter, then `check_tests proxy::security::`.              |
| AN1  | `security/protocol_adapters/anthropic/request.rs`, `mod.rs`, `tests.rs` | System/message sites, structured tool input string leaves and tool-result text; distinguish readable-but-immutable thinking from opaque signature/redacted payload; fail mandatory immutable patches before forwarding.                                                                                           | `check_tests proxy::security::protocol_adapters::anthropic::tests::` |
| AN2  | `security/protocol_adapters/anthropic/response.rs`, `tests.rs`          | JSON and block-indexed SSE; `input_json_delta`, start/delta/stop consistency, message terminal reason and entity end, tool barrier, error envelopes; thinking immutable from block start, including late `signature_delta`.                                                                                       | Same Anthropic filter.                                               |
| AN3  | `security/protocol_adapters/anthropic/history.rs`, `tests.rs`           | Exact delivered block/call history, first and edited tool results, signed-thinking two-turn replay, immutable authorized-reference failure, absent session/eviction and binding isolation. Synthetic signatures prove preservation, not provider acceptance.                                                      | Same Anthropic filter, then `check_tests proxy::security::`.         |
| GE1  | `security/protocol_adapters/gemini/request.rs`, `mod.rs`, `tests.rs`    | Generation route/query classification, `contents/parts`, native system-instruction spelling, `functionCall.args` and `functionResponse.response` string leaves; no common JSON argument string; unsupported framing denied before reversible forwarding.                                                          | `check_tests proxy::security::protocol_adapters::gemini::tests::`    |
| GE2  | `security/protocol_adapters/gemini/response.rs`, `tests.rs`             | JSON and `alt=sse`, candidate/Part isolation, finish reasons plus entity end, native function argument objects, unknown/usage-only frames and failure projection; immutable signed Parts retain signature association, including late-association hold where required.                                            | Same Gemini filter.                                                  |
| GE3  | `security/protocol_adapters/gemini/history.rs`, `tests.rs`              | Native call/result correlation, exact delivered history and signed Part replay; first results untrusted; use native IDs when present and only unambiguous native ordering when absent. Same-name/parallel ambiguous pairing grants no historical privilege; do not invent IDs or a cursor.                        | Same Gemini filter, then `check_tests proxy::security::`.            |

Before implementation, each lane derives a small protocol-state table in its `tests.rs` comments: indexed states, supported text carriers, allowed terminal reasons, error/incomplete states, transport finalization, and unsupported cases. Reuse strict existing Chat/Anthropic checks; resolve any Gemini terminal-contract uncertainty with protocol evidence before writing a permissive success path. This is a bounded protocol-validation task, not permission to broaden the architecture.

**Lane handoff:** owned files only; frozen base/contract revision; named RED/GREEN evidence and test counts; full lane suite green; shared security regressions green; remaining unsupported cases listed; independent protocol review PASS. Adapter readiness does not claim route enablement or mainline acceptance.

### M2-M4: sequential integration — integrator

For each protocol, complete both tasks before enabling the next one. Apply the lane patch to the current integration state, resolve only owned conflicts, and rerun affected tests; pre-integration evidence alone is insufficient.

| Task  | Shared files and tests                                                                                                                                                                                        | Required verification/outcome                                                                                                                                                                                                                                                                                                                            |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I-CH1 | `handlers.rs` (`handle_chat_completions`), `response_processor.rs`, `forwarder.rs`, `session.rs`, `error_mapper.rs`, `security/http_restoration.rs`, registry; new `security/tests/native_chat_http_tests.rs` | TDD the full native request/response path. Preserve `Admitted` through context/forwarder and consume the returned `dlp_attempt` at the final client exit. Bind actual `openai_chat`, not the current default Responses label. Run `check_tests proxy::security::native_chat_http_tests::`, Chat lane, route-scope, security and protocol-matrix filters. |
| I-CH2 | Registry and `native_route_scope_tests.rs`; evidence file                                                                                                                                                     | Enable only the approved direct Chat/native-Chat cell. Test route aliases, absent/invalid identity, current redaction/restoration, genuine upstream HTTP/SSE errors, pre/post-commit errors, delivery cancellation, unsupported provider retry, and indirect ingress exclusion. Run Integration Gate; record M2 PASS before I-AN1.                       |
| I-AN1 | Same shared files, `handle_messages_for_app` and its response exits; new `security/tests/native_anthropic_http_tests.rs`                                                                                      | TDD native Anthropic final wrapping and distinct Claude/Claude Desktop namespace. Do not enable `handle_claude_transform` conversion cells. Run its native HTTP filter, Anthropic lane, all already-integrated native filters, route-scope, security and protocol-matrix filters.                                                                        |
| I-AN2 | Registry and `native_route_scope_tests.rs`; evidence file                                                                                                                                                     | Enable only the approved direct Anthropic/native-Anthropic cell. Prove first-result and signed-thinking two-turn behavior, conflicts before forwarding/after commit, and Chat/Responses nonregression. Run Integration Gate; record M3 PASS before I-GE1.                                                                                                |
| I-GE1 | Same shared files, `handle_gemini`; new `security/tests/native_gemini_http_tests.rs`                                                                                                                          | TDD retaining `Admitted` (currently discarded), context/forwarder handoff, actual `gemini_native` binding and final wrapping. Determine streaming from generation URI/query, not `body.stream`. Preserve GET/model listing and non-generation routes. Run native HTTP/Gemini lane/all integrated native/route-scope/security/protocol-matrix filters.    |
| I-GE2 | Registry and `native_route_scope_tests.rs`; evidence file                                                                                                                                                     | Enable only approved JSON/`alt=sse` native Gemini cells. Test unsupported framing with/without required restoration, query preservation, signed native calls, first results, absent identity, and Chat/Anthropic/Responses nonregression. Run Integration Gate; record M4 PASS.                                                                          |

**Integration Gate:** all present native HTTP and lane filters, `check_tests proxy::security::`, `check_tests proxy::protocol_matrix_tests::`, `check_tests proxy::session::tests::`, and the Full Gate. If global-state fixtures interfere, retain serial test execution and isolate the cause; do not remove assertions or widen fail-open behavior to get green.

### M5: integrated acceptance — integrator + independent reviewer

- [ ] **F1 — Close the acceptance matrix.** Map architecture Section 9 cases to actual test names and results in the evidence file: byte/delta splits, authority isolation, histories/results, signatures, budgets, completion/transport, retries, errors and disabled cells. Run all mapped filters with nonzero counts on the final integration state. No placeholder rows or unverified claims.
- [ ] **F2 — Update shipped documentation.** Edit `docs/architecture/security-audit.md` and `docs/troubleshooting.md` for exactly the enabled routes, identity sources, signed-unit limitation, unsupported Gemini framing and generic errors. Verify with `pnpm exec prettier --check docs/architecture/security-audit.md docs/troubleshooting.md` and a source/route/readability review. Do not rewrite the architecture's design-only status as evidence that every native client is supported.
- [ ] **F3 — Final review and checks.** Run Full Gate on the final state; record command context, test counts, failures/limitations and independent PASS/FAIL review. Reuse earlier evidence only if covered source/environment inputs are unchanged. Resolve blocking findings via test-first changes and rerun affected checks before completion.

### Full Gate

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
pnpm format:check
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --lib --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
git diff --check
```

Run commands with exit-code checking; one later success must not hide an earlier failure. Full test commands exclude ignored tests by default; never add `--ignored` or live-test environment flags. A failing baseline/final check remains a blocker or an explicitly accepted limitation, not a passing stage. Local fixtures do not establish installer/signing/release or every provider/model's live compatibility.

## 6. Parallel workspace and evidence discipline

- Use one integrator and three implementation lanes; independent reviewers may run read-only in parallel. Agent context isolation alone is **not** filesystem isolation.
- Establish one user-approved CORE-1 Git base containing intended source and the applicable plan/spec. Do not commit or discard the user's dirty tree automatically. If a baseline commit cannot yet be authorized, keep lane implementation paused rather than using a stale HEAD that lacks the frozen core.
- After approval, create three separate worktrees/branches from that exact base. The integrator validates each working directory and `git rev-parse HEAD` before delegation. Briefs include the architecture/plan, contract revision, owned directory, forbidden shared paths, tests and expected handoff; agents do not rely on parent conversation history.
- Give each worktree an exclusive Cargo target directory; do not let different working copies write into one target directory. Bound compilation jobs and concurrent builds to available RAM/disk. Parallel editing/review can continue while heavy checks are scheduled; do not infer a threefold compilation speedup.
- Core amendments are integrator-only and versioned. Do not combine a shared-file edit from one lane with an older core in another lane. Stop/rebase/retest all affected work before accepting a changed contract.
- Evidence records include task ID, source/contract identity, worktree, command, nonzero test count, intended RED cause, GREEN result and independent verdict. Store only sanitized structural results, never fixture plaintext, credentials, raw tool arguments, local configuration, or real session/cursor values.
- Do not stage, commit, push, install, launch, or run live acceptance as an implicit part of this plan. Explicitly authorized baseline/integration commits must use English messages and exclude generated artifacts/secrets.

## 7. Risk controls and rollback

| Risk                                                          | Control and stop condition                                                                                                                                                  |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Three lanes discover incompatible core requirements           | Freeze a non-Responses-tested contract first; use the integrator-only amendment path. Stop a lane rather than fork authority logic.                                         |
| Admission is enabled without a final response wrapper         | Native HTTP tests must observe both tokenized upstream input and restored final client output through the real handler; no direct helper-only acceptance.                   |
| Routing/failover silently reaches an unsupported cell         | Revalidate every actual attempt before sending. A request requiring restoration must fail safely rather than send tokens to an unhandled response path or resend plaintext. |
| Indirect gateway routes acquire shared session privilege      | Trusted ingress classification, explicit negative tests and no new gateway enablement. Consumer-scoped expansion requires a separate plan.                                  |
| Signed payload mutates or partial tool output escapes         | Immutable-unit checks, bounded late-association hold, two-turn fixtures and release/publication negatives. No dual-version cache workaround.                                |
| Native test names conceal Responses-client fixtures           | Separate new native HTTP modules; assert client request shape, actual upstream protocol and final native envelope.                                                          |
| Full-suite instability or resource pressure hides regressions | Record baseline, serialize stateful fixtures/builds where necessary, diagnose flakes, and rerun invalidated evidence. No zero-test or skipped-coverage acceptance.          |

Rollback is protocol-local: disable the new native cell in the internal registry and rerun route-scope/previously-enabled protocol checks, or revert only the integrator-owned change set after approval. Do not clear unrelated user work or relax redaction to keep a broken route available. On disablement, retain the prior unsupported-restoration behavior for requests requiring restoration; ordinary supported nonreversible forwarding remains unchanged. No persistent schema/settings migration is introduced, so rollback needs no storage migration. Memory-only context loss grants no new authority.

## 8. Schedule and approval boundary

Budget under continuous AI-assisted execution with prompt maintainer decisions: M0-M1 approximately 2-4 working days; parallel lanes approximately 2-4 days on the critical path; sequential integration/final acceptance approximately 2-4 days. Total approximately **6-12 working days**, not a measured delivery promise. Re-estimate after CORE-1 using actual test/build times, contract churn and review findings. New conversion cells and live provider acceptance are additional work.

Approval of this plan permits execution within the listed paths and gates; it does not waive TDD, freeze review, native enablement tests, or later independent acceptance. Until approval, only planning/document validation is authorized. The first execution milestone is M0-M1, not launching three implementation agents immediately.

## 9. C3 publication transaction refinement — approved execution spec

**Approved scope:** implement this bounded C3 refinement within the existing shared ownership table, with behavioral TDD and an independent implementation review. P1/P2 history and P3 provenance are recorded in evidence Sections 20-21; Section 22 records final P3/P4 verification and independent scoped PASS, closing only this publication/proof increment. The requirements below remain binding, not permission to enable native adapters. The authoritative architecture is unchanged. C3-C5/CORE-1 remain unchecked.

### Problem and selected approach

At the refinement's baseline, the mandatory SSE publisher committed before the final HTTP wrapper yielded its terminal batch. The two original `c3_mandatory_sse_publication_*` tests were valid behavioral RED, not passing controls. Evidence Sections 17-18 record the corrected provenance increment and this separate defect; Section 21 records the current sealed-path results.

Reject a saved-table swap after yielding: mapping writes change `Namespace::version`, and other namespaces can consume global logical context capacity. A late version/capacity failure cannot retract tools or append another terminal. Also reject a second authority store, mutexes held across yields, reconstruction from raw inputs after release, and using optional-history current-policy eligibility for mandatory completion.

Use unpublished slots **inside the existing store**, with logical capacity and allocation ownership acquired before release; activate only the matching slot after the final wrapper resumes all yields. The trade-off is retained, budgeted slot capacity and bounded publication-owner bookkeeping while a consumer is slow. Concurrent mapping writes and unrelated context publications must not invalidate an admitted pending publication.

### Store state and ownership

All production paths below are relative to `src-tauri/src/proxy/`:

- `security/tokenization.rs`: replace the context index with the existing budget-owned `StoreVec<Option<ContextSlot>>`. Each occupied slot contains one immutable `ContextEntry`, a checked namespace-local slot serial, hidden/visible state, outstanding owner count, and its record charge/logical entry delta. Serial numbers distinguish removal/reuse only; they are not completion or mapping authority. No new UUID or unbudgeted collection is introduced.
- A non-Clone pending owner contains store/namespace/generation/batch binding, slot serial, its own bookkeeping charge and cleanup obligation. **It never holds a record reference.** Hidden records exist only in their slot; no authority reader or pending owner clones them. This removes the overlapping-cancellation reference-retirement race identified in independent candidate review `250986c4025a485897701c7da09506fb`.
- Namespace/global pending logical entry counters reserve the same `1 + opaque_count` units as existing completion, separately from committed entries. Every admission checks committed plus reserved occupancy. Record metadata reservation is charged once per unique hidden record; every owner, including duplicate reuse, has a positive bookkeeping charge under the existing `global_metadata_bytes` ceiling. Checked namespace/slot owner counts are bounded by that finite global ceiling divided by the per-owner charge; there is no existing namespace metadata ceiling. Counter and serial overflow rejects admission before release.
- Slots store plain `Charge` values and logical counters only, never a store-owning `Reservation` or pending/retirement guard. The provisional RAII reservation remains outside `StoreInner`, transfers and disarms its obligation under the final insertion lock, and drops candidate payload before refund on rejection. This prevents a strong store-to-slot-to-store ownership cycle and store-locking destruction inside a slot.
- A namespace publication pin is held per outstanding owner. Idle expiry requires both request leases and publication pins to be zero. Pins survive owner cleanup and any retired record destruction. Existing `Reservation::drop` cannot outlive the namespace it charges.
- Physical context-index backing remains charged at retained capacity, including empty slots. Growing the index temporarily charges old and replacement backing simultaneously. Do not claim that cancellation restores an earlier physical high-water mark; exact-baseline controls distinguish reusable retained backing from leaked record/owner/reservation charges.

### Atomic transitions and retirement

1. **Prepare:** reuse existing actual binding, Responses-scope, principal, dependency, call/delivered-snapshot and opaque validation. Allocate the immutable candidate, its dependency/index backing, final frames and all required payload/metadata reservations before tool release. A provisional reservation owns candidate work until insertion or destruction.
2. **Insert/join:** under the final store lock, recheck live namespace/generation/admitted policy, slot identity, duplicate equality, capacity, serial and all counter arithmetic. A visible identical duplicate acquires only its own owner/pin; it never acquires deletion ownership. A hidden identical duplicate atomically joins its owner count and pin; reserve the record and logical delta once. A conflicting duplicate fails before release. No stale lookup-and-compare result may authorize a join.
3. **Grow if needed:** preallocate and initialize replacement `Option` slots outside the mutex using existing `StoreVec` APIs. Under lock, recheck current capacity/duplicates and all fallible conditions; then move the **current** table's occupied slots into the replacement with no remaining fallible move. Destroy displaced backing outside the lock. Retry only before release. Migrate every writer, including synchronous JSON, so no old whole-table context writer remains. Context-slot mutation does not increment mapping `namespace.version`.
4. **Activate:** after the sealed delivery continuation, validate exact owner/batch/namespace/generation and slot serial, then atomically flip hidden to visible and transfer its record metadata/logical units from reserved to committed once. Ordinary concurrency cannot cause new version, entry-capacity, allocation or raw-input work here. Already-visible equal records are idempotent. Release this owner's count/charge/pin exactly once; another owner may still be pending.
5. **Cancel:** under the cleanup lock, decrement only this owner's obligation. A visible record is never removed. If another owner remains on a hidden record, leave it hidden. The last hidden cancellation detaches that exact slot into a retirement guard, leaving its logical/metadata reservation and last owner pin charged. Drop the detached record outside the mutex; only then refund the detached record reservation and owner bookkeeping and release the pin. The guard carries exact counters, not a future slot lookup; a reused slot cannot be erased/refunded by old cleanup. Nonlast owners have no record references to retire.
6. **Exceptional state:** normal cancellation must not poison or mark the store unavailable. An unpoisoned but unavailable store permits cleanup-only locking/refunds, never activation. A poisoned mutex stays fail-closed; do not recover it as usable or fabricate refunds from uncertain state. Conservatively retain unreconciled store-owned charges until the poisoned store is destroyed and report that limitation separately from normal exact-cleanup tests. No new production invalidation API is needed: pinned namespaces cannot expire. Any tested forced revocation must preserve cleanup obligations and deny activation, not remove a namespace behind a live reservation.

Every authority reader selects visible records **under the mutex** before taking its readable snapshot: cursor/call/opaque resume, independently known-reference expansion and output-call-ID verification (`tokenization.rs::resume_completed_context_extracted`, `resume_completed_tokens_extracted`, `verify_output_call_ids_extracted`). Hidden-only tables count as no completed records. Candidate lookup for duplicate comparison is not an authority reader. Preserve all existing full dependency and operation-transfer semantics for visible records.

### Sealed verifier, finalization and delivery boundary

This refinement includes sealing; do not expose a new pending publisher while deferring this requirement. Concrete type names below are proposed internal signatures, not frozen CORE-1 contracts:

- A private verifier submodule inside `security/http_restoration.rs` owns the final-client protocol/transport verifier and required original tracker. `finish(self) -> Result<VerifiedCompletion, RestoreError>` consumes their real states, checks expected source/exact actual attempt (also with zero mappings), complete transport and framing, and returns a non-Clone proof with private fields. Its constructor/issuer is inaccessible to sibling adapters. There is no boolean constructor or handler-completion parameter. Observation and legacy conversion APIs remain non-authoritative.
- A finalization submodule in `security/response_restoration.rs` consumes `SseRestorationState` and that proof, verifies matching attempt, strict terminal/snapshot/tool consistency, and constructs a non-Clone finalized batch with complete pre-staged frames plus a sealed publication specification. Only this verifier-backed path can construct the specification; the store consumes it rather than accepting raw publication arguments. Consuming finalization removes successful re-entry as an issuance route.
- Collocate the opaque finalized batch, its private delivery state machine and `DeliveryResume` issuer in that same finalization module. Expose only a consuming `FinalizedBatch::into_delivery_stream(self)`; the HTTP wrapper delegates its returned stream, never supplies or extracts a frames/pending pair. Each batch has a budget-owned opaque release identity tied to its exact attempt/framing (correlation, not authority). The stream owns all frames and its pending owner, yields them in order, and constructs a matching private `DeliveryResume` only in the continuation after the last yield. `activate(self, DeliveryResume)` consumes both. Re-export only the opaque verifier proof/publication-spec/resume types through their ancestor modules at the common `security` scope so sibling consumers can name them; fields and issuance remain private to the owning submodule. Do not attempt Rust sibling-only `pub(in ...)` visibility or expose raw assembly/extraction. Poll/drop can consume or cancel the stream, not skip the release state machine. An unpolled stream never publishes.
- Remove production `CompletionWitness::from_verified_completion(bool)`, the ignored-witness wrapper and raw callable publication path in `completed_context.rs`/`tokenization.rs`. Any low-level fixture construction is explicitly `#[cfg(test)]`, inaccessible to production adapters. Compile-time privacy checks supplement, but do not replace, runtime behavioral RED/GREEN with real strict donor proofs and mismatched attempts/batches.
- The JSON path uses the same slot transaction and sealed specification, but retains its existing boundary: original-body verification, restoration and mandatory charged final-serialization validation precede activation and history publication in both changed and unchanged cases. A private JSON serialization owner issues matching activation permission only after that encoding succeeds. Unchanged delivery still uses the original input bytes and discards the validation encoding; never replace original bytes with a reencoded equivalent. Do not defer JSON publication until an SSE-only yield boundary. Preserve final-encoding-denial negatives (including unchanged output), pre-body-poll publication positives, headers/bytes and temporary-owner cleanup controls in `tests/json_restoration_tests.rs`.

Allocation failure, conflicting duplicates, invalid protocol evidence and strict snapshot/tool failure occur before releasing privileged terminal/tool output. Current admitted mandatory policy revision remains valid for that request; optional delivered history retains its separate current-policy check. If actual store unavailability/revocation occurs after terminal release, never publish, append `response.failed`, retry raw input, or erase an existing record. Propagate a fixed sanitized body-stream error without a second protocol terminal. This is exceptional failed delivery, not synthetic successful completion or remote-receipt proof; add an explicit control for the projection and acceptance limitation.

### Ordered behavioral tasks and proving commands

Use the repository-root `check_tests` convention in Section 5 (list first, require nonzero count, then run serially). New tests stay in `security/tests/unified_core_tests.rs` and existing security test files; helpers remain in `unified_test_support.rs`. A compiler error, timeout, skipped filter or zero-match listing is never RED. For a new boundary, run its failing test before changing implementation. Do not use `--skip` as acceptance.

| Task | Files and acceptance                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Test command                                                                                                                                                                                                                                                                                                                                                                                               |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1   | Strengthen the existing publication tests with separate cursor, call, opaque and mapping-authority absence probes; mapped and zero-token branches must run independently. Add cancellation at each release yield, unpolled cancellation and completed positive controls.                                                                                                                                                                                                                                        | `check_tests proxy::security::unified_core_tests::c3_mandatory_sse_publication_`                                                                                                                                                                                                                                                                                                                           |
| P2   | In `tokenization.rs`, implement hidden-slot visibility and reservation/retirement only behind runtime controls: same-namespace mapping writes, two distinct pending records, global capacity contention, duplicate join/visible reuse/conflict, removal/reuse during admission, overlapping duplicate cancellations and exact cleanup. Update every context reader/writer and idle expiry.                                                                                                                      | `check_tests proxy::security::unified_core_tests::c3_publication_transaction_` then `check_tests proxy::security::reversible_context_tests::`                                                                                                                                                                                                                                                              |
| P3   | In `http_restoration.rs`, `response_restoration.rs`, `completed_context.rs`, `tokenization.rs` and `restoration_core.rs`, seal consuming proof/finalization/release and wire the final wrapper. Add isolated same-provenance/wrong-protocol, wrong-attempt/batch, repeated finalization, interruption, unavailable activation, policy-change and serialization-failure controls. Strict verification helpers may change only in approved `completion_evidence.rs`, `completion_json.rs`, `completion_tools.rs`. | `check_tests proxy::security::unified_core_tests::c3_` then `check_tests proxy::security::reversible_completion_tests::`                                                                                                                                                                                                                                                                                   |
| P4   | Verify allocation denial before release, retained/overlapping index charges, owner-count exhaustion, final store destruction without an ownership cycle, no privileged partial release, fixed errors/headers/native byte preservation and current Responses/Compact conversion cells on the integrated source. Obtain independent implementation PASS; update evidence with actual hashes/commands/results. This completes only this publication/proof increment, not remaining shared-mechanics extraction/C3. | `check_tests proxy::security::unified_core_tests::`; `check_tests proxy::security::`; `check_tests proxy::protocol_matrix_tests::`; `check_tests proxy::session::tests::`; `check_tests proxy::session_identity_tests::`; `cargo check --manifest-path src-tauri/Cargo.toml`; `cargo fmt --check --manifest-path src-tauri/Cargo.toml`; `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` |

Document checks: `pnpm exec prettier --check docs/design/security-dlp-unified-core-parallel-implementation-plan.md docs/design/security-dlp-unified-core-implementation-evidence.md` and source/contract-readiness review. Full Gate remains required before C5/CORE-1. Stay within the shared ownership table: no payload/store-owner module edit, new dependency/storage schema, native adapter/route implementation, application launch/install, staging/commit or worktree creation. If those constraints prevent safe implementation, stop for a scoped amendment rather than weakening the transaction.

## 10. Approved Chat-only integrator amendment: current execution boundary

**Do not distribute the new shared contract yet.** The user approved a bounded integrator amendment after the Section 53 readiness findings and subsequently authorized one C5 test-file extension. [Evidence Section 54](security-dlp-unified-core-implementation-evidence.md#54-authorized-chat-only-shared-amendment-execution-checkpoint) records the exact RED/GREEN attempts, interrupted execution, parent checks and bounded independent F1 closure. Latest parent checks pass **24 CH2-focused tests / 1941 shared-security tests**; these do not close the amendment or any lane.

### Owned changes and fixed boundaries

Rust paths are relative to `src-tauri/src/proxy/security/`: `completion_json.rs`, `completion_evidence.rs`, `completion_tools.rs`, `http_restoration/completion_verifier.rs`, `restoration_core.rs`, `restoration_core/native_response.rs`, `response_restoration/finalization.rs`, `tests/unified_core_tests.rs`, and, if needed, `tests/native_test_support.rs`. The additional authorization covers **only** the no-data/DONE characterization in `tests/c5_native_sse_tests.rs`, now named `c5_native_sse_frame_no_data_and_done_have_no_source_or_sites`. Documentation changes are limited to this plan and the evidence record. The architecture remains authoritative and unchanged.

Preserve default conversion behavior, exact token/store/session/generation/provider/operation/actual-attempt binding, existing budget ownership, one dependency ledger, strict independent final validation, charged encoding, sealed publication and SSE last-yield continuation. Do not change the three request lanes, merge lanes, implement complete CH2/CH3, expand Anthropic/Gemini proof, ingress, identity sources, routes or the conversion matrix. No new dependency, schema, UI, setting, credential source or token store. Native registrations remain **Disabled**; no post-baseline commit or push is authorized by this checkpoint.

### Execution checklist and stop conditions

- [x] Obtain genuine runtime RED before explicit native multi-choice JSON and SSE/tool-index mechanics; keep conversion constructors strict.
- [x] Obtain genuine runtime RED before decoded object-argument staging; retain full tool hold, mixed-source rejection and a single ledger.
- [x] Obtain genuine runtime RED before exact ordered choice selection and complete assistant/call snapshot composition; keep legacy frame projection separate and independently validate actual final bytes.
- [x] Obtain genuine runtime RED before native carriers/no-data support; retain known-field type checks and no source/sites/completion authority for keepalives.
- [x] Obtain explicit authorization before reconciling the single old C5 no-data assertion; retain data and forged source/sites negatives. Treat this update and the later sealed-refusal/extension test as characterization, not RED.
- [x] Verify the independent snapshot F1 against strict SSE null semantics; obtain four genuine runtime failures, then retain non-null tool identity and strictly validate assembled messages/calls. The historical independent FAIL is not superseded by parent GREEN alone.
- [x] Close snapshot F1 with a bounded read/search/find-only independent re-review; record explicit unreviewed boundaries.
- [ ] Complete typed policy selection for readable refusal/reasoning without using ordinary output restoration for reasoning. Add genuine runtime RED before production changes in the already owned native staging file; reuse `TextPolicy` and existing lifecycle behavior, not a new policy/store owner.
- [ ] Establish supported semantics and tests for evolving opaque/structured snapshot fields, strict final-candidate contradictions, immutable conflicts, resource refunds and cancellation. Stop for design/scope approval rather than inventing protocol semantics or adding unowned files.
- [ ] Re-run current-source shared-security, protocol-matrix and session selections with the common lock, jobs bound, successful nonzero listing and single-thread runtime. Keep any ignored count separate; do not execute ignored/live tests.
- [ ] Apply bounded Rust formatting only to approved changed files with `--config skip_children=true`; preserve unrelated Rust/Markdown formatter history. Run diff checks, Cargo check and Clippy under the same serialization rule.
- [ ] Update signatures/visibility and input identity, pass human fresh-reader and LLM execution-readiness document gates, freeze the candidate and obtain independent whole-amendment PASS before distribution or any lane rebase/consumption.

### Reproducible checks and acceptance limits

From the repository root, use a unique artifact stem per selection:

```bash
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  python3 .voidx/verification/core1-20261106/run-native-core-focused.py \
  UNIQUE-STEM 'proxy::security::unified_core_tests::ch2_core_'
```

Repeat with `proxy::security::`, `proxy::protocol_matrix_tests::`, and `proxy::session::tests::`. The local parent runner records complete pre/post input maps and acquires `native-lane-cargo.lock`; it requires successful nonzero discovery before runtime. The underlying commands are `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, then the same selection with `-- --test-threads=1`. Cargo check and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` must also run globally serial with `CARGO_BUILD_JOBS=2` and the main target. No overlapping lane Cargo is permitted.

All checks use synthetic values, in-memory stores and dynamic loopback ports where transport is needed; never use the application port, real credentials, ignored/live checks or app launch/install. Parent executions are not reviewer execution evidence. A focused GREEN is not full protocol/HTTP or release acceptance. The existing linker warning remains disclosed. Lane resource handoff gaps, CH2/CH3, AN2/AN3, GE2/GE3, full-protocol lane reviews, sequential M2–M4 and M5 remain open. Do not close the overall workflow.

## 11. Approved typed-policy increment: execution result and remaining hold

**Do not distribute the shared contract or integrate lanes yet.** The user approved the four-file typed-policy subplan. [Evidence Section 55](security-dlp-unified-core-implementation-evidence.md#55-approved-typed-policy-increment-parent-verification-not-amendment-acceptance) records genuine runtime RED, API-construction changes, GREEN, characterization and final-source parent checks: **CH2 38/38, security 1955/1955, session 34/34, protocol matrix 2 passed / 1 ignored**, plus Cargo check/Clippy and bounded Rust formatting. Independent review is **PASS for the typed-policy increment only**; whole-amendment acceptance is not granted. Section 10 remains the historical pre-increment checkpoint.

### Completed scoped work, not a frozen API

- [x] In `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, first reproduce JSON/SSE reasoning restoration at runtime with the existing Output-default API. Two Output/Refusal controls passed; two reasoning safety assertions failed. Compilation-only failures are not RED.
- [x] In `src-tauri/src/proxy/security/restoration_core/native_response.rs`, add `NativeJsonObservation::new_with_policies(..., policies, budget)` and `NativeSseTextStream::with_policy(fragments, policy)` using existing `TextPolicy`/lifecycle. Keep default constructors unchanged. Explicit callers select Reasoning rather than relying on field-name inference. Tests switch construction only, without weakening safety assertions.
- [x] Obtain eight genuine runtime guard failures, then reject JSON policy cardinality/source mismatches, explicit policies on non-lifecycle JSON staging, and SSE per-fragment source/policy disagreement before matcher work. Keep strict ToolArguments object decoding, immutable checks, exact site coverage, one charged dependency ledger and binding. SSE's old entry cannot receive explicit stream policies; no runtime rejection on that entry is claimed.
- [x] Characterize shared-ledger mixed policies, unchanged immutable reasoning, charged policy-vector allocation failures/refunds, split refusal/reasoning, sealed delivered history, borrowed byte preservation and canceled last continuation. Characterization/control passes are not new RED.
- [x] Run final-source regressions via the Section 10 serialized runner and unique stems, and check/Clippy with the same `native-lane-cargo.lock`, `CARGO_BUILD_JOBS=2` and main target. Run `rustfmt --edition 2021 --config skip_children=true --check src-tauri/src/proxy/security/restoration_core/native_response.rs src-tauri/src/proxy/security/tests/unified_core_tests.rs` and `git diff --check`; both pass. The known linker unwind warning remains.
- [x] Pass dual-audience documentation gates and bounded independent typed-policy review; record exact reviewed boundaries without elevating parent checks to independent execution. Rune returns scoped code PASS; Lyra returns human fresh-reader and LLM checkpoint-readiness PASS, with no mandatory findings. Section 55 preserves the nonblocking explicit SSE ToolArguments positive-test gap.
- [ ] Obtain an approved design for evolving opaque/structured snapshot fields and final-candidate contradictions/immutable conflicts, then implement authorized tests/changes test-first. Do not infer protocol merge semantics from this increment.
- [ ] Revalidate affected checks after further changes, freeze the whole amendment's identity/visibility/signatures and obtain independent whole-amendment PASS before distribution or lane consumption.

Only the two Rust files above and appends in this plan and `docs/design/security-dlp-unified-core-implementation-evidence.md` belong to this increment. Preserve all previous code/docs and the authority architecture. No changes to other shared/C5 files, three request lanes, protocol conversions, HTTP proof issuers, snapshot composer, ingress, routes, dependencies, storage, UI or settings are authorized. No new commit/push; native registrations remain **Disabled**. CH2/CH3, AN2/AN3, GE2/GE3, lane resource handoff, sequential M2–M4, M5 and live/release acceptance remain incomplete. Whole-project/full-protocol acceptance is not implied by the bounded commands above.

## 12. Conservative Chat snapshot increment: execution plan

**Awaiting implementation-plan approval.** The user approved the conservative design direction; this executable plan and [evidence Section 56's contract](security-dlp-unified-core-implementation-evidence.md#56-conservative-chat-snapshot-contract-approved-design-implementation-pending) do not themselves authorize production edits. First pass both documentation audience gates and obtain approval, then follow the ordered TDD tasks below. The entire Chat-only shared amendment remains unfrozen/unaccepted; native registrations remain **Disabled** and the three isolated lanes remain unintegrated.

### Goal, approach and owned files

Close the Chat-only shared amendment by rejecting ambiguous opaque snapshot evolution, preserving known delta/identity semantics, and proving the cumulative amendment before any lane consumption. Keep the existing Rust/serde_json projection, borrowed native inputs, `PayloadBudget` owners and shared lifecycle. Replace opaque last-value-wins only inside the existing composer with bounded first-value/equal-repeat selection; preserve strict original/final validation and the sealed delivery path. No new library, authority owner or storage is needed.

Only these four repository files may change relative to `.voidx/verification/core1-20261106/ch2-core-opaque-start-01.json` and its four snapshots:

| Exact path                                                              | Responsibility in this increment                                                                                                                                                                                                |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/proxy/security/restoration_core/native_response.rs`      | Sole production edit: conservative opaque selection, bounded inspection and explicit known-text/structured-reasoning dispatch in `compose_chat_snapshots`; keep existing public/internal API shapes unless separately approved. |
| `src-tauri/src/proxy/security/tests/unified_core_tests.rs`              | Test-first conflicts, independent history/wire oracles, supported controls, resource/cancellation cases and explicit SSE ToolArguments coverage. Test helpers stay in this file.                                                |
| `docs/design/security-dlp-unified-core-parallel-implementation-plan.md` | Append English plan, approval and closure checkpoint; do not revise historical sections/status headings.                                                                                                                        |
| `docs/design/security-dlp-unified-core-implementation-evidence.md`      | Append English contract, actual RED/GREEN/control history, candidate identity, current checks and independent verdicts.                                                                                                         |

Read-only references: `docs/design/security-dlp-unified-core-protocol-adapters.md` (authority); `src-tauri/src/proxy/security/completion_json.rs`, `completion_evidence.rs`, `completion_tools.rs` (native/conversion strict checks); `restoration_contract.rs`, `restoration_core/lifecycle.rs` (typed policy/dependencies); `http_restoration/completion_verifier.rs`, `response_restoration/finalization.rs` (real EOF, final wire and sealed release); `tests/native_test_support.rs` (existing synthetic admission/transport fixtures), all Rust paths under `src-tauri/src/proxy/security/` unless fully qualified. Preserve existing dirty changes in those read-only files. Local `.voidx/verification/core1-20261106/` artifacts may be added with unique stems; do not modify runners or overwrite evidence.

### Fixed invariants and stop conditions

Evidence Section 56 is the support/null/equality table. Missing never erases a value; opaque null is a real value; different opaque values reject without string concatenation, array/index synthesis or deep merge. Known text/null and non-null identity semantics remain separate. Non-text `reasoning` uses opaque rules; readable string `reasoning` still needs explicit typed policy. Complete native JSON content-array support remains unchanged; SSE arrays are not added. Unknown payloads are never scanned/restored for tokens. Preserve first-value object order, ordered arrays, ordered complete assistant/call snapshots, exact node/site/attempt/binding coverage and one dependency ledger.

The composer gets one shared **4 Mi work-unit allowance per invocation**, depth **64**, charged at field-occurrence lookup, node visit and inspected key/string byte length; first opaque values are bounded even without repeats. Use the existing `same_value` pattern without unbounded `Value` equality or uncharged cloning/serialization. Keep existing charged metadata/output and RAII cleanup. This is a CPU bound plus existing memory accounting, not a new token/work store or a budget reset per field. Stage and final-wire composition each apply the same rules with their own bounded invocation.

Do not modify other shared production files, C5, isolated Chat/Anthropic/Gemini lanes, HTTP proof/finalizer, routes, registry, conversion matrix, trusted ingress or identity sources. Preserve CORE-1, Responses, P3/P4 and all historical work. No new dependency, schema, storage, UI, setting, token store, `unwrap()`/`expect()` in proxy code, fail-open forwarding, early tool release or publication. No commit/push, ignored/live tests, real credentials, application launch/install or lane enablement. If correct enforcement or any test requires new protocol semantics, an API expansion or an unowned production/test file, **stop before making that change and request separate approval**. An unexpected regression requires evidence-based root-cause investigation, not weakening an assertion or skipping coverage.

### Ordered tasks and test matrix

All new test functions use `ch2_core_opaque_` except the explicit-policy positive `ch2_core_policy_sse_explicit_tool_arguments_restore_with_one_ledger`. These names are planned additions, not existing coverage. The shared helper command below selects each group by substring, lists first and runs single-threaded only after successful nonzero discovery. No number of expected tests is an execution result until logs establish it.

| Order | Small action and exact edit scope                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Proving selection / expected result                                                                                                                                                                                                                                                                                             |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | In `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, add conflict tests with valid synthetic Chat streams for unknown message/tool/function fields and structured reasoning. Include changed nested values, unknown string evolution, reversed arrays, type switches and null/non-null conflicts in both directions. Verify current proof/staging setup reaches the intended composition boundary.                                                                                                                 | `ch2_core_opaque_conflict_`: genuine runtime failures on accepted ambiguous snapshots before any production edit; original-frame/type setup errors are not the intended RED. Record each failure assertion.                                                                                                                     |
| 2     | Still before any production edit, in `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, add work/depth/low-budget and allocation/copy-failure cases. Isolate composer inspection from earlier frame/transport limits using direct `compose_chat_snapshots` calls where necessary. Include repeated large equal values whose individual comparisons fit but aggregate exceeds 4 Mi work, plus first-value bounds.                                                                                                    | `ch2_core_opaque_resource_`: require genuine runtime RED for the absent aggregate-work cap. Record actual failing assertion/cause. First-value size/depth, low-budget and allocation/refund cases may already reject through existing output/parser/budget limits: first-run passes are controls, never RED.                    |
| 3     | In `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, before production edits, add controls for single/omitted/equal opaque scalars, strings, objects, arrays and null; reordered object membership with first-object field order retained; literal reference-shaped opaque text untouched/no dependency; split known text/null/empty strings and escaped arguments; interleaved choices and sparse choice-local tools. Hand-author full assistant-plus-call expected history and serialized ordering expectations. | `ch2_core_opaque_supported_`: record initial results without claiming all pass. First-run passes are characterization/controls. Any missing first-value/order behavior that fails at runtime becomes an additional recorded RED before implementation. Supported controls must also reach sealed delivery/history verification. |
| 4     | Only after tasks 1–3 establish intended runtime failures, in `src-tauri/src/proxy/security/restoration_core/native_response.rs`, implement conservative first-value/equal-repeat selection together with shared checked work accounting and first-value depth-bounded traversal using the existing comparison pattern. Split known text from non-text reasoning; preserve F1 identities and strict assembled validation. Both assistant and standalone-call emission consume the same invocation allowance.                  | Re-run `ch2_core_opaque_conflict_`, `ch2_core_opaque_resource_` and `ch2_core_opaque_supported_`: all GREEN with unchanged safety assertions. Do not introduce temporarily unbounded equality or defer work/depth guards.                                                                                                       |
| 5     | Check the integrated producer against prior snapshot/typed-policy tests and all new opaque tests without widening production scope. Refactor only behind green tests.                                                                                                                                                                                                                                                                                                                                                        | `ch2_core_snapshot_`, `ch2_core_policy_`, then `ch2_core_opaque_`: nonzero, all nonignored GREEN. All temporary payload/metadata charges return to measured baselines on rejection/drop; no uncharged retained copy.                                                                                                            |
| 6     | In `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, add final-wire contradictions with genuine original HTTP proof: changed opaque value, previously hidden earlier value, reordered arrays/choices/calls and conflicting final candidate. Use hand-authored expected history, not composer-generated expectations. Include immutable editable-site restoration conflict and opaque-only immutable preservation. Reuse existing final verifier entry; do not expose proof constructors.                           | `ch2_core_opaque_final_`: final validation rejects before tools/release/publication; stable original/wire controls pass. Record existing-behavior passes as controls. A failing immutable/finalizer boundary outside the sole owned producer is a stop, not permission to edit it.                                              |
| 7     | In `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, test complete delivery, unpolled cancellation and drop after the last yield before continuation with stable opaque history; check pending/visible record counts and full refunds. Add explicit SSE `with_policy(..., TextPolicy::ToolArguments)` with fragmented escaped object arguments, restored output and the same single ledger. Preserve wrong-attempt/EOF/truncation negatives through existing fixtures/tests.                                       | `ch2_core_opaque_delivery_`, then `ch2_core_policy_sse_explicit_tool_arguments_restore_with_one_ledger`: nonzero GREEN. Existing-correct behavior is characterization; no new RED is claimed for it.                                                                                                                            |
| 8     | Run bounded formatting/checks and all four current-source regression selections; inspect exact source maps and unchanged pre/post inputs. Production/test paths remain the two approved Rust files; no broad formatter write.                                                                                                                                                                                                                                                                                                | Commands below: zero exits, nonzero listed tests, all nonignored runtime tests pass; ignored counts separate. Document actual counts/warnings, not assumed historical counts.                                                                                                                                                   |
| 9     | Append results to the two owned documents; audit the entire cumulative amendment since CORE-1, callable signatures/visibility, input identities and current evidence. Obtain dual-audience documentation PASS and independent whole-amendment PASS before marking the candidate frozen/accepted.                                                                                                                                                                                                                             | Append-only boundary/hash checks and `git diff --check`; independent read/search/find-only review against actual cumulative changes and exact parent evidence. A scoped opaque/policy PASS is insufficient.                                                                                                                     |

Conflict RED and missing aggregate-work RED must both precede **any** production change. Do not demand RED for a guard already enforced by an earlier charged parser/output limit; preserve it as a control and document which boundary rejected it. For each newly required behavior: write the failing test, run it, confirm the intended runtime assertion, then implement only that behavior. A compiler error, zero discovery, timeout or first-run passing control is never RED. Keep negatives/controls distinct in evidence. Test assertions report fixed sanitized labels rather than dumping JSON/tool inputs. Use synthetic fixtures, in-memory stores/temp directories and dynamic ports only; never compete with the application on port 15722.

### Reproducible commands and serialization

Run from the repository root on the main worktree/target. All Cargo invocations, including listing/check/Clippy, are globally serial with `.voidx/verification/core1-20261106/native-lane-cargo.lock`, `CARGO_BUILD_JOBS=2` and `CARGO_TARGET_DIR="$PWD/src-tauri/target"`. No lane Cargo may overlap. The unchanged focused runner already acquires this lock, saves full pre/post source maps and uses `-- --test-threads=1` after successful nonzero listing. Replace `UNIQUE-STEM` for every attempt; never reuse historical names:

```bash
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  python3 .voidx/verification/core1-20261106/run-native-core-focused.py \
  UNIQUE-STEM 'proxy::security::unified_core_tests::ch2_core_opaque_conflict_'
```

Use the same command with each task's filter, prefixing short selections with `proxy::security::unified_core_tests::`. The final four separate selections are exactly:

```text
proxy::security::unified_core_tests::ch2_core_
proxy::security::
proxy::protocol_matrix_tests::
proxy::session::tests::
```

The actual Cargo commands are `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, then `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1`. Do not add `--ignored`, live flags or skips. Record list exit/count, runtime summary/exit, environment, source hashes and sanitized failure cause. Ignored tests stay unexecuted.

For check and Clippy, this lock wrapper also records unique logs and complete pre/post maps (replace the stem once per invocation of the wrapper; it executes the two commands sequentially and stops on failure):

```bash
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
python3 - UNIQUE-STEM <<'PY'
import fcntl, hashlib, json, os, re, subprocess, sys
from pathlib import Path
root = Path.cwd()
base = root / '.voidx/verification/core1-20261106'
stem = sys.argv[1]
assert re.fullmatch(r'[a-z0-9-]+', stem)
record_path = base / (stem + '-source.json')
assert not record_path.exists()
lock = (base / 'native-lane-cargo.lock').open('a')
fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
assert os.environ['CARGO_BUILD_JOBS'] == '2'
assert Path(os.environ['CARGO_TARGET_DIR']) == root / 'src-tauri/target'
def hashes():
    paths = set()
    for args in (['ls-files', '-z'], ['ls-files', '--others', '--exclude-standard', '-z']):
        paths.update(p for p in subprocess.check_output(['git', *args]).decode().split('\0') if p)
    return {p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
            for p in sorted(paths) if Path(p).is_file() and not Path(p).is_symlink()}
record = {'cwd': str(root), 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
          'target': os.environ['CARGO_TARGET_DIR'], 'jobs': '2', 'pre_sha256': hashes(), 'commands': []}
record_path.write_text(json.dumps(record, indent=2) + '\n')
status = 0
for name, tail in [('check', []), ('clippy', ['--', '-D', 'warnings'])]:
    command = ['cargo', name, '--manifest-path', 'src-tauri/Cargo.toml', *tail]
    log_path = base / (stem + '-' + name + '.log')
    with log_path.open('x') as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    record['commands'].append({'command': command, 'log': str(log_path), 'exit': result.returncode})
    record['post_sha256'] = hashes()
    record_path.write_text(json.dumps(record, indent=2) + '\n')
    if result.returncode or record['pre_sha256'] != record['post_sha256']:
        status = result.returncode or 3
        break
sys.exit(status)
PY
```

Bounded Rust formatting and whitespace checks:

```bash
rustfmt --edition 2021 --config skip_children=true --check \
  src-tauri/src/proxy/security/restoration_core/native_response.rs \
  src-tauri/src/proxy/security/tests/unified_core_tests.rs
git diff --check
```

If formatting changes are needed, run the same rustfmt command without `--check` **only** for those two files, then revalidate executable input identity/affected evidence. Do not use whole-tree formatter writes. Format only the appended Markdown suffixes using installed `pnpm exec prettier --parser markdown` on stdin, append the result after verifying the original byte prefixes, then check suffix idempotence. Whole-document formatting would rewrite history and is forbidden. `pnpm format:check` does not cover these Markdown files. Parent-local boundary evidence must compare all tracked/untracked non-artifact source inputs against the start manifest, confirm exactly the allowed increment, and prove both historical document prefixes unchanged.

### Whole-amendment freeze and handoff gate

Do not confuse two baselines: the four-file snapshot increment is measured against `ch2-core-opaque-start-01`; the whole shared amendment is measured against CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`. Preserve the existing 11-file dirty HEAD diff, earlier runtime logs and all scoped/historical review verdicts. New evidence must identify which baseline it uses.

The freeze review must cover cumulative `completion_evidence.rs`, `completion_json.rs`, `completion_tools.rs`, `http_restoration/completion_verifier.rs`, `response_restoration/finalization.rs`, `restoration_core.rs`, `restoration_core/native_response.rs`, `tests/c5_native_sse_tests.rs`, `tests/unified_core_tests.rs` (all under `src-tauri/src/proxy/security/`) and the appended plan/evidence contracts. Audit API signatures/visibility, native versus conversion strictness, exact choices/sites/attempts, typed policies, opaque semantics, budgets/refunds, immutable behavior, strict original/final validation, real EOF, tool hold, sealed publication and last-yield continuation. Resolve mandatory findings through verified feedback; if resolution exceeds four-file ownership, stop for approval.

Assign a candidate contract identifier and full source/contract input manifest; map each current regression/log and reviewed cumulative diff to it. Reviewers may use **read/search/find only**: no commands, edits, LSP, hashing or delegation. They may inspect parent logs but must not be credited with independently executing/hashing those checks. Record human fresh-reader and LLM execution-readiness PASS for the final docs and explicit independent **whole-amendment PASS**, with any unreviewed limits. Only then record the final frozen identity/acceptance checkpoint. A documentation-only append may reuse executable evidence only after confirming every executable/config/test input unchanged; changed or uncertain inputs require affected checks/review again.

This gate closes only the Chat-only shared amendment. The listed bounded checks do not establish Full Gate, complete CH2/CH3, AN2/AN3, GE2/GE3, lane resource handoff, M2–M5 or live/release acceptance. Those retain their own authorization, native HTTP tests, current-tree/full-protocol reviews and sequential **M2 → M3 → M4**, then **M5** gates. Do not rebase, distribute to lanes, integrate, enable registrations, commit/push or launch/install the application as an implicit consequence of this plan.

### Documentation readiness checkpoint

The first independent document review returned NEEDS_CHANGE on TDD ordering and resource RED/control classification. Both were corrected in this new section, not historical Section 11. Flux `f028ebe63de146acaec978cf15b0bf9a` then returned **human fresh-reader PASS / LLM execution-readiness PASS**, explicitly resolving both findings with no mandatory additions; read/search/find only, no independent command/test/hash execution. Evidence Section 56 preserves both decisions and parent suffix-format/source-boundary checks. This is readiness to request approval, not approval itself: production/tests remain unchanged for this snapshot increment. Await explicit approval of this four-file plan before starting tasks 1–3, and keep all later freeze/lane/integration gates intact.
## 13. Snapshot implementation checkpoint and remaining freeze tasks

**Candidate 02 remains unfrozen and unavailable to lanes.** Section 12 was explicitly approved by the user before implementation; its historical pending/readiness text is not edited. Snapshot composition and verified whole-review F1 feedback now have runtime evidence. The next steps below are freeze/review tasks, not permission to integrate or enable anything. This mixed human/LLM checkpoint links to [evidence Section 57](security-dlp-unified-core-implementation-evidence.md#57-conservative-snapshot-implementation-and-whole-review-f1-checkpoint) for results and failure classification.

### Implemented scope and evidence

- [x] In `src-tauri/src/proxy/security/tests/unified_core_tests.rs`, establish 21 genuine snapshot runtime failures after successful nonzero listing and before composer changes (`ch2-core-opaque-red-02-mapping`). The preceding helper compile failure is not RED; first-passing resource/characterization cases are controls.
- [x] In `src-tauri/src/proxy/security/restoration_core/native_response.rs`, preserve the first opaque value, require bounded structural-equal repeats, distinguish missing from opaque null, and use one composer-wide 4 Mi work allowance/depth 64. Preserve ordered known-text deltas, identity-null protection and strict complete assistant/call validation. Snapshot GREEN: 23/23; expanded candidate 01 CH2: 73/73.
- [x] Verify Onyx's whole-amendment FAIL finding before fixing it. `ch2-core-isolation-red-01` lists 7 and runs 6 genuine failures plus 1 positive control: same-source/policy cross-choice, legacy-selection cross-choice, same-choice cross-call and cross-carrier streams were incorrectly accepted. Zero publication/refund checks precede the intended failures.
- [x] Add only private exact frame-node-derived choice/call/text-carrier keys in `native_response.rs`; reject mixed keys before lifecycle/matcher work, with charged metadata/work. No public staging signature or API expansion. `ch2-core-isolation-green-02`: 7/7. The earlier `green-01` lock-wait timeout never entered runtime and is neither RED nor GREEN.
- [x] Add sparse reordered-call positive coverage without claiming a new RED. Final `ch2-core-isolation-controls-01`: CH2 81/81; `final-security-01`: 1998/1998; `final-session-01`: 34/34; `final-matrix-01`: 2 passed / 1 ignored. All use the full `ch2-core-isolation-` prefix. `ch2-core-isolation-check-clippy-01`: Cargo check and Clippy `-D warnings` both exit 0; two-file rustfmt and `git diff --check` pass. These are parent executions, not reviewer executions.
- [x] Save `chat-native-shared-conservative-20261108-candidate-02` as `ch2-core-isolation-candidate-02.json`, including full source input map, cumulative nine-file Rust diff (`-whole.diff`) and F1 two-file incremental diff (`-increment.diff`). Preserve candidate 01 and all earlier logs.

All artifacts are under `.voidx/verification/core1-20261106/`. The executable SHA-256 values and exact reproduction filters/commands are in evidence Section 57. The focused runner remains unchanged, requires the shared Cargo lock, uses `CARGO_BUILD_JOBS=2` and main `src-tauri/target`, and executes single-thread runtime only after successful nonzero listing. No ignored/live test or real credential was used. The linker compact-unwind size warning remains recorded, not suppressed.

### Remaining ordered actions and acceptance criteria

1. **Whole-amendment code decision:** independently inspect all nine Rust paths listed in Section 12 against CORE-1, not just F1 or opaque/policy changes. Vesper `0cc25edd17e04a64b83e3f59fa71c167` is reviewing candidate 02; no verdict is assumed. Retain the full returned report and explicit whole-amendment PASS/FAIL with limits. Read/search/find only; no commands, edits, LSP, hashes or delegation. A mandatory finding goes through code verification and genuine runtime RED before production changes. Stop if it needs an unowned file, API extension or protocol semantics.
2. **Final document gates:** append results only to this file and `docs/design/security-dlp-unified-core-implementation-evidence.md`. Preserve every historical byte prefix/status header. Format only the new Markdown suffix with `pnpm exec prettier --parser markdown` on stdin and verify suffix idempotence. Obtain human fresh-reader and LLM execution-readiness PASS for the latest appended checkpoint, including explicit current versus historical evidence and remaining holds.
3. **Final identity/evidence mapping:** compare all tracked/untracked non-artifact inputs with `ch2-core-opaque-start-01.json`, confirming the allowed four-file increment, and with candidate 02, confirming only appended docs differ. Verify original document prefixes, exact source/config/test map, unchanged runner and retained evidence. Use two-file rustfmt plus `git diff --check`; preserve command results in a new parent manifest. Reuse executable results only when every executable/config/test input remains identical. Changed/uncertain inputs require affected tests/checks and re-review.
4. **Freeze checkpoint only after steps 1–3:** append the actual final frozen contract identity, source/contract manifest and current evidence/review mapping. Until then the amendment remains unfrozen/unaccepted and cannot be consumed by lanes. A document PASS or another scoped code PASS cannot substitute for whole-amendment PASS.

### Invariants, forbidden changes and later gates

Maintain exact attempt/binding, one dependency ledger, allocation/work budgets and refunds, immutable restrictions, strict original/final validation, real HTTP EOF, complete tool hold, sealed publication and last-yield SSE continuation. The final verifier and staging share the composer; do not claim an implementation-diverse oracle. Keep conversion strictness and the supported protocol matrix unchanged. Native registrations stay **Disabled**.

Ownership still consists of the two Rust paths above and these two documents; no other shared production/C5/lane/HTTP proof/finalizer/route/registration/identity source may change. No dependency, schema, storage, UI, setting or token store is authorized. Do not overwrite historical artifacts, reformat historical Markdown, commit/push or launch/install the app. A future shared-amendment freeze closes only this bounded amendment: CH2/CH3, AN2/AN3, GE2/GE3, lane resource handoffs, sequential **M2 → M3 → M4**, **M5**, current Full Gate and live/release acceptance remain separate unfinished gates. No lane rebase/distribution/integration or activation follows implicitly.

## 14. Candidate 03 continuity correction and remaining closure gates

**Candidate 03 remains unfrozen, unaccepted and unavailable to lanes.** Vesper's whole-amendment review of candidate 02 returned **FAIL**, not PASS; the parent verified both findings and implemented the bounded correction test-first. This mixed human/LLM append supersedes only the current-state interpretation of Section 13. Historical headers, statuses and evidence remain untouched. See [evidence Section 58](security-dlp-unified-core-implementation-evidence.md#58-candidate-02-whole-review-fail-and-candidate-03-continuity-checkpoint) for the complete report mapping, failure classifications, hashes and current commands.

### Completed bounded work

- [x] Retain Lumen's Section 13/57 human fresh-reader and LLM execution-readiness **PASS**, scoped to that prior checkpoint only. Retain Vesper's complete candidate 02 **whole-amendment FAIL**, `finish_reason=time_limit`, in `ch2-core-isolation-vesper-0cc25edd-review-01.json`. Reviewers used read/search/find only; parent runtime/hash/format evidence is not independent reviewer execution.
- [x] Verify F1 in `src-tauri/src/proxy/security/restoration_core/native_response.rs`: same-carrier jumping/partitioning survives key equality and global coverage. Add eight tests in `src-tauri/src/proxy/security/tests/unified_core_tests.rs`; `ch2-core-continuity-red-01` lists 8 and produces **6 genuine runtime failures / 2 first-passing controls** before production changes. Four policy/selection hopping combinations, contiguous partitioning and omitted middle-site acceptance fail after atomicity/refund checks. The already-rejected sparse-arguments case is a control, not RED.
- [x] Verify F2's masked SSE negatives in the owned test file. Replace generic `text` sources with valid Chat choices/delta and exact carriers; pair each policy/classification/immutable rejection with a valid restoration control. Preserve `ch2-core-continuity-policy-controls-01`'s two mistaken occurrence-count failures as test-expectation errors, **not production RED**. Corrected `policy-controls-02` passes 7/7 before production changes; first-passing coverage repair is not RED.
- [x] In the only owned production file, reuse private carrier traversal to require the complete original sequence of each selected carrier before lifecycle/matcher work, including any omitted original string node. Preserve exact node identity, choice-local sparse calls, interleaving, classification, one shared bounded lookup allowance and charged/refunded metadata. No staging/API/protocol-semantic extension. `ch2-core-continuity-green-01`: 8/8.
- [x] Parent current checks: `ch2-core-continuity-controls-01` CH2 89/89; `final-security-01` 2006/2006; `final-session-01` 34/34; `final-matrix-01` 2 passed / 1 ignored. Prefix shortened stems with `ch2-core-continuity-`. `ch2-core-continuity-check-clippy-01` check/Clippy `-D warnings` exit 0. Two-file rustfmt and `git diff --check` pass. All five current manifests have complete equal pre/post input maps matching candidate 03.
- [x] Save `ch2-core-continuity-candidate-03.json`, its cumulative nine-Rust-file `-whole.diff` against CORE-1 and two-file `-increment.diff` against `ch2-core-continuity-start-01`. Preserve all earlier candidates, failures and artifacts. Snapshot-start ownership remains exactly four files.

### Remaining ordered actions

1. **Whole code review:** Kai `c49bac7c31c94a099aa2030b6fa39374` reviews the entire candidate 03 amendment, including all nine cumulative Rust paths listed in Section 12, not just continuity or policy fixtures. No verdict is assumed. Save its complete explicit whole-amendment PASS/FAIL and coverage limits. Review is read/search/find only, with no commands, edits, LSP, hashes or delegation. Verify mandatory feedback and obtain genuine runtime RED before production changes; stop if another file, API extension or protocol semantics are required.
2. **Latest document gates:** obtain human fresh-reader and LLM execution-readiness PASS for these latest append-only sections. Format only their new suffixes via `pnpm exec prettier --parser markdown` on stdin and check idempotence. Preserve exact prefixes in `ch2-core-continuity-doc-prefix-01.json`, including Sections 13/57; the earlier Lumen PASS is not a PASS for these new sections.
3. **Final parent identity and evidence check:** compare all tracked/untracked non-artifact inputs to `ch2-core-opaque-start-01.json` (only four owned files differ) and candidate 03 (only appended docs differ). Verify the unchanged runner, current source/config/test maps, exact command/log mapping, original and later document prefixes, retained artifacts and unchanged architecture/Disabled registrations/conversion boundary. `ch2-core-isolation-boundary-preclosure-01.json` provides a 1,836-artifact retention baseline, not pre-existing directory-wide history. Save a new parent manifest. Reuse executable evidence only for identical executable/config/test inputs; changes or uncertainty require affected reruns and re-review. Run the two-file rustfmt command and `git diff --check` from evidence Section 57; use the unchanged focused runner with fresh stems and nonzero listing before single-thread runtime if reruns are needed.
4. **Actual acceptance only after 1–3:** append the frozen source/contract identity and acceptance decision, without changing this historical pending checkpoint. Scoped code/document PASS cannot replace whole-amendment PASS. Until then no freeze, distribution, lane rebase/integration or enablement is permitted.

### Unchanged safety and operation boundaries

Preserve exact attempt/binding, one dependency ledger, budget/work limits/refunds, immutable restrictions, strict original/final validation, genuine HTTP EOF, complete tool hold, sealed publication and last-yield SSE continuation. Final verification and staging share the composer, not an implementation-diverse oracle. The authoritative architecture, conversion strictness and protocol matrix remain unchanged; native registrations remain **Disabled**.

Only `restoration_core/native_response.rs`, `tests/unified_core_tests.rs` under `src-tauri/src/proxy/security/` and these two documents are owned. No other shared/C5/lane/HTTP proof/finalizer/route/registration/identity-source change, dependency, storage, schema, UI, setting or token store is authorized. Preserve all user work and artifacts; no commit/push, app launch/install, ignored/live tests or credentials. Cargo stays globally serial under `.voidx/verification/core1-20261106/native-lane-cargo.lock`, `CARGO_BUILD_JOBS=2`, main `src-tauri/target`; successful nonzero listing precedes single-thread runtime. No current Full Gate is claimed. A later shared freeze still leaves CH2/CH3, AN2/AN3, GE2/GE3, resource handoffs, M2 → M3 → M4, M5 and live/release acceptance separate and unfinished, without implicit lane consumption or activation.

## 15. Chat-only shared amendment freeze and bounded acceptance

**The Chat-only shared amendment is frozen and accepted as `chat-native-shared-conservative-20261108-frozen-01`; no lane consumption, distribution, rebase, integration or enablement is authorized.** This decision follows the completed candidate 03 whole-amendment static PASS, completed Plan14/Evidence58 document dual PASS and parent closure-ready identity/boundary checks. It changes only the current interpretation of the historical pending checkpoints; their text and status headers remain unchanged.

- Source/contract identity SHA-256: `0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`. `.voidx/verification/core1-20261106/ch2-core-continuity-closure-ready-01.json` contains the canonical identity payload, all nine cumulative Rust hashes, immutable reviewed contract/document checkpoint hashes, evidence and review mapping. Acceptance suffix hashes are recorded separately to avoid a circular identity. CORE-1 remains `ca7f883c742df07abe9bcaa9b0eade02aae5e119`; all work remains uncommitted and unpushed.
- Kai `c49bac7c31c94a099aa2030b6fa39374`: **whole-amendment PASS**, not a scoped continuity PASS. Onyx `b2d291875b97491fa3183d84915b2fd9`: **human fresh-reader PASS / LLM execution-readiness PASS** for Plan14/Evidence58. Complete reports and limitations are retained; neither reviewer executed parent commands. Nexus's returned dual PASS with `finish_reason=contract_unsatisfied` is preserved but is not the sole document gate; the completed Onyx review is independent of it. Prior Onyx/Vesper whole FAILs remain historical FAILs.
- Parent closure checks establish 1,536 stable non-artifact inputs: only four owned paths differ from snapshot-start, only append-only documents differ from candidate 03, the cumulative whole diff/runner agree, all five current evidence maps equal candidate 03, all historical document prefixes remain intact, and the 1,836-artifact baseline plus later recorded artifacts remain unchanged. See [evidence Section 59](security-dlp-unified-core-implementation-evidence.md#59-chat-only-shared-amendment-frozen-identity-and-acceptance) for exact artifact mapping, results and limitations. The parent inventory-script assertion is retained as a verification-tool error, not production RED.

This accepts only the bounded shared mechanics under the Section12 design and Section14 constraints. Keep exact attempt/binding, one ledger, charged budgets/refunds, immutable restrictions, strict original/final validation, genuine HTTP EOF, sealed publication and last-yield continuation. No new API or protocol semantics are granted. Native registrations remain **Disabled**. No other shared/C5/lane/HTTP proof/finalizer/route/registry/conversion-matrix/identity-source change, dependency, storage, schema, UI, setting or token store is authorized.

**Stop at this checkpoint.** CH2/CH3, AN2/AN3, GE2/GE3, resource handoffs, sequential M2 → M3 → M4, M5, current Full Gate and live/release acceptance remain separate and unfinished. A future authorized consumer must first verify this manifest identity and its own lane prerequisites, then perform its own checks; executable/config/test changes require affected reruns and whole re-review. Section57 remains the reproduction-command entry point with fresh stems, global Cargo lock/jobs2/main target, successful nonzero listing and single-thread runtime. Do not automatically start lanes or integration, commit/push, launch/install the app, run ignored/live tests or use credentials.

## 16. Approved Chat-only consumer delivery and independent CH2/CH3 closure

**Execution is approved by the user's explicit agreement to the seven-step Chat-only checkpoint.** This authorization supersedes only Section15's prohibition on consuming the frozen amendment in the Chat worktree. It does not authorize M2, other lane consumption, shared redesign, registration enablement, Git rebase, commit or push. The main-tree shared freeze remains `chat-native-shared-conservative-20261108-frozen-01`, identity `0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`. Historical statuses and failures remain unchanged. This mixed human/LLM execution appendix and [evidence Section60](security-dlp-unified-core-implementation-evidence.md#60-approved-chat-only-consumer-delivery-and-execution-boundary) must receive human fresh-reader and LLM execution-readiness PASS before behavior implementation.

### Ownership and contract consumption

- Consumer root: `.voidx/native-lanes/chat/`, still at CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`. Preserve existing uncommitted CH1 `mod.rs`, `request.rs` and `tests.rs`. `request.rs` is read-only in this stage; behavior edits are limited to `src-tauri/src/proxy/security/protocol_adapters/chat/{response,history,mod,tests}.rs` inside that worktree.
- The parent has copied exactly the nine Rust paths in `ch2-core-continuity-closure-ready-01.json`'s `identity_payload.source_sha256` from the main tree, byte for byte, into the Chat worktree. This includes the frozen C5 test and finalizer, but authorizes no further edits to any shared file. `chat-lane-consume-20261108-start-01.json` preserves original target hashes, complete input maps, CH1 snapshots/dirty patch, exact document prefixes and a recursive old-artifact inventory; `chat-lane-consume-20261108-delivery-01.json` records the exact copy and preserved boundaries. Both are under `.voidx/verification/core1-20261106/`.
- Main-tree executable/config/test inputs, authority architecture, other lanes, routes, identity sources and conversion matrix stay unchanged. Only these two main-tree documents may receive English appendices. Do not copy main-tree documents over the lane's historical documents. No API, dependency, store, schema, UI, setting or protocol-semantic expansion is authorized. Preserve exact binding/attempt, one dependency ledger, charged budgets/refunds, immutable constraints, strict original/final validation, real HTTP EOF, sealed publication and last-yield continuation.
- Stop and request a scoped decision if correct implementation needs any shared change or unsupported contract capability. Preserve the reproducer; never replace the core with permissive lane-local authority, a boolean completion proof, a second token store, ordinary-forwarding fallback or synthetic identity.

### Ordered test-first work

1. **Delivery and document gate.** The parent verifies frozen receipt/source/contract identity, all main inputs and retained artifacts, Chat HEAD and all three lane dirty/input states before copying; unknown target drift stops delivery rather than overwriting user work. Save new immutable stems. Format only these new suffixes using `pnpm exec prettier --parser markdown` on stdin; check idempotence and exact old prefixes. Obtain independent dual document PASS using read/search/find only before step 2.
2. **Consumed baseline.** Execute the Chat lane and full security filters below before editing lane behavior. Save complete pre/post maps, listing/runtime logs and counts. First-passing baseline/characterization cases are controls, not RED; a failed baseline must be investigated, not silently repaired outside ownership.
3. **CH2, JSON then SSE.** In lane `tests.rs`, extend the English protocol-state table before implementation. Each new behavior task writes its failing test first, executes a successful nonzero listing and genuine runtime failure, then changes only `response.rs`/`mod.rs` and reruns GREEN. Use exact native nodes and sparse choice/tool indices, complete ordered same-carrier fragments, supported content/refusal/readable reasoning and string arguments, conservative unknown preservation, strict finish/DONE/entity-end facts, and separate original versus client-release evidence. Test byte/fragment splits, malformed/incomplete/error/cancellation paths, budgets/refunds, no partial privileged calls, zero early publication and sealed last-yield delivery. Reuse frozen staging/strict verifier/finalizer; presence of legacy function fields is not completion authority. Unsupported evolution fails through existing safe failure rules, not a broadened protocol or fail-open fallback.
4. **CH3.** Add exact ordered delivered assistant/call replay and native call/result pairing tests in `tests.rs` before implementing `history.rs`/`mod.rs`. Obtain delivered records through real fixture HTTP proof/EOF and final delivery. First/edited client tool-result bodies remain untrusted. Cover no-session, cross-session/provider/protocol, unknown references, eviction and retry; use existing strict native replay validation, never token syntax, a generated ID or an incoming tool result as authority. Each newly implemented capability needs intended runtime RED then GREEN; unchanged negatives/controls may pass initially and must not be relabeled RED.
5. **Current Chat handoff checks and whole review.** Run all four filters, static checks and input/boundary checks below on the actual consumer state. Independent whole-Chat protocol review covers retained CH1 plus CH2/CH3 and the exact frozen consumption, not only the latest patch. Reviewer tools are limited to read/search/find: no commands, edits, LSP, hashing or delegation; parent checks must not be described as independently executed by the reviewer. Resolve mandatory findings through verified feedback/TDD within ownership; otherwise stop for scope approval.
6. **Final record and stop.** Append actual named RED/GREEN, controls/failures, supported/unsupported behavior, source/contract identity, environment/command context, counts, whole review and limits to these main documents. Obtain final document dual PASS, prove historical prefixes/artifacts and other lanes/main shared files unchanged, then save a new Chat handoff receipt. Reuse runtime evidence only for identical executable/config/test/environment inputs. Changed or uncertain inputs need affected reruns and whole re-review. No lane handoff PASS is assumed now; do not proceed to M2, other lanes or enablement after it.

### Commands and acceptance criteria

Run focused commands from the **main repository root**, one at a time, replacing `UNIQUE-STEM` with a new lowercase/digit/hyphen stem for every invocation:

```bash
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat UNIQUE-STEM proxy::security::protocol_adapters::chat::tests::
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat UNIQUE-STEM proxy::security::
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat UNIQUE-STEM proxy::session::tests::
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat UNIQUE-STEM proxy::protocol_matrix_tests::
```

The unchanged lane runner SHA-256 is `f0df435c652265d0084aead8627e136ddab5c730b9a70fff504124ecf2f17085`. It acquires the same main-repository `native-lane-cargo.lock`, sets `CARGO_BUILD_JOBS=2`, changes cwd to the Chat worktree and uses its exclusive `.voidx/cargo-targets/chat`. The user explicitly approved this target exception; the main tree retains `src-tauri/target`. No concurrent Cargo command is permitted. A lock failure/wait, compilation/format error, incorrect fixture expectation or parent-script failure is not production RED. Runtime runs with `--test-threads=1` only after exit-0 nonzero listing; acceptance requires green runtime and complete identical pre/post inputs, not listing alone. Ignored/live tests are not executed.

For Cargo check/Clippy, the parent acquires that **same main-repository global lock** with Python `fcntl.flock(LOCK_EX | LOCK_NB)`, sets `CARGO_BUILD_JOBS=2` and absolute `CARGO_TARGET_DIR=<main-repository>/.voidx/cargo-targets/chat`, and runs these sequential commands with cwd `.voidx/native-lanes/chat`. Save exit codes, logs and pre/post inputs under new artifact stems; require both exit 0. Do not invoke these Cargo commands unlocked:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Run these non-Cargo checks from the main repository root; require exit 0:

```bash
rustfmt --edition 2021 --config skip_children=true --check .voidx/native-lanes/chat/src-tauri/src/proxy/security/protocol_adapters/chat/{response,history,mod,tests}.rs
git -C .voidx/native-lanes/chat diff --check
git diff --check
```

Only those four lane files may be formatter-written if required. This stage is independent adapter readiness, not current Full Gate, M2-M5, installed-route or live/release acceptance. Native registration remains **Disabled**. Do not commit/push, launch/install the application, run ignored/live tests, use credentials, modify other lanes, rebase or integrate.

## 17. Chat CH2/CH3 closeout and Disabled handoff boundary

**The approved Chat-only consumer work has reached bounded runtime and protocol-review PASS; the final document gate and exact-state receipt are the remaining handoff steps.** This appendix records the actual Section16 outcome, not a new implementation authorization. Preserve all earlier status headers and pending statements as historical checkpoints. [Evidence Section61](security-dlp-unified-core-implementation-evidence.md#61-chat-ch2ch3-consumer-results-and-handoff-evidence) gives the named test history, failures, environment and review limits. The final receipt, `chat-lane-ch23-final-handoff-01.json` under `.voidx/verification/core1-20261106/`, may declare handoff only after both document gates and final continuity checks pass; this text does not predict those results.

### Delivered implementation and preserved authority

- Consumer root remains `.voidx/native-lanes/chat/` at CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`. Relative to the exact frozen delivery, only `src-tauri/src/proxy/security/protocol_adapters/chat/{response,history,tests}.rs` changed. `mod.rs` and read-only `request.rs` retain their delivered CH1 bytes; CH1 tests and uncommitted work are preserved.
- CH2 JSON observes exact native message nodes, supported content arrays, refusal, readable reasoning and function argument strings in original choice-array order. Output/refusal may restore; reasoning uses the frozen non-restoring policy. JSON staging does not grant completion, publication or release authority.
- CH2 SSE parses complete exact source frames using charged parser/buffer/metadata owners, groups fragments by choice and carrier, and keeps tool indices choice-local. It uses typed output/refusal/reasoning/argument streams and exact ordered `chat_choice` selections. Opaque siblings remain unchanged. No SSE content-array expansion is included.
- CH3 imports only strictly verified, complete ordered delivered assistant/call evidence through the frozen atomic importer. Its borrowed call/result pairing is structural metadata for unique unconsumed IDs in the adjacent assistant/tool group, not authentication of tool execution or result text. First, repeated and edited result bodies remain current untrusted DLP inputs.
- All nine consumed shared files still match `chat-native-shared-conservative-20261108-frozen-01`, source/contract identity `0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`. No shared/C5/finalizer, HTTP proof, route, identity, conversion, dependency, store, schema, setting or UI change was needed. One ledger, exact attempt/binding, charged budgets/refunds, immutable limits and strict original/final validation remain shared responsibilities.
- The sole sealed finalizer preserves distinct delivery timing: JSON activates only after strict proof/final encoding succeeds; SSE stays pending throughout every yield and activates only on the continuation after the last yield. The tests use the real asynchronous proof driver and actual entity `None` EOF, not a lane-created completion flag.

### Candidate-bound acceptance evidence

`chat-lane-ch23-candidate-01.json` (SHA-256 `7d1768d0628bfec24125bba95a09089cc89181e46d1463f5679d7977ca0e2f72`) binds the whole Chat diff, all 1,536 consumer inputs and preserved boundaries. `chat-lane-ch23-runtime-boundary-01.json` (SHA-256 `f6973e18dfe5deac30794a1a6dc838b4bb87f1970caa820b96e69e31f9591165`) records **parent-executed** current checks: Chat 45/45, security 2050/2050, session 34/34, matrix 2 passed/1 ignored, Cargo check and Clippy `-D warnings` exit 0, four-owned-file rustfmt and both diff checks exit 0. Ignored tests were not run. Listing was successful and nonzero before each serial runtime; complete pre/post input maps match candidate01. Later documentation-only appends do not invalidate unchanged lane executable/config/test inputs.

Nova `1cdb215839de44688daf6afaf40a7217` returned whole-Chat CH1/CH2/CH3 protocol **PASS**, no mandatory findings, but the tool reported `completed; finish_reason=step_limit`. Cipher `543bbab752ef48898cfa4a6f9c2fec1b` returned bounded confirmation PASS with `finish_reason=contract_unsatisfied`; neither is described as a normal tool finish. Nexus `e581f04f0dca49c7a9719fb008d72f87` then completed a narrower review-package/source confirmation with PASS and no mandatory findings; its tool supplied no finish reason, which is not inferred. Nexus does not replace Nova's whole scope. All three reports, literal status details and limitations are retained separately. Reviewers used only read/search/find, not commands, edits, LSP, hashing or delegation; parent tests and hashes are not their independent executions.

### Final handoff procedure and stop

1. Review only this English suffix and Evidence61 as mixed human/LLM documentation. Require separate human fresh-reader and LLM execution-readiness PASS with no mandatory findings; save the complete report and actual tool status under a fresh stem. Do not rewrite historical text to make old pending states appear current.
2. Format only the new suffixes with `pnpm exec prettier --parser markdown` on stdin. Require idempotence, exact entire pre-append prefixes and original Section16/60 prefixes. `chat-lane-ch23-prehandoff-boundary-01.json` preserves the immediate prefixes (plan 131,042 bytes; evidence 429,316 bytes) and 1,961 then-existing artifacts; the original delivery snapshot preserves 1,889 artifacts. Retain every listed byte/hash, including failed attempts and anomalous review finishes.
3. Recheck candidate/runtime input equality, nine frozen hashes, CH1 preservation, HEADs, unchanged Anthropic/Gemini and unchanged main inputs except the two append-only documents. Require the whole Chat diff to match the reviewed candidate and native registry to remain **Disabled**. Reuse checks only while relevant inputs/environment remain identical. Any source change requires a new candidate, affected reruns and whole re-review; an out-of-scope need requires a saved reproducer and scoped user decision.
4. Recheck the four-file rustfmt command and both `git diff --check` commands from Section16 after documentation formatting. Save current results and a recursive retained-artifact map, source/contract/review/runtime/document bindings in the final receipt; exclude only the receipt itself to avoid a circular digest. The final receipt closes the document/review checkpoint without editing these reviewed suffixes again.
5. **Stop after Chat handoff.** Native remains **Disabled**. Do not distribute to other lanes, modify frozen shared files, rebase, commit/push, integrate into main, run ignored/live tests, use credentials, launch/install an app, start M2–M5, Full Gate or live/release work.

This is independent private-adapter readiness, not shipped HTTP/router behavior. Real-network tee/backpressure, complete client two-round integration, every UTF-8/network-byte boundary, HTTP error projection/optional-history degradation and live upstream error preservation remain future integration acceptance. Frozen finite SSE limits and conservative snapshot composition remain in force; staging/final validation share the composer, so no implementation-diverse oracle is claimed. CORE-1, Responses, P3/P4, the authority architecture and the shared freeze are preserved; AN2/AN3, GE2/GE3, M2 → M3 → M4, M5 and all enablement decisions stay independent.

## 18. Approved direct-native shared amendment and full developer closure

**Current authorization:** the user approved remaining lanes and sequential M2–M5 closure, then separately approved the minimum shared amendment (“Agreed, continue”; literal approval retained in the start receipt). Sections 16–17's Chat-only stop remains historical. Preserve CORE-1, Responses, P3/P4 and the accepted Chat handoff. The authoritative [architecture](security-dlp-unified-core-protocol-adapters.md) is unchanged. [Evidence 62](security-dlp-unified-core-implementation-evidence.md#62-approved-direct-native-shared-amendment-start-and-protocol-evidence) binds approval, starting identity, blocker and protocol references. This appendix claims no new runtime or integration acceptance.

### Architecture, technology and ownership

Extend existing native staging and HTTP-owned opaque proof verification for direct Anthropic/Anthropic and Gemini/Gemini. Adapters provide exact native observations and structural metadata; one shared lifecycle owns edits/dependencies, and the sole sealed finalizer retains publication. Do not introduce a common wire representation or second authority implementation.

Use existing Rust, serde JSON, Tokio/futures, charged payload owners and Cargo tests; pnpm checks cover final renderer acceptance. No dependency, schema, UI, persisted setting, credential source or token store is added. Approved paths are:

- `src-tauri/src/proxy/security/completion_{json,evidence,tools}.rs`: strict native completion and tool validation; preserve Responses/conversion behavior.
- `src-tauri/src/proxy/security/http_restoration/{completion_verifier,native_driver}.rs`: exact source, actual attempt/binding, real asynchronous EOF and opaque native proof.
- `src-tauri/src/proxy/security/restoration_core.rs` and `restoration_core/native_response.rs`: one typed lifecycle, native object argument string leaves, charged frame staging and complete delivered block/Content/Part composition.
- `src-tauri/src/proxy/security/response_restoration/finalization.rs`: default unchanged. Only a test-proven necessary adjustment to sealed protocol/snapshot verification dispatch is approved, never a new issuer/publisher/activation path.
- Tests in the above internal modules and `src-tauri/src/proxy/security/tests/{unified_core_tests,c5_native_sse_tests,c5_native_finalization_tests,c5_native_sse_finalization_tests}.rs`.
- English suffixes only in this plan/evidence; unique artifacts in `.voidx/verification/core1-20261106/`. After re-freeze, exact approved shared delivery to `.voidx/native-lanes/{chat,anthropic,gemini}/`, preserving lane-owned work.

Keep a single dependency ledger, precharged retained metadata/payloads, refunds on failure/drop, exact actual-attempt/binding, immutable signed units, strict original/final validation and delivered-snapshot/wire correspondence. JSON activates after proof and final encoding, before returning the body. SSE stays pending through every yield and activates only on the continuation polled after the last yield. Pairing is structural metadata, not proof of tool execution or result authenticity.

Do not merely delete Chat guards, issue lane-local proofs, stringify native argument objects, invent Gemini IDs/cursors or fall back to unredacted forwarding. Cross-protocol cells, gateway expansion, `agent_clients.rs`, `native_protocol.rs` and intent/global restoration remain excluded. Preserve native types, unknown fields and unmodified raw bytes. No commit/push/rebase, app launch/install, ignored/live/provider tests, real credentials or release. Synthetic tests use temporary/in-memory stores and dynamic ports, never `127.0.0.1:15722`. Any required expansion outside these paths/semantics stops with a reproducer and a new scoped decision.

### Protocol evidence gate

`native-direct-shared-protocol-evidence-01.json` retains official fetched sources and limitations. Anthropic has indexed block start/delta/stop, message terminal evidence and independent transport EOF. Thinking is immutable from block start, even when its signature arrives later. Reuse strict existing Anthropic JSON/tool checks, but never equate them with native opaque HTTP proof.

Gemini's absent `finishReason` means generation has not stopped; STOP is natural/stop-sequence termination, MAX_TOKENS is truncation. Initially only structurally valid STOP for **every observed candidate**, plus clean EOF, may authorize a completed record. Other/unknown/blocked/error/truncated reasons grant no completed record; preserve genuine protocol outcomes when no restoration dependency requires failure. Do not invent `[DONE]`. Native scope is `generateContent` JSON and `streamGenerateContent?alt=sse` only.

Google's signature guidance corroborates exact Part retention, not a universal cross-frame merge oracle. **Before S4 positive implementation**, retain source-linked evidence and a state table for supported Content/Part composition and late-signature association. Never infer identity solely from repeated array position, manufacture Part IDs or merge signed and unsigned Parts. Ambiguous association remains held/unsupported. A narrower passing subset does not complete GE2/GE3; unresolved required semantics stop that task with a concrete decision request. This gate need not delay independently evidenced Anthropic JSON work.

### Ordered executable tasks

Below, `security/` means `src-tauri/src/proxy/security/`. Each behavioral task writes named tests, compiles/lists successfully with nonzero count, confirms intended runtime RED, then implements minimally and obtains GREEN. Compile/format/lock/timeout/fixture/parent-script errors and first-passing controls are not production RED. No implementation before S0's document gates.

| Task                          | Exact paths and requirement                                                                                                                                                                                                                             | Test filter or command and acceptance                                                                                                                                                                                                                                                                            |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [ ] S0                        | This Section 18, Evidence 62 and unique approval/protocol artifacts.                                                                                                                                                                                    | Prettier on suffix stdin twice, exact historical-prefix checks, `git diff --check`; read/search/find-only human fresh-reader and LLM execution-readiness PASS.                                                                                                                                                   |
| [ ] S1 AN JSON                | Internal `security/http_restoration/{completion_verifier,native_driver}.rs` tests, `security/tests/unified_core_tests.rs`, and approved shared sources: native JSON proof/lifecycle, tool-input string leaves, immutable blocks, exact delivered nodes. | Main runner filter `native_shared_an_json_`, separate RED/GREEN. Real EOF positive; duplicate/trailing/malformed JSON, source/final mismatch, invalid/incomplete tool input, unsupported pair, truncation/error, immutable edit, attempt/binding and budget/refund negatives.                                    |
| [ ] S2 AN SSE                 | Same shared sources, `security/tests/c5_native_sse_tests.rs`, `security/tests/c5_native_sse_finalization_tests.rs`: block-indexed fragments, tool barrier, complete block/call composition, thinking/late signature.                                    | Filter `native_shared_an_sse_`, separate RED/GREEN. Start/delta/stop/message consistency, cross-block isolation, real async EOF/Pending/error/cancel, incomplete arguments, signature conflicts, wire/snapshot mismatch and publication timing. Failure grants no record/partial privileged call/success marker. |
| [ ] S3 GE JSON                | `security/completion_{json,evidence,tools}.rs`, verifier/driver/core staging and internal/unified tests: candidate completion, native object string leaves, candidate/Part isolation, exact signed Parts/full Content.                                  | Filter `native_shared_ge_json_`, separate RED/GREEN. STOP positive per candidate; MAX_TOKENS/other/unknown/blocked/error/malformed negatives; signed mandatory-edit failure, object types/unknown fields, attempt/binding, refunds.                                                                              |
| [ ] S4 GE SSE                 | Complete the protocol evidence gate first; then approved shared sources plus SSE/finalization tests: supported Part composition, candidate isolation, late-signature hold, terminal plus real EOF.                                                      | Filter `native_shared_ge_sse_`, separate RED/GREEN. Multi-frame/usage-only frames, signed/unsigned isolation, exact signature association, post-terminal contradictions, unsupported framing, interruption/cancel, final-wire/snapshot consistency. Unresolved protocol evidence blocks this row.                |
| [ ] S5 sealed delivery        | `security/tests/c5_native{,_sse}_finalization_tests.rs`, native SSE/internal verifier tests; finalizer only if test-proven dispatch change is needed.                                                                                                   | Filter `native_shared_finalization_`, separate RED/GREEN for changed behavior. JSON activation before body return; SSE pending through last yield, only next poll activates; drop/cancel before continuation, snapshot/source/final mismatch and charged-owner refunds.                                          |
| [ ] S6 shared freeze          | Whole approved diff and new evidence artifacts.                                                                                                                                                                                                         | Main native HTTP/core/finalizer, unified, all lane, security/session/matrix filters plus Full Gate below. Current-state whole-amendment independent PASS, no mandatory findings, then versioned source/contract identity; no delivery before this freeze.                                                        |
| [ ] S7 exact delivery         | Only newly frozen shared files delivered to all three isolated lanes; snapshot existing AN1/GE1 and Chat request/mod/CH1 test prefix first.                                                                                                             | Each full lane/security/session/matrix selection via lane runner, source identity equality and full pre/post maps. Preserve Chat CH1–CH3 behavior and AN1/GE1; no mixed old/new shared contract.                                                                                                                 |
| [ ] S8 sequential integration | AN2/AN3 and GE2/GE3 lane-owned paths, followed by Sections 3/6's approved main integration paths and `security/tests/native_{chat,anthropic,gemini}_http_tests.rs`.                                                                                     | Each lane separate RED/GREEN and reviewed Disabled handoff; M2 Chat → M3 Anthropic → M4 Gemini each runs every existing native HTTP/lane/route-scope/security/session/matrix selection plus current Full Gate. Record stage PASS before next; enable only its approved direct-native cell.                       |
| [ ] S9 M5                     | Architecture Section 9 matrix, `docs/architecture/security-audit.md`, `docs/troubleshooting.md`, evidence suffix/final receipt; authority design unchanged.                                                                                             | Current integrated Full Gate, applicable matrix cells, both documentation gates and final independent review. No installed/live/provider/installer/signing/notarization/release claim. Stop without commit/push.                                                                                                 |

### Concrete test commands and Cargo boundary

From repository root, first S1 invocation:

```bash
python3 .voidx/verification/core1-20261106/run-native-core-focused.py native-direct-shared-an-json-red-01 native_shared_an_json_
```

Every later run uses a new unused stem, including retries/GREEN. The main runner SHA-256 is `2183fe4d952857891e713e66bd694679422a661f4e0e4ab5234d0149e2015bd8`. It acquires exclusive nonblocking `fcntl.flock` on `.voidx/verification/core1-20261106/native-lane-cargo.lock`, sets `CARGO_BUILD_JOBS=2` and absolute main `src-tauri/target`, records full pre/post input maps and requires successful nonzero `--list` before `--test-threads=1`. S2–S5 use their exact row prefixes. No ignored/live execution.

For S6, invoke that runner with fresh stems and each of `proxy::security::http_restoration::`, `proxy::security::c5_native_`, `proxy::security::unified_core_tests::`, each `proxy::security::protocol_adapters::LANE::tests::`, `proxy::security::`, `proxy::session::tests::`, `proxy::protocol_matrix_tests::`. Replace LANE separately with chat/anthropic/gemini. Expected filters must list nonzero after their owner task.

For S7:

```bash
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py LANE UNIQUE-STEM FILTER
```

Replace LANE with each lane and FILTER with its full protocol-adapter test module, then security/session/matrix. Lane runner SHA-256 `f0df435c652265d0084aead8627e136ddab5c730b9a70fff504124ecf2f17085`; target is absolute `.voidx/cargo-targets/LANE`, not shared with main/other lanes. New integrated native HTTP filters must also list successfully/nonzero before runtime.

Full Gate commands remain:

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
pnpm format:check
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --lib --manifest-path src-tauri/Cargo.toml -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml -- --list
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
git diff --check
```

The parent must wrap the Full Gate in the same exclusive global `fcntl.flock(LOCK_EX | LOCK_NB)`, jobs2 and absolute main target, with unique exclusive-created logs/receipt, exact cwd/commands, independent exits and full input maps. Each runtime line is conditional on its matching successful nonzero listing and unchanged inputs. A blocked listing fails the gate. Do not invoke old fixed-stem Full Gate scripts: some overwrite logs or lack this lock. A new artifact-only wrapper may implement precisely these commands/boundaries; verify its syntax/control flow before use, preserve every run and stop on input drift. No result is inferred from merely writing that wrapper.

If the Cargo lock is busy, save the failure and use already-approved parent read-only `ps/lsof`; do not kill unknown processes. Only format newly appended suffixes, never old status headers/body. Reviewers are restricted to read/search/find: no commands, edits, LSP, hashes or delegation; save complete findings and literal tool finish status. Parent runtime/hashes are not reviewer executions. Evidence reuse requires matching relevant inputs/environment; any source drift requires affected reruns/review. Final acceptance binds new freeze, exact lane delivery, separately accepted integrated stages and current M5, not a listing/baseline/old passing control.
## 19. S1–S4 direct-native shared amendment execution results

**Status:** S1 Anthropic JSON, S2 Anthropic SSE, S3 Gemini JSON and S4 Gemini SSE (evidence gate plus implementation plus malformed-candidates fix closure) are complete and sealed at stage level. S5 sealed delivery, S6 whole shared gate/freeze, S7 exact three-lane delivery, S8 lanes/M2–M4 and S9 M5 remain pending. This appendix records outcomes only; it does not anticipate later acceptance.

### Stage results

| Stage      | Outcome                                                                                                                                                                                                                                                                                                                                                                                                                   | Key evidence (SHA-256)                                             |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| S1 AN JSON | Sealed: real EOF, strict three-view validation, native object decoded string leaves, single lifecycle/ledger, immutable thinking, complete ordered blocks, sole finalizer. Focused 13/13.                                                                                                                                                                                                                                 | `7000f4841a7cdb3b3693f77eea99e8e521e8e8532fb6c9cc84a7327adee9200b` |
| S2 AN SSE  | Sealed: strict proof, block-indexed carriers, charged snapshots, final-wire dispatch, lifecycle staging, native-only malformed `message_delta.delta` sticky rejection. Focused 18/18. Umbra `2599eab63f5649debee62d6bd28d5750` body PASS, actual completed.                                                                                                                                                               | `494a0b02d1a1255ecd515d9f8ebaceb3623d5d071921a221cf95a0ab17f87bb8` |
| S3 GE JSON | Sealed: private `native_gemini`, all observed candidates valid and STOP, real EOF, decoded argument string leaves, complete ordered Content, signed Part immutable, legacy staging rejection. Helix `4c62b8a3294f415d8394ffe88a4bd030` five-file static review body PASS, actual completed.                                                                                                                               | `80a75ac6a93f4c3738fb8c61675aa2c1bd08823d5810db93ffe38c345ecddb8f` |
| S4 GE SSE  | Sealed after fix closure: official schema/SDK evidence gate (append-only whole-Content subset), carrier dispatch, per-observation candidate/Part/leaf isolation, signed Part immutable, single native-object lifecycle, exact candidate-root selections, composed snapshots, finalizer snapshot validation. Vesper `5017ff4aeb7b439289f1155f258197c6` body PASS, actual completed. Juno FAIL closed by fix and Mira PASS. | `f18513f11bf9a208a4b88efe8fddfc36e5dfa5f547ff17eed827fd55aa918fa6` |

### S4 malformed-candidates defect closure

Juno `e6a10077b5be4acdb5c71af44bff1f5d` returned a static FAIL with tool status `completed; finish_reason=time_limit`: `gemini_chunk` conflated "candidates field absent" with "candidates present but non-array", so a malformed frame such as `data: {"candidates":null,"usageMetadata":{}}` appended to an otherwise valid all-STOP stream was accepted as usage-only by the completion proof and by `validate_restored`. The finding was valid; the abnormal finish is recorded separately and is not a production runtime FAIL.

Protocol evidence `native-direct-shared-ge-sse-protocol-evidence-01.json` state G3 accepts only frames **without** a candidates field; present-but-empty or non-array candidates are malformed. The empty-array acceptance was implementation-convenience drift and was narrowed in the same fix.

TDD closure: RED `native-direct-shared-ge-sse-malformed-candidates-red-01` (12 listed, 11 passed/1 failed; mode 13 accepted the malformed frame and polled the client stream, polls=2!=0), then minimal fix in `completion_evidence.rs` `gemini_chunk` (explicit `None`/`Some` match; empty array rejected), then GREEN `...-green-01` 12/12 and `...-green-02` 12/12 after a rustfmt-only line-wrap of mode 15. Modes 13–17 cover candidates null/object/string/number/empty-array with valid usageMetadata against original stream, client stream and final-wire `validate_restored`.

Mira `6fc35d17c9f54a998c15acb7af597df6` completed a read/search/find-only five-file static review with body PASS and normal tool completion, confirming the fix covers final-wire validation and that the JSON path and snapshot staging were already strict. Reviewer runtime/hashes remain parent-owned.

### Current-source regression and compile controls

All runs are parent-owned, use new stems, the global serial Cargo lock, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, main target `src-tauri/target`, successful nonzero listing before serial runtime, and preserve superseded attempts:

- GE JSON 8/8, AN JSON 13/13, AN SSE 18/18, c5 native 176/176, CH2 89/89, unified 305/305, security 2057/2057, session 34/34, matrix 2 passed/1 ignored; all exit 0.
- rustfmt check, `git diff --check`, `cargo check` (0 warnings/errors) and `cargo clippy -- -D warnings` all exit 0; rustfmt changed only the mode-15 line-wrap in `native_driver.rs`.
- Superseded first-attempt runs (`*-rerun-01` for unified/security/session/matrix) used an expanded `proxy` filter due to a parent shell-parsing error; their only failure was the out-of-scope Chat-protocol port-binding test `agent_clients.rs:738` (503!=200) with no uncommitted `agent_clients.rs` changes. Failure evidence is preserved; correct-filter `rerun-02` runs supersede them.
- Aggregate: `native-direct-shared-ge-sse-stage-evidence-01.json` indexes 50 round artifacts and records both review verdicts with literal tool finishes.

### Explicit non-claims

Not a shared Full Gate, whole-amendment review, new freeze, exact three-lane delivery, or M2/M3/M4 integration or enablement. No commit/push/rebase, application launch/install, ignored/live/provider tests, real credentials, release, or `127.0.0.1:15722` binding occurred. The authoritative architecture document is unchanged. Native remains Disabled.

## 20. S5 sealed-delivery execution results

S5 is complete at stage level. Three `native_shared_finalization_` tests now cover the Anthropic and Gemini sealed-delivery boundaries through the existing sole finalizer and internal verifier. No production source, finalizer dispatch, publication path or activation path changed in S5. S6 whole shared gate/freeze, S7 exact three-lane delivery, S8 lanes/M2–M4 and S9 M5 remain pending.

| Check | Result |
| ----- | ------ |
| Focused controls | `native-direct-shared-finalization-control-01` and `-control-02`: successful nonzero listing, 3 listed, 3/3 passed, unchanged pre/post input maps. |
| JSON sealed delivery | Anthropic and Gemini; mapped/zero-mapping; activation after proof/final encoding and before body return; drop before body poll; unavailable-store activation denial; retained charged-body owner refund. |
| SSE sealed delivery | Pending through every yield; cancellation before the first yield and after the last yield without continuation; only the continuation poll publishes; activation failure returns the fixed stream error without publication; retained frame owners refund after their last drop. |
| Strict mismatch rejection | Anthropic and Gemini snapshot, source and final-wire mismatches reject with zero publication and a full budget refund. |
| Regression | AN JSON 13/13; AN SSE 18/18; GE JSON 8/8; GE SSE 12/12; c5 native 176/176; CH2 89/89; unified 305/305; security 2060/2060; session 34/34; protocol matrix 2 passed/1 ignored. |
| Scoped compile controls | Scoped rustfmt check, `git diff --check`, Cargo check and Clippy `-D warnings` all exited 0 in `native-direct-shared-finalization-compile-controls-02`. |

Plan 18 requires separate RED/GREEN for changed behavior. The new tests passed their first valid control, so S5 has no valid production RED and required no implementation change. Compilation, formatting and parent artifact-script failures remain classified as non-production failures.

The S5 aggregate is `.voidx/verification/core1-20261106/native-direct-shared-finalization-stage-evidence-01.json`, SHA-256 `7fd62d702906e96d5c2f01972e3208d737dc72739938b9ffab9ff8b916bb51ed`. It indexes 51 prior artifacts; it is not a Full Gate, whole-amendment review, freeze, lane delivery or integration acceptance.

### S6 handoff

The full `cargo fmt --check` reproducer in `native-direct-shared-finalization-compile-controls-01` remains preserved. It reports only pre-existing formatting in the already-approved shared `completion_tools.rs`; the other three commands exited 0. S6 must apply formatting-only changes to that approved shared source, record its new identity and run the required current-state Full Gate before independent whole-amendment review and versioned freeze. No semantic expansion is authorized by this formatting closure.

## 21. S6 whole shared gate and versioned freeze

S6 is complete. The current whole shared amendment passed all required selections, the current Full Gate and an independent static whole-amendment review. The resulting freeze authorizes only S7 exact delivery of the same ten frozen shared source/test files to the three isolated lanes. It does not authorize mixed contracts, lane-owned edits, integration, route enablement or a direct-native cell.

| Gate | Result |
| ---- | ------ |
| Whole selections | HTTP restoration 233/233; direct c5 native 63/63; unified 305/305; Chat/Anthropic/Gemini adapter contract probes 1/1 each; security 2060/2060; session 34/34; matrix 2 passed/1 ignored. |
| Frontend and format | `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit` and `pnpm format:check` exited 0. |
| Rust compile controls | Cargo check, full `cargo fmt --check` and Clippy `-D warnings` exited 0. |
| Library runtime | 6562 listed after successful listing; 6547 passed, 15 ignored, 0 failed. |
| Full Cargo runtime | 6725 listed after successful listing; every runtime summary passed with 0 failed. |
| Input integrity | `git diff --check` exited 0; pre/post input maps are identical and every runtime was gated by its own successful nonzero listing. |

The S5 full-format reproducer is closed without semantic expansion: `completion_tools.rs` changed by rustfmt only from `dfb0474007ef1ebbc2ab66a9ebab572b25a0337a80499909677ac1c7f75c3e00` to `a38d6258cfb608ea1032d7b350752f7e5d88b8b1b98ef53ce8edfa7af604cf47`. The current Full Gate receipt is `native-direct-shared-s6-full-gate-01.json`, SHA-256 `76361da7bf674fb06c8085db319653f1cf1cbde5fdeb7a46018ebf6030404294`.

Nova (`75ff307c64f441e29c127770131c2937`) completed normally with body verdict PASS and no mandatory findings. The review was limited to read/search/find static inspection; Nova did not execute tests, commands or hashes. Its artifact is `native-direct-shared-s6-nova-review-pass-01.json`, SHA-256 `ea5cdbf0f24293533921c919854d13c28fb2929b783e89a73a137f3443e4956c`.

The versioned freeze is `native-direct-shared-conservative-20261109-frozen-01`, contract identity `c5bc94249a2252dc0b77b64543e98dc9f92f086dfdd7d4dac45e37e6f2e1ed27`. Its receipt is `.voidx/verification/core1-20261106/native-direct-shared-s6-freeze-01.json`, SHA-256 `ae0d005936195b28126185075b374c153361e1d358dbd9cdfa611a0f84159065`. All reviewed source and document identities were rechecked without drift before freezing.

### S7 boundary

Snapshot the existing Chat/AN1/GE1 bytes and test prefixes first. Deliver exactly the frozen ten-file shared set to Chat, Anthropic and Gemini, preserve every lane-owned path and verify full source equality plus pre/post input maps with each lane's exclusive target. Documents are not delivery payloads. S8 Disabled handoffs and ordered M2 → M3 → M4 remain separate gates.

## 22. S7 exact three-lane shared delivery

S7 is complete at stage level. The exact ten-file frozen shared source/test set was delivered to the Chat, Anthropic and Gemini lanes without delivering documents or overwriting lane-owned files. Chat received eight changed shared files and retained two byte-identical files; Anthropic and Gemini each received all ten current frozen identities. No mixed old/new shared contract remains.

| Lane | Adapter | Security | Session | Protocol matrix |
| ---- | ------- | -------- | ------- | --------------- |
| Chat | 45/45 passed | 2104/2104 passed | 34/34 passed | 2 passed, 1 ignored |
| Anthropic | 15/15 passed | 2074/2074 passed | 34/34 passed | 2 passed, 1 ignored |
| Gemini | 33/33 passed | 2092/2092 passed | 34/34 passed | 2 passed, 1 ignored |

Every selection had a successful nonzero listing, serial runtime, zero failed tests and equal pre/post input maps. The final aggregate independently rechecked the main and all three lane shared identities against the freeze, every lane-owned byte, all Chat CH1/CH2/CH3, Anthropic AN1 and Gemini GE1 test names and prefix counts, and each lane's `git diff --check`.

The pre-delivery snapshot is `native-direct-shared-s7-pre-delivery-01/manifest.json`, SHA-256 `94480a7039ac6ebe684f9428e38c5bb86c3bf6e398e193d51de871977ebf0922`. The exact-delivery receipt is `native-direct-shared-s7-delivery-01.json`, SHA-256 `e561c6eb4931ea2cdfe187aa3871067f4145b5a755f803614503c09a63133bb2`. The stage aggregate is `.voidx/verification/core1-20261106/native-direct-shared-s7-stage-evidence-01.json`, SHA-256 `d36a67c4ad60e33e9e5b37e13cda02e07c52f624a7e9521d8aaa055cb0a0185e`, indexing 88 artifacts.

The first aggregate builder failed after the lane tests because it assumed a top-level `filter` key that lane source manifests do not contain. Its script and error receipt are preserved; builder-02 parses the actual command schema and completed all assertions. This was a parent artifact-generation error, not a production RED.

### S8 boundary

S7 grants no lane-local publication authority, integration, route or direct-native cell. Preserve Chat CH1–CH3 and complete the AN2/AN3 and GE2/GE3 lane-owned RED/GREEN work, regressions, scoped reviews and Disabled handoffs before ordered M2 → M3 → M4 integration. M5 remains the final acceptance gate.

## 23. M2 Chat integration closure and ordered continuation

This suffix records the current Chat-only integration. Earlier pending and Disabled statements describe their historical snapshots and are not rewritten. The M2 acceptance receipt is `native-direct-m2-acceptance-01.json` under `.voidx/verification/core1-20261106/`; it becomes authoritative for this stage only after the suffix checks recorded there succeed.

### Accepted boundary

Only direct native Chat Completions (`openai_chat`), JSON and SSE, is enabled. The four existing Chat aliases retain query strings. Trusted internal `RequestIngress`, app namespace, operation, actual provider/target/principal and retry binding determine authority; client headers cannot select ingress. Responses/CORE-1 and existing P3/P4 behavior remain separate preserved paths. Anthropic and Gemini candidates remain outside the main tree at M2 closure. Gateway, agent, intent/global and cross-protocol cells acquire no new permission.

The implementation uses the existing request lease, completed-context store, dependency ledger and sealed finalizer. Real upstream EOF is mandatory. JSON activates only after proof and final encoding, before returning the body. SSE remains pending after the last yield and activates only on continuation polling. Drop/failure refunds remain mandatory. Unknown native fields, native argument types, immutable signatures and unchanged original bytes are not normalized away.

### Review and verification closure

- Normal cumulative review: Sol `f4aae0d1a5194e5e9153d667b37f3443`, original `NEEDS_CHANGE`, with literal `terminal`. The raw report is preserved as `native-direct-m2-cumulative-sol-review-f4aae0d1a5194e5e9153d667b37f3443.txt`. Its single P2 finding was reproduced by a compiled, nonzero runtime test and minimally fixed; the original verdict is not relabeled PASS. Under the approved small-feedback rule, parent verification closes that finding without another independent review. See `native-direct-m2-sol-feedback-closure-01.json`.
- Current Integration filters: `native-direct-m2-sol-green-01-54d4b97d05`: HTTP 48, Chat adapter 45, route-scope 22, security 2,153 and session 34 passed; protocol matrix 2 passed and 1 ignored. Both formatting passes were identical. Each filter has successful nonzero listing, runtime logs and unchanged source maps.
- Full Gate: `native-direct-m2-actual-full-gate-04-003f63fffd`, real runner and parent exit 0, all twelve commands complete with unchanged inputs. Renderer unit tests: 254 files / 3,064 tests. Rust library: 6,640 passed / 15 ignored. Full Rust run: 6,803 executed / 15 ignored with ordered listing/runtime identity checks.

### Executable next stages

The original I-AN1/I-AN2, I-GE1/I-GE2 and F1-F4 plan remains approved. Do not repeat ordinary approval requests.

1. After the M2 acceptance receipt, copy only the five owned Anthropic candidate files from `.voidx/native-lanes/anthropic/src-tauri/src/proxy/security/protocol_adapters/anthropic/`, validating them against `native-direct-shared-s8-an-disabled-handoff-01.json`; leave the lane untouched. Add `security/tests/native_anthropic_http_tests.rs` and integrate `handle_messages_for_app`, `response_processor.rs`, `forwarder.rs`, `http_restoration.rs` and registry using the existing authority/finalizer. Preserve distinct Claude / Claude Desktop namespaces and reject conversion cells. TDD first results, signed thinking replay, errors, cancellation and isolation. Run the Anthropic HTTP/adapter filters and all integrated filters plus the Integration/Full Gate before recording M3 PASS.
2. Only after M3 PASS, deliver the five Gemini candidates against `native-direct-shared-s8-ge-disabled-handoff-01.json`. TDD `handle_gemini` admission retention, actual `gemini_native` binding and final wrapping in `security/tests/native_gemini_http_tests.rs`. Permit only `generateContent` JSON and `streamGenerateContent?alt=sse`; derive framing from URI/query. Preserve GET/model listing and non-generation behavior. Run all integrated filters and the Full Gate before M4 PASS.
3. Only after M4 PASS, close the Section 9 acceptance matrix and update the shipped English `docs/architecture/security-audit.md` and `docs/troubleshooting.md`. Run final current-state checks and the mixed-audience document gate, then the final independent review for M5.

Use `run-native-core-focused.py` with the exact native HTTP, adapter, `proxy::security::`, `proxy::protocol_matrix_tests::` and `proxy::session::tests::` filters. All Cargo commands retain the shared nonblocking exclusive lock, two jobs, no incremental build and absolute main-tree target. Long runs remain detached with unique launch/list/test/source/parent evidence; freeze source until their real terminal receipts.

No commit/push/rebase, app launch/install, fixed proxy port, real credentials or ignored/live/provider opt-in is authorized by this suffix. This is development evidence, not provider, installer, signing, notarization or release acceptance.

### Append-only document verification

The historical prefix is retained byte-for-byte from the M2 start snapshots. A whole-document Prettier attempt changed historical formatting and failed the prefix assertion; its output and diagnostic are preserved in `native-direct-m2-documents-prefix-diagnostic-01.json` and the referenced failure snapshots. Only this new English suffix is formatted twice and checked for idempotence. The repository `pnpm format:check` checks `src/`, not these complete Markdown files; no whole-document Prettier PASS is claimed.

Gate04 predates this documentation-only suffix. The final acceptance compares its input inventory with the current tree, permits only these two appended documents, and reuses unchanged production/test evidence rather than claiming the Gate tested a later document snapshot. Current prefix, suffix, repository format, diff, source identity and preservation checks are recorded in `native-direct-m2-documents-closure-01.json`.

### M3 Anthropic closure status

I-AN1 and I-AN2 are delivered on the main tree: native Anthropic request inspection, history restoration for `/v1/messages`, strict Claude Desktop model routing, and the guarded vendor-normalization boundary described in the evidence document. The five previously handed-off `protocol_adapters/anthropic` candidates were integrated without changing the authoritative architecture or the frozen adapter-facing contract.

The M3 verification chain runs on one frozen input map. After the boundary fix and its predicate unit matrix, Integration05 is green (`native-direct-m3-vendor-integration-05-eb7b886cc3`): Anthropic HTTP 64, Anthropic adapter 39, Chat HTTP 48, Chat adapter 45, route scope 23, `proxy::security` 2,258, session 34, protocol matrix 2 passed with 1 pre-existing ignored test; formatting is idempotent and changed no file. The M3 Full Gate is green (`native-direct-m3-vendor-actual-full-gate-04-eac8336f71`): all twelve commands exited 0, every listed test name ran, and the input map was unchanged. The earlier Full Gate failure `native-direct-m3-actual-full-gate-01-b79f827191` remains preserved as a real Clippy failure, and the earlier Integration02 failure `native-direct-m3-resolver-integration-02-755a03d4ac` remains preserved as two stale M2-only registration expectations; neither is relabeled PASS.

M3 stage acceptance is recorded separately in `native-direct-m3-acceptance-01.json` after the scoped independent review returned PASS. No M4 Gemini work starts before that receipt. This suffix does not authorize live/provider, installer, signing, notarization or release checks, and it does not claim a whole-document Prettier result for these historical Markdown files.

### M4 Gemini closure status

I-GE1 and I-GE2 are delivered on the main tree: the five lane-owned `protocol_adapters/gemini` candidates are integrated, `handle_gemini` retains `RestorationAdmission::Admitted`, forwards the prepared attempt, derives streaming from the handler-owned route scope (never `body.stream`), and finalizes through the shared native response path. Admission is restricted to the two canonical cells `(Direct, Gemini, GenerateContent, Json)` and `(Direct, Gemini, StreamGenerateContent, Sse)`; Gateway ingress and every mismatched operation/framing pair stay on the legacy path. The registry gains only `Registration::NativeGemini` for that same-protocol cell.

The M4 verification chain runs on one frozen input map. Integration02 is green (`native-direct-m4-integration-02-d766637c01`): Gemini HTTP 11, Gemini lane 59, Anthropic HTTP 64, Anthropic lane 39, Chat HTTP 48, Chat lane 45, route scope 23, `proxy::security` 2,328, session 34, protocol matrix 2 passed with 1 pre-existing ignored test; formatting is idempotent and changed no file. The M4 Full Gate is green (`native-direct-m4-actual-full-gate-02-f2a8d63207`): all twelve commands exited 0, every listed test name ran, and the input map was unchanged.

A scoped independent review returned `NEEDS_CHANGE` and its findings are closed in `native-direct-m4-lumen-findings-closure-01.json`: Gemini SSE restoration failure now emits a fixed Gemini error frame instead of a silently broken 200 stream (real RED `m4-gemini-sse-frame-red-01-fb401156ac`, GREEN `m4-gemini-sse-frame-green-01-30fdbfe43c`), the strict-terminal fail-closed shapes are pinned by three new tests, native Gemini attempts carry no converted-JSON protocol label, and both ProtoJSON signature spellings are treated as immutable. The original review body is retained unchanged.

M4 stage acceptance is recorded in `native-direct-m4-acceptance-01.json`. No M5 work starts before that receipt. This suffix does not authorize live/provider, installer, signing, notarization or release checks, and it does not claim a whole-document Prettier result for these historical Markdown files.

### M4 final gate correction

After the M4 suffixes above were appended, the tests for the review findings gained two additions: the fixed Gemini SSE error frame is now compared byte-for-byte, and a Gemini upstream error envelope case was added. The test-only rename of the second route-scope test was also completed. Because those test bytes changed, the final M4 evidence is `native-direct-m4-actual-full-gate-04-989751878c` (twelve commands exit 0, `all_passed` and `unchanged_inputs` true) with `native-direct-m4-integration-03-72cab97748` (ten groups green, format idempotent, no changed path). Gate `-02` remains valid for the production code but predates these test additions.

One gate attempt, `native-direct-m4-actual-full-gate-03-d14572c18b`, failed `all-test` on `reversible_closure_resp4_reasoning_part_added` (a Responses-path test outside M4: the continuation send count was zero). The same test passed in isolation and in two subsequent full library runs, and the failure was not reproducible; it is retained as a real failure with the order-sensitivity noted, not as a pass.

The narrow re-review that verified the findings is preserved unchanged at `native-direct-m4-gemini-athena-recheck-6e8ac91fa0b94e97a66168363254fdce.txt` with verdict `NEEDS_CHANGE`; only its naming item was still open at review time and is fixed afterwards. The closure record for both reviews is `native-direct-m4-lumen-findings-closure-02.json`.

## Post-M5 amendment: degrade instead of failing, and say what to change

Approval: the user authorized scoped behaviour beyond the accepted M2–M5 scope for a single purpose — unsupported native cases must either degrade without disrupting use, or fail with a clear, actionable message instead of a generic 500.

### What changed

1. **Token-free Gemini answers are no longer errors.** `native_chat.rs` now records a verbatim ready state when the captured Gemini entity contains no local placeholder marker, so strict completion-shape checks do not reject an answer that has nothing to restore. `MAX_TOKENS`, `promptFeedback.blockReason`, media Parts and unknown Part keys are returned to the caller unchanged. No completion record and no restoration authority is created, and the payload budget is refunded.
2. **Refusals name a cause.** `DlpForwardError::RestoreUnsupportedFor(RestoreUnsupportedCause)` adds three structural, user-actionable categories — unsupported endpoint/framing, immutable signed content, and unrestorable placeholders. `ContractError::ImmutableSite` and `TokenError::ImmutableSite` preserve that cause instead of collapsing into an anonymous preparation failure. The generic `RestoreUnsupported` family also gained actionable wording. None of the messages contains a rule name, matched value, credential, checkpoint or stage label.
3. **Chat and Anthropic failure delivery is unchanged.** Adding fixed frames there broke five accepted M2/M3 tests that assert the transport-error contract; since those clients already observe a stream failure rather than a silent success, the change was reverted. Only Gemini's local terminal failure uses a fixed frame.

### Evidence

- Same-file RED/GREEN on identical bytes: `m6-degrade-red-01-a264a9a3ea` (13 listed, 3 failed, exit 101 — the three degradation cases) and `m6-degrade-green-01-e7533ce8a9` (13 passed, exit 0). The RED was produced by disabling only the token-free decision in a snapshot of `native_chat.rs`.
- Cumulative integration: `native-direct-m6-integration-01-7ff192014c`, ten groups green (13 / 59 / 64 / 39 / 48 / 45 / 23 / 2330 / 34 / 2+1 ignored), format idempotent with no changed path.
- Full Gate: `native-direct-m6-actual-full-gate-02-6b7d2e9e35`, twelve commands exit 0, `all_passed` and `unchanged_inputs` true.
- Preserved failure: `native-direct-m6-actual-full-gate-01-9d07ddae93` failed `all-test` with 31 environment-shaped errors (`Os { code: 2 }`, `Unavailable`). Re-running the identical command under the identical isolated environment exited 0, and the direct run also passed, so the failure is retained as an unreproducible environment event and is not presented as a pass.

### Limits

- Deferral applies only when the answer carries no placeholder. A placeholder-bearing answer that cannot be restored still refuses, because leaving the placeholder visible would break the restoration contract.
- Chat and Anthropic keep transport-error failure delivery; the three protocols are intentionally not symmetrical there.
- Gemini media is passed through, never decoded or inspected for sensitive content.
- This remains local development evidence: no commit, no install, no live provider, no signing or release acceptance.

## M6 amendment correction after independent review

An independent read-only review returned `FAIL` on the first M6 amendment and its original verdict is preserved in `native-direct-m6-final-review-1dbb693768b249a981bf85e05ee0844f.txt`. Every finding was addressed; `native-direct-m6-review-findings-closure-01.json` records the mapping.

What changed relative to the first amendment:

- **Projection consistency.** Attributable refusals now return HTTP 400 with `invalid_request_error` and `dlp_restore_unsupported`, and the tests assert the exact status/code/message triple for each cause instead of "any message with the generic code". The generic `RestoreUnsupported` family keeps its historical message, status and code so legacy and gateway paths are unaffected; the endpoint cause is limited to direct native scopes.
- **Fail-safe degradation.** The token-free decision no longer scans raw bytes only. `entity_contains_token_marker` first checks raw bytes, then decodes text leaves (JSON object, or each SSE `data:` payload) and feeds them in order with a carried tail, so a marker split across Parts or frames is still found. Any parse, depth or work failure returns "may contain a marker", which forces the strict path.
- **Smaller blast radius.** `TokenError::ImmutableSite`, `PlanError::ImmutableSite` and the placeholder cause were removed after they forced two incompatible internal error shapes and changed the accepted JSON finalizer projection. Two causes remain, both attributable at a single site: unsupported endpoint (direct native admission) and immutable signed content (the vendor-normalization guard).
- **Collateral repair.** A mechanical edit used while threading the admission flag damaged an unrelated canonical-JSON fixture and a `["inert",false,7]` expectation. Both were reverted; gate `-03` (fixture damage) and `-04` (formatting drift) are preserved as real failures.

Final evidence: same-file RED `m6b-degrade-red-01-d8f62583e5` (13 listed, 3 failed, exit 101) with the GREEN source snapshot in `m6b-degrade-red-01-native-chat-green-source.rs`, GREEN `m6b-degrade-green-01-dd5762f989` (13 passed, exit 0), integration `native-direct-m6-integration-02-f3c8e2bae9` (ten groups green, format idempotent, no changed path), and Full Gate `native-direct-m6-actual-full-gate-05-03615edda5` (twelve commands exit 0, `all_passed` and `unchanged_inputs` true). This documentation suffix postdates that gate; only these two files differ from the gated map.

Remaining limits: degradation applies only when the answer carries no marker; a marker-bearing answer that cannot be restored still fails as before. Gemini media is passed through and never inspected. Chat and Anthropic failure delivery is unchanged. No commit, install, live provider, signing or release acceptance is claimed.

## M6 second amendment: attributable refusals replace the generic failure

Approval: the user required that unsupported native cases must not surface as a generic HTTP 500, because a generic failure is hard to diagnose.

### What changed

1. **Request-side cause preserved.** `PlanError::ImmutableContent` is now set where the cause is provable and nowhere else: an authorized reference on an unwritable carrier, a mandatory edit that the atomic plan rejects as an immutable site, and the same conflict surfaced by the Gemini observation adapters. `prepare_observed_request` records the immutable-site conflict out of band because the validator callback can only return `TokenError`; the cause is re-raised with its exact value afterwards.
2. **Attribution is native-only.** `DlpForwardError::from_plan_error_native` converts that cause into `RestoreUnsupportedFor(ImmutableSignedUnit)`, and only the four native call sites use it. Legacy Responses and the conversion facades keep the previous generic projection, so their accepted contract is unchanged.
3. **Response-side cause.** `RestoreFailureCause::UnrestorablePlaceholders` plus `DlpForwardError::RestoreFailedFor` give an already-received answer that cannot be restored a specific client projection: HTTP 502, `dlp_restore_failed`, and a fixed message naming the cause. The cause is claimed only when the collected body actually contained a placeholder marker, and never for Chat, whose failure delivery stays a transport error.
4. **Marker scan repaired.** The carried tail was destroyed by feeding JSON object keys between sibling values, so a placeholder split across two Parts or two SSE frames was missed and could have been passed through verbatim. Keys no longer participate, the marker stem is now `rredact` without the separator so a doubly escaped dash still matches, and eight unit tests pin single-leaf, cross-Part, cross-sibling, cross-frame, escaped, legacy-prefix, top-level-array, clean-body and fail-closed behaviour.

### Evidence

- Marker scan RED before the repair, then 8/8 green (`marker_scan_tests`).
- Same-file RED on the final code: `m6c-attributable-red-01-d685835153` disables only the new plan cause (snapshots `m6c-red-01-{plan,prep}-green-source.rs`), listed 2358, 20 failed, exit 101, with `m4_gemini_signed_part_is_never_rewritten` failing on `500 ... proxy_processing_failed`.
- GREEN on the restored bytes: `m6c-attributable-green-01-9e2ec1e1cf`, 2338 passed, exit 0.
- Integration `native-direct-m6-integration-03-bb00c995b9`: ten groups green (13 / 59 / 64 / 39 / 48 / 45 / 23 / 2338 / 34 / 2+1 ignored), format idempotent with no changed path.
- Full Gate `native-direct-m6-actual-full-gate-06-96bc601c32`: twelve commands exit 0, `all_passed` and `unchanged_inputs` true.
- Library suite 6826 passed / 15 ignored on the same tree.

### Client-visible contract now

| Situation | Status | Code | Meaning |
| --- | --- | --- | --- |
| Unsupported direct native route or framing | 400 | `dlp_restore_unsupported` | Fix the route/framing |
| Signed/immutable content a mandatory edit would touch | 400 | `dlp_restore_unsupported` | Remove the signed content or disable DLP |
| Upstream answered but its body cannot be restored | 502 | `dlp_restore_failed` | Retry; if it persists, disable DLP |
| Cause not established | 500 | `proxy_processing_failed` | Unchanged generic projection |

### Limits

- The generic projection still exists for failures whose cause cannot be proven; it is no longer used for the three cases above.
- Chat keeps transport-error failure delivery; its response-side failures are not re-projected.
- Gemini media is passed through and never inspected.
- The docs suffix postdates the final gate; only documentation differs from the gated map.
