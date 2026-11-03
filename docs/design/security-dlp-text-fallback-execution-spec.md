# Reversible DLP: safe text projection execution specification

> Status: phase-A private, synthetic-test-only implementation and backend verification recorded on 2026-10-30; no production activation. Audience: implementers and security reviewers (human + LLM). Production delivery remains **blocked by client compatibility**. See [Implementation and verification record](#implementation-and-verification-record-2026-10-30) for evidence and limitations.

## Current production distinction

The record below describes the earlier private projection experiment, not the current production handling of ordinary output-text nonmatches. Production now uses exact reference recognition only in ordinary message output text: nonmatches remain unchanged, and every complete reference must still resolve under the original lease. Strict excluded carriers, request admission, completion evidence, snapshots and tool barriers remain unchanged. All projection entry points remain test-only; there is no production whole-field replacement. See [Exact recognition in ordinary output text](../troubleshooting.md#exact-recognition-in-ordinary-output-text) for the behavior and targeted verification command. Historical phase-A counts and limitations below are not current verification results for that separate change.

## Decision and scope

Retain strict refusal for request-context failures and unsafe restoration. Investigate a narrowly bounded alternative: after the original response is completely validated, replace an entire damaged `output_text` logical field with one fixed, non-sensitive notice. This is a **sanitized projection**, not successful restoration and not recovery of the missing text.

The first implementation phase is private, synthetic-test-only groundwork. Production continues using the existing fail-closed path. It must not promise uninterrupted Codex conversations: the repository does not establish how the actual client will handle a projected response, continuation cursors, or retries.

A projection is eligible only when all of the following hold:

- The original upstream transport and protocol completion are verified, including clean EOF where required.
- Original delta/done/part/item/completed snapshots and identities are consistent before replacement.
- Every complete reference resolves under the original request lease, with exact category, namespace, generation and attempt constraints. Malformed references cannot hide complete references elsewhere.
- The only restoration defects are `token_format` defects in explicitly supported `message.content[].output_text.text` fields.
- The entire response is text-only: message items containing `output_text` parts, with ordinary validated metadata. Any tool, reasoning, refusal, compaction, encrypted or unknown output carrier makes a damaged response ineligible in this first version. Existing successful handling of those carriers stays unchanged.
- All reservations, limits, final encoding and cancellation rules succeed. No payload has already been released to the client.

Only the damaged logical text fields are replaced. Preserve other validated text and protocol fields. Never copy a damaged substring into a notice. A proposed fixed English notice is: `Some response text was omitted because it could not be safely restored.` Treat this as a degraded-content notice, not an assertion that the answer or restoration succeeded.

## Source map and current behavior

Paths below are relative to the repository root. Function names are the stable locators; line numbers will drift.

| Source                                                                                             | Existing boundary and required use                                                                                                                                                                                                                                                                                                                           |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src-tauri/src/proxy/security/restore_diagnostics.rs`                                              | Fixed `RestoreStage`, `RestoreReason` and `ContextCheckpoint` labels already exist in the working tree. Preserve them and their error contracts. Never add payload-derived diagnostics.                                                                                                                                                                      |
| `src-tauri/src/proxy/security/tokenization.rs`                                                     | `visit_token_references` stops at the first malformed recognized candidate; `DlpRequestLease::resolve` verifies exact local authority/category and current namespace generation. Do not loosen either API's existing callers.                                                                                                                                |
| `src-tauri/src/proxy/security/response_restoration.rs`                                             | `restore_text_with_lease` collects ranges before resolving. `check_no_tokens_in_str` can return a scan failure before reporting an unauthorized reference. `SseRestorationState::process_sse_block` makes its first error sticky and stops processing. These are not safe catch-and-continue entry points.                                                   |
| `src-tauri/src/proxy/security/response_restoration.rs`                                             | `record_raw_event_state`, `check_snapshot_carriers`, `validate_raw_snapshot`, `finalize_and_flush_inner` and `stage_json_response` provide strict carrier/identity/snapshot constraints. Split validation from projection without deleting checks. `StagedJsonResponse::publish` and SSE finalization publish only after final immutable output preparation. |
| `src-tauri/src/proxy/security/http_restoration.rs`                                                 | `DlpPreparedAttempt::restore_client_http_response` validates and stages JSON before serialization/publication. `wrap_sse_stream_with_restoration_and_evidence` currently yields pre-barrier frames before EOF and returns on the first error. Whole-field replacement cannot start only after that error.                                                    |
| `src-tauri/src/proxy/security/completion_evidence.rs`, `completion_json.rs`, `completion_tools.rs` | Original protocol evidence, transport completion, terminal reasons and executable-item shape remain authoritative. Converter-generated completion is not evidence. No permissive completion branch is required.                                                                                                                                              |
| `src-tauri/src/proxy/security/http_restoration.rs`                                                 | `prepare_owned_attempt` resumes completed context and referenced tokens under the actual binding, then validates final outgoing carriers. Do not change request admission or resume behavior to accept projections.                                                                                                                                          |
| `src-tauri/src/proxy/providers/codex_chat_history.rs`                                              | Recording and enrichment are data/cache behavior, not authorization. A cached response or call cannot authorize a projected continuation.                                                                                                                                                                                                                    |
| `src-tauri/src/proxy/handlers.rs`, `response_processor.rs`, `forwarder.rs`                         | Existing attempt/evidence handoff and retry behavior stay intact. Do not add a fallback retry or mutate provider selection.                                                                                                                                                                                                                                  |

Earlier architecture documents describe historical implementation stages; inspect current code rather than assuming their old activation defaults still apply. This proposal introduces no new production activation surface.

## Failure classification

| Condition                                                                                                                            | First-version decision                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| Malformed recognized reference in an eligible `output_text` field; all independent checks pass                                       | Candidate for fixed whole-field projection, private tests only              |
| Complete reference with failed resolution, category mismatch, missing authority, namespace/generation mismatch, or unavailable store | Hard refusal, even before/after a malformed candidate                       |
| `dlp_context_unavailable` during request scan/admission/resume/final validation                                                      | Preserve refusal and fixed checkpoint; response projection cannot repair it |
| Capacity/allocation/encoding failure, arithmetic overflow, unsupported carrier or unknown failure                                    | Hard refusal                                                                |
| Tool arguments/input, tool-associated reasoning, keys, IDs, metadata or annotations contain malformed references                     | Hard refusal; no replacement authority                                      |
| Damaged text plus any executable tool anywhere in the round                                                                          | Hard refusal with zero tool frames released; do not drop the tool silently  |
| Missing/invalid completion, transport error, timeout, truncated frame, trailing event, or snapshot/identity mismatch                 | Hard refusal even if the first recorded defect was format-only              |

Do not infer eligibility from `TokenError::ContextUnavailable` or a generic HTTP status: scan-origin format errors and lease-origin context errors share a low-level code today. `token_format` is necessary but not sufficient.

## Complete validation scan

Add a response-only, allocation-free validation visitor in `tokenization.rs`; keep existing strict request scanners unchanged. Its result records format defects without returning early for them. It never grants authority or returns plaintext.

1. Visit every UTF-8-safe candidate start using the existing recognized-prefix/category grammar. Unknown lookalike prefixes retain current semantics.
2. Count recognized candidates, both malformed and complete, with checked arithmetic. Permit at most 4096 per logical field; the next candidate is a hard capacity refusal. Input size is independently bounded. This is no weaker than the existing complete-reference limit.
3. Validate the existing complete-reference format. For every complete reference, call the original `lease.resolve` and discard its plaintext borrow. Any resolution error is hard, regardless of a previous format defect.
4. For a malformed candidate, retain a format flag and continue searching from the next safe candidate position. Do not guess the malformed span's length, skip its suffix, or skip a later nested candidate. Never treat failure to allocate or validate as a format defect.
5. Finish all fields needed to decide projection eligibility. A format defect never stops validation of later fields/items. Hard failures can terminate the round.
6. Add a narrow private lease-liveness check, if needed, for a format-only field with no complete reference: verify the store/namespace/generation under the same rules used by `resolve`, without authorizing any new reference or changing TTL. Recheck before final delivery eligibility. Keep actual-attempt binding requirements intact.

For SSE, scan the original reassembled logical field, not independent deltas: a reference may cross any delta boundary. Snapshot copies must match that same original field before projection. Keys and non-whitelisted fields still use strict no-reference checks; format defects there are never eligible.

Required adversarial cases include an unauthorized complete reference before a malformed candidate, after it, embedded after a malformed prefix, and split across deltas. A catch around `restore_text_with_lease` followed by deletion is explicitly forbidden.

## Separate validation, restoration and delivery state

Introduce private typed results rather than resetting `SseRestorationState::failed`:

- **Original validation:** pending, verified, or hard-failed. Own original fields/events and validate original identities/snapshots/completion.
- **Restoration result:** unchanged success, or the first sticky restoration failure. Once failed, never become `DlpRestoreOutcome::Completed`.
- **Delivery eligibility:** unchanged strict delivery, candidate text projection, prepared projection, or refused. A later hard failure always revokes projection eligibility, without erasing the first diagnostic.

Only explicit format findings in whitelisted logical fields enter the candidate branch. Validation must continue without calling a permissive substitute for completion checks. Retain the first fixed stage/reason for diagnostics; use separate typed hard-failure state to prevent it from masking subsequent refusal. If a new log is needed, emit only fixed values describing projection/refusal, never parser detail or identifiers.

Successful restoration continues preparing immutable output, checking for residual references, publishing completed context, then releasing bytes. A projection is a different terminal result: restoration remains failed, no completed snapshot is published, no authority is transferred, and no automatic whole-round retry is scheduled.

## SSE transaction boundary and budget

The private candidate policy must buffer from the **first frame**, not the first error. Otherwise an already released prefix cannot be replaced, and later tools or inconsistent snapshots cannot revoke the response.

- Capture the private policy before response processing; no header, body field, environment variable, public configuration or runtime reload can enable it. Default is strict. The non-strict constructor is `#[cfg(test)]` only in phase A.
- Under that test-only policy, retain all events and original logical tracks until clean EOF and original upstream evidence verification. Emit no content, tool or success-terminal frames before the decision. Comments/heartbeats must not create an unbounded queue.
- Use `PayloadBudget`, `PayloadBuffer`, `PayloadText`, `PayloadJson` and `PayloadVec`; reserve before every copy, parse, replacement, replay and serialization. Charge simultaneous original, parsed, prepared and encoded representations, including container growth. Drop/cancellation must release every owner.
- Keep the existing 1 MiB frame and text-track bounds, 1 MiB argument bound, 8 MiB buffered-event bound, 4096 event/item bounds and 128-tool bound. Apply the 8 MiB/4096 event bounds to the entire candidate-policy round from its first event; do not loosen successful strict-path limits or the global budget. JSON keeps the existing `MAX_RESPONSE_BODY_BYTES` bound and payload budget.
- Extract original raw validation from restoration as necessary; do not feed the sanitized projection to the original completion-evidence tracker. Preserve raw/callback pairing and the original xAI-output comparison.
- After verification, prepare every projected replay frame and ensure final output has no reserved references. A damaged field uses the fixed notice in its first existing text delta, empty subsequent deltas, and the same notice in every present done/part/item/completed copy. Preserve an empty added part and preserve a missing field; do not invent a delta or terminal event. Snapshot-only text remains snapshot-only.
- If any tool or excluded output carrier occurs anywhere with a damaged field, refuse the entire round. Never replay a tool without its normal publish-before-delivery transaction, and never remove it to manufacture text-only success.

The strict policy retains current streaming behavior. Any future public policy enabling transactional buffering must explicitly accept higher first-content latency and additional capacity refusals; that rollout is not part of phase A.

## JSON and output invariants

Retain the original parsed `PayloadJson` until all completion, carrier, reference and liveness checks finish. Refactor `stage_json_response` validation only as needed; do not mutate the original object before validation. A private projection builder may apply fixed text replacements to eligible fields without invoking `StagedJsonResponse::publish`.

The candidate output is immutable, independently budgeted and residual-reference checked before it can leave the test fixture. Preserve IDs, model parameters, usage and other validated fields. Any changed entity representation must use the existing entity-header invalidation logic; do not carry a stale content length, digest, encoding or ETag. No compression bypass or raw-body fallback is permitted.

A projection does not enter completed-context publication. Subsequent attempts using its response ID, tool IDs, references or opaque state must not gain authority through this work. Ordinary legacy cache writes may have happened before final wrapping, so tests must prove cache presence cannot turn projection into authorization. Do not broaden request resume to avoid a later `dlp_context_unavailable`.

## Client compatibility gate: intentionally unresolved

The repository owns the proxy, not the Codex client state machine. A verified upstream `completed` status does not prove successful local restoration, nor does a notice or an ignored extension field prove safe client continuation.

Phase A may build/test an internal projected representation and exercise it through a synthetic private fixture. It must keep the production HTTP entry on the existing refusal path. Do not publish an external degraded-success schema or assume that a custom SSE event, JSON metadata field, or log will be honored by the real client.

Before proposing a separately approved public activation plan, record client/version-specific evidence for:

1. An explicit, displayed local degraded result distinct from successful restoration, without inventing original protocol completion.
2. No automatic whole-round retry or duplicated tool/action after the notice.
3. A next-turn request that does not rely on the projected response as a completed restoration cursor, and that succeeds through ordinary independent-history admission when supported. Cursor-only continuation must still refuse if authority is absent.
4. Identical safeguards for native Responses and converted Chat/Anthropic/xAI routes that are proposed for support; a successful synthetic client is not real-client acceptance.

If the actual client cannot satisfy these conditions, retain refusal and a fixed safe error. Selecting an external wire contract, changing IDs/status to coerce the client, deleting a requested cursor, manufacturing history, migrating storage, or enabling a public setting is out of scope. Do not claim the latest screenshot's request-context failure is fixed by this response-only design; no new live-data evidence was collected here.

## Implementation file responsibilities

The table describes the approved responsibilities. Implemented private entry points and current evidence are listed in the record below; it does not authorize a production API.

| Path                                                           | Planned responsibility                                                                                                                                             |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src-tauri/src/proxy/security/tokenization.rs`                 | Response-only complete validation visitor and narrow lease-liveness predicate; preserve existing strict scanner/resolver semantics.                                |
| `src-tauri/src/proxy/security/response_restoration.rs`         | Validation/restoration/eligibility separation, explicit output-text whitelist, immutable projection preparation and no-publication terminal result.                |
| `src-tauri/src/proxy/security/http_restoration.rs`             | Private policy capture, test-only candidate entry, whole-round SSE ownership and strict production/default dispatch.                                               |
| `src-tauri/src/proxy/security/restore_diagnostics.rs`          | Only if needed, fixed projection decision labels; preserve all current failure/checkpoint labels.                                                                  |
| `src-tauri/src/proxy/security/reversible_response_tests.rs`    | Scanner, whitelist, whole-field replacement, hard-error precedence and zero-publication tests.                                                                     |
| `src-tauri/src/proxy/security/reversible_completion_tests.rs`  | Raw evidence/snapshot/EOF, buffered SSE ordering, tools, budgets and cancellation tests.                                                                           |
| `src-tauri/src/proxy/security/reversible_context_tests.rs`     | No new cursor/reference/opaque authority after projection, including stale generation.                                                                             |
| `src-tauri/src/proxy/security/reversible_integration_tests.rs` | Dynamic-port synthetic upstream/private client matrix, strict default isolation, unchanged request checkpoints and single-attempt assertions.                      |
| `docs/troubleshooting.md`                                      | After implementation, explain fixed projection diagnostics, retained hard refusals and private-only status in English; preserve current uncommitted documentation. |

Do not add dependencies, database schema changes, public flags, provider adapters, localization changes, unsafe code, or `unwrap()`/`expect()` in production proxy code. Do not read live databases, real conversations or tool inputs. All new fixtures are synthetic, local and non-executing. Never launch, install or replace the running app.

## Executable test matrix and acceptance

Use test names beginning `text_fallback_` for the new tests, in the existing modules above. Tests must exist and fail for the intended reason before implementation; a filter matching zero tests is not evidence.

| Test family          | Required assertions                                                                                                                                                                                                                                                                                                        |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Complete scan        | Format-only field is a candidate; authorized complete references do not cause context failure; unauthorized complete references before/after/nested near malformed candidates hard-refuse; 4096/4097 candidate boundary; UTF-8 and every reference split point. Never print fixture references on failure.                 |
| Whitelist/JSON       | Only damaged `output_text` fields change; other validated fields stay exact; metadata/keys/annotations/refusal/reasoning/compaction/unknown and tool defects refuse; format plus later item failure refuses; no mutation/publication on failure.                                                                           |
| Original evidence    | Format followed by mismatched delta/done/part/item/completed identity/text refuses; missing/duplicate/invalid terminal, invalid finish reason, missing protocol sentinel where required, no EOF, malformed JSON/UTF-8, I/O failure, timeout and trailing event refuse. Preserve raw/callback and xAI comparison tests.     |
| SSE replay           | No early content/terminal/tool frame under candidate policy; notice is consistent across all existing damaged-field carriers; valid fields remain consistent; late tool always refuses; strict successful tools still publish before delivery.                                                                             |
| Budget/cancellation  | Exact and over-limit frame/track/event/count boundaries, 8 MiB whole-round boundary, final prepare/encode allocation failure and cancellation release owners; capacity never becomes an eligible format error.                                                                                                             |
| Continuation/default | Zero completed publication and no acquired reference/opaque/cursor authority after projection; cache data cannot authorize it; one upstream attempt and no tool execution; production/default/header/body values cannot enable the private policy; ordinary strict paths and request refusal/checkpoints remain unchanged. |

Targeted commands from the repository root:

```bash
# Confirm discovery; inspect the matching names/count before relying on a filter.
cargo test --lib --manifest-path src-tauri/Cargo.toml text_fallback_ -- --list
# RED, then GREEN for each newly added test/task (use its full name with --exact).
cargo test --lib --manifest-path src-tauri/Cargo.toml text_fallback_
# Preserve the existing fixed-diagnostic and request-checkpoint implementation.
cargo test --lib --manifest-path src-tauri/Cargo.toml reversible_restore_diagnostic
cargo test --lib --manifest-path src-tauri/Cargo.toml reversible_context_checkpoints
# Broader affected modules, including registered loopback integration fixtures.
cargo test --lib --manifest-path src-tauri/Cargo.toml proxy::security::
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Before a code submission, also run all checks required by `AGENTS.md`:

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
pnpm format:check
cargo test --lib --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Acceptance for phase A requires recorded RED/GREEN, nonzero test discovery, current-state regression evidence and independent safety review. It does **not** mean the client compatibility gate, screenshot root cause, live dialogue, installer/signing or release acceptance has passed. The execution record below is separate from the approved requirements above.

## Implementation and verification record (2026-10-30)

### Implemented boundary

Phase A now has test-only `validate_response_references` and `DlpRequestLease::validate_response_liveness` in `tokenization.rs`, `prepare_text_projection` and `TextProjectionStream` in `response_restoration.rs`, and `prepare_text_projection_http_for_test` in `http_restoration.rs`. All new projection entry points are gated by `#[cfg(test)]`. The HTTP fixture supports native Responses JSON/SSE only; it is not called by production dispatch. No configuration, request option, environment variable, dependency or database migration was added.

The scanner preserves the strict scanner and resolver. The stream buffers from its first frame, validates original completion/identity/snapshots through clean EOF, prepares budget-owned replay, and never publishes completed context. Only damaged logical text fields receive the fixed notice. Undamaged cross-delta references still use the original lease and safe-prefix restoration. Event-specific carrier checks, native response object identity, explicit index validation and closed part/item lifecycle checks remain mandatory. The private parser call reserves its temporary event-kind copy before parsing; this is not a claim that all production transport allocations are accounted for.

Fixed `stage`, `reason` and `checkpoint` contracts remain unchanged. The diagnostic module SHA-256 at verification was `faeac58ef8573f801d1245e24b88afa5800cc13249dc0a741adf53c3dc575baa`. No projection labels were added to production errors.

### Coverage reconciliation

| Matrix area                | Current evidence and boundary                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Complete scan              | 22 `text_fallback_scan_` tests cover ordering/nesting, UTF-8, category/namespace/generation/liveness, unavailable context, and 4096/4097 recognized-candidate limits, with strict scanner/resolver controls.                                                                                                                                                                                                                                                                                                                  |
| JSON/whitelist             | Four JSON tests cover whole-field replacement, unchanged valid fields, later hard failures, native object identity, capacity and no publication.                                                                                                                                                                                                                                                                                                                                                                              |
| SSE/evidence/replay        | Sixteen stream tests cover original snapshots/identity/terminal/EOF, late and duplicate part/item completion, event-specific carriers, invalid explicit indices, valid delta ordering and safe-prefix restoration. `text_fallback_sse_every_reference_split_restores_authorized_text` and `text_fallback_sse_every_reference_split_rejects_unauthorized_before_or_after_damage` iterate every byte boundary of generated ASCII references, including endpoints, as two **semantic JSON deltas**, not merely transport chunks. |
| Budget/cancellation        | The stream tests include exact/over frame, text-track, 8 MiB round and event-count limits, cancellation release, final buffer allocation fault injection and pre-parser reservation. These are targeted logical-budget checks, not exhaustive fault injection for every JSON/metadata allocation or a physical-memory/RSS bound.                                                                                                                                                                                              |
| Native HTTP/default        | Six private HTTP tests use a dynamic loopback port or synthetic body. They check no output before EOF, one upstream attempt, entity-header invalidation, output-owner lifetime, encoding/status/carrier rejection, bytewise LF/CRLF framing, capacity/cancellation/truncation/I/O/UTF-8 and the ordinary strict path.                                                                                                                                                                                                         |
| Continuation               | Five JSON-projection context tests cover zero cursor/reference/call/opaque publication, cache non-authority, actual-attempt binding isolation, stale generation and independent-history/strict-completion positive controls. They are not a real client's next-turn behavior.                                                                                                                                                                                                                                                 |
| Existing protocol controls | The complete `proxy::security::` regression set retains raw/callback, converted-protocol completion and strict successful-tool controls. New private HTTP coverage is native only; this record does not claim a projection-specific Cartesian matrix for converted Chat/Anthropic/xAI routes.                                                                                                                                                                                                                                 |

Independent final safety review returned **PASS for the private synthetic-only scope**, after reviewing the fixes for delta preservation, carrier exemptions, lifecycle identity, explicit indices, native object identity, temporary parser budget and late/duplicate completion. The reviewer inspected the final split-test source and earlier behavioral RED logs; the parent executed the final 53-test state and full regression commands. The review is not client acceptance, exhaustive allocation verification or unrestricted merge/release approval.

### Reproducible results

Commands ran from the repository root on macOS arm64. The final implementation was unchanged between the 53-test run and the following backend checks; subsequent edits were documentation only.

| Command                                                                                                       | Result                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml text_fallback_ -- --list`                              | 53 tests discovered; nonzero count explicitly checked.                                                                                                  |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml text_fallback_`                                        | 53 passed, 0 failed.                                                                                                                                    |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml reversible_restore_diagnostic`                         | 5 passed, 0 failed.                                                                                                                                     |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml reversible_context_checkpoints`                        | 1 passed, 0 failed.                                                                                                                                     |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml proxy::security::`                                     | 931 passed, 0 failed.                                                                                                                                   |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                            | Exit 0.                                                                                                                                                 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                                      | Exit 0.                                                                                                                                                 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                            | Exit 0.                                                                                                                                                 |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                                                       | 4886 passed, 15 ignored, 0 failed.                                                                                                                      |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                                             | Same library result, plus 163 integration tests passed; 0 failed.                                                                                       |
| `pnpm typecheck`, `pnpm build:renderer`                                                                       | Exit 0; frontend inputs were unchanged by the final Rust fixes.                                                                                         |
| `pnpm test:unit`                                                                                              | **Failed:** nested `.voidx/pr-worktree` tests were also discovered; 27 test files and 124 tests failed. Do not report this original command as passing. |
| `pnpm test:unit --exclude '**/.voidx/**'`                                                                     | Root-worktree-only rerun: 214 files, 2590 tests passed. This exclusion is not a repository configuration change.                                        |
| `pnpm format:check`                                                                                           | Exit 0 on the final state; this script checks renderer source, not Markdown.                                                                            |
| `pnpm exec prettier --check docs/design/security-dlp-text-fallback-execution-spec.md docs/troubleshooting.md` | Exit 0 for both documents.                                                                                                                              |
| `git diff --check`                                                                                            | Exit 0; no unrelated work was reset or reverted.                                                                                                        |

Local, ephemeral evidence is in `/tmp/vary-text-fallback-every-split-final.log` and `/tmp/vary-text-fallback-verification/latest/` (`backend.results` and per-command logs). Earlier frontend results and the original/excluded unit logs are in `/tmp/vary-text-fallback-verification/`. These paths are session artifacts, not committed fixtures; rerun the listed commands to reproduce.

Behavioral RED evidence preceding review fixes is retained in `/tmp/vary-text-fallback-review-more-red.log` (3 passed / 5 failed), `/tmp/vary-text-fallback-object-red.log` (0 / 2) and `/tmp/vary-text-fallback-final-review-red.log` (0 / 2). The scanner's original behavioral RED was 4 passed / 18 failed before its 22-test GREEN. Later coverage-only additions, including every-split and continuation controls, passed against existing behavior and required no implementation change.

### Remaining acceptance limits

The original frontend unit command is still a failed check; the root-only rerun does not erase it. No live database, real conversation or executable tool input was used, and no running application was launched, installed, replaced or updated. Existing uncommitted work remains uncommitted.

Real Codex degraded-result display, automatic retries, duplicate actions and continuation/history/cursor behavior remain unverified. Production activation and converted-route projection support require separate approval and the client compatibility gate above. This response-only work neither diagnoses nor fixes the observed `dlp_context_unavailable` incident. It provides no installer, signing, notarization or live release acceptance.
