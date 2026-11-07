# Unified reversible DLP core and four protocol adapters

> Status: architecture approved on 2026-11-05; design only. This document does not authorize implementation, enable additional routes, or describe already-shipped native Chat Completions, Anthropic Messages, or Gemini restoration.
>
> Audience: human architecture/security reviewers and future implementing agents. The behavioral baseline is [DLP architecture](../architecture/security-audit.md). The earlier [reversible-tokenization proposal](security-dlp-reversible-tokenization-spec.md) remains historical context rather than a description of this target architecture.

## 1. Decision

Extend reversible DLP through one shared security core and four native HTTP protocol adapters:

- OpenAI Responses, including the existing Codex `/responses` and `/responses/compact` behavior;
- OpenAI Chat Completions;
- Anthropic Messages;
- Gemini Native generateContent.

The adapters identify native text sites, protocol lifecycle events, tool-call structure, completion evidence, and history dependencies. The shared core owns token mappings, request-local authority, resource budgets, restoration decisions, tool-release barriers, completed-context publication, and client-safe failure policy.

Do not serialize an entire request or response into a lossy common JSON intermediate representation. Native JSON values, unknown members, event types, protocol ordering, and unmodified wire bytes remain under adapter control. Small borrowed facts and scoped text patches cross the adapter/core boundary; they are metadata, not a replacement protocol document.

Only an explicit `Block` rule may intentionally reject content for security policy. Missing restoration context, cursor state, or in-memory history must not block an otherwise valid conversation. This does not require forwarding after a genuine capacity, encoding, authentication, network, mandatory-redaction, or restoration failure.

## 2. Current baseline and coupling points

Reversible restoration currently admits only the Codex Responses HTTP path. `admit_http_request` in `src-tauri/src/proxy/security/http_restoration.rs` requires both `AppType::Codex` and an endpoint marked as supported Codex Responses. Other clients still receive ordinary DLP inspection, but a reversible-only dependency or recognized restoration placeholder is not evidence of native restoration support.

Codex Responses-to-Chat/Anthropic conversion means that a Codex client can use a different upstream protocol. It does not mean that a native Chat Completions, Anthropic Messages, or Gemini client already has reversible restoration. Native Chat response handling currently returns through `process_response` without the Codex `DlpPreparedAttempt` response wrapper; the native Gemini handler likewise does not create a reversible attempt.

The extraction should start from these existing responsibilities:

| Existing source                                        | Current responsibility to preserve or separate                                                                                                                                                                                  |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/proxy/security/http_restoration.rs`     | HTTP admission, `DlpPreparedAttempt`, actual-attempt binding, response wrapping, limited `ContextUnavailable` degradation, and client error projection. This currently combines shared authority with Codex HTTP orchestration. |
| `src-tauri/src/proxy/security/tokenization.rs`         | `DlpTokenStore`, `DlpRequestLease`, namespace/generation binding, mappings, and strict token authority. Its permission decisions must not be relaxed by HTTP degradation.                                                       |
| `src-tauri/src/proxy/security/request_preparation.rs`  | Codex request-history classification and budgeted request preparation. Native history classification belongs in adapters; token transactions and authority remain shared.                                                       |
| `src-tauri/src/proxy/security/response_restoration.rs` | Codex JSON/SSE restoration, delta/snapshot checks, tool commit barriers, and `response.failed`. Split reusable stream/token mechanics from Responses event knowledge.                                                           |
| `src-tauri/src/proxy/security/completion_evidence.rs`  | Borrowed original-protocol facts and transport state for Chat, Anthropic, and native Responses. These observations do not themselves grant restoration authority.                                                               |
| `src-tauri/src/proxy/security/completion_json.rs`      | Budgeted JSON/SSE completion validation. Reuse its strict checks in protocol observers instead of adding a second completion model.                                                                                             |
| `src-tauri/src/proxy/security/completed_context.rs`    | Actual-attempt bindings, completion witnesses, delivered call snapshots, and completed-context records.                                                                                                                         |
| `src-tauri/src/proxy/providers/codex_chat_history.rs`  | Late Chat-history construction for Codex conversion. Without a current lease, protected cached completion content is discarded while the original request survives; capacity and encoding failures remain strict.               |
| `src-tauri/src/proxy/handlers.rs`                      | Client request admission, conversion, Codex client error projection, and response exits. Each new native adapter must reach its final client response boundary here.                                                            |
| `src-tauri/src/proxy/response_processor.rs`            | `process_response`, `process_codex_response`, streaming/nonstreaming response ownership, and native response exits. Native Chat and Gemini integration must be wired here as well as in request handlers.                       |
| `src-tauri/src/proxy/session.rs`                       | Strict Codex restoration identity extraction and legacy session identity extraction. Add protocol-specific stable-identity parsing without reusing a generated per-request UUID.                                                |
| `src-tauri/src/proxy/forwarder.rs`                     | Actual provider attempts, protocol conversion, retries, and the optional `DlpPreparedAttempt` carried with a `ForwardResult`.                                                                                                   |
| `src-tauri/src/proxy/error_mapper.rs`                  | Client status/message mapping and preservation of genuine upstream error semantics.                                                                                                                                             |

The current limited degradation applies at named HTTP restoration boundaries, including `resume_completed_context_diagnosed` and `verify_output_call_ids`. It must not become a global interpretation of `TokenError::ContextUnavailable` as permission to continue.

## 3. Target boundaries

```text
                         native client request
                                  |
HTTP route --> client protocol adapter
                                  |  native text sites, history claims,
                                  |  strict session hints
                                  v
                       shared reversible DLP core
             rules / references / mappings / leases / budgets
             authority / tool barriers / completion / audit
                                  |
                       existing provider conversion
                                  |
                           actual upstream request

original upstream response --> upstream completion observer
                                  |  borrowed native facts only
                                  v
                     existing protocol converter, if needed
                                  |
                                  v
                client protocol adapter + shared core
                    native JSON/SSE restoration and proof
                                  |
                                  v
                             local agent
```

There are two protocol identities for every attempt:

- **Client protocol** determines the request and response contract exposed to the local agent and the history model used for subsequent requests.
- **Upstream protocol** determines the original completion observations required before conversion.

For example, Responses-to-Chat produces Responses client history, never Chat client history. Completion must combine original upstream facts, conversion validity, final client snapshots, and the actual provider-attempt binding. Neither conversion nor observation alone can publish a completed context.

### Shared core responsibilities

The following names describe target responsibilities, not existing public API commitments:

| Core component        | Responsibility                                                                                                                                                                               |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AttemptCore`         | Own the request lease, policy generation, provider-attempt binding, payload budget, and audit state.                                                                                         |
| `RequestTransaction`  | Detect enabled rules at adapter-supplied text sites and return an atomic set of replacements respecting native-unit edit constraints. Never silently replace mandatory redaction with allow. |
| `TokenAuthority`      | Generate and match references, resolve authorized mappings, and enforce namespace, generation, request, and binding checks.                                                                  |
| `StreamTextState`     | Match complete references across arbitrary delta and TCP boundaries, hold ambiguous tails, and reconcile deltas with final snapshots.                                                        |
| `ToolCommitBarrier`   | Hold tool output until call identity, arguments, restoration, and protocol completion permit an atomic release.                                                                              |
| `CompletionAuthority` | Accept only protocol-verified evidence and issue completion authority for the exact actual attempt and delivered snapshots.                                                                  |
| `HistoryAuthority`    | Publish completed records, validate exact client-replayed history, and handle Responses cursor continuation separately.                                                                      |
| `FailurePolicy`       | Classify internal failures, permit only whitelisted HTTP context degradation, and produce the fixed client-safe error semantics.                                                             |

Adapters cannot construct a completion witness, mark an arbitrary response complete, resolve a mapping, or publish history. Completion booleans supplied by a handler are not proof.

### Adapter/core data contracts

The contracts below are conceptual and should use existing budget-owned payloads and private capability types during implementation:

| Contract           | Contents                                                                                                                                                                                                     | Authority                                                                         |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| `TextSite`         | Opaque adapter-assigned site ID, borrowed native string, source class such as current input, historical text, tool arguments, or tool result, and a native-unit edit constraint (`editable` or `immutable`). | Inspection only; no restoration authority. A readable site need not be writable.  |
| `TextPatch`        | Editable site ID, source range, and replacement text, all covered by the attempt budget and validated against the native-unit edit constraint.                                                               | A proposed native edit, not permission to disclose a mapping.                     |
| `HistoryClaim`     | Source-classified native history item, call/result pair, opaque item, or Responses cursor presented for validation. Distinguish delivered assistant history from client-supplied result input.               | A claim only; syntax and pairing never prove delivery or execution.               |
| `CompletionProof`  | Protocol-specific verified facts plus common transport and actual-attempt evidence.                                                                                                                          | Can be constructed only by the strict protocol verifier and accepted by the core. |
| `RestoreDecision`  | Emit unchanged, emit a native patch, hold, or fail internally.                                                                                                                                               | Does not expose internal diagnostics to the client.                               |
| `CompletionRecord` | Common binding and one protocol-specific evidence variant with exact dependency and delivered-response snapshot digests; no attestation of client-supplied result bodies.                                    | Published only after final client delivery conditions succeed.                    |

Use a common record binding with `ResponsesEvidence`, `ChatEvidence`, `AnthropicEvidence`, or `GeminiEvidence`; do not erase those variants into a generic cursor or a stringly typed success flag. The evidence records facts needed for authorization, not a second full copy of the response.

### Native adapter contract

Each client adapter must provide equivalents of:

| Operation                                        | Input                                               | Output                                                                                                       |
| ------------------------------------------------ | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `observe_request`                                | Native `Value`, route, and headers                  | Text sites, native history claims, and a strictly validated client-provided session identity when available. |
| `apply_request_patches`                          | Native request and an atomic patch set              | The same native request with only approved string sites changed.                                             |
| `observe_upstream_json` / `observe_upstream_sse` | Original pre-conversion response or complete frames | Borrowed transport/protocol facts; no completed record.                                                      |
| `restore_client_json`                            | Final client-protocol JSON and core authority       | Native edits and verified completion facts.                                                                  |
| `push_client_sse`                                | A complete native client SSE frame                  | Original frame, changed frame, held output, or an internal failure.                                          |
| `finish`                                         | End-of-entity facts and native terminal state       | A strict completion proof or failure; never synthetic success.                                               |

If no edit is required, pass through the original bytes. If an edit is required, rebuild only the affected native JSON value or frame while preserving unknown members and protocol semantics. Byte identity after mutation is not a goal; lossless preservation of untouched protocol data is.

## 4. Native protocol adapters

| Adapter                | Request/history surface                                                                             | Response/stream surface                                                                                                                       | Protocol-specific requirements                                                                                                                                                                     |
| ---------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Responses              | `input`, instructions, reasoning items, function calls/results, `previous_response_id`, and Compact | Output items, text/reasoning/refusal deltas, argument deltas, snapshots, and `response.completed` / `response.failed` / `response.incomplete` | Preserve existing cursor verification, operation lineage, encrypted-reasoning treatment, Codex history proof, and neutral `resp_proxy_failed` failures.                                            |
| Chat Completions       | `messages`, string or array content, assistant `tool_calls`, and role-`tool` results                | Choices, content deltas, tool calls keyed by choice and tool index, argument deltas, finish reasons, and `[DONE]`                             | Isolate parallel choices and calls. There is no server-side Responses cursor. Do not confuse legacy function-call fields with proof of completion.                                                 |
| Anthropic Messages     | `system`, message content blocks, `tool_use`, and `tool_result`                                     | Block start/delta/stop, `input_json_delta`, message delta/stop, and stop reasons                                                              | Isolate block-index state. Buffer and restore structured tool arguments without treating arbitrary partial JSON as complete text. Preserve immutable thinking/signature blocks as specified below. |
| Gemini generateContent | `contents/parts`, system instructions, `functionCall`, and `functionResponse`                       | Candidates, native parts, function calls, finish reasons, and `streamGenerateContent?alt=sse` frames                                          | Function arguments are native JSON, not necessarily Chat argument strings. Do not invent a cursor or decode opaque signatures/binary parts. Signed Parts are immutable as specified below.         |

For every adapter:

- Images, inline binary data, files, and other opaque media remain opaque; do not decode them for DLP scanning.
- Ordinary text inside tool results remains subject to DLP.
- Unknown fields and unsupported carriers are not new restoration authority.
- Opaque fields such as encrypted reasoning or Gemini `thoughtSignature` receive no permission merely because they resemble history.
- Initial streaming coverage is Responses, Chat, and Anthropic SSE plus Gemini `streamGenerateContent?alt=sse`. Nonstreaming `generateContent` returns ordinary JSON. Gemini streaming without `alt=sse`, including JSON-array/sequence framing, is not part of the first adapter: retain existing nonreversible behavior only when no reversible action or dependency requires restoration; otherwise return the generic internal processing error before forwarding. Do not tokenize a request whose response framing cannot be restored.
- WebSocket support and unlisted stream framings are outside this design.
- Existing provider conversion remains responsible for protocol conversion; the reversible layer must not silently discard semantics that the converter would otherwise reject.

### Signed native units: immutable in the first adapters

Preserving a signature field alone is insufficient. Anthropic requires the entire unmodified thinking block, including its signature, when returning the associated tool results; see [thinking and tool use](https://platform.claude.com/docs/en/build-with-claude/context-windows#the-context-window-with-thinking-and-tool-use). The first native adapters use the following conservative contract:

- Anthropic `thinking` blocks, their `signature`, and `redacted_thinking` blocks are immutable native units. Gemini Parts carrying `thoughtSignature` are likewise immutable, including associated text or `functionCall` arguments. Keep each signature on its original Part; do not split, merge, move, strip, or synthesize signed content. This Gemini restriction is a local compatibility policy, not a claim that its signature authenticates every argument byte.
- Immutability forbids patches, not inspection of supported readable fields. Continue ordinary DLP inspection of readable thinking/text and structured argument string sites. Signatures, redacted thinking payloads, and binary data remain opaque. A signed unit is not proof of local delivery, session identity, tool execution, or mapping authority.
- Pass an immutable unit unchanged only when no mandatory patch is required and it contains no reference authorized for restoration in that unit. Unknown, unauthorized placeholder syntax remains literal under the existing authority rules; it does not by itself force a failure or grant restoration. Mandatory redaction or an authorized reference requiring restoration in an immutable unit is an unsupported processing boundary: fail with the Section 7 generic local error rather than edit the unit, expose unredacted input, or silently leave a required restoration undone. An explicit `Block` still uses its policy-block projection.
- Decide immutability before applying a patch. For SSE, mark Anthropic thinking immutable from block start, including when its signature arrives later. If a protocol can attach a signature after readable deltas, hold the affected native unit within the attempt budget until its editability is established. Do not emit an edited prefix and then discover that the unit is signed. Budget exhaustion fails under Section 7; no partial privileged tool call may escape the commit barrier.
- No original-versus-delivered signed-payload replay cache is introduced in this design. Client replay must retain the original signed unit; do not restore it for display and try to recreate its signed bytes by retokenizing the next request. A missing stable identity or completed record removes historical restoration privilege, but does not require rejecting an otherwise valid unchanged signed unit or waive mandatory inspection. Never reconstruct a lost original from a digest or guess.

This limitation applies to native Anthropic/Gemini admission in Stages 3 and 4 and their later conversion cells, not to the Stage 1 Responses-equivalence extraction. A conversion cell that cannot preserve the required signed-unit contract stays disabled for reversible processing. Supporting mutations through a dual-version replay design requires separate approval and two-turn protocol evidence.

## 5. Request and response lifecycle

### Request

1. Decode and parse the native request using existing HTTP limits and error behavior.
2. Let the client adapter enumerate supported text sites with native-unit edit constraints, native history dependencies, and a strict session identity. Distinguish client-supplied tool results from delivered assistant history. Do not derive restoration identity from an arbitrary response ID or a generated per-request UUID.
3. Run ordinary rule inspection and explicit `Block` handling before forwarding. Create a request-local lease only when reversible processing or validation of a prior dependency requires it.
4. Validate history through the appropriate model below. A missing optional completed context may remove historical restoration privilege; it may not disable mandatory current-input redaction or turn an explicit block into allow.
5. Apply budgeted patches atomically to editable native sites without rebuilding unrelated fields through a common IR. A mandatory patch conflicting with an immutable unit fails before forwarding.
6. Run the existing provider conversion and late-history construction. Revalidate the final actual outbound attempt and its dependencies before sending. Conversion cannot expand authority.
7. Carry one attempt object through retries and response processing. A retry needs the existing actual-attempt binding rules; it must not inherit success from a different provider or operation.

### Response

1. Observe original upstream JSON or frames before conversion. Preserve non-success upstream status, body, and error semantics without entering local successful-response restoration.
2. Convert to the final client protocol when necessary. Original upstream terminal evidence and converted terminal evidence are separate facts.
3. Restore authorized references only at editable native client text sites. Use the shared stream matcher and the adapter's native item/call indexing; never perform unscoped global string replacement. Apply Section 4's immutable-unit failure contract rather than patching signed payloads.
4. Hold tool calls behind the commit barrier until their complete native arguments and required snapshots are valid. A malformed or interrupted original response publishes no completion and releases no partial privileged tool call.
5. Reconcile final snapshots and transport completion. Publish only authorized dependencies and verified client-delivered history variants for this bound attempt, including unchanged native units where appropriate. Do not label client-supplied tool-result bodies as delivered response snapshots.
6. An original failure, cancellation, truncation, invalid UTF-8/JSON, unsupported encoding, or capacity failure cannot be converted into synthetic successful completion.

## 6. History and authority model

Keep four permissions distinct:

1. **Current-request restoration.** Mappings created by the current lease may be restored at editable sites in its bound response. This remains available without a stable multi-turn session identity; immutable-unit restrictions still apply.
2. **Responses server-side continuation.** Only the Responses adapter interprets `previous_response_id` and Compact. It retains the current store identity, namespace, generation, policy revision, provider/operation lineage, completed context, call/output binding, and opaque-history checks.
3. **Client-replayed delivered history.** Chat, Anthropic, and Gemini validate exact ordered native assistant items and tool calls against records of what the proxy actually delivered. Client-supplied tool results have the separate input contract below, not delivered-response proof. This is not a cursor protocol and must not reuse Responses cursor semantics.
4. **Independent known-reference proof.** An independently recognized reference may use an existing exact completion proof where current behavior permits it. A degraded request therefore need not have no active tokens.

### Delivered history versus client-supplied tool results

| Input class                                                                                       | Validation and authority                                                                                                                                                                                                                                                                                                              |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Replayed assistant content and tool calls                                                         | Require exact delivered snapshots, native ordering, and the applicable session/provider/attempt binding before granting historical restoration authority. A call ID alone is insufficient.                                                                                                                                            |
| First client tool result: Chat role-`tool`, Anthropic `tool_result`, or Gemini `functionResponse` | Validate native call/result pairing and, where historical authority is sought, the corresponding delivered call proof. The new result body has never been delivered by the proxy: treat it as untrusted current input, inspect supported text, and apply current DLP rules. Do not require a previously delivered result-body digest. |
| Replayed or edited older client tool result                                                       | Continue treating its body as untrusted request input and inspect it again. The first adapters do not create an inbound-result history store or infer exact-delivery proof from previously accepting a request. Edits do not inherit the prior result's mappings or prove that a tool ran.                                            |

Pairing establishes only the applicable call relationship. It neither authenticates the result body nor proves actual execution, grants execution permission, or independently authorizes a historical mapping. A reference in any result body still needs its own applicable core authority; current-input mappings may be created only through ordinary rule processing. An unverified call/result relationship or missing optional call record grants no historical privilege, but does not by itself reject otherwise valid ordinary input. Preserve native request validation, explicit blocking, mandatory redaction, and strict non-degradable failures.

For example, after delivering `tool_use(id=c1)`, the proxy may accept the first `tool_result(c1, "ok")` without a prior snapshot of `"ok"`. It validates the applicable call relationship separately from inspecting the new result. Replaying that result later does not turn it into proxy-delivered history.

Default-deny cases include:

- cross-store, cross-session, cross-generation, or cross-attempt restoration;
- cross-provider or cross-client-protocol transfer not explicitly authorized by an existing binding rule;
- edited, reordered, truncated, or merely similar assistant/call history presented as exact delivered history;
- legacy or unknown placeholders without a valid mapping and completion proof;
- forged cursors, fabricated call IDs, or completion observations without strict verification;
- deriving historical restoration privilege solely from placeholder syntax or call/result pairing.

**Approved identity limitation:** without a valid client-provided stable session identity, grant current-request restoration only. Do not substitute a generated request UUID, a content hash, or a conversation guess. Some native Chat/Gemini clients may consequently lack cross-request restoration until they provide a supported identity; this is preferable to cross-session disclosure.

Loss of optional in-memory history does not invalidate an otherwise independent current request. It also does not create authority to reconstruct old plaintext, approve unknown placeholders, or fabricate a completion record.

## 7. Failure and client-projection contract

The core remains strict. Only the HTTP orchestration boundary may classify an error as a nonblocking context degradation, and only at an explicit whitelist of restoration-history operations.

| Condition                                                                          | Required result                                                                                                                                               |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Explicit `Block`                                                                   | HTTP 400, `dlp_blocked`, and `Request blocked by local security policy.`                                                                                      |
| Whitelisted missing session/cursor/completed-memory context                        | Continue the independent request without historical restoration privilege; emit no internal DLP diagnostic to the agent and publish no fabricated completion. |
| Any other internal DLP processing failure not downgraded by the whitelist          | HTTP 500, logical `server_error / proxy_processing_failed`, and exactly `The local proxy could not process the request. Please retry.`                        |
| Mandatory redaction or restoration failure                                         | Fail closed at that boundary; never send or return the unredacted original as a fallback.                                                                     |
| Capacity, encoding, authentication, network, and other genuine processing failures | Preserve their real failure semantics; a general requirement to continue does not override them.                                                              |
| Genuine upstream HTTP error or native error event                                  | Preserve the upstream status and error semantics rather than projecting it as local DLP failure.                                                              |
| Auxiliary audit/usage/tool-observation failure                                     | Do not interrupt otherwise valid forwarding; preserve intentional security and safe-redaction failures.                                                       |

Protocol adapters may use their native error envelope, but the logical code, fixed message, and absence of implementation detail are invariant. In particular, agents must never receive `Reversible DLP session or cursor context is unavailable.`, internal checkpoint/stage/reason labels, sensitive plaintext, matched secrets, credentials, or raw tool inputs.

Streaming has a transport boundary that cannot be hidden by this API. The commit point is the first commitment of response status/headers or body bytes to the client:

- Before that commit point, an internal processing failure returns the HTTP 500 projection above.
- After that commit point, HTTP status cannot be rewritten. Emit at most one native error envelope carrying the same generic code/message and no internal details, then terminate the stream without publishing completion:

| Client protocol    | Post-commit local failure envelope                                                                 | Forbidden synthetic success                                                              |
| ------------------ | -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Responses          | Existing `response.failed` event with neutral ID `resp_proxy_failed`.                              | No `response.completed`.                                                                 |
| Chat Completions   | One SSE `data` frame containing an OpenAI-style `error` object.                                    | No `[DONE]`, successful `finish_reason`, or completed context after the error.           |
| Anthropic Messages | One `event: error` frame containing a native Anthropic error object with the generic message/code. | No `message_stop` or successful terminal stop reason after the error.                    |
| Gemini `alt=sse`   | One SSE `data` frame containing a native Gemini `error` object with the generic message/code.      | No synthetic successful candidate, `finishReason`, or completed context after the error. |

If the adapter cannot safely emit that error envelope, terminate the stream without a success sentinel. Do not buffer an entire stream merely to pretend that all failures can become HTTP 500, and do not replace an upstream's own error event with the local DLP error projection.

Internal error codes, checkpoints, and stage/reason logging remain available only through redacted diagnostics. The client projection is a separate decision, not a formatting change to `TokenError` or the core authority checks.

## 8. Incremental migration

Each stage requires its own approved implementation plan and test-first changes. Keep the existing public behavior behind temporary facades rather than rewriting all routes at once.

### Stage 0: freeze the Codex contract

Use the existing security suites as the behavior baseline, including limited context degradation, mixed media, forged cursors, stale placeholders, cache authority, original failures, retries, and upstream errors. Record client-visible and internal-permission outcomes separately.

### Stage 1: extract the core with Responses behavior unchanged

- Introduce the client/upstream protocol distinction inside existing admission and attempt orchestration.
- Move reusable token matching, stream buffering, budget ownership, tool barrier mechanics, and completion publication behind core interfaces.
- Wrap existing Responses behavior in its native adapter; retain `previous_response_id`, Compact, conversion evidence, and all current client projections.
- Keep `admit_http_request` and `DlpPreparedAttempt` as temporary facades if that minimizes call-site churn.
- Do not change native Chat, Anthropic, or Gemini admission in this stage.

### Stage 2: add native Chat Completions

- Implement native request/history sites and choice/tool-indexed JSON/SSE restoration.
- Separate delivered assistant/call proof from first and replayed client tool-result input under Section 6; do not require a prior delivered result-body snapshot.
- Connect the native Chat handler and response exits to the prepared attempt; current `process_response` passthrough is not sufficient.
- Enable native Chat upstream first. Enable each converted upstream direction only after its original-evidence and final-client-evidence tests pass.

### Stage 3: add native Anthropic Messages

- Implement content-block lifecycle, `input_json_delta`, tool use/result pairing, and native terminal validation.
- Add exact delivered assistant/call history authorization independently of Responses cursor handling, while treating `tool_result` bodies as untrusted input under Section 6.
- Enforce immutable thinking/signature and redacted-thinking units, including late signature deltas, before enabling native reversible admission. Cover unchanged two-turn replay and mandatory-patch conflicts; no dual-version replay cache is included.

### Stage 4: add native Gemini generateContent

- Implement `contents/parts`, candidates, native function-call JSON, nonstreaming `generateContent`, and `streamGenerateContent?alt=sse` restoration.
- Add the pre-forwarding guard for Gemini streaming without `alt=sse`: no reversible tokenization is admitted for that unsupported framing, and no active restoration dependency is passed through.
- Add strict session/history handling and Section 6's untrusted `functionResponse` input contract without treating thought signatures or binary parts as readable text.
- Enforce immutable signed Parts, including native `functionCall` arguments and signature association, before enabling native reversible admission. Validate supported readable sites without mutating the signed unit.

### Stage 5: enable conversion cells individually

Validate and enable each `(client protocol, upstream protocol)` cell independently. Do not infer the entire conversion matrix from a successful native route, and do not infer restoration support from the existing protocol-routing matrix. Preserve existing authentication, retry, usage, and provider semantics.

## 9. Verification and acceptance

### Shared contract tests

Add or retain tests proving:

- reference matching at every byte/delta split, including UTF-8 boundaries, ambiguous prefixes, final snapshots, and budget exhaustion;
- isolation by lease, namespace, generation, store, stable session, provider, operation, and actual attempt;
- no authority for unknown legacy placeholders, forged cursors, fabricated call IDs, or unverified completion;
- zero completion publication and zero privileged partial tool release on malformed arguments, interrupted transport, original response failure, or encoding failure;
- no unredacted fallback after mandatory redaction failure;
- nonblocking auxiliary audit/observation failures and unchanged genuine forwarding failures;
- client errors containing no internal codes/checkpoints/stage/reason labels, cursor detail, sensitive fixture values, credentials, or raw tool input;
- redacted internal diagnostics retaining only safe structured codes/checkpoints/stage/reason labels, never sensitive fixture values, private configuration, credentials, raw cursor values, or raw tool input;
- an independent subsequent request still works after optional context loss.

### Per-protocol tests

Every adapter needs native JSON and streaming tests for:

- unmodified byte passthrough and preservation of unknown fields and event semantics after an edit;
- opaque images/binary parts and continued DLP on ordinary tool-result text;
- parallel Responses items, Chat choices/calls, Anthropic blocks, or Gemini candidates as applicable;
- complete and fragmented tool calls, argument restoration, tool-result pairing, terminal success/failure, and cancellation;
- exact history replay, edits, reordering, truncation, wrong session/provider/binding, and retries;
- pre-commit HTTP errors and post-commit native stream errors;
- all deliberately enabled converted-upstream cells, not just native upstreams.

Protocol-specific regressions must additionally cover Responses Compact and forged cursors; Chat `[DONE]` and parallel tool arguments; Anthropic `input_json_delta`; and Gemini native function argument objects, `alt=sse` framing, and pre-forwarding behavior for streaming without `alt=sse`.

The Stage 2-4 plans must map these additional cases to named native JSON/SSE tests and require the stated outcomes before enabling the affected adapter:

| Case                                                                                         | Required outcome                                                                                                                                                                                                                                                                                                                                         |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Anthropic signed thinking plus a tool call, followed by the first `tool_result`              | With no mandatory patch in the thinking unit, preserve the complete thinking block and signature across delivery and replay; validate the delivered call separately and inspect the new result without requiring an old result-body snapshot. Use an opaque synthetic signature; the fixture proves preservation, not provider cryptographic acceptance. |
| Anthropic thinking with a late `signature_delta`, or Gemini signed text/`functionCall` Parts | Preserve native unit boundaries and signature association. A core-authorized reference requiring restoration must fail without editing the signed unit or releasing a privileged partial call. If signature association is resolved late, no edited prefix may have escaped.                                                                             |
| Mandatory request redaction in an immutable readable site                                    | Reject before forwarding with the generic processing error; no mutation under the original signature and no unredacted fallback. Separately retain the explicit `Block` projection and literal handling for unauthorized placeholder syntax.                                                                                                             |
| Signed-unit replay without a stable identity or after completed-context eviction             | When no mandatory patch or independently authorized restoration is required, pass the unchanged unit as ordinary input without historical privilege. Do not reconstruct an original, fabricate proof, or relax inspection on surrounding input.                                                                                                          |
| First Chat role-`tool`, Anthropic `tool_result`, and Gemini `functionResponse` bodies        | Accept an otherwise valid result without a prior delivered result-body digest. Sensitive supported text still receives current-input DLP; pairing alone authorizes neither historical references nor tool execution.                                                                                                                                     |
| Replayed/edited results, forged or ambiguous pairing, and wrong session/provider             | Grant no historical mapping or execution authority from the result or pair. Inspect every result as untrusted input; withhold unverifiable historical privilege without turning that alone into a policy block. Retain genuine native validation and non-degradable failures.                                                                            |

For signed-unit failure cases, assert the Section 7 pre-commit HTTP or post-commit native stream projection as applicable, zero completed-context publication, and zero privileged partial tool release. Exercise these cases in each deliberately enabled conversion cell as well as on native routes.

Existing regression sources include:

- `src-tauri/src/proxy/security/tests/request_literal_tests.rs`;
- `src-tauri/src/proxy/security/tests/reversible_completion_tests.rs`;
- `src-tauri/src/proxy/security/tests/reversible_integration_tests.rs`;
- `src-tauri/src/proxy/security/tests/upstream_error_tests.rs`.

Use synthetic secrets, in-memory or temporary storage, nonexecuting tool recorders, and loopback listeners on dynamic ports. Do not start or replace the user's application, use production credentials, execute generated tools, or compete for `127.0.0.1:15722`.

For each implementation stage, run its new named tests first and require a nonzero match count. Then run the affected existing filters, including `proxy::security::` and any changed protocol-matrix suites:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml proxy::security:: -- --test-threads=1
cargo test --lib --manifest-path src-tauri/Cargo.toml proxy::protocol_matrix_tests -- --test-threads=1
```

Before claiming a stage complete, run the repository checks from the root:

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

A passing unit or fixture suite establishes only the tested local contract. It does not establish compatibility with every provider, client, model, installer, or live service.

## 10. Non-goals and forbidden implementation shortcuts

- No complete unified request/response JSON IR and no lossy normalization of native protocols.
- No unscoped global string replacement as a substitute for stream and tool state.
- No image decoding/scanning, remote plaintext lookup, shared remote mapping service, or restoration inside upstream hosted tools.
- No promise that a model can understand unseen plaintext or that restoration repairs computation, encoding, slicing, or schema-incompatible values.
- No generic cursor shared by Responses, Chat, Anthropic, and Gemini.
- No restoration authority from placeholder syntax, completion observation, call ID, or generated session UUID alone.
- No relaxation of `tokenization` permission checks, resource budgets, mandatory redaction, explicit blocking, or genuine upstream/network/authentication failures.
- No forwarding of unredacted originals to satisfy a nonblocking requirement.
- No exposure of internal DLP diagnostics, sensitive values, private configuration, credentials, or raw tool input to clients.
- No `unwrap()` or `expect()` in the proxy hot path, no database locks or blocking transactions in forwarding, and no change to asynchronous audit/usage persistence requirements.
- No automatic enablement of untested protocol-conversion cells, WebSocket routes, or client capabilities.

Implementation remains a separate task after this architecture document. It must follow the staged path above and preserve the current Codex behavior while each new native adapter is introduced.
