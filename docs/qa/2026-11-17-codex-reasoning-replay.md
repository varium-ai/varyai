# Responses reasoning replay after model switching

## Reproduction and reference

The reported Codex desktop request failed at `/backend-api/codex/responses` with
HTTP 404: `Item with id 'rs_0' not found`, followed by the upstream explanation
that items are not persisted with `store=false`. The user switched models inside
one conversation. The supplied log does not include that request ID or failure;
its final HTTP 502 entries do not establish the source of this particular item.
No private request body, conversation transcript or credential was collected.

The regression uses a synthetic upstream that rejects an unsealed reasoning item
and a second case that rejects a named sealed reasoning item. Before implementation,
`native_model_switch_filters_bare_reasoning_and_recovers_named_404` failed with
404 instead of 200 on its first case.

Reference: Magpie revision
[`f9492fd8ed2c6fd3c65f1dab00062771b551d7b5`](https://github.com/yetone/magpie/tree/f9492fd8ed2c6fd3c65f1dab00062771b551d7b5),
reviewed on November 17, 2026:

- `internal/gateway/codex_backend.go`: `codexInputAs`, `withoutUnreadable`,
  `withoutItem` and the native HTTP 400/404 retry loop.
- `internal/gateway/codex_backend_test.go`:
  `TestCodexOwnModelAfterAnotherVendor` reproduces the same refusal vocabulary.
- `internal/gateway/azure_reasoning_test.go` and `bare_reasoning_test.go`:
  storeless reasoning and provider-specific refusal handling.

## Implementation contract

The shared Responses helper filters unsealed or text-bearing foreign reasoning
before official Codex forwarding, retaining valid native seals. Explicit
`store=true`, unknown/malformed field shapes, other input kinds and unknown
protocol fields remain intact. Raw rewrites preserve untouched numeric lexemes.
Managed upstreams receive their original reasoning until they explicitly reject
an item; no global deletion of reasoning is applied to other models.

Native Codex, direct intent Responses, the managed forwarder and the shared
transparent native HTTP adapter recognize HTTP
400/404 JSON errors naming a missing reasoning item. Only that exact, unique
reasoning input item can be removed. Each send loop permits at most three such
recovery retries, at the same destination with the same model, authentication,
workspace and conversation headers. No retry happens for unrelated errors,
missing or repeated IDs, messages, tool calls/results, compaction checkpoints,
successful responses, or ambiguous connection failures. Errors embedded in
successful HTTP 200 streams are outside this recovery contract.

Error inspection retains the original status, headers and chunks. If an error
stream exceeds 64 KiB, stalls for five seconds, or fails mid-read, inspection
stops and returns the original stream without a recovery retry. Error text and
item IDs are not added to diagnostics or client errors.

Restoring requests retain their original prepared attempt and token authority.
Every amended request must pass final history validation and budgeted outbound
encoding before it can be sent. If deleting a reasoning item would invalidate
a completed-call witness, the deletion is rolled back and the original upstream
error follows the existing safe-error path. Authentication, DLP admission,
restoration, security decisions and checkpoint protections are not relaxed.

This is a focused adaptation of Magpie's reasoning replay behavior. It does not
adopt arbitrary named-item deletion, capability/refusal caching, compaction
fallbacks or successful-stream replay. See the per-Agent matrix in
[Agent feature coverage](agent-feature-coverage.md#responses-history-after-model-switching).

## Regression paths

- Official Codex native model switching with inspection disabled, ordinary
  inspection, and active reversible redaction. Native encrypted state, visible
  messages, tool pairs, credentials, workspace and session headers are retained;
  current-turn placeholders still restore after the retry.
- Every shared connection registry entry, both Codex menu/native managed entry
  points, TRAE China/international, streaming/nonstreaming and named/unnamed
  sessions, with protection disabled and reversible restoration enabled.
  The preceding turn actually returns the tool call replayed by the next turn.
- Independent native Responses adapters for Kimi, Qwen, Z Code, Harness, TRAE
  China and Qoder custom models, with protection disabled/enabled and JSON/SSE
  responses. These use each adapter's existing inspection/redaction policy.
- Raw numeric lexeme preservation; explicit stored history; malformed or unknown
  input; duplicate IDs; protected tool/message/checkpoint kinds; history
  validation rollback; retry limits; unrelated statuses/errors; oversized and
  interrupted error streams.

An initial cross-connection fixture reused a tool call ID across unrelated
conversations. The existing collision guard correctly returned 409; the fixture
now returns distinct IDs and replays each actual completed call. The assertions
and ownership guard were not weakened. An existing Qwen native suffix test also
failed with 503 when run alone: it assumed global redaction policy without
installing it. That test now initializes its required redaction policy, holds the
shared serial guard and restores the previous policy; production security and
its redaction assertions are unchanged.

## Verification

Base revision: `eb24a0e3b947ba89bd9aa668148e23b0e9bbe59c`, plus this working tree.
Pre-existing tray security localization and concurrent diagnostic-export changes
are retained; they are not owned by this replay change. Host: macOS 15.3.1
arm64; Rust/Cargo 1.98.1; Node 24.11.0; pnpm 10.12.3. Node differs from the pinned
repository version. Rust checks use `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`.

The nine owned proxy source/test files are frozen in
`/tmp/varyai-responses-replay-fix-20261117/owned-source-final.json`; their bytes did
not change during verification. Manifest SHA-256:
`23f351e695bdc91f2982071cadd42d42d823a7940fc7eb7bc9a1a9a5128e1c2a`. Verification completed on November 18, 2026.
Results and command logs are under that same directory. Final command outcomes
are recorded in `backend-accepted-results.json`, `frontend-final-results.json`,
`frontend-results.json` and `unit-bounded-result.json`.

| Check | Result |
| --- | --- |
| Test-first native replay regression | Failed before implementation with HTTP 404 instead of 200; passed after implementation |
| All model-switch regressions | Passed, including native adapters, managed connections and active reversible restoration |
| `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` | Passed |
| `pnpm test:unit --maxWorkers=2` | 324 files, 4,400 tests passed; existing assertions and timeouts unchanged |
| `pnpm audit --prod` | Passed; no known production vulnerabilities |
| Development, CI-policy, packaging, bridge, model-default, Pi session, credential, check-in, reward, codec and storage helpers | Passed; exact commands in `frontend-results.json` |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Final library command passed 7,732; 21 ignored |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Final full command passed 7,734 library and 165 integration tests; 21 library tests retain explicit ignore prerequisites |
| Qwen native suffix regression, isolated after policy initialization | Passed without relying on another test's global policy |
| `git diff --check` | Passed |

The default-concurrency frontend suite initially passed 323 files / 4,394 tests.
A concurrent UI test edit required a rerun. That rerun had one five-second test
timeout in `ToolRiskEventsPanel` and a worker-startup timeout for
`UsageSourceLabel`, plus an interrupted/skipped worker case. The complete suite
then passed with two workers and the original timeouts and assertions. No
production frontend change or test weakening was used to obtain that result.

Concurrent diagnostics implementation/test edits occurred during the earlier
backend pass; they are listed in `backend-source-changed.json`. The replay-owned
files remained identical. Subsequent complete backend checks are recorded in
`backend-accepted-results.json` and their source changes in
`backend-final-source-changed.json`. This report does not claim acceptance of
unrelated diagnostic-export edits that changed after a check began. Revalidate
the final combined checkout before a commit or merge. Library test counts differ
between commands because concurrent diagnostic tests were added; no replay test
was removed or ignored. The final type check was repeated after the unrelated
frontend test edits, and the final coverage-ledger test also passed.

Generated stale Rust incremental/object files and duplicate top-level library
outputs were removed after inspection to recover disk space. Only verified orphan
library compiler processes from cancelled builds were stopped; no source,
configuration, account data or installed application was removed. Rust test builds
emit the existing unused test-method and macOS compact-unwind warnings.

Fixtures use dynamic loopback ports, synthetic credentials, in-memory databases
and temporary storage. No native client self-test, installer acceptance,
Windows/Linux runtime test or remote CI result is claimed. No commit, push,
installation or explicit application launch is part of this change.
