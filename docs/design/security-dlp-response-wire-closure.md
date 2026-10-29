# xAI JSON response wire-buffer accounting: closure record

## Decision and status

Audience: human reviewers and an implementing/reviewing agent. **Draft; paused and not sealed.** The minimal production change exists, but current-source TDD controls and final verification are incomplete. A resumed diagnostic command was explicitly denied; a renewed permission question timed out without an answer. No alternate execution path was attempted. This record must not be used as a completion or submission approval.

The user approved exactly the five paths in [the execution specification](security-dlp-response-wire-execution-spec.md), then explicitly authorized child implementation of its three Rust paths. The parent owns the two new documents. No further production repair is authorized here.

| Decision | Current status | Boundary |
| --- | --- | --- |
| Independent xAI nonstreaming rewritten JSON wire buffer | NOT COMPLETE / NOT-PROVEN | Valid RED and an earlier single-case GREEN exist; final success/lifetime controls and current-source verification remain incomplete. |
| Broad functionality | FAIL / NOT-PROVEN | Compact SSE and the full routing, retry, history, policy and side-consumer matrix remain open. |
| Whole-resource acceptance | FAIL / NOT-PROVEN | One serialized buffer does not close parser/converter/dependency/transport allocations. |
| Whole specification | FAIL | Required functional, resource and side-effect evidence is missing. |
| Code submission / merge readiness | FAIL | No staging, commit, push, PR, release or production authorization. |

Protocol completion/restoration does not establish task correctness or authorize tools or generated-command execution. Every recorder in this work is nonexecuting.

## Scope and reproducibility

Starting HEAD: `4dbfc90d1ff495c04b95bb04962bde57df799d82`. Prior identity: `/tmp/vary-dlp-remaining-final-identity.json`, SHA-256 `0cda90d5b3195e07fc8c85d09d0ce1e0e66176bf4be0ea550a7337070b342d6e`. Preflight matched all 71 prior pending paths, protected/context hashes and the empty index before creating the new execution specification. Preserve the prior dirty tree.

Only these paths belong to this extension:

- `src-tauri/src/proxy/handlers.rs`: active-lease xAI native nonstreaming JSON rewrite serialization; test-only boundary seam.
- `src-tauri/src/proxy/security/http_restoration.rs`: narrowly named response entry point reusing the existing fallible two-pass budget-owned encoder.
- `src-tauri/src/proxy/security/reversible_integration_tests.rs`: actual HTTP target-boundary regression and ownership/compatibility controls.
- `docs/design/security-dlp-response-wire-execution-spec.md`: approved execution specification; quality review by Quill `71e42483c9c84a269b8b21a244b948c9` passed both mixed-audience gates.
- `docs/design/security-dlp-response-wire-closure.md`: this evidence record.

No overlapping writes or parallel Cargo compilation. Tests use synthetic fixtures, in-memory databases and actual listeners on `127.0.0.1:0`; no live models, application startup, packaging or tool execution. Registry/codec-source access remains denied, with no alternate-path bypass. No dependency, schema or configuration changes.

## Repair contract and TDD evidence

The target is the independent serialized buffer in `handlers.rs::handle_codex_xai_native_responses_rewrite`, after original JSON completion verification and existing namespace/numeric conversion, before tool observation and the outer final restoration wrapper. Only a nonempty restoration lease may acquire its response owner. Ordinary legacy `redact` and no-active-token traffic must not gain mappings, restoration tables/namespaces or restoration buffering.

A valid RED must exhaust the real shared payload budget only at target serialization, then release injected occupancy before the outer wrapper. Unchanged production serialization must return successful HTTP 200 against an expected 502 runtime assertion. The seam may reserve capacity but cannot fabricate an error or replace serialization. Compilation, fixture, instrumentation-only, wrong-boundary, timeout and zero-test failures do not qualify.

On active-lease serialization/capacity failure, the contract is fixed HTTP 502 `dlp_restore_failed`, no fallback original body, no retry, no client tool/completion and no reusable completed context. The original upstream send may already have occurred once. Cloned `Bytes` share one owner; final drop/cancellation releases its reservation. This is logical buffer ownership, not physical OOM recovery, RSS or zeroization proof.

Implementation agent: Nexus `a11df75b938043118d41eb31bb80f13c`. A valid intended-boundary runtime RED was retained before production changes: `/tmp/vary-dlp-response-wire-red-target.log`, SHA-256 `c320f84db43e72dcf628acc4e2d9512e0bcbd757f7c2d0956bc0314bed2654ba`; metadata `/tmp/vary-dlp-response-wire-red-target.json`, SHA-256 `36812a02bc167a6b2f9915eec552a10688805525947b88123ee621e4316e91b9`, contains exact three-file test-state hashes and command. The individual `reversible_wire_xai_json_serialization_budget_failure` command below exited 101: zero passed, one failed, zero ignored, 4,697 filtered. Runtime evidence: `initial_sends=1`, `target_entries=1`, active lease, full shared budget, serialization reached once, injected occupancy released once, budget recovered, HTTP 200. The intended assertion was actual 200 versus expected 502 at target serialization. This is not an admission/final-wrapper/fixture/compile failure. The RED build emitted a test-hook unused-field warning; it is not a passing lint result. Current GREEN controls, final source checks and artifact sealing remain pending.

### Interrupted continuation: evidence classification

Nexus's session lease was revoked before handoff. Cipher `843f4e549be646088fa9a58f22fcdfa9` resumed only the delegated scope. The parent confirmed no Cargo/rustc process before delegation and did not run Cargo in parallel.

| Artifact under `/tmp/vary-dlp-response-wire-` | Result and classification | SHA-256 |
| --- | --- | --- |
| `green-individual-failure.log` | Earlier one-case GREEN: 1 passed / 0 failed / 0 ignored; active/full budget, encoded=0, recovered budget and HTTP 502. Test source later changed; not final current-source acceptance. | `98a8bef6ac584ab9f70d069135b9b67d0fb6ed35815dfeb4bcbaf88cbeec91d2` |
| `green-individual-success.log` | 0 passed / 1 failed / 0 ignored after HTTP 200. Incorrect test expectation that numeric `-0.0` must remain signed; existing xAI normalizer maps it to integer zero. Test expectation adjusted to existing normalization with signed-zero string control. Not target RED; no production numeric change. | `28ff8ec72fd3ed1e21753516c441e260817aeaf73cd588fa2c378dae99a51270` |
| `green-individual-success-current.log` | 0 passed / 1 failed / 0 ignored after HTTP 200. Budget release assertion observed 1,589,919 bytes versus foreign occupancy 47. Not target RED and not GREEN. | `0d6b65f9b215a07f4ff7781a8dc2c6684e42cef09fa2411acdaafc3bb286c10d` |

Read-only source inspection found that the final restored client `Bytes` retains the upstream processing owner and remains alive at the failing fixture assertion. Cipher added temporary test-only before/after-drop sampling, retaining the original failing before-drop assertion. The diagnostic command was denied before Cargo started. Therefore this owner-lifetime hypothesis has no confirming runtime evidence; the fixture is not yet repaired. Do not remove or weaken the assertion speculatively. No new current-turn test ran, and the six named controls plus four prescribed filters and final Rust checks remain pending.

Helix `083946c8adfe457dbb9ae01c30475dba` inspected the frozen production delta and retained RED read-only: no blocking production finding in that bounded static scope. Its final verdict is **FAIL due to incomplete evidence**, not a newly identified production defect: final tests were not frozen/verified and no current GREEN or later checks were supplied. It did not execute checks or independently calculate hashes. Production hashes previously checked by the parent at pause: `handlers.rs` = `92d7abbf8615b674f02f72e844838746a2554802cd483e36861575a0821173b7`; `http_restoration.rs` = `a1d3bd3a359ce91a9121a067ecd4413be7e08f1e9c07363287a53302b17e5b2e`. The third file contains unexecuted temporary test diagnostics; no current tested-source seal exists. This draft's final mixed-audience quality review and seal are also pending.

### Required commands on an authorized resumption

The following are **required commands, not a claim that they all ran**. Run from the repository root, sequentially, with Rust `CARGO_INCREMENTAL=0`:

```bash
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml TEST_NAME -- --test-threads=1 --nocapture
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml reversible_wire_xai_json_ -- --test-threads=1
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml reversible_http_xai -- --test-threads=1
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml p1_acceptance_xai_raw_completion -- --test-threads=1
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml proxy::security:: -- --test-threads=1
git diff --check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
CARGO_INCREMENTAL=0 cargo check --offline --locked --manifest-path src-tauri/Cargo.toml
CARGO_INCREMENTAL=0 cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
CARGO_INCREMENTAL=0 cargo test --offline --locked --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1
CARGO_INCREMENTAL=0 cargo test --offline --locked --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

Logs for this extension use `/tmp/vary-dlp-response-wire-*`. Each final claim must name a command, tested state, exit, nonzero matching count and hash; ignored tests are not passes. A timeout remains incomplete. Standard Clippy and supplemental strict panic lint are separate claims.

## Frontend evidence reuse

Parent read-only comparison found all 702 non-Rust entries in `/tmp/vary-dlp-remaining-final-verified-source.json` unchanged, including source/tests and frontend configuration represented there. Node remains `v25.2.1`; pnpm remains `10.12.3`. The five context hashes (`package.json`, `pnpm-lock.yaml`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `rust-toolchain.toml`) match the prior identity. Recheck before sealing; changed or uncertain inputs invalidate reuse.

Prior retained checks are `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, and `pnpm test:unit --exclude '**/.voidx/**'` (201 files / 2,095 tests passed), all exit 0. Exact log hashes remain in [the previous closure record](security-dlp-remaining-closure.md#current-source-verification). Node local-storage and React `act(...)` warnings remain disclosed. These are reused frontend checks, not newly executed Rust-extension checks.

## Read-only remaining-blocker assessment

### Actual side-consumer ordering

- `src-tauri/src/proxy/response_processor.rs:245-254` mounts nonstreaming observation on upstream bytes before returning the response; usage parsing/log scheduling follows at lines 256-325. This is not final-restored client output.
- `src-tauri/src/proxy/tool_observation/mount.rs` feeds immutable source bytes to taps at its mounted boundaries, including before conversion where mounted.
- `src-tauri/src/proxy/tool_observation/channel.rs` clones `Bytes` into nonblocking bounded ingress. The source declares queue capacity 4,096, maximum piece 1 MiB, queued bytes 16 MiB, active reservation budget 64 MiB and per-response reservation 3 MiB. `try_send` failure faults observation without blocking forwarding. These bounds do not prove all downstream allocations or custom-DLP safety.
- `src-tauri/src/services/tool_risk/worker.rs:25-111,185-263` parses observations in the background, evaluates policy, projects shell command summaries through `sanitize::summarize_command`, batches events and persists through `spawn_blocking`. Persistence errors use fixed detail-omitting warnings. The summary sanitizer is not the reversible custom-DLP policy engine.

These source facts clarify the real wiring but do **not** establish actual-handler end-to-end queue/cache/usage/persistence or all-diagnostic-sink safety. Prior mocked projections disclose custom-DLP plaintext retention counterexamples; mock counterexamples alone are not proof of actual handler leakage. No new end-to-end runtime proof or side-consumer repair is claimed.

### Compact SSE

`src-tauri/src/proxy/security/http_restoration.rs::into_sse_restorer` explicitly rejects `endpoint_is_compact`. The SSE restoration state in `response_restoration.rs` has no Compact mode field. JSON `stage_json_response` passes its Compact flag into output classification: `compaction` requires Compact mode and token-free opaque content; function/custom tool handling is non-Compact only. Removing the SSE entry rejection alone would therefore not propagate Compact semantics to snapshot/type/opaque-provenance checks. Original protocol completion and clean natural EOF would still be required. This round does not modify these rules or claim Compact SSE support.

### Resource and acceptance inventory

| Independent boundary | Continued status | Why this extension does not close it |
| --- | --- | --- |
| Request sanitizer/provider-model/TOML | FAIL | Independent clones/parses/serializations remain outside this response buffer. |
| Response fields/parser/serializer/converter | FAIL overall | Only the xAI nonstreaming rewritten wire buffer is targeted; parsed trees, converted snapshots and other serializers remain separate. |
| Persistent Chat metadata/index/container/snapshots | FAIL | Finite entry counts and logical JSON owners do not cover all container/index/snapshot allocations. |
| General processing workspace | NOT-PROVEN | Finite admission allowance is not a dependency allocation bound. |
| Codec constructors/internal buffers/reads | NOT-PROVEN | Source access denied; no guessed bound or bypass. |
| Headers/transport | FAIL | Header clones and raw transport/header buffers remain separate. `hyper_client.rs` already moves the original body and writes headers/body separately; do not misreport a full-body concatenation here. |
| Charged response namespace table | LIMITED PASS | Prior bounded ownership proof only; not the other six boundaries. |

The workspace allowance `64 KiB + 4096 × cumulative input` is finite admission only: a 64 MiB budget admits at most 16,368 input bytes before other charges, not a full 8 MiB input. No larger multiplier is introduced or treated as proof.

Historical security **607 passed / 1 failed** remains unexplained. A read-only search of 618 `/tmp/vary-dlp-*` text artifacts no larger than 4 MB found eight summary references but no original matching result log or failed-test identity. Current GREEN cannot explain that failure. Prior strict panic lint exited 101 with zero proxy / 41 nonproxy primary-span diagnostics; this is neither whole-library nor macro-expansion PASS.

The prior recorder collect-timeout limitation, nonexhaustive snapshot matrix, direct-wrapper foreign-witness poison scope, and full aliases/conversion/retry/history/policy/cache/audit/usage/error/observation matrix remain unchanged. No RSS cap, performance, physical OOM recovery, zeroization, actual Codex client/model, semantic task-correctness or production acceptance proof is added.

## Final seal requirements

Before replacing Draft status: record valid RED classification and final GREEN evidence; verify the frozen three-file delta and nonexecuting recorder/owner controls; run sequential current-source checks; obtain bounded source review and both mixed-audience document quality gates; compare exactly against the prior identity allowing only the five approved paths; recheck protected/context hashes and empty index. Seal source/evidence/identity outside this file without circular self-hashing. No staging, commit, push, PR or release actions.
