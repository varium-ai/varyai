# DLP architecture and safety boundaries

The DLP engine inspects supported outbound text extracted from requests handled by enabled local routing. It is not a disk scanner or a guarantee that all client traffic or every JSON field is inspected.

## Processing

1. Extract the supported text fields without changing unrelated protocol values.
2. Use a multi-pattern prefilter to select applicable built-in expressions, PII checks and configured custom rules.
3. Apply the runtime rule settings. Risk levels classify findings; they do not independently decide the action.
4. When redaction is required, update only selected text values in a copy and inspect the result again. Return a protocol-shaped error if safe redaction cannot be established.
5. Queue audit work for bounded asynchronous persistence rather than writing to SQLite in the forwarding hot path.

## Codex history provenance

Immutable Codex input is classified as `FreshReadable`, `ExplicitDependency`, or `UnknownPassthrough` before recovery obligations are selected. Fully recognized, unchanged readable tool history can use per-call completion checks, including when `previous_response_id` is present. The cursor itself is still strictly resumed under the same binding and policy; selection never grants authority to unrecorded calls. Authorized references, non-null encrypted context, and cache/history enrichment disable per-call selection and keep all extracted recovery obligations strict. Live recorded calls and plaintext call reasoning attached to otherwise unknown history retain strict resume and completion-witness requirements. Authorized references in unsupported or immutable carriers, capacity exhaustion, internal failures, and explicit `Block` decisions are not downgraded.

`UnknownPassthrough` covers unrecognized media, fields, item shapes, and unprovable call/output pairing. It forwards the request without granting session, cursor, cached-history, restoration, or tool-execution authority, and it does not fabricate completion evidence. Chat cache enrichment is skipped for this classification. Supported text is still inspected and existing redaction or blocking rules still apply, but unknown fields and media are not guaranteed to be inspected or redacted.

### Missing memory-only completion context

Completion records and their restoration authority are memory-only. At the HTTP recovery boundary, `ContextUnavailable` from completed-context resume or output-call-ID verification degrades to forwarding without that recovery authority instead of rejecting the conversation. This does not relax the lower-level tokenization, cursor, binding, or completion-proof checks, and it never synthesizes a completion record. Unknown historical placeholders remain literal unless an independent exact completed mapping authorizes them; guarded cached tool history is discarded when no current restoration lease exists.

This degradation is limited to missing recovery context. Supported ordinary text, including text in tool results, still undergoes DLP inspection. Images remain opaque and are not decoded for scanning. Capacity, encoding, and necessary-redaction failures still fail closed; an explicit `Block` remains the only security rule that blocks ordinary conversation. Real upstream network, authentication, and HTTP failures retain their existing behavior.

Non-blocking internal processing failures use HTTP 500 with `server_error` / `proxy_processing_failed` and the fixed message `The local proxy could not process the request. Please retry.` Codex SSE failures retain the `response.failed` envelope with the same generic projection and neutral `resp_proxy_failed` ID. Explicit policy blocks retain HTTP 400, `dlp_blocked`, and `Request blocked by local security policy.` Internal codes, checkpoints, stages, and reasons are diagnostic-only and are not projected to the Agent.

## Direct native routes

The unified native restoration path handles only these direct native routes: Codex `/chat/completions` as `openai_chat`, Claude and Claude Desktop `/v1/messages` as `anthropic`, and Gemini `:generateContent` or `:streamGenerateContent?alt=sse` as `gemini_native`. Routes outside it keep whatever behavior they already had. In particular, the existing Codex Responses facade and gateway Responses recovery continue to restore their own supported text; neither they nor any other route gain authority from this path.

Identity is taken only from the client's own session signal: Codex session headers or body fields, Anthropic session headers or `metadata` session values, and the Gemini `x-session-id` header. A missing or invalid identity still restores the current request, but grants no historical privilege. Gemini framing comes from the method, path and query only; `body.stream` is never a Gemini streaming signal.

Signed reasoning units are immutable. Anthropic `thinking` signatures and `redacted_thinking` blocks, and Gemini Parts carrying `thoughtSignature` or `thought_signature`, are replayed unchanged or the request is refused; they are never edited to satisfy a redaction or placeholder. Vendor history normalization for MiMo/DeepSeek is therefore refused for native Anthropic requests that carry such units.

Gemini supports only `POST` generation RPCs: `:generateContent` with an absent or `json` `alt`, and `:streamGenerateContent` with exactly one `alt=sse`. Media Parts, `countTokens`, model listing, duplicated or bare `alt` keys and other framings are not restored. When a rule would require restoration, such a request is refused instead of being forwarded unredacted.

Failures are fixed and sanitized. Internal codes, checkpoints, stage labels, matched values and credentials never reach the client.

- **Degraded, not failed.** A Gemini answer that contains no restoration placeholder cannot need restoration. Shapes the strict checker does not model (an unsupported terminal reason such as `MAX_TOKENS`, media Parts, or an upstream safety block) are returned to the caller verbatim instead of being replaced by a proxy error. Nothing is published for them, so no completion authority is created. Restoration is likewise never a gate on an answer that carries no local placeholder: forwarding that body delivers exactly what restoration would have delivered, so the redacted upstream bytes are used unchanged.
- **Actionable refusal.** Refusals whose cause is provable report it instead of a generic failure.
  - Request side, nothing was sent: HTTP 400 with `invalid_request_error` and `dlp_restore_unsupported`, for a direct native route or framing reversible DLP cannot restore, and for signed or otherwise immutable content that a mandatory edit would have to change. This covers both mandatory rules and authorized references landing on a signed carrier.
  - Response side, the upstream already answered but its answer cannot be delivered: HTTP 502 for the native Anthropic and Gemini routes with `server_error` / `proxy_processing_failed` and the fixed processing message. This covers an answer that never verified, one that carries local placeholders that could not be restored safely, and a real local resource limit.
  - These messages never name a cause. A restoration limitation is not a DLP-module fault, and a placeholder that could not be restored is not a caller mistake, so the projection stays generic and the failing stage appears only in sanitized diagnostics. No message includes a rule name, matched value, credential or internal label.

- **Failure framing.** Nothing is yielded before these failures, so a native Anthropic stream ends with one fixed `event: error` frame and a native Gemini stream ends with one fixed `data: {"error":...}` frame instead of a silently broken stream. Chat keeps its accepted transport-error delivery. Unclassified local failures outside the response path keep HTTP 500 with `server_error` / `proxy_processing_failed` and the fixed message `The local proxy could not process the request. Please retry.` Explicit `Block` keeps HTTP 400 with `dlp_blocked`.

These statements describe the tested local contract. They are not a claim of provider compatibility, live-service behavior, installer acceptance or release readiness.

## Prepared-request target boundary

A prepared DLP attempt is bound to its final upstream target and sending authority, even when it has no active token mappings. The reqwest path disables automatic redirects for these attempts. Both reqwest and Hyper reject upstream 3xx responses with a fixed status-only DLP error before parsing the response body; neither follows `Location` with the original request body or inherited history permission. Configure the final upstream endpoint directly. Requests without a prepared DLP attempt retain the existing redirect behavior.

This boundary does not establish complete implementation acceptance. See the [history provenance execution specification](../design/security-dlp-history-provenance-spec.md) for the full contract and required verification.

## Current actions

The base credential/PII rules, including private-key material, default to redaction; private RFC1918 IPs default to audit-only. Optional catalog rules default to allow. The persistence boundary normalizes saved/imported `block` settings to `redact`; historical audit events retain their original action. The engine still has protective error paths when redaction fails.

The generic `sk-` notice supports audit, allow, redaction and local restoration,
matching the other key rules. Its default remains allow. Redaction replaces
matching text with an irreversible placeholder; local restoration uses the same
captured-policy admission, token authority and supported-route checks as other
restoration rules. Explicit action changes invalidate the previous restoration
policy generation. Its prefix pattern remains broad and does not attribute a key
to a particular provider.

Confirm behavior in `builtin_rules.rs`, `runtime.rs`, and the security DAO before changing settings or documentation.

## Unified security events

The Security Center reads DLP audit rows and tool-risk observations from the Vary extension database through `get_security_events`, `get_security_events_summary`, and `clear_security_events`. These bounded, filtered read/clear operations run in a blocking worker, not in the request-forwarding path. The default security-events view combines DLP rows with risky tool observations; the tool-calls view also includes unchecked, uncertain, low-risk, and historical observations. Prefixed keys distinguish the sources. The existing DLP and tool-risk APIs remain available.

Filtering and clearing share validated predicates. Clearing event rows does not remove policies, rules, or settings; database failures return fixed codes rather than stored content or query details. Tool summaries retain the existing read-time privacy projection, including for imported rows. Event results describe observations and DLP actions, not enforcement of tool execution. The queries cannot guarantee every observation was recorded.

## Privacy and persistence

Client errors and notifications must not contain matched sensitive plaintext. Stored audit summaries are masked; masking is not a proof of complete anonymity. Event aggregation and asynchronous notifications do not establish that every attempted event was persisted or displayed.

The Chat-to-Responses streaming converter's dropped-tool warning records only structural facts (index, empty-ID status, argument byte count, finish-reason presence and tool count). It does not log upstream model names, finish-reason strings or tool arguments. Final response rejection cannot undo an earlier diagnostic disclosure, so this restriction applies before completion validation and with or without active restoration mappings.

The detector itself executes locally. That statement does not describe the whole application's network behavior: see the [statistics and update review](../data-and-network.md). No performance benchmark is asserted here.

Large-payload regression tests verify clean-text passthrough and detection/redaction of trailing credentials. They do not assert wall-clock thresholds, which vary with shared CI runner load; performance measurement requires a separate controlled benchmark.

## Implementation references

- [Rules](../../src-tauri/src/proxy/security/builtin_rules.rs)
- [Extraction](../../src-tauri/src/proxy/security/extractor.rs)
- [Engine and redaction](../../src-tauri/src/proxy/security/engine.rs)
- [Runtime policy](../../src-tauri/src/proxy/security/runtime.rs)
- [Audit worker](../../src-tauri/src/proxy/security/audit_logger.rs)
- [Persistence normalization](../../src-tauri/src/database/dao/security.rs)
- [History classification](../../src-tauri/src/proxy/security/request_preparation.rs)
- [HTTP recovery admission](../../src-tauri/src/proxy/security/http_restoration.rs)
- [Regression tests](../../src-tauri/src/proxy/security/tests.rs)

Pattern literals, private-IP examples and synthetic credentials in tests are intentional detector fixtures. They must not be replaced with live credentials or removed merely because a secret scanner flags their shape. Existing third-party notices remain applicable.
