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

## Prepared-request target boundary

A prepared DLP attempt is bound to its final upstream target and sending authority, even when it has no active token mappings. The reqwest path disables automatic redirects for these attempts. Both reqwest and Hyper reject upstream 3xx responses with a fixed status-only DLP error before parsing the response body; neither follows `Location` with the original request body or inherited history permission. Configure the final upstream endpoint directly. Requests without a prepared DLP attempt retain the existing redirect behavior.

This boundary does not establish complete implementation acceptance. See the [history provenance execution specification](../design/security-dlp-history-provenance-spec.md) for the full contract and required verification.

## Current actions

The base credential/PII rules, including private-key material, default to redaction; private RFC1918 IPs default to audit-only. Optional catalog rules default to allow. The persistence boundary normalizes saved/imported `block` settings to `redact`; historical audit events retain their original action. The engine still has protective error paths when redaction fails. The generic `sk-` notice has its own restricted actions.

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
