# Vary AI troubleshooting

Use synthetic examples when reporting a problem. Logs, provider configuration, client authentication files and database backups may contain credentials or private prompts. Review and redact them before sharing.

## CC Switch rejects a shared database as too new

This section concerns historical shared-storage builds. Current VaryAI uses an independent native database pair and keeps its files separate from CC Switch. The historical compatibility baseline was CC Switch v4.0.21 shared schema v23; its portable monolithic export and extension formats are distinct.

An earlier compatibility development build mistakenly added two default-valued `enabled_dsh` columns and advanced the shared file to unreleased v24. Do not delete the file, blindly change `user_version`, or overwrite it with an older backup: that can hide unsupported data or discard newer edits.

For this specific mistake, close **both** applications and stop the development watcher before using the explicit helper. Select the verified v23 backup from before the upgrade under the configuration directory's `backups/storage-split/`. From the repository root, validate without writing:

```bash
python3 -B scripts/recover-shared-v23.py \
  --database "$HOME/.cc-switch/cc-switch.db" \
  --reference /absolute/path/to/verified-v23-backup.db \
  --backup-dir "$HOME/.cc-switch/backups/shared-v23-recovery"
```

Only after validation succeeds, repeat with `--apply`. The tool backs up the current file, verifies the complete reversed schema against the reference, checks that the new columns contain only defaults, and uses an exclusive transaction with integrity checks. It retains current data rather than restoring old rows. A failure stops or rolls back the change; preserve the recovery backup. Rebuild VaryAI with the released-v23 fix before starting it again, or an old development build can repeat the unwanted upgrade. Recovery is not applicable to other future schemas or databases with v24-specific data.

## Independent storage recovery cannot complete

New VaryAI installations create independent empty storage. Configure providers and resources in VaryAI or restore a VaryAI backup. There is no old VaryAI installation-upgrade workflow.

Historical files named `cc-switch.db` or `vary-ai.db` inside the actual default `~/.varyai` directory are retained. They do not require deletion, renaming or a directory change: VaryAI opens its own `varyai.db` / `varyai-ext.db` pair beside them without converting their contents. Actual CC Switch source directories and aliases remain invalid native destinations.

A missing configured native data directory opens recovery: reconnect it, choose another native directory, use the default root or explicitly create empty storage. Device settings and accounts remain under `~/.varyai`. Native pair mismatch requires a matching pair or a portable restore, not an arbitrary extension file. See [storage and recovery](architecture/storage.md).

Configure cloud accounts and the initial sync direction independently. VaryAI uses its own remote namespace and does not transfer old application credentials or remote state.

## A new installation cannot enable client routing

Existing native client configuration remains in its original directory after VaryAI data separation. If a previously working client suddenly appears unconfigured in a development test, check the resolved client directory first. `CC_SWITCH_TEST_HOME` redirects client paths as well as VaryAI data; an empty temporary home will hide the original client files. Use private copies or existing-client fixtures instead of interpreting this as a production migration requirement.

For a genuinely absent client configuration, add or import a provider and apply it on Home, or initialize the client before enabling routing.

Claude, Codex, Gemini and Grok Build show setup guidance when native configuration is missing, without starting the gateway or creating a live backup. Other integrations also require supported models, native product configuration, connection consent or certificate trust. Follow the [per-agent prerequisites](guides/provider-routing.md#routing-prerequisites-on-a-new-installation). Empty synthetic files used in isolated tests are not installed for ordinary users.

## WorkBuddy reports an unsupported cross-protocol field

A `400 Unsupported field in cross-protocol request` from local routing means the request could not be converted before contacting the provider. WorkBuddy sends Chat Completions requests, including local conversation markers and optional reasoning controls. Use a build that handles these markers during conversion and preserves explicit reasoning and verbosity controls for a Responses upstream. An explicit `reasoning_effort: "none"` also disables reasoning when converting to Anthropic or Gemini.

Reload the updated VaryAI backend, leave WorkBuddy routing enabled, and retry the message. Ordinary Chat forwarding and official WorkBuddy traffic retain their native payloads. Unsupported protocol semantics still produce an explicit error rather than silently dropping model settings or tool data.

## Local proxy port conflicts

The local proxy normally listens on `127.0.0.1:15722`. An existing application instance can cause `address already in use` errors. A second launch can also foreground the existing instance and exit because the application uses a single-instance plugin.

On macOS or Linux, identify the listener:

```bash
lsof -nP -iTCP:15722 -sTCP:LISTEN
```

Quit Vary AI normally from its tray menu before restarting. Confirm which process owns the port before stopping anything. For automated tests, use temporary configuration directories and dynamic ports instead of interfering with a running application.

## Gemini routing is enabled but no requests reach the gateway

VaryAI writes Gemini routing variables to `~/.gemini/.env`. Gemini CLI 0.66 loads a single environment file: a nearer project `.env` can prevent the global Gemini file from loading, even when the project file contains no Gemini variables. Environment variables inherited from the shell also take precedence. Restart the CLI after changing its routing configuration.

With Gemini API-key authentication selected and the default VaryAI listener running, exit the existing Gemini session and test from the same project with explicit routing variables:

```bash
GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:15722 GEMINI_API_KEY=PROXY_MANAGED gemini
```

Use the configured listener address if it differs. `PROXY_MANAGED` is the local takeover placeholder; the gateway obtains the upstream credential from the selected provider. This command applies only to the new process and does not overwrite the project's `.env`. Send a small synthetic prompt and verify that a Gemini request appears in VaryAI. A successful direct gateway probe establishes forwarding, but does not establish that the CLI loaded its routing configuration.

## Proxy request errors

Inspect the relevant status code and sanitized logs. Current logs use `~/.varyai/logs/`; historical logs may remain in `~/.cc-switch/logs/`. Use the application's diagnostics to locate the active file.

For a generic Gemini `INVALID_ARGUMENT`, compare the `GeminiRequestShape` entries for the affected clients. These entries record message and function counts, parameter formats, and output-token ranges without model names, prompts, tool names, schemas, or argument values. The summary helps identify request differences; it does not establish which field the upstream rejected.

### HTTP 404: endpoint or protocol mismatch

An upstream that implements `/v1/chat/completions` may not implement `/v1/responses`. Check the provider's configured API format and base URL. When the upstream requires Chat Completions, select the matching conversion/routing mode supported by that client integration. Confirm the resulting endpoint with a synthetic request. Changing a model name alone does not add an unsupported API endpoint.

Protocol detection uses a bounded 1,024-token output budget so reasoning models can finish thinking before returning the short test answer. Older 128-token probes could receive only reasoning with `max_tokens` or `incomplete`, even when authentication and the API format worked. Detection still requires the exact test answer and normal completion; a truncated response never establishes compatibility.

A successful minimal Responses probe does not establish support for every model and native coding-agent input. If ZCode receives an upstream 502, compare the same synthetic request with another model on that gateway. Current company acceptance passes with GLM while the reported Qwen model can fail on ZCode's full input despite accepting a minimal message. Preserve the original protocol parameters when diagnosing this difference; record the model ID and a sanitized status, without credentials or raw client input.

### Gemini model discovery fails with a valid API key

Select **Gemini Native** for a Google Generative Language API key. Model discovery uses the `x-goog-api-key` header, not a Bearer token or a generated `key` query parameter. For `https://generativelanguage.googleapis.com`, it requests `/v1beta/models`; base URLs ending in `/v1` or `/v1beta` are normalized while gateway path prefixes are retained. An explicit model-list URL takes precedence.

The catalog reads `models[].name`, removes the `models/` resource prefix, and follows `nextPageToken` on the same endpoint. Results are sorted and deduplicated. Repeated tokens or more than 100 pages fail rather than silently returning an incomplete catalog. Switching protocols refreshes the discovery catalog. Other protocols retain their existing model-discovery authentication behavior.

Fetching the catalog does not generate model output. If discovery still fails, check the HTTP status, API restrictions, and network connectivity without publishing the API key. A successful catalog request does not establish access or quota for every inference model.

### HTTP 503: unavailable model or channel

Check the upstream error and the models available to the account. Fetch the provider's model list when supported, review the imported catalog and default model, and select an available model in the client. If the gateway supports model mapping, map only to an actual available target. Check account quota and routing rules as well as model spelling.

### HTTP 413: request too large

The upstream reverse proxy or provider can reject a request even when it is below the local proxy's limit. Reduce oversized attachments and pasted logs, or compact the conversation using the client's supported command. Operators of an upstream gateway should check its documented body limits before changing them. A local setting does not override the provider's limit.

## Upstream HTTP errors during reversible DLP forwarding

For an initial non-success upstream HTTP response, the proxy preserves the status. A prepared DLP request, including one with no restoration references, uses a separate safety check before returning the body. Safe JSON fields are retained; safe text is wrapped in a JSON error. Sensitive values are redacted and unsafe object keys are removed. This is a JSON error even when the client requested streaming, not a successful SSE response. Ordinary Codex upstream errors no longer receive a local-failure prefix or added provider, model or endpoint details.

If reading, decoding, privacy checks or capacity limits prevent a safe return, the response keeps the upstream status and uses `type=upstream_error` with `code=upstream_error_body_unavailable`. Its message includes only the status and a fixed failure category, such as `body_read_or_decode_failed`, `body_safety_failed` or `body_read_timeout`. This fallback does not establish why the upstream rejected the request. Earlier versions always withheld the body; still older versions could replace the status with HTTP 502 and `dlp_restore_failed`.

The client-only safety-checked body is not reused in diagnostics or request-error summaries. Internal accounting retains the actual attempted provider and status without adding those private details to the client error. Prepared DLP errors do not retry or fail over, restore error text, or authorize history continuation. A local request rejection such as `dlp_context_unavailable` at `reference_scan`, or a failed 2xx success validation, is a different failure and is not evidence of an upstream HTTP rejection. Nearby timestamps alone do not link two failures to one request.

Error-body handling limits raw and each decoded layer to 1 MiB, bounds source indexing and cumulative inspection work, and checks loop deadlines. Encoding inspection covers selected JSON escapes, Base64 and percent-encoded forms; it does not guarantee detection of arbitrary encodings or private data absent from the available safety context. Synchronous decoder calls are not forcibly preemptible by the asynchronous timeout, and these limits do not prove a total-memory bound for decoder-internal allocations.

Use the status and any safe reason to guide investigation: check authentication for 401/403, request/protocol compatibility for 400/422, quota for 429, and upstream availability for 5xx. Neither status nor fallback category proves the root cause. Report only sanitized categories, checkpoints and synthetic reproductions; do not share raw upstream bodies, references, prompts, tool arguments or credentials.

## Placeholder literals and restoration authority

In protected requests and successful responses, a placeholder is replaced only when it is complete, valid and authorized by the current request's restoration lease. Malformed, partial, unmapped or unauthorized placeholders remain literal text; their appearance alone does not cause a context error or grant access to a mapping. Request literals still undergo independent DLP inspection, including blocking rules. Tool arguments must still be valid protocol JSON.

Streaming restoration buffers only a bounded possible placeholder suffix across chunks. A partial suffix is released unchanged at a valid field or response boundary. This does not synthesize a completion event, permit tool execution, or authorize a follow-up request. Explicit continuation dependencies, binding checks, tool-completion evidence, snapshot consistency and resource limits remain enforced. Initial upstream HTTP error bodies use the separate safety path described above, not this restoration path.

After verified completion, tool continuation accepts the original exact snapshot and a bounded set of exact delivered snapshots under the same binding and completion record. This includes restored arguments and validated complete SSE argument carriers whose raw text was preserved. JSON-equivalent spellings that were not delivered do not gain continuation authority. Missing completion evidence, changed tool identity or reasoning, binding mismatches and budget failures remain rejection boundaries.

Exact delivered-message proof supports text inspection; it does not by itself make unrelated, fully paired readable tool history depend on a prior completion. Recorded tool identities still require strict completion checks, even when their name, arguments, reasoning or one identity field changes. Identity-only candidates do not prove changed text. Authorized restoration references, explicit continuation and opaque dependencies retain their independent checks.

When readable history mixes a recorded tool continuation with unrelated, fully paired calls, completion checks apply to the recorded calls only if the original input is unchanged and has no authorized restoration references, explicit continuation or opaque dependencies. Cache-enriched or otherwise changed input keeps the full witness requirements. Once selected, a strict completion obligation cannot be removed by later eviction of optional delivered-history records. Independent DLP inspection, actual-binding checks and exact completed snapshots still apply.

Inside decoded function-call JSON arguments, business keys named `encrypted_content` or `encrypted_reasoning` are tool data, not protocol continuation dependencies. Their values still undergo DLP inspection and reference checks. Actual encrypted fields on protocol history items, explicit continuation, authorized restoration references and recorded tool calls retain their independent completion requirements; JSON parsing and resource limits are unchanged.

A `dlp_context_unavailable` message can represent different request checkpoints; the message alone does not prove session loss. Use fixed checkpoint categories and synthetic reproductions without publishing placeholder values or private content. Source tests do not update the running application.

## Codex stream fails during reversible DLP restoration

`Reversible DLP response restoration failed.` means the protected response did not complete successfully, even if reasoning was already visible. The generic `dlp_restore_failed` code can represent several failures; it does not identify a particular upstream model or protocol defect. A request audit entry with `redact_restore` records the request action, not proof that the final outbound payload was redacted or the complete response was restored.

One reproducible Chat Completions conversion defect was treating `tool_calls: []` as the start of a real tool call. This prematurely closed reasoning, so later reasoning deltas failed consistency validation. The converter now ignores empty tool arrays as boundaries while preserving real tool boundaries and strict response validation. Synthetic regressions cover ordinary and inline reasoning, a placeholder split across reasoning deltas, restored text/tool arguments, and protected tool-follow-up preparation. Missing terminal evidence still prevents tool release and continuation authorization.

For an affected session, correlate sanitized timestamps and session identifiers, distinguish an initial HTTP error from a mid-stream failure, and verify outbound redaction, final response completion and follow-up preparation separately. Historical audit hits or partial reasoning cannot establish end-to-end success. Do not publish prompts, credentials or tool arguments, or disable fail-closed checks to hide the error. A running application needs a deliberately rebuilt and restarted version to use source changes; tests do not update it automatically.

### Fixed failure checkpoints

New diagnostic builds append a fixed suffix, for example `Reversible DLP response restoration failed. [stage=sse_snapshot]`. The stage identifies the check that failed, **not the root cause**. The `dlp_restore_failed` code, HTTP status and `response.failed` envelope are unchanged. Failures also emit a `[DLP] restoration failure` warning containing only the fixed code, stage and optional reason; normal frames are not logged by this diagnostic.

| Stage                                        | Failed checkpoint                                                           |
| -------------------------------------------- | --------------------------------------------------------------------------- |
| `upstream_guard` / `upstream_timeout`        | Upstream response admission/decoding guard or its deadline.                 |
| `success_validation` / `response_processing` | Success-response validation or downstream decoding/conversion.              |
| `response_encoding` / `sse_init`             | Final response encoding or SSE restorer initialization.                     |
| `sse_read` / `sse_frame` / `sse_eof`         | Stream read, frame parsing/budget/UTF-8, or an unterminated frame at EOF.   |
| `sse_event`                                  | Event structure or an upstream failure/incomplete event.                    |
| `token_restore`                              | Token recognition/authorized restoration operation.                         |
| `sse_snapshot`                               | Snapshot, identity, delta/done or final-output consistency checks.          |
| `completion_evidence`                        | Required original-protocol or final-response completion evidence.           |
| `tool_commit` / `context_publish`            | Preparation of buffered output for commit or completed-context publication. |
| `json_restore`                               | Buffered JSON read, parsing, restoration or serialization.                  |
| `unknown`                                    | An unspecialized failure exit; do not infer a more specific cause.          |

Token-restoration diagnostics also include a fixed reason, for example `Reversible DLP response restoration failed. [stage=token_restore reason=token_format]`. The warning uses the same fixed fields: `[DLP] restoration failure code=dlp_restore_failed stage=token_restore reason=token_format`. A reason survives outer HTTP/SSE error wrapping and sticky failure; it can also appear with another stage when that stage invokes the same restoration helper. Unclassified `token_restore` failures use `reason=unknown`. Unsupported carriers keep their existing error instead.

| Reason                                      | Failed checkpoint or grouped error                                                                                                                                                       |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `token_format`                              | A strict internal restoration operation rejected malformed input. Ordinary request and response placeholder literals do not require strict resolution.                                |
| `token_context`                             | Request-lease authorization, exact stored category, namespace/generation or storage-context lookup. The diagnostic does not distinguish these checks.                                    |
| `restore_capacity`                          | A restoration scan/reference/count/length limit, payload budget, allocation or copy-capacity failure. Ordinary text without tokens can also reach this checkpoint.                       |
| `arguments_empty` / `arguments_size`        | Empty/whitespace-only tool arguments or arguments over the existing byte limit.                                                                                                          |
| `arguments_json`                            | Tool-argument JSON parsing/validation, including duplicate keys; parser details are not exposed.                                                                                         |
| `arguments_shape`                           | Parsed tool arguments are not a top-level JSON object.                                                                                                                                   |
| `arguments_rebuild` / `arguments_serialize` | Restored argument-tree reconstruction or serialization, including helper errors already collapsed at that boundary. These labels do not imply valid syntax or exclude capacity failures. |
| `arguments_copy`                            | Final argument text conversion/copy checkpoint, when no more specific capacity reason is available.                                                                                      |
| `unknown`                                   | No reliable subreason is available; do not infer one from partial output.                                                                                                                |

Reasons describe rejected checkpoints, not a diagnosis of a past incident. They contain no dynamic upstream error text, tokens, matched text, tool inputs or configuration. Request authorization, fail-closed behavior, original-protocol completion evidence, snapshot consistency, tool commit barriers and completed-context publication order are unchanged.

Before reproducing, explicitly agree how to build and load the new application binary. Source edits and `cargo test` do not update or restart a running application. Prefer a fresh synthetic session. Record the stage suffix, model, sanitized timestamp and whether the failure was an initial HTTP error or followed partial output. If the suffix is absent, first verify the loaded build. Do not send raw upstream errors, tokens, prompts, tool arguments or private configuration. HTTP 200, an audit hit, partial reasoning or an ordinary final reply alone still does not prove end-to-end redaction and restoration.

### Reference-format migration and legacy compatibility

The approved reference-format migration is implementation-in-progress in the current working tree, not a release or live-client acceptance claim. Check the loaded build before comparing sessions. Only newly generated references use the parameterized grammar `rredact-{type}-{token}-ai`: `{token}` is exactly 20 lowercase hex characters, the first 10 bytes of HMAC-SHA-256. The namespace key and internal domain `b'vary-dlp-token-v1\0'` are unchanged. Types are exactly `cn_phone`, `cn_id_card`, `private_key`, `api_token`, `db_conn`, `password`, `infra`, `personal`, `custom` and `mixed`; inner underscores are required. `infra` labels Infrastructure, not only IP addresses. New references occupy `32 + type.len()` ASCII bytes (37–43), not the legacy fixed 31 bytes. See the [token and authorization contract](design/security-dlp-reversible-tokenization-spec.md#token-and-authorization-contract) for category meanings and historical proposal boundaries.

The legacy recognition-only grammar is `vary_<cc>_<token>_ai`, with the known two-character codes `ph`, `id`, `pk`, `ak`, `db`, `pw`, `ip`, `pi`, `cu` and `mx`. Complete legacy references remain recognized but are rejected as context unavailable: request admission can return `dlp_context_unavailable`, while response restoration fails the existing context check. They are never rewritten or authorized as new-format aliases, even if their digest matches a current mapping. In strict carriers, damage or incompleteness after a known new or legacy header still rejects. Unknown type/code nonmatches grant no authority.

Request admission and enriched-body checks without a captured restoration lease scan raw keys and values first, then additionally decode only `arguments` strings containing a backslash on actual `function_call` items in the top-level `input[]` array. The existing prepaid `PayloadJson` parser enforces the 1 MiB encoded-size limit and depth/node limits, reserving retained-tree and parser-scratch allocations against the shared payload budget before decoding. Decoded keys and values retain strict reference checks: unleased enriched bodies and disabled gateway/runtime paths reject references; malformed escaped JSON or capacity failures fail closed. Ordinary strings, `custom_tool_call.input` and metadata strings are not recursively JSON-decoded. This inspection grants no authorization and relaxes no budget.

A format migration cannot recover lost or legacy context. Rebuild readable context from trusted original sources under the current policy; do not manually rewrite references, infer mappings from a matching digest, or disable authorization to continue. Keep complete references and digest values out of documentation and reports; describe synthetic tests with parameterized grammar only.

### Exact recognition in ordinary output text

Production restoration recognizes only complete, exact-format references in `message.content[].output_text.text` and its corresponding Responses SSE text events. Malformed, lookalike, unknown-type and field-end incomplete text stays **exactly unchanged**, including unfinished known-header literals: it is not truncated, replaced with a notice or rejected solely for not matching. Scanning continues at nested and later starts after a nonmatch, so it cannot hide a complete reference of either recognized grammar.

A complete new-format match still requires the original request lease, the full stored mapping with exact type/spelling, and current namespace/generation/attempt authority; a matching digest alone is insufficient. An unauthorized complete match still fails, and a complete legacy match is rejected as context unavailable, not upgraded. Request admission and history, tool arguments and inputs, reasoning, refusal, metadata and keys retain strict handling. Original completion evidence, raw delta/snapshot consistency, tool commit barriers, completed-context publication, capacity limits and retry behavior are unchanged.

SSE retains only a suffix that could become a complete reference, bounded at 42 ASCII bytes (maximum new reference length minus one), and releases it unchanged at the validated field boundary if it stays incomplete. Event positions are preserved, but adjacent delta text may be buffered and coalesced; clients must concatenate deltas rather than rely on original chunk sizes. Successful authorized references are restored normally. This behavior does not activate the private whole-field projection below or grant continuation authority to nonmatching text.

Synthetic JSON, native HTTP and Chat-to-Responses SSE tests cover nonmatch preservation, split references, nested/later unauthorized matches, strict excluded carriers, snapshot/completion refusal and context isolation. Migration regressions exercise every supported type and split, the 37–43-byte length bounds, the 42-byte incomplete tail and legacy recognition without alias authorization, without publishing complete reference or digest vectors. From the repository root, use `cargo test --lib --manifest-path src-tauri/Cargo.toml reference_format_migration_` for migration regressions and `cargo test --lib --manifest-path src-tauri/Cargo.toml output_text_exact_` for ordinary output-text checks. These are suggested code-verification commands, not results of this documentation update; they do not update a running application or establish live-client, installer or release acceptance.

### Bounded token-format structure warnings

Production SSE restoration now adds at most one `token_format v=1` warning per restorer, on its first actual `token_format` failure. It uses the existing application log sink with target `token_format_diagnostics` and obeys the application's warning-level filtering; it does not add a capture file, database record, setting or request switch. Successful streams and failures with other reasons do not emit this warning. Existing stage/reason/checkpoint labels and client error envelopes are unchanged.

The summary contains only fixed enums, booleans, a sequence counter capped at 255, and byte-length buckets. It keeps exactly four fixed-size recent-event slots, oldest first; unused slots have sequence zero. Unknown event names become `other`. It never retains or logs message text, raw deltas, token markers, matched fragments, tool inputs, content hashes, illegal-character values, session/response/provider identifiers or configuration.

- `site` distinguishes delta splitting/restoration, done-text restoration, supported snapshots, and final text/tool restoration. `carrier` names only a fixed supported field class.
- `shape` groups the first observable defect as `digest`, `suffix` or `incomplete`; `invalid` groups the character as Markdown, whitespace, ASCII, non-ASCII or end-of-input. These are observations of the rejected input, not a second acceptance decision.
- When `boundary_known=true`, `cross=true` means the observed candidate spans the pending-tail/current-delta boundary, and `cross=false` means it does not. When `boundary_known=false`, the boundary was not observed and `cross=false` cannot rule out cross-delta splitting. `partial` identifies the branch that permits a valid partial candidate; it does not excuse the actual failure.
- `input`, `pending`, and recent-event lengths use buckets: zero, 1–7, 8–15, 16–30, 31–63, 64–255, 256–1023, or 1024-plus bytes. Recent entries include `barrier_before` and `structure_known`. Before/after tail buckets describe committed state; a rejected delta leaves the tail unchanged. When `structure_known=false`, its default zero buckets do not establish an empty payload.

Some nested restoration helpers do not expose the failing decoded leaf. Their summary intentionally leaves the shape unclassified (`none`) and may use `carrier=other` or `site=unknown`; it does not scan unrelated metadata or guess from encoded tool JSON. The recent events describe admission order, not necessarily the event whose buffered content failed final validation. Transport/frame failures outside this restorer have no such structure summary. The log cannot reconstruct content or guarantee a precise explanation of a past failure.

The warning is observation only. Ordinary output-text nonmatches follow exact recognition above and do not emit a format-failure warning; strict excluded carriers still do. Authorization, snapshot checks, completion evidence, tool barriers and retry behavior remain unchanged. The private text-projection experiment remains test-only. Synthetic regressions verify bounded output, non-disclosure and sticky failures; they are not real-client acceptance and do not establish a fix for request-side `dlp_context_unavailable`.

To see the new warning, explicitly rebuild the application, quit the old process and start the new build yourself. Tests do not update a running application. No launch, installation, replacement or automatic restart is part of these checks. Share only the bounded warning and a sanitized timestamp after reviewing surrounding log entries; do not enable raw payload capture to investigate it.

### Request context is unavailable

`dlp_context_unavailable` is a request/continuation refusal, not a response `token_restore` failure. New diagnostic builds emit a warning such as `[DLP] context failure code=dlp_context_unavailable checkpoint=token_resume`. The HTTP 400 status, error code and client message remain unchanged. Only fixed checkpoint labels are logged, on failure; no payload, token, tool input, credential, provider configuration or normal frame is attached.

| Checkpoint group                                               | Failed boundary                                                                                                                                                 |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reference_scan` / `unleased_reference`                        | Reserved-reference scanning, or a late reference without a captured request lease.                                                                              |
| `gateway_state` / `gateway_missing` / `runtime_disabled`       | Gateway access, or a reference presented without an active restoration gateway/runtime.                                                                         |
| `namespace_admission` / `history_extract` / `history_classify` | Namespace admission, history extraction or immutable-history classification.                                                                                    |
| `admission_audit` / `admission_prepare`                        | Initial audit-summary validation or request preparation. These labels do not mean persistence failed.                                                           |
| `attempt_fork` / `attempt_binding` / `attempt_history`         | Actual-attempt lease creation, binding or enriched-history extraction.                                                                                          |
| `context_resume`                                               | Completed cursor/call/opaque context restoration, including binding, policy and snapshot checks; the label does not distinguish them.                           |
| `output_call_ids`                                              | Orphan tool-output ownership verification.                                                                                                                      |
| `token_resume`                                                 | Historical-reference authorization through the request lease or compatible completed dependencies; the label does not identify a particular missing dependency. |
| `attempt_audit` / `request_prepare`                            | Actual-attempt audit-summary validation or candidate preparation/proof.                                                                                         |
| `request_transform` / `final_validation`                       | Transformation refusal or final outbound validation.                                                                                                            |

A format-complete reserved reference in documentation or tool output is still a context dependency, not authorization. A complete legacy reference is rejected, not translated into a current mapping; unknown type/code nonmatches grant no authority. Complete recognized references can make otherwise readable history require strict continuation checks. Those checks restore call/cursor/opaque context and verify orphan outputs **before** historical-reference authorization. A sample's presence therefore does not establish which check refused the actual request. Namespace mappings, earlier completed turns and HTTP 200 responses are not substitutes for request-level authorization.

User-message `input_image` parts no longer invalidate otherwise complete, readable tool history at `history_classify`. This compatibility path accepts only an `image_url` string or an object containing a string `url`, with optional `detail` (`auto`, `low` or `high`); URL strings must be nonempty and additional image fields are refused. Images remain opaque: no fetching, decoding, OCR, image-content inspection or replacement is performed. Their fields are forwarded unchanged on native Responses routes. This is not general media support: files, audio, video, unknown shapes, images in other message roles, tool outputs, reasoning and refusal payloads do not gain this exception.

Readable text and tool inputs are still inspected. Reserved references (including references in image fields or keys), encrypted dependencies, prior cursors, call pairing/uniqueness, budgets, completion evidence, tool-commit barriers and context publication retain their existing checks. Synthetic classifier and dynamic-port HTTP regressions can be run with `cargo test --lib --manifest-path src-tauri/Cargo.toml image_history_`. These checks do not inspect real images, prove the exact failing client request, update a running app or establish live-client/release acceptance.

Record only the fixed checkpoint and a sanitized timestamp when correlating failures. A checkpoint is the first observed refusal boundary for that attempt, not a precise root cause or a unique request identifier; retries may produce separate warnings. Some labels cover multiple internal checks. This logging change neither bypasses fail-closed checks nor implements conversation fallback. Source edits and local tests do not update the running application.

### Private safe-text projection is not a production fallback

A separate synthetic-only experiment can prepare a sanitized native Responses JSON/SSE projection after original completion, snapshots, identity and all complete-reference authorization checks pass. It replaces an entire damaged `output_text` field with a fixed notice; capacity, context, metadata, excluded carriers and completion errors still refuse. SSE emits nothing until clean EOF and final preparation. Projection never publishes completed cursor/reference/call/opaque authority and does not schedule a retry.

Every projection entry point is `#[cfg(test)]` only. The production exact-recognition behavior above neither replaces whole fields nor enables this experiment. There is no configuration, request flag, environment variable or production dispatch path to enable it. Fixed `stage`, `reason` and `checkpoint` contracts remain unchanged. In particular, response projection does **not** repair request-side `dlp_context_unavailable`, and no new evidence establishes the root cause of the observed incident.

The private regression filter discovers and passes 53 synthetic tests, including every reference boundary as semantic SSE deltas, budget/EOF refusal, zero continuation authority and dynamic-port native HTTP fixtures. Independent safety review passed only this private scope. Backend regression checks passed; the original frontend unit command still failed by discovering nested `.voidx/pr-worktree` tests, while the explicit root-only exclusion rerun passed. See the [execution and verification record](design/security-dlp-text-fallback-execution-spec.md#implementation-and-verification-record-2026-10-30) for exact commands, counts and limitations.

Real Codex notice display, automatic retries, duplicate actions and next-turn history/cursor behavior remain unverified. Converted-route projection, production activation and client/release acceptance are not established by these fixtures. No running application was updated by this work.

## Reversible Codex HTTP DLP: temporary limits

- Compressed SSE with an active restoration lease is unsupported and fails closed. Compact SSE is separately unsupported; Compact JSON coverage does not establish SSE support. Do not remove the rejection checks to enable either mode.
- New tool-risk events omit command summaries until custom-DLP policy can be carried safely through projection and persistence. Risk detection and structured events remain enabled; existing stored summaries are not rewritten.
- Full allocation accounting remains incomplete for request sanitizers, provider/model configuration, response parsers/converters, persistent Chat metadata/index snapshots, codecs and HTTP transport. Logical budgets are not a proven physical-memory or RSS cap; header allocation cleanup does not close transport accounting.
- Raw observation queues, caches, asynchronous audit/usage persistence and every diagnostic sink are not established as custom-DLP-safe end to end. Summary omission and a nonblocking queue regression do not close that wider boundary.
- Restored protocol output is not execution authorization or proof of tool-task correctness. Shell quoting, embedded JSON and patch semantics still require independent validation; no generated commands are executed by the regression tests.

These limits are deferred, not release acceptance. The local WIP and follow-up checks do not establish merge readiness or live Codex/model behavior. Earlier execution/closure records remain historical snapshots, not current test results.

## macOS launch and installation problems

Local development packages may be unsigned or use an ad-hoc signature. Gatekeeper warnings and signature failures require checking the artifact's origin, hash and signing status. Follow the [macOS build guide](guides/local-macos-build.md) for deliberate local build and signing options. Unit-test success does not establish notarization or installer acceptance.

A missing Launchpad entry does not establish that an application failed to install. Check the actual application path and package before changing system registration or replacing the installed app.

## Agent installation checks

Settings > About > Local environment check lists every supported agent. The eight managed CLI cards retain version checks and installation/update actions. A missing Codex CLI shows **Not installed** and an **Install** button; an installed CLI that cannot run has a separate environment warning. Failed checks show a retry message rather than an installation prompt.

Claude Desktop, WorkBuddy, Qoder CN, Doubao Work, TRAE CN, Kimi Code, Qwen Code and ZCode also display their installed version. On macOS, desktop cards read the product version from the installed application's bundle metadata without launching it. Desktop installations take precedence over CLI installations and are labeled explicitly; an embedded editor's version is not the desktop product version. Kimi can instead run a bounded `--version` check when only its CLI is installed. Qwen Code uses the same version checks, installation diagnostics and upgrade pipeline as the other managed CLI tools. A WorkBuddy launcher alone establishes presence but is not executed as a version-only CLI. Missing metadata or failed version commands leave the installation marked present and show a version-read failure. Desktop application detection currently supports macOS; unsupported checks on other platforms are labeled explicitly.

The About page uses one shared card layout for CLI and desktop agents: current version, latest version and an Upgrade button when a newer version is available. On macOS, Claude Desktop, WorkBuddy, Qoder CN, Doubao Work, TRAE CN, Kimi Desktop and ZCode query verified publisher release sources. Missing or older remote metadata is not evidence that an installation is current. Refresh reruns detection.

Desktop upgrades run inside Vary AI and are included in Upgrade All. They download the matching official package to a temporary directory, verify any published checksum, product identity, product version, CPU architecture, code signature and the installed publisher's signing team. The updater stages the new application beside the existing installation before requesting a normal quit. It replaces only the application bundle at its existing location and restores the previous bundle if replacement fails; configuration and user data are not removed. A previously running application is reopened. Unsaved-work dialogs can prevent quitting; save and fully quit the agent, then retry. Folder permission failures do not trigger elevation or installation to a different location. If both replacement and recovery fail, the previous bundle remains in a `.varyai-agent-update-` folder beside the installation for recovery. Desktop automatic upgrades currently support macOS. Installation or upgrade failures show fixed localized reasons without exposing raw command output.

TRAE's displayed product version is distinct from its editor/build version. Doubao Work packages are distinct from the standard Doubao desktop app; Kimi Code Desktop packages are distinct from the Kimi chat app. The updater preserves these identities and never substitutes a similarly named product.

## Client takeover and recovery

### Claude Desktop refuses a shared connection

A saved or applied legacy Vary profile is not proof of a live proxy takeover.
Earlier builds blocked the switch when the old profile existed or was applied.
Disconnecting correctly restored the original profile, but that restored profile
could then block reconnection. An explicit shared connection now journals the
current Desktop documents, including an applied legacy profile, before changing
its owned fields. Disconnect restores that baseline; reconnect can acquire it
again. Pending legacy proxy backups, malformed or unreadable configuration and
conflicting external edits still fail safely rather than being discarded.

Claude Desktop 4.4212 encrypts gateway keys in its profile after startup. Earlier
Vary builds compared that ciphertext with the original plaintext, incorrectly
reported configuration drift, and refused disconnect with a configuration-conflict
message. The Desktop adapter now recognizes this at-rest sealing, preserves it
on refresh, and restores the managed profile and previous selection on disconnect.
Existing recovery receipts remain usable; do not delete them or replace the
encrypted key manually. User changes to deployment mode and unrelated profiles
are retained. A genuinely different gateway address or plaintext key still
requires explicit recovery. This compatibility check does not prove the contents
of an opaque macOS key or that an upstream request succeeds.

The Desktop model-list control remains available while disconnected. Saving a
selection only updates the private visibility preference; it does not enable
the Agent, change Desktop's active model or edit its configuration. The next
connection uses that saved list. Desktop itself remains responsible for choosing
the conversation's model.

Shared model names use Desktop's accepted Claude role prefix and a stable hash,
with the actual model name shown through `labelOverride`. Older `claude-vary-…`
aliases remain accepted by the authenticated local ingress for existing chats,
but are no longer published to Desktop's model picker. After loading the fixed
build, turn on Claude Desktop in Tools, then fully quit and reopen Desktop to
load the shared menu. Configuration success alone does not verify an upstream
model; send a new message and inspect request statistics. No login credentials
need to be deleted to resolve the inactive-profile conflict.

TRAE CN does not need to be open to select or edit a provider. Vary AI saves the selection locally and shows that client delivery is pending; keep Vary AI running and connect TRAE to apply it automatically. Switching to Official while offline also queues withdrawal of previously managed models. A successful local selection does not mean the closed client has already received it. Connection setup and auditing retain their existing consent requirements.

In Settings → Advanced → Local routing, a selected TRAE CN or Doubao Work switch can be waiting for its client connection. The waiting hint means enablement was saved, but traffic is not yet protected. Open the client and follow the displayed connection instructions; deselecting the switch cancels the pending request. Waiting enablement does not show an active-routing success notification.

Gateway key budget exhaustion is a permanent policy rejection until the budget resets or its policy is changed. Vary AI returns HTTP 402 with `gateway_budget_exhausted`, `insufficient_quota` and `x-should-retry: false`, so clients do not spend minutes retrying it as temporary rate limiting. Check the key's usage and reset time in Local routing → Gateway keys. Ordinary upstream rate-limit responses retain their original handling.

For Gemini, save the selected provider again after changing its model, then restart the CLI. Active proxy takeover now refreshes both the model environment and native model selection while retaining the local routing address. Custom models are registered in the dynamic `/model` registry supported by current Gemini CLI versions. A CLI `--model` argument or an inherited `GEMINI_MODEL` can still override the saved default.

In the Gemini provider editor, **Default model** controls the startup selection. The separate **Selectable models** section contains menu entries, with distinct display-name and request-model column headers. **Fetch model list** loads candidates and checks the current default. An empty default or a default absent from the fetched catalog is replaced with its first valid model; an advertised default is preserved. **One-click setup** applies the same check and imports all fetched models into the saved model list, skipping duplicate and empty IDs while preserving existing model metadata. Codex, Grok Build, WorkBuddy and Qoder follow the same default-model rule; Claude Code checks its role defaults, and Claude Desktop checks its model-mapping roles. Save the provider and restart Gemini CLI, then open `/model`, select **Manual** and press Enter to see the imported models. The list uses Gemini's native `modelConfigs.modelDefinitions` with dynamic model configuration enabled. Changing the endpoint or API key clears fetched candidates so a later fill action cannot reuse another connection's model list.

Qwen distinguishes models by their effective API protocol, model ID and endpoint, as described in its [model-provider documentation](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/model-providers/). Chat Completions and Responses can use the same model ID, and another endpoint can expose the same ID. Switching between saved Vary AI suppliers with the same protocol, ID and endpoint replaces the conflicting supplier's entire managed model group and credential environment entry before inserting the target. Both saved suppliers remain available for subsequent switches. Unrelated model groups, manual entries and MCP settings remain intact. Conflicting manual models or externally changed credentials and endpoints still require resolution. Vary AI retains the selected endpoint through takeover and recovery, and keeps model routes stable when the native list is reordered.

An OpenAI-only upstream key can work with an Anthropic-native client when the selected upstream format enables protocol conversion. That success alone does not mean the key supports Anthropic. To diagnose a mismatch, record the selected upstream format and a sanitized error; never include the key or full client configuration.

If a Claude Code upgrade reports that another instance is updating, Vary AI retries a single-tool update twice after short delays. A persistent lock still reports the native error: let the other updater finish before trying again. Vary AI also prevents concurrent installation/update commands within its backend. Other command failures and multi-tool batches are not automatically replayed.

Common configuration locations include:

| Client      | Configuration                                |
| ----------- | -------------------------------------------- |
| Codex       | `~/.codex/config.toml`, `~/.codex/auth.json` |
| Claude Code | `~/.claude/settings.json`                    |

Local proxy configuration can refer to `http://127.0.0.1:15722` or its `/v1` endpoint. If a client still points at that endpoint after an abnormal exit, review the application's takeover state and recovery options. A normal disable/exit path can restore the saved configuration; inspect the relevant backup before manual recovery. Back up the current files before editing them and preserve unrelated client settings. Do not publish authentication files or entire backups in an issue.

### Codex stays on "Loading models..." after enabling routing

Keep Vary AI and its local proxy running. After enabling the unified Codex model menu, fully quit and reopen Codex rather than only closing its window. Start a new conversation and select a published provider model or routing group. Existing conversations may keep their previous provider.

The current Codex routing switch represents a live connection, not a persisted startup preference. Normal application shutdown restores its configuration and removes its recovery receipt/catalogs; startup also recovers an interrupted projection without automatically reconnecting the menu. Consequently this switch returns to off after restart even when other agents resume ordinary proxy takeover. This is a lifecycle limitation, not evidence that routing groups or model windows were erased.

A partial leftover configuration can select `model_provider = "varyai-routing"` without a valid `model_catalog_json`, while retaining a raw upstream model ID instead of a published alias. The provider label alone does not prove that model-menu takeover is active. If explicit reconnect adopted this partial configuration as its baseline, disconnect correctly restores that same baseline rather than guessing a different provider; the orphan hint can therefore return on every reopen. The original writer of such a baseline must be established separately. Use VaryAI's explicit Codex reconnect/recovery flow to publish a complete catalog and current local credential; automatic refresh must not claim receiptless configuration. Reconnect keeps a private baseline backup and does not change the login file. If configuration was repaired after Codex's last model-list load, fully quit/reopen Codex so it reads the new projection. Do not manually invent a catalog path or delete recovery files.

An older unified-menu writer always set `requires_openai_auth = false`. Codex then hid a preserved OpenAI login from account discovery even though `auth.json` was unchanged. Desktop authentication and workspace initialization could fail while the model picker stayed on "Loading models...". The writer now derives this flag from the effective authentication store: file-backed OpenAI login material keeps it enabled, absent or unsupported login material leaves it disabled, and keyring/auto stores preserve the active provider's policy because disk alone cannot establish their login state. An ephemeral store remains signed out.

The local `experimental_bearer_token` remains installed on the Vary AI provider and takes precedence for forwarded request authentication; the preserved OpenAI credential is not substituted for it. Publishing and restoring the menu do not rewrite `auth.json`. Refreshing or reconnecting with a fixed build repairs a still-owned projection; external configuration edits still require the normal conflict/recovery flow. Do not delete authentication files, conversation databases, or recovery receipts to clear a loading indicator. A stale desktop initialization failure can still require a full Codex restart after repairing configuration.

### Codex shows only 31k context for a routing model

Earlier routing catalogs advertised a fixed 32,768-token window for every entry. With Codex's 95% effective-window reserve this appears as approximately 31k, even when an upstream has a larger window. This is a client context budget, not the account's remaining token allowance.

The corrected backend retains explicit model windows, maximum windows, effective percentages and separate compaction thresholds through official discovery and Codex publication. Select **Reload** in Routing to replace an older account discovery cache, then reload Codex configuration or fully restart Codex; existing conversations may retain their initial model metadata. This requires the updated Rust backend, not just frontend hot reload. User-set Codex `model_context_window` and `model_auto_compact_token_limit` can still override catalog behavior.

A group intentionally publishes no more than its smallest prepared destination's capacity, including inactive manual members that older conversations may still be bound to. Unknown destinations retain a conservative 32k fallback; no model capacity is inferred from its name. A relay with no exact-model capacity metadata can therefore keep both its direct entries and a mixed official/relay group at about 31k even after a successful Reload. Official discovery cannot establish the same-named relay model's capacity. Review the actual provider's exact-model catalog metadata rather than setting every model to 1M. Existing opaque continuations stay on their original account/model, and increasing a menu value never migrates them.

### Codex reports routing_unknown_continuation after reconnecting

This 409 means VaryAI cannot establish one exact owner for the supplied response ID, tool call or opaque continuation. Older builds retained ordinary ownership only in memory, so restarting VaryAI or reconnecting the Codex menu could lose it. Starting a new conversation avoids submitting unknown account-bound state; do not remove reasoning or compaction items from an existing history to bypass the check.

Updated builds asynchronously save bounded, encrypted ownership records for normal Codex-menu/native and authenticated gateway conversations. After reconnecting and reloading Codex configuration, the same consumer, alias and complete destination set can resume observed bindings. An old local bearer token still requires configuration reload; persistence never bypasses current authentication. Records expire after 24 hours, are bounded to 512 conversations, and are not portable conversation history. Temporary editor sessions remain memory-only.

Previously lost bindings cannot be recovered retroactively. Changed accounts, API keys, endpoints, models or execution settings; expired/evicted records; corrupt or unavailable local recovery files; and conflicting IDs still fail closed. A crash before queued writes reach disk can lose recent observations. No request is moved to another account, and no opaque history is silently discarded. See [continuation recovery](guides/intent-routing.md#continuation-recovery-after-restart).

### Codex Official login asks to sign in again after switching

Codex can rotate its refresh token while an Official provider card still has an older login snapshot. The fixed OpenAI Official card follows the current native login when no managed account is bound: switching only updates `config.toml` and leaves `auth.json` untouched, including imported token bundles without timestamps and a native logout. An explicitly bound managed account still supplies that account's credentials. Separately saved official account cards retain their account-switching behavior, with newer native tokens adopted only for the same user and workspace. A token already invalidated by the server requires valid login material again; this change cannot restore it.

### A configuration scheme references a removed supplier

Scheme switching changes only Agents captured in the scheme whose saved supplier references still exist. An Agent added after the scheme was saved keeps its current supplier, model and files. If a saved primary supplier or enabled supplier has been removed, the entire Agent entry is treated as uncaptured: the preview names the Agent, keeps its current configuration and lets the remaining entries switch. No replacement supplier is guessed and the saved scheme is not rewritten. Update its snapshot explicitly to adopt current selections.

Rollback and undo cover only Agents actually applied. If no entries remain applicable, the scheme does not report success or replace the current scheme or undo record. Other preparation failures name the Agent and a fixed, localized reason without exposing configuration or credentials.

### Shell command list cannot be saved

The editor distinguishes an existing match type and pattern (including disabled entries), a stale or deleted entry, and the 256-entry limit. Edit the existing item for a duplicate, reopen the latest item after a concurrent edit, or remove an unused item at the limit. Failed saves keep the draft. Other storage failures retain a generic message so database internals and command contents are not exposed. A successful regex preview validates the pattern; it does not establish that a later database write will succeed.

A successful save closes the editor and shows a success toast. If the entry was persisted but runtime application or audit recording failed, the existing warning appears instead of a success toast.

### No tool risk records after a Claude Desktop request

Enable tool risk collection and confirm the request passes through Vary AI. A text answer containing a command does not create a Shell event: the model must return a supported tool call. For Claude Code, ask it to call Bash with `pwd`, then refresh the tool risk records. A low-risk command can still produce a record.

Find low-risk calls under Security Center → Security Events → All Tool Calls. They do not increase the risk-warning total. For a Shell list pattern such as `^kubectl\s+delete\s+`, select Regex rather than Command Prefix; regex syntax in a prefix entry is treated as literal command words. Validate the regex, confirm its scope and save. The rule sandbox can verify a match without executing the command or creating a historical record.

The time-range summary counts sensitive-data records plus all saved tool-call records, including low-risk calls. Its tool-call total is independent of page filters and pagination. The warning card continues to count warnings only. A failed tool-count query displays an unavailable count and supports retry instead of substituting zero or an old warning count.

Claude Desktop uses a separate client identity. Builds missing its tool-schema registration discard otherwise supported calls. The registration fix recognizes its declared built-in Bash, Read, Write, Edit and WebFetch schemas while retaining a distinct Claude Desktop source. MCP tools and unknown schemas remain unsupported by these built-in adapters. Model-response records indicate a proposed operation, not proof of execution. Load the updated backend before testing the fix; the command-list editor can validate and save a pattern even when historical preview has no records.

Claude Code and Desktop providers using OpenAI Chat, Responses or Gemini conversion require observation before response conversion. This path binds native upstream calls to matching validated definitions from both the client request and the actual outbound request. Converted output is not promoted to trusted native evidence. A successful request or visible tool execution alone does not establish that an older backend recorded it.

### A Claude provider incorrectly shows no configured credential

Claude providers can authenticate with `env.ANTHROPIC_AUTH_TOKEN` as well as `env.ANTHROPIC_API_KEY`. The home card checks both fields and displays only the credential status. Older cards omitted the auth-token field and could report missing credentials even while proxy requests authenticated successfully.

### Qoder or WorkBuddy reports missing provider fields

Custom providers require a provider name, model ID, URL, and API key. Saving lists all empty or whitespace-only fields together and omits fields already filled in. Additional model rows also require an ID. Official login entries do not require custom model credentials unless a custom model is being added. A toast showing `providerForm.nameRequired` indicates a build with a missing translation reference; use a build containing the fix.

### Qoder asks to install the certificate after every restart

The certificate lasts 30 days and is retained across normal restarts. An older trust check supplied only the localhost server certificate, which could fail to locate its CA even when the CA was already trusted. The check now supplies the server and CA certificates together and still uses macOS SSL trust evaluation; it does not force the CA to be trusted. A valid, trusted saved certificate is reused without another installation prompt.

### Kimi shows an internal ID as the provider group

Apply the saved provider again to migrate its Kimi group to the provider name shown in Vary AI. Renaming it also updates model references while preserving model aliases and the selected model. Choose a unique provider name: Vary AI refuses to overwrite an existing native group with the same name. Restart Kimi when it needs to reload its configuration.

### TRAE CN switching reports a model configuration failure

TRAE can keep an older model-list cache after a custom model is added. Older Vary AI bridges may then report a failed switch even though the model was saved; retrying can return a native duplicate-model error. The bridge now refreshes TRAE's native model store before matching existing models and after each addition. It recovers models carrying the provider's legacy ownership label and retains readable labels without duplicating or adopting unrelated user models. Fully quit and reopen TRAE CN after loading a build containing this fix so its in-memory bridge uses the updated script, then apply the provider again. Direct custom-provider switches also withdraw the previous managed models before adding the target, because TRAE rejects overlapping request model IDs under different labels. A failed switch attempts to restore the previous models and retains the previous provider selection; an incomplete restoration is reported explicitly.

## Windows VC++ runtime errors

If launch fails with a missing `VCRUNTIME140_1.dll` or `VCRUNTIME140.dll`, an older artifact may still depend on the external Visual C++ runtime. Use the platform vendor's official runtime installer for the artifact's architecture when necessary; do not download individual DLL files from third-party sites.

For a new build, use `pnpm build:windows` and follow the [Windows build guide](guides/local-windows-build.md#cross-build-windows-x64-from-macos). The build helper requests `STATIC_VCRUNTIME=true` and rejects a generated EXE whose import table still contains the checked external VC++ runtime dependencies. Verify the exact public source SHA and artifact SHA-256: a version label alone cannot distinguish rebuilt packages.

Check a downloaded artifact in PowerShell, substituting its actual filename:

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath "$env:USERPROFILE\Downloads\Vary AI-Windows-setup.exe"
```

## macOS notifications

If events and in-app messages appear but system banners do not, check the notification permission and presentation options in Vary AI settings and macOS System Settings. Test foreground and background delivery separately. System acceptance of a notification does not prove that a banner appeared.

The macOS implementation uses `UNUserNotificationCenter` for permission and delivery; it does not fall back to legacy notification APIs when the application identity is unavailable. Source changes require a rebuilt development instance or a newly packaged application. Development builds use a separate temporary app bundle and do not replace an installed application. Exit an existing instance normally before starting a separate one.

Screen sharing or mirroring can suppress notification banners and sound. Check the macOS setting that permits notifications during sharing. In-app warnings are independent of system delivery; a recorded warning does not prove that the user saw a banner.
