# Reversible DLP: REQ-1 and RESP-1 closure

## Decision

**REQ-1 and RESP-1: limited PASS. Whole-feature acceptance: FAIL. Not ready for a feature-acceptance or merge-ready PR.**

This is a post-implementation record for contributors, dated 2026-10-29. It closes two reproduced P1 defects, not every P1 finding. The authoritative [reversible tokenization specification](security-dlp-reversible-tokenization-spec.md) and [security architecture](../architecture/security-audit.md) remain unchanged. Protocol completion and restoration do not prove task correctness or authorize tool execution.

The approved repair scope is exactly:

- `src-tauri/src/proxy/forwarder.rs`: reject unleased references introduced by Chat history enrichment.
- `src-tauri/src/proxy/handlers.rs`: connect original xAI SSE evidence to the final restoration boundary.
- `src-tauri/src/proxy/security/http_restoration.rs`: borrowed late-reference validation and original-byte completion observation.
- `src-tauri/src/proxy/security/reversible_integration_tests.rs`: retained regressions, compatibility controls and lifecycle coverage.
- `docs/design/security-dlp-p1-closure.md`: this record.

The repair was made on a dirty working tree at HEAD `4dbfc90d1ff495c04b95bb04962bde57df799d82`, not on a standalone repair commit. The original 66 pending files were preserved: four were edited by this repair, and the other 62 retained their hashes. This document adds one untracked file. No staging, commit, push, PR creation, application startup, packaging, live model or actual tool execution was performed.

## Root causes and repairs

### REQ-1: late Chat cache dependencies bypassed current authority

A token-free follow-up could pass initial admission with restoration disabled. Chat history enrichment then inserted a cached old token, but an allocation owner did not establish a captured restoration lease. Before repair, the request reached a second actual upstream send with that token and returned HTTP 200.

After enrichment and retention of its allocation owners, `forwarder.rs` now calls `validate_unleased_enriched_body` when `dlp_request` is absent. The borrowed recursive reference visitor inspects keys and nested values; a late reference returns `ContextUnavailable` before protocol conversion or send. This is conservative refusal, not opportunistic lease creation. Requests with captured leases retain existing generation checks and actual provider/protocol/full-target-URL/resolved-auth binding.

### RESP-1: xAI conversion manufactured apparent completion

The legacy xAI SSE converter repairs unterminated tails, uses lossy UTF-8 and normalizes parsed JSON. Previously, final restoration could accept this repaired output without authoritative evidence from the original bytes. An unterminated original completed frame released five tool-related events, restored plaintext and allowed a continuation to reuse the completed context.

For successful xAI SSE responses with active tokens, both Responses handler branches now pass the same evidence `Arc` to an observer before conversion and to final restoration. The observer:

- Forwards original chunks unchanged; converted JSON cannot supply original authority.
- Keeps local native completion and transport trackers, plus one charged frame limited to 1 MiB. Strict parsing and associated temporary payload owners share the request budget.
- Rejects malformed UTF-8, duplicate JSON keys, invalid completion, unfinished framing, read errors and capacity failures. It does not repair tails.
- Leaves evidence pending until strict natural EOF. Cancellation or an unpolled drop cannot synthesize EOF. Failure is sticky; poisoned or pending evidence refuses final restoration.
- Holds shared locks only to clone the budget or record terminal facts, never across parsing, scanning, allocation or I/O.

The final evidence check precedes tool-buffer release and completed-context publication. Ordinary requests without active tokens bypass this new observer and restoration buffer. No codec, xAI sanitizer, completion parser, response-restoration implementation or binding relaxation was included in this repair.

## Functional RED and retained GREEN evidence

All runtime fixtures are synthetic. HTTP servers bind `127.0.0.1:0`; databases are in memory. No generated arguments are executed.

The formal pre-repair runs used the following command, once per regression filter:

```bash
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib \
  --manifest-path src-tauri/Cargo.toml <filter> -- --test-threads=1
```

| Filter | Intended runtime failure before repair | Result |
| --- | --- | --- |
| `p1_acceptance_late_cache_authorization` | Disabled policy, initially token-free follow-up: second actual upstream send was **1**, expected **0**; old token was sent, HTTP 200 | 0 passed / 1 failed / 0 ignored; Cargo 101 |
| `p1_acceptance_xai_raw_completion` | Unterminated final original frame: **5** tool events, restored plaintext, **1** continuation send/context reuse, expected zero | 0 passed / 1 failed / 0 ignored; Cargo 101 |

The expanded Unicode/whole-float response fixture reproduced the same truncated-frame runtime failure before its production repair. Other matrix modes added after repair are additional GREEN coverage, not independently demonstrated REDs.

The final `p1_` filter passes **5 Rust tests**, including:

- Disabled-policy rejection plus valid continuation, changed policy, changed provider identity and token-free ordinary cache controls.
- One actual HTTP xAI test containing **12 internal scenarios**, not twelve separate Rust tests:

  | Original stream | Client and continuation observations |
  | --- | --- |
  | Complete, CRLF chunks, one-byte chunks | Each releases five tool events, restores synthetic plaintext without exposing references, and permits one continuation send/context reuse |
  | Unterminated final frame, duplicate status key, invalid UTF-8, unfinished UTF-8 tail, late failure, second completed event, read error | Each releases zero tool events, exposes neither plaintext nor references, emits a failure event, and permits zero continuation sends/context reuse |
  | Cancellation mid-frame or after a terminal frame but before EOF | Client prefixes actually received before cancellation are retained and inspected afterward: zero tool events, no plaintext/reference exposure; zero continuation sends/context reuse |

- A direct observer-to-final-restoration test using real admission/preparation: exact 1 MiB comment frame succeeds; 1 MiB + 1, exhausted budget, cancellation and unpolled drop cannot authorize restoration. All cases release their own charges while preserving an independent 59-byte reservation, then reach zero after that reservation drops. **These resource-limit cases are not actual HTTP limit tests.**
- Sensitive ordinary legacy `redact`: native namespace compatibility and legacy tail handling still work with an unrelated restoration budget fully exhausted. The request is redacted, no token namespace/mapping is created, and no restoration charge is acquired. The separate token-free legacy cache control is not itself a sensitive-redaction proof.

Compact compatibility is demonstrated by four existing **JSON** controls (native, xAI, Chat, Anthropic). Compact SSE remains refused by the existing restoration entry point; no compact SSE success is claimed. Both handler branches were statically reviewed, but the twelve adversarial HTTP scenarios target ordinary `/v1/responses`.

## Verification on the repaired tree

Execution context: repository root, macOS arm64, Rust/Cargo 1.98.1, Node v25.2.1, pnpm 10.12.3. Rust commands below used `CARGO_INCREMENTAL=0`; dependency operations used `--offline --locked`. Only the four owned Rust files were formatted; no global formatting was applied.

| Command | Observed result |
| --- | --- |
| `cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml p1_ -- --test-threads=1 --show-output` | 5 passed / 0 failed / 0 ignored, including the strengthened cancellation-prefix assertions |
| Same library command with `reversible_http_xai`, `reversible_http_chat`, or `compact_json` | Respectively 5, 8 and 4 passed; zero failed/ignored; overlapping filters are not additive unique-test counts |
| Same library command with `reversible_http_ownerless_request_guarded_history_capacity_refuses_before_send` or `reversible_http_guarded_history_owner_releases_when_send_is_cancelled` | 1 passed each; zero failed/ignored |
| `cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml proxy::security -- --test-threads=1` | 717 passed / 0 failed / 0 ignored |
| `cargo check --offline --locked --manifest-path src-tauri/Cargo.toml` | Exit 0 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Exit 0 after final test-only edit |
| `cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml -- -D warnings` | Exit 0 |
| `cargo test --offline --locked --manifest-path src-tauri/Cargo.toml` | Final post-review run: **4701 passed / 0 failed / 13 ignored** across 16 test binaries; library 4549, integration 152 |
| `pnpm typecheck` / `pnpm build:renderer` | Both exit 0 |
| `pnpm test:unit --exclude '**/.voidx/**'` | 201 files / 2095 tests passed; exit 0 |
| `pnpm format:check` / `git diff --check` | Both exit 0 |

The final Rust edit only strengthened test observation; the three production file hashes stayed unchanged. P1, Rust formatting and the complete Rust suite were rerun afterward. Other checks above retain valid evidence for their unchanged inputs; frontend sources, dependency manifests, lockfiles and configuration hashes were checked. Node `--localstorage-file` and React `act(...)` warnings remain; passing tests do not erase them.

An additional non-test whole-library panic lint was run:

```bash
CARGO_INCREMENTAL=0 cargo clippy --offline --locked --lib \
  --manifest-path src-tauri/Cargo.toml --message-format=json -- \
  -D clippy::unwrap_used -D clippy::expect_used
```

It exited **101**, with **0 proxy / 41 nonproxy primary-span diagnostics**. This is not a whole-library PASS or macro-expansion proof. The repair adds no production `unwrap()` or `expect()`; test-only uses remain permitted.

Excluded results are not counted as RED or GREEN: one controls fixture reused a completed tool call incorrectly; an observer implementation hit E0597 during compilation; the strengthened test initially omitted `StreamExt` and hit E0599/E0277; the first parallel verification command timed out at 120 seconds before completing its full Rust run. A separate full run and the final post-review full run both completed successfully. Earlier full-suite results are historical, not substitutes for these repaired-tree results. The historical security 607/1 result remains unexplained.

## Review and evidence identity

A fresh bounded read-only review of the integrated four-file repair returned **PASS for REQ-1/RESP-1 only**. It checked unleased late dependencies, unchanged actual binding, both same-Arc handler paths, raw-before-converted ordering, short shared locks, EOF/cancellation behavior, legacy bypass and final tool/context publication ordering. It did not independently execute tests. Poison handling is source-reviewed, not independently runtime-tested.

Final source SHA-256 values:

| File | SHA-256 |
| --- | --- |
| `forwarder.rs` | `b69a16c3df145fb49c4448ddfa784a15078e5644e20a5da8639c5bd3100f35f9` |
| `handlers.rs` | `ddbf97ccfd88cc7ac98a64b8be12efdfc644e83a422bd704f214171f490de79a` |
| `security/http_restoration.rs` | `ee1f1e44b02209fd40c4da88106df890209745f331ac82fb1d0b0530095f9721` |
| `security/reversible_integration_tests.rs` | `4383fb668d97f2941960f13446b89a314c0726cdbe83ff0affdcec42b2e38953` |

Local evidence files are ephemeral, not repository dependencies. The retained tests and commands above support reproduction. In the originating workspace, `/tmp/vary-dlp-p1-closure-` prefixes these artifacts:

| Suffix | SHA-256 / purpose |
| --- | --- |
| `request-red.log` | `40c22203b8159513dcce3b410803d164de20682dc3ee97eb4849e919a97e4754` |
| `response-red.log` | `2e9132c5c7f0534da7c10ac5c627079b245e5a94e29936b9204cf6ff8b56db0d` |
| `response-extended-red.log` | `2f6a12b6cd97cc80aa043ceda3b3227b7a5e40835407a938fd29eca7ca165fb8` |
| `final-focused.log` | `fcd1fa93d6933a0aa16af00c553c262333eb0b5dfde71f48c93791605ce22e39` |
| `final-rust.log` | `b6d584c454103b8a585e1a4fab2d0c6368df4302a7255c222b9bb40062cb6404`; security/check/fmt/Clippy completed, final full-suite section interrupted |
| `final-frontend.log` | `1fc4e27e95e866ab1f186e578c568b08a885bdacb66dff0911f5df9be87648c5` |
| `post-review-green.log` | `45eec53185e38c0aa09a6c904a6751a8b849c81f1b32ed69f17a28faaf9a8d80`; final P1/fmt/full Rust |
| `final-strict-panic-summary.json` | `4a0637af7ccf130b4fcc279602868fa76ee28cbee64cfd43e5a21dfcad76d106` |

`/tmp/vary-dlp-p1-closure-source-final-identity.json` records all original pending-file hashes and protected/context hashes. Final identity, evidence and review records use the same prefix. Original temporary-regression and whole-spec FAIL records were not overwritten.

## Remaining blockers and separate PR assessment

The two reproduced defects are closed within this scope. **The specification-level FAIL is not cleared.** The earlier clause review recorded 20 clause groups and 11 blockers; nine remain open, rather than being silently accepted:

| Severity | ID | Remaining issue |
| --- | --- | --- |
| P1 | RESP-2 | Some message/reasoning/content-part snapshots are not retained and compared; replay can mask contradictions |
| P1 | RESP-3 | Authorized references in unknown Chat tool/function sibling carriers can disappear during conversion |
| P2 | REQ-2 | Exact full-target-URL binding prevents Responses/compact cross-operation continuation; needs safe lineage design, not weakened binding |
| P2 | REQ-3 | Fully readable explicit tool history cannot establish a fresh generation without completion ownership |
| P2 | REQ-4 | Auxiliary audit fingerprint failure propagates a core 503 instead of isolated degradation |
| P2 | RESP-4 | Supported nonempty added snapshots containing authorized references are refused rather than restored |
| P2 | RESP-5 | Native argument comparison is byte equality instead of parsed semantic JSON equality |
| P2 | RESP-6 | Restored identity responses can retain stale `Content-Digest` / `Repr-Digest` |
| P2 | RESP-7 | Anthropic JSON to synthetic SSE can leave outer completion evidence empty and fail valid completion |

The prior seven-boundary resource review also remains unchanged: xAI request/model/TOML allocations; response field/parser/serializer/converter copies; persistent Chat metadata/index backing; general processing workspace; codec internals; headers/transport/physical memory; and the separately limited-PASS response namespace table. This repair proves neither the other boundaries nor a whole-request memory cap. The finite admission formula `64 KiB + 4096 × cumulative input bytes` is not an allocation proof; a 64 MiB budget admits at most 16,368 input bytes before other charges, not a full 8 MiB input. Dependency registry-source access remains denied; no alternate access or guessed container/codec bounds were used. RSS limits, physical OOM recovery, zeroization and performance are unproven.

There is no final-client mock proving exactly-once success submissions and zero failure submissions of buffered tools. The complete task/model/cache/audit/usage/error/tool-observation matrix, protocol-versus-task examples, real Codex/model behavior and production acceptance remain unproven.

Loopback binding, protocol keys `openai_chat` / `anthropic` / `openai_responses`, default `[REDACTED]` / `[REDACTED_PRIVATE_KEY]`, generic `sk-` audit/allow-only restrictions, security-redaction failures, shared-before-extension DB locking and asynchronous audit/usage queues were not relaxed. Ordinary legacy redaction acquires no token mappings, restoration namespaces/tables or restoration buffering; native namespace compatibility remains separate.

**PR assessment:** the required checks listed above pass; the additional strict panic lint still exits 101. These two repairs have a bounded review, but the complete pending feature is **not merge-ready** under its specification. This is not a PR approval, production authorization or instruction to stage/commit/push. Further production repairs require a separately approved concrete scope; staging and PR operations remain outside the current approval.
