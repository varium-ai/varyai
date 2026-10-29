# Reversible Codex HTTP DLP: remaining-closure evidence

## Decision

**Whole-specification acceptance: FAIL. Code-submission readiness: FAIL.** The approved functional repairs have bounded passing evidence; this is not whole-feature, merge, release, or production acceptance.

Audience: contributors and a future implementing/reviewing agent (human + LLM). This is a post-implementation evidence record, not authorization for another implementation round. The [approved 23-path plan](security-dlp-remaining-closure-plan.md) defines the permitted changes. The [authoritative specification](security-dlp-reversible-tokenization-spec.md), architecture, completed-context execution specification, and existing [P1 closure](security-dlp-p1-closure.md) remain unchanged.

| Gate | Decision | Meaning |
| --- | --- | --- |
| Existing REQ-1 / RESP-1 closure | Prior LIMITED PASS retained | Its original scope and limitations still apply. |
| Approved remaining functional repairs | LIMITED PASS | Retained runtime REDs, current GREEN tests, and bounded static reviews support the repaired cases below. |
| Broad functional completeness | FAIL / NOT-PROVEN | Compact SSE is refused; the complete route, conversion, retry, history, policy, cache, and side-consumer matrix is not established. |
| Resource acceptance | FAIL / NOT-PROVEN | Seven independent allocation boundaries are not collectively closed. |
| Whole specification | FAIL | Required functional/resource/side-effect proof is missing. |
| Code submission / merge readiness | FAIL | Passing ordinary checks cannot override the above gates. No submission operations were authorized or performed. |
| Task correctness / tool execution | Not established / not authorized | Protocol completion and restoration neither prove semantic correctness nor grant execution authority. |

## Source identity and scope

Execution date: 2026-10-29 CST. Repository-root commands ran on macOS arm64 with Rust/Cargo 1.98.1, Node v25.2.1, and pnpm 10.12.3. HEAD remains `4dbfc90d1ff495c04b95bb04962bde57df799d82`; the index remains empty. Rust builds were sequential, with `CARGO_INCREMENTAL=0`, `--offline --locked`; tests used `-- --test-threads=1`. No live models, application startup, packaging, generated-command execution, or generated tools were used. HTTP fixtures used actual `127.0.0.1:0` listeners, synthetic data, temporary files, and in-memory databases.

The admission tree had **55 tracked modified + 12 untracked = 67 paths**. Before this record, it had **57 tracked modified + 13 untracked = 70 paths**. The additional tracked paths are `security/identity.rs` and `security/reversible_preparation_tests.rs`; the additional untracked path is the execution plan. Creating this record adds the fourteenth untracked path, for **71 total pending paths**. All differences from admission are confined to the approved **15 production + six test + two documentation paths**. Existing unrelated dirty work is preserved. This does not mean the full dirty diff against HEAD consists of this round's changes.

Local evidence artifacts are ephemeral and are not committed attachments. Preserve them separately if an enduring audit trail is required:

| Artifact | SHA-256 | Purpose |
| --- | --- | --- |
| `/tmp/vary-dlp-remaining-start-identity.json` | `cff2ebc73a90846de3d1a2423f9ffb984d3f4f5210d3abbefc383c644dc6acf9` | Admission hashes/status. |
| `/tmp/vary-dlp-remaining-final-pre-document-identity.json` | `ce7b097f653168221d9a8553d9893a59def2b290f43e7d9ec56161ae7717f082` | Exact allowed-path comparison, protected/context hashes, empty index. |
| `/tmp/vary-dlp-remaining-final-verified-source.json` | `7c97917b041038dc1dbd307b7a42033d6cae56a438bfed0a34efbf38b470c000` | 1,081 source/test/manifest hashes and current Rust check commands/results/log hashes. |
| `/tmp/vary-dlp-remaining-final-evidence-index.json` | `a5cedacfe2ee77a82875466433316a15eb4ebb2e05971f4b8a0bfa923f700213` | Metadata/hash index of 227 retained artifacts, not automatic RED qualification. |

The tested source digest is `3b0b0c06243f651da2b4ccd77701b038e248a7485773d634c9e5c285e69a8265`, computed from the sorted compact JSON path-to-SHA-256 map in the verified-source artifact. Source hashes were equal before and after the final Rust checks. Only this documentation record is written afterward. At draft time, final identity registration is pending; the seal procedure must create `/tmp/vary-dlp-remaining-final-identity.json` with this record's final hash, without a self-referential document hash. This paragraph specifies that procedure, not evidence that it has run. No dependencies, schema, lockfiles, or build configuration were changed in this round.

Protected document hashes:

| Document | SHA-256 |
| --- | --- |
| `docs/design/security-dlp-reversible-tokenization-spec.md` | `0e7265a8eb9952cd9e642dcf74d0d0d9a9cee43f48e9b2e2564754da0da01d8a` |
| `docs/architecture/security-audit.md` | `fd93bf1639532320163c053a43435ac75ffb8811fef9344864b8d35ba66ceb3c` |
| `docs/design/security-dlp-completed-context-execution-spec.md` | `1ccc3f4a548b6035e453a4e23949a23e9cd605085576581a678285e5b58f01a8` |
| `docs/design/security-dlp-p1-closure.md` | `c3026bb37a7c0ca1bab07f09c7a25fab8d74e68036be1f1f1042641df2f39995` |

Preserved constraints include loopback-only binding; `openai_chat`, `anthropic`, `openai_responses`; `[REDACTED]` / `[REDACTED_PRIVATE_KEY]`; generic `sk-` audit/allow-only restrictions; security-redaction failures; shared-before-extension locking; asynchronous audit/usage queues; and native namespace compatibility. Ordinary legacy `redact` must acquire no token mappings, restoration tables/namespaces, or new restoration buffering. Existing broad GREEN supports regressions only; it does not prove every legacy path or queue lifecycle.

## Functional repairs and retained REDs

A qualifying functional RED is a compiled, executed test failing at the intended runtime boundary before its repair. Compilation, fixtures, instrumentation-only checks, wrong-boundary assertions, timeouts, and zero-test runs are not REDs. Each original shared-snapshot case ran separately before the shared production change. Later coverage controls are GREEN coverage, never retrospective REDs. Counts below overlap the final aggregate and must not be added together.

All Rust paths in this section are relative to `src-tauri/src/proxy/`.

| Requirement | Repair and boundary | Evidence and limits |
| --- | --- | --- |
| RESP-2 / 4 / 5 | `security/response_restoration.rs` retains charged raw snapshots, validates item/part identity and membership, compares semantic function arguments, then restores/replays supported leaves. Nonempty added values are independent snapshots; empty added placeholders and absent claims differ from authoritative present done/final empties. | **12 separately executed HTTP REDs** precede shared production repair: eight contradictory snapshot cases, three added-value cases, one semantic-argument case. Subsequent review findings also have retained REDs. `reversible_closure_resp24_`: **16 passed**. Object order/whitespace/equivalent escapes may compare equal; numeric types, signed zero, array order, and duplicate rejection remain strict. Unknown token-free metadata and event count/order/type/IDs/sequence are retained. |
| RESP-3 | `security/completion_tools.rs` classifies original Chat tool/function siblings; `completion_json.rs` wires classification before lossy conversion. Unknown null siblings and legal u64 indexes are accepted; all nonnull unknown siblings are unsupported, even token-free. Malformed known shapes remain strict ordinary failures. | **Six actual HTTP REDs** across JSON, streaming SSE, and buffered SSE. Strengthened GREEN uses an independent positive session to establish the converted Responses ID differs from original Chat ID and supports continuation; both IDs are refused for the rejected session, with no extra send. Two additional direct shape/index controls each passed separately; these are not new HTTP or RED evidence. |
| REQ-2 | `forwarder.rs`, `providers/codex.rs`, and private context/tokenization seams separate exact actual-send binding from configured operation lineage. Only deterministic Responses/Compact transitions are allowed; same-operation binding remains exact URL. | Retained HTTP lineage RED: second-stage 400/one send instead of allowed transfer. Three forwarded account-header REDs, three duplicate-Authorization REDs, and three derived-Authorization **reqwest** REDs cover same/cross/reverse operation. Changed derived credential vectors previously yielded 200/three sends instead of refusal before the next send. Focused REQ-2: **13 passed**. |
| REQ-3 | `security/request_preparation.rs` classifies immutable original input before tokenization/enrichment. Complete unique ordered call/result histories without old token/cursor/opaque dependencies are reinspected transactionally; independent readable provenance skips cache contributions. | Three fresh function/custom/Chat HTTP REDs, five object-key/nested-shape HTTP REDs, escaped decoded-argument-key HTTP RED, corrected nonstring-type runtime RED, and two direct classifier REDs retained. Focused REQ-3 **17 passed**; preparation **52 passed**; P1 **five passed**. Budget/rollback, malformed histories, stale/foreign dependencies, and cache-filling refusals remain closed. |
| REQ-4 | `security/identity.rs` isolates an audit-only fingerprint fault seam. `security/restoration_audit.rs` skips only affected auxiliary summary/dedup work, using fixed `identity_unavailable` diagnostics. Core randomness and token ownership are unaffected. | Actual HTTP RED: 503 instead of 200, plus separate preparation/audit unit RED. Audit tests **eight passed**; focused REQ-4 **eight passed**, including seven zero-send core controls for entropy/lease/mapping capacity/reserve/collision/redaction/unsupported carrier. No plaintext, unkeyed hash, constant identity, or token-store-key fallback. |
| RESP-6 | `security/http_restoration.rs::clear_entity_headers` also removes `content-digest` and `repr-digest` after entity mutation. Ordinary unmodified traffic is unchanged. | Two separately executed actual HTTP JSON/SSE header REDs retained. Ordinary control fixture initially failed and was corrected; that failure is not functional RED. |
| RESP-7 | `handlers.rs`, `security/http_restoration.rs`, and `security/completion_evidence.rs` carry genuine strict fully-read original Anthropic JSON evidence through synthetic SSE conversion. No fabricated original SSE/EOF witness. | Complete JSON-to-SSE HTTP RED retained; malformed/incomplete/error controls were already GREEN. Three actual HTTP body-I/O/cancellation controls passed. Six direct single/double/mixed/no-EOF/I/O witness controls each passed; they are wrapper, not listener evidence. |
| xAI consistency and completion | Original arguments/snapshots compare before conversion; charged completed output transfers only on clean natural EOF and is compared against converted output under existing strict normalization before release. A refused foreign JSON witness permanently poisons pending or verified xAI completion. | Retained original-snapshot, signed-zero, and no-delta actual HTTP REDs. Two foreign-witness REDs are **direct wrapper** tests, not actual HTTP. Pairing keeps position/count/type/item ID/call ID, but not exact tool name, preserving native namespace restoration. Legacy sanitizer is not weakened. |

Exact-send identity includes actual provider/protocol/full URL and SHA-256 of complete ordered resolved credential/account header vectors. It mirrors send ordering: an alternate credential first can inject derived headers before incoming Authorization; Authorization first suppresses derived headers. Managed account overrides and Anthropic stripping match actual sends. Lineage separately retains configured prefix/query/route mode/auth; no host-only matching, query sorting/removal, arbitrary `/compact` stripping, or ambiguous full-URL inference is authorized.

The test-only loopback adapter retains real `CodexAdapter::name()`, auth, and URL behavior so current official duplicate/derived-auth controls use the production reqwest choice without external networking. Earlier duplicate-auth REDs used hyper; they establish a local identity defect, not prior reqwest runtime coverage or official upstream acceptance. The custom-prefix allegation was withdrawn after the corrected fixture passed without production changes.

### Historical evidence locators

Local prefix for this table: `/tmp/vary-dlp-remaining-`. Each listed JSON manifest contains commands/exits/log hashes, or the evidence index supplies the individual log hash. A filename ending in `red` is not itself proof.

| Manifest / log group | Classification |
| --- | --- |
| `snapshot-red-runs.json` | Original 12 separate intended-boundary HTTP REDs; SHA-256 `899c5659bf677710e93a261248426bb898b5ffecda4decdc3bfa5bdec1d44461`. |
| `resp3-red-runs.json` | Six original Chat HTTP REDs; SHA-256 `b8af59bc9ad738f512e57c916c26559bdad6807e0444b736c8f8ed469a25dadd`. |
| `snapshot-review-red.json`, `review2-red.json` | Review follow-ups; inspect each entry. The first also contains a GREEN control, not seven REDs. |
| `req2-lineage-red.log` | Valid transfer RED; SHA-256 `4ee3780b2c3dbd55fd5c7f499e980cb34891a5532989fdd7687a9df740aaac9c`. |
| `reversible_closure_req2_changed*workspace*-red.log` | Three actual forwarded-account identity runtime REDs. |
| `reversible_closure_req2_duplicate_authorization_{same,cross,reverse}_operation-unique-red.log` | Three corrected-fixture runtime REDs, original hyper transport. |
| `reversible_closure_req2_derived_authorization_{same,cross,reverse}_operation-reqwest-red.log` | Three actual reqwest intended-boundary runtime REDs. |
| `reversible_closure_req3_*-red.log` | Use only the specifically classified cases above; the corrected nonstring-type log is authoritative, not its initial failed instrumentation. |
| `req4-red.json` | Two intended-boundary REDs, one actual HTTP and one direct unit. |
| `resp6-red.json` | Two valid JSON/SSE header REDs plus one nonqualifying ordinary-control fixture failure. |
| `resp7-red.json` | One complete JSON HTTP RED and four already-GREEN negative controls. |
| `xai-original-extra-red.json`, `xai-no-delta-red.json` | Original xAI runtime REDs plus GREEN whole-float controls. |
| `resp7-foreign-witness-red.json` | Two direct-wrapper sticky-rejection REDs; SHA-256 `257cec2c3d14ded1952f1df9c9f09340161bc10f0c21b9894de4c3cfac4cf326`. |

Other excluded failures remain explicit: REQ-4 initial 5/1 and 7/1 batches were fixture/wrong-boundary failures; initial duplicate-auth TLS-provider panic and cross/reverse 502/200 fixture incompatibilities were corrected before valid REDs; compilation/lifetime/import errors, timeout exit -1, disk exhaustion, and an ad-hoc source-inspection regex error are not functional evidence. Some `*-red.log` controls are GREEN. A namespace regression produced **775 passed / one failed**; removing only exact tool-name attestation restored compatibility, without relaxing parameter matching. Historical security **607 passed / one failed remains unexplained**; current GREEN does not explain it.

## Nonexecuting client and task evidence

`security/reversible_integration_tests.rs::FinalClientRecorder` records function/custom submissions at `response.output_item.done`, not only after successful terminal completion. It executes nothing.

- `p1_acceptance_xai_raw_completion` covers 12 actual HTTP modes: complete, truncated, CRLF, one-byte chunks, duplicate JSON, invalid UTF-8, unfinished UTF-8, late failure, second completion, body I/O, partial cancellation, and terminal-without-EOF cancellation. Complete/CRLF/one-byte modes record exactly one restored tool and completion; invalid/cancelled modes record zero.
- Four actual native/xAI client-budget controls wait at an upstream tail gate after a complete tool prefix. Success records one restored tool and reusable context. Response-stage budget exhaustion records zero tools/completions, fixed failure, and subsequent context refusal with no new send after budget recovery; retained owners release to zero. Each passed separately. These prove logical-budget/client-buffering cases, not dependency allocation bounds. A pending `collect()` timeout alone proves only that the whole response has not ended, not that no earlier frame was emitted.
- Six direct Anthropic witness controls and two direct Chat shape/index controls each passed separately. They do not expand actual HTTP coverage or create retrospective REDs.
- Eight synthetic task cases use real forwarder/send/request-action audit, with explicitly **mock** cache/usage/observation consumers: literal handoff positive, shell quoting, nested JSON string, patch line structure, partial reference, percent-encoded reference, missing reference, and wrong-valid-token substitution. Each passed separately; the strengthened aggregate is **eight passed**.

The partial reference is refused. Other cases can protocol-complete while independent task comparison fails except the literal control. Outer JSON validity does not prove embedded JSON validity; literal restoration can change shell quoting and patch lines, and a wrong authorized token restores another mapping. The observation parser returns `ObservationStage::Proposed`, never execution permission. Mock usage checks numeric counts without payload; request-action audit records two nonsensitive `redact_restore` findings independently of task outcome.

**Side-consumer safety is not closed.** Raw proposed observations and the independent built-in summary projection can retain custom-DLP plaintext in memory. Mock cache can retain completed but semantically wrong tool input. Tests deliberately expose those limits without printing or persisting the fixtures. Manually driven mocks and the request-audit observer do not prove actual handler cache publication, asynchronous audit/usage queues, persistence, or every diagnostic sink. The full side-effect matrix remains unproven; do not describe the projections as a full-chain sanitization PASS.

## Resource inventory: still blocking

Rune's completed read-only workspace review supersedes the earlier incomplete inventory, not the specification. `FAIL` below means a required allocation-closure condition is not met, **not** a reproduced OOM. Earlier Chat metadata and header/transport statuses were NOT-PROVEN; this review identifies concrete uncovered allocations and records resource-closure FAIL. Production was not changed to address them.

All cited Rust paths are relative to `src-tauri/src/proxy/` and refer to the tested source identity.

| Boundary | Decision | Concrete source evidence / missing proof |
| --- | --- | --- |
| Request sanitizer / provider-model / TOML | FAIL | `providers/transform_codex_responses_xai_sanitize.rs:173–182` clones input and builds vectors; `forwarder.rs:1980–1983` calls compatibility/model resolution; `providers/codex.rs:935–940` parses TOML. No closed accounting of these allocations and simultaneous copies. |
| Response copies / parser / serializer / converter | FAIL | `handlers.rs:595–599,658–676,719–722,1552–1602` parses, converts, and reencodes bodies. Logical workspace allowance does not bound actual allocation peaks. |
| Persistent Chat metadata / indexes | FAIL | `providers/codex_chat_history.rs:15–41,120–133,482–508` retains HashMap/Vec/VecDeque indexes and clones metadata snapshots. A 512-response cap plus JSON ownership does not establish container capacity, metadata, or overlapping-snapshot allocation bounds. |
| General workspace | NOT-PROVEN | `security/upstream_guard.rs:24–29,122–153` reserves a finite allowance; its own comments explicitly disclaim proof of arbitrary converter/cache amplification. |
| Codec constructors / internal buffers / reads | NOT-PROVEN | `security/upstream_guard.rs:310–331` constructs gzip/deflate/Brotli/zstd decoders; lines 326–327 explicitly identify an internal allocation gap. Registry-source access remains denied; no alternate path or guessed constructor/window bound was used. |
| Headers / transport | FAIL | `security/upstream_guard.rs:347–348` clones headers before creating the owner; `hyper_client.rs:311–321,607–620,645–651` moves requests and constructs raw requests using ordinary Vec. Ownership transfer is not allocation-accounting closure. |
| Response namespace table | LIMITED PASS | `providers/transform_codex_responses_namespace.rs:45–58,107–115` distinguishes Legacy/Charged and uses fixed flat-name size; `security/http_restoration.rs:1094–1153` uses charged fallible PayloadVec/PayloadText insertion. This covers only that charged table, not Legacy, flattening, converters, or total peaks. |

`64 KiB + 4096 × cumulative input bytes` is finite admission, not measured or proven allocation amplification. A 64 MiB budget admits at most **16,368 input bytes before other charges**, not a full 8 MiB response. Raw limits of 1 MiB/chunk, 8 MiB cumulative/per-layer decoded output, and eight encoding layers do not prove codec internals. Compressed leased SSE is refused before polling. Logical reserve/release tests do not establish RSS caps, physical OOM recovery, zeroization, or performance.

`security/http_restoration.rs:2392–2400` refuses Compact SSE, even with HTTP 200. Compact JSON controls do not establish SSE support. Supporting it or repairing resource boundaries requires a separately approved concrete design; a path already listed in the 23-path plan does not authorize unrelated resource repairs.

## Current-source verification

The following results were reproduced against the frozen source identity above. The verified-source manifest captures exact Rust commands, exits, result blocks, and log hashes. Final frontend logs include commands and exits. Counts overlap; the full-library and all-target counts must not be added.

Rust command prefixes are exactly those in the [plan's final verification section](security-dlp-remaining-closure-plan.md#final-verification-and-evidence). Test commands include `CARGO_INCREMENTAL=0 cargo test --offline --locked --manifest-path src-tauri/Cargo.toml`, `--lib` where indicated, and `-- --test-threads=1`. Focused filters are `reversible_closure_` and `proxy::security::`.

Local prefix for the next table: `/tmp/vary-dlp-remaining-final-`.

| Check | Result | Log suffix | Log SHA-256 |
| --- | --- | --- | --- |
| `pnpm typecheck` | exit 0 | `frontend-typecheck.log` | `06632ebcacbbf6a5379320e786850d1b962ee69b3d5cf5a0a3f912d198757a56` |
| `pnpm build:renderer` | exit 0 | `frontend-renderer.log` | `41971f1fcfbc775052ceea727a28db95347cad7171976b60858d3b14b6d38b7e` |
| `pnpm format:check` | exit 0 | `frontend-format.log` | `93fef39dd90c9847e719a25b408aff8b8c8c751c932f55122e6496f54bcf1c1d` |
| `pnpm test:unit --exclude '**/.voidx/**'` | exit 0; 201 files / 2,095 tests passed | `frontend-vitest.log` | `aaeb320d836e295bbdd7fee3ede6222c946e3e49f6e75bcb25270705a19d1c5a` |
| `cargo check --offline --locked --manifest-path src-tauri/Cargo.toml` | exit 0 | `cargo-check.log` | `f2415032355422d41391385940d2319a5c5561acac400210f1ae2d78ed25b593` |
| `cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0 | `cargo-clippy.log` | `2319c9e1ba8760ce5f6a8454fba9daca701e105113d34a8ec3ca7b771949686e` |
| Focused closure | 134 passed / zero failed / zero ignored; exit 0 | `closure.log` | `3ffcabe62632bae71a999ff67e45c172f67353422079b123221ef552d5c35ab9` |
| Security group | 852 passed / zero failed / zero ignored; exit 0 | `security.log` | `810da1b5153b94cf80aaafe9e97607be794fe305592d6604eaa855cd46f91d1c` |
| Full Rust `--lib` | 4,684 passed / zero failed / 13 ignored; exit 0 | `cargo-test-lib.log` | `ecfce61eeb5f03de2ce2a6e9808953521b288783f8e3063f3c24e7313f4ef1f1` |
| Full Rust all targets/doc tests | 16 GREEN result blocks; summed 4,836 passed / zero failed / 13 ignored; exit 0 | `cargo-test-all.log` | `778aac16f70574bcb6cb86c8bb20e780028767b2a4f76bb25b62c1e2ec6c4589` |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | exit 0; check only, no global formatting | `rustfmt.log` | `a08226c56d65225545daae853fc77f3196fbd47409981196c0e85a45211304ea` |
| `git diff --check` before document | exit 0 | `diff.log` | `8f5d216fff3900bf4cd6ad874f6dda4f9ba634f1afed5883f0b9c72c6fffbc75` |

The 13 ignored Rust tests remain excluded, not passed. Node localstorage and React `act(...)` warnings remain in frontend logs; zero exit does not mean warning-free. Only the 21 approved Rust files were previously formatted with `rustfmt --edition 2021 --config skip_children=true`; no global formatter write was run. The final document/scope check is separate from the pre-document diff log.

### Supplemental panic evidence, not whole-library PASS

```bash
CARGO_INCREMENTAL=0 cargo clippy --offline --locked --lib \
  --manifest-path src-tauri/Cargo.toml --message-format=json -- \
  -D clippy::unwrap_used -D clippy::expect_used
```

Current result: **exit 101, zero proxy / 41 nonproxy primary-span diagnostics**. JSON log: `/tmp/vary-dlp-remaining-final-strict-panic.jsonl`, SHA-256 `b188e1515e388d4e6a8c3eff37ed81aa3ca1931cf8e08801be5dfaa926f01eda`; summary SHA-256 `b348142fcb495ae90d36c4719377f31aa5a47d332406a10cd50f6034cc88ce5c`. This is neither a whole-library PASS nor macro-expansion proof.

A limited literal source inspection skips explicitly test-only modules and inspects before the first test-only cfg. Its sole residual candidate, `providers/adapter.rs:65`, is a documentation example, not a production call. `qoder_codec.rs:63–102` is explicitly test-only, including the apparent panic extractors. Both locally installed Qoder app paths were absent; no codec application was launched by that optional test. This limited scan is not a Rust parser or proof of all conditional/macro expansions. Artifact: `/tmp/vary-dlp-remaining-final-panic-source-inspection.json`, SHA-256 `514517209c6b1baee13d2140698a2a5aaf02ea7fb78b130c61242d7ae03df36e`.

## Independent reviews and remaining proof gaps

All agents were read-only; the parent owned every write. Review PASS means only the stated bounded static scope, not independent execution, resource closure, functional RED proof, or whole-feature acceptance.

| Review | Result / limit |
| --- | --- |
| Orion `285e575f8dd143d8b7ee588df91d11fa` | Revised execution-plan readiness PASS only, after initial ambiguity FAIL. |
| Pixel `fbce85362d624ec586d55fce8a9c7f72` | Bounded snapshot static PASS; no independent execution or resource acceptance. |
| Lyra `ae6d26534ef5487b929c78f12858a0bd` | RESP-3 classifier and existing six HTTP fixtures static PASS. |
| Iris `185d376458b7480084084c4d8aad5969` | Actual-send auth mirroring static PASS; some synthetic literals were tool-hidden, with no bypass. |
| Helix `e338f13316864182a5cae7ab48d54e8c` | Fresh-history classifier bounded static PASS. |
| Juno `f4def84cde2747f0ab40ac22bf5a66a6` | xAI handoff/attestation, foreign-witness poison, charged ownership and namespace compatibility static PASS. |
| Prism `8e632e2b0db2432e9f9c5778d6d06d8b` | New recorder/budget/witness/task tests static PASS; collect-timeout and mock-side-consumer limitations explicitly retained. |
| Rune `fabfeb3c11464d34a3c4fed62bed1fa3` | Completed seven-boundary resource inventory: overall FAIL; Compact SSE FAIL; no commands/tests. |
| Lumen `9ca74814867e4ea6ba4eb5a9cfb960df` | Final bounded seams show no new concrete defect; functional repairs LIMITED PASS, broad functionality/resource/specification/submission FAIL or NOT-PROVEN. No commands/tests. |
| Earlier resource agent `27a218df34ae42eb9a9354b76ccaa038` | Timed out, cancelled, no deliverable; no evidence or PASS credited. |

Additional unresolved coverage:

- Snapshot cases are nonexhaustive: not every nonempty-added contradiction, equivalent escape, or reasoning/refusal empty combination has an independent actual HTTP case. The two `call_identity_before/after` controls both mutate tool `output_item.done.call_id`; their before/after placement moves ordinary text relative to the barrier, not two independent tool stages.
- Foreign-witness poison is direct-wrapper-tested and source-reviewed, not independently actual-HTTP-tested. Added malformed Chat controls are direct tests, not HTTP variants.
- The recorder does not prove every timeout/cancel/truncation/budget path or arbitrary third-party client behavior. No real Codex client/model or actual task/tool execution was approved.
- Full aliases/conversion/retry/history/policy/cache/audit/usage/error/tool-observation coverage, real handler side-consumer wiring, and the unexplained historical 607/1 require further evidence.
- No RSS/physical OOM/zeroization/performance, installer/signing/notarization, shipping, or production authorization is established.

## Handoff and document quality gate

Do not stage, commit, push, create a PR, ship, or authorize production from this record. Do not edit the four protected documents to make unresolved requirements disappear. Do not reduce the acceptance scope or replace denied codec evidence with guessed dependency/container multipliers.

Any further production work needs a concrete approved expansion. Candidate investigation/repair paths include `providers/transform_codex_responses_xai_sanitize.rs`, provider/model/TOML resolution, response converters/handlers, `providers/codex_chat_history.rs`, `security/upstream_guard.rs`, `hyper_client.rs`, Compact SSE entry handling, and real side-consumer boundaries. Some are outside the 23-path set; others are inside it but outside the approved repair design. This list is an inventory, not a new design or authorization. The historical failure additionally needs its original failing test/log and matching source identity.

Reproduction commands and path ownership remain in the approved plan; use the source manifest to confirm unchanged inputs before reusing results. Re-run affected checks after any semantic change. Keep current GREEN, historical RED qualification, bounded static reviews, full acceptance, and submission decisions separate.

This record uses a mixed-audience quality gate: a fresh reader must identify the decision, blockers, and next authorization boundary; an implementing agent must find exact source paths, invariants, forbidden changes, commands, counts, hashes, and evidence limitations. Tess's first read-only review (`aeffcfff2eab42dc8194913f8e3da5c1`) requested clarification of draft-versus-sealed status; the parent revised that wording. Final quality recheck and identity/scope registration are pending at this draft checkpoint. To seal the record, obtain a terminal document-quality verdict, write `/tmp/vary-dlp-remaining-final-document-review.json` with the actual review/accuracy-check results, then create the final identity artifact and verify the document/source hashes are unchanged. Only those matching artifacts attest that sealing completed. No seal step changes the FAIL acceptance decisions above.
