# Unified DLP implementation evidence

> Status: M0/C1/C2/P2 and the [P3/P4 scoped closure](#22-p3p4-integrated-verification-and-publication-proof-closure) are preserved. **C3/C4/C5 and CORE-1 technical acceptance are complete** at the [post-F1 freeze, Full Gate and independent PASS](#50-whole-task-reviews-and-approved-f1-evidence-amendment). [Section 51](#51-approved-baseline-commit-isolated-workspaces-and-request-lane-red) records the authorized baseline commit and initial request RED; [Section 52](#52-request-lane-green-characterizations-and-scoped-review-findings) preserves the earlier checkpoint and GE1 FAIL. [Section 53](#53-ge1-signature-alias-correction-and-request-review-closure) records **GE1 F1 resolved**, isolated request checks **CH1 19/19, AN1 15/15, GE1 33/33** and security checks **1935/1931/1949**. [Section 54](#54-authorized-chat-only-shared-amendment-execution-checkpoint) records the authorized **unfrozen Chat-only amendment**, parent checks **24/24 CH2 / 1941/1941 security**, C5 coordination, snapshot F1 repair and bounded independent closure. Earlier failures and verdicts remain historical; typed policy and whole-amendment gates remain open. Native registrations remain **Disabled**; no lane handoff, M2–M5, new Full Gate, native enablement or live/release acceptance is granted.
>
> Audience: maintainers and implementing/reviewing agents. This is a sanitized verification record, not a replacement for the [approved execution plan](security-dlp-unified-core-parallel-implementation-plan.md) or the [authoritative architecture](security-dlp-unified-core-protocol-adapters.md).

## 1. Scope and source identity

Sections 1-7 record the historical M0 test-only milestone: a protocol-matrix fixture prerequisite, two `cfg(test)` module registrations, and 13 characterization tests over the existing Responses/Compact facade. M0 made no production restoration change; its harness is not the frozen non-Responses harness required by CORE-1. Sections 8-12 record C1 foundations and C2 handoff; Sections 13-16 record C2 extraction and the source identity/gates at that earlier checkpoint. Section 21 is the latest P3 status and scoped source identity. Earlier hashes and counts remain historical evidence, not identities or acceptance results for the current P3-modified files.

- Repository: `varyai`; branch: `main`.
- Inspected and tested HEAD: `2b00c4573098f4da31345c788aacf321ef685060`.
- Planned inspection baseline: `ebfca878`. `git diff --stat ebfca878..HEAD -- src src-tauri scripts package.json pnpm-lock.yaml` was empty.
- Initial dirty tree: only the untracked architecture and execution-plan documents linked above. Both were preserved during source work. The architecture remains unchanged; the plan receives approval/progress annotations with this handoff.
- No staging, commits, pushes, worktrees, application launches, installations, or ignored/live tests were performed.

Final test patch, with SHA-256 identities:

| Path                                                         | Change                                        | SHA-256                                                            |
| ------------------------------------------------------------ | --------------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/proxy/protocol_matrix_tests.rs`               | Explicit test runtime publication at line 681 | `2a80f6f393d977397dbf4fdd5492b11e7da0805cbd51c0b73e7ee3bf207a1c8f` |
| `src-tauri/src/proxy/security/mod.rs`                        | Test-only registrations at lines 147-152      | `22be48688b93f31e65e797605c11cb5778d5ef4d61975b6b7e367ace5e319567` |
| `src-tauri/src/proxy/security/tests/unified_core_tests.rs`   | 13 tests, lines 1-520                         | `e1f79731a8c93dc9884840cfcfcb24a77cc204694667daae6e738406406f4904` |
| `src-tauri/src/proxy/security/tests/unified_test_support.rs` | Synthetic facade harness, lines 1-148         | `cb12e335c1f6e418e72cb29da700e140e857927cdfea9f024bf88bdd562b9721` |

Original document hashes, before progress annotations:

- Execution plan: `e62f580a7b239d3b9b8799bd5c16958bd00e48c46247ce487c6aba27cd71cb8a`.
- Architecture: `13a1ea25967d570679cc94a0f61cc6257ddd0bd06243cef4ffeda8728e3a6166`.

## 2. Execution environment and evidence location

All commands were run from the repository root on macOS arm64, using the existing Cargo target cache. Heavy Rust checks were serial, with no concurrent worktree builds.

| Item          | Observed value                                                           |
| ------------- | ------------------------------------------------------------------------ |
| Rust          | `rustc 1.98.1 (48a229cea 2026-09-01)`                                    |
| Cargo         | `cargo 1.98.1 (797e8a9bc 2026-08-05)`                                    |
| Node          | `v25.2.1`                                                                |
| pnpm          | `10.12.3`                                                                |
| Memory / CPUs | 24 GiB / 12                                                              |
| Free disk     | Approximately 33 GiB earlier; 27 GiB at final Rust/renderer verification |

Identity commands: `git rev-parse HEAD`, `git status --short`, `rustc --version`, `cargo --version`, `node --version`, `pnpm --version`, `uname -sm`, `sysctl -n hw.memsize hw.ncpu`, and `df -h .`.

Local command logs are outside the repository at `~/.voidx/verification/varyai-unified-core-20261105/`. Filenames below are relative to that directory. This document records outcomes and assertion labels, not fixture plaintext, raw arguments, credentials, or session/cursor values. The logs are local execution evidence, not a committed or portable CI artifact.

## 3. Baseline failure and test-only repair

The initial Full Gate passed: 254 renderer test files / 3064 tests, 5721 Rust library tests with 15 existing ignored tests, and all Rust integration targets. The focused security filter passed 1251 tests; session filters passed 17 and 19 tests respectively. Logs: `baseline-{typecheck,build-renderer,test-unit,format-check,cargo-check,cargo-fmt,cargo-clippy,cargo-test-lib,cargo-test,security,session,session-identity}.log`.

The separate protocol-matrix prerequisite did **not** initially pass. `claude_converted_tool_calls_reach_history_from_native_responses` returned HTTP 500 instead of 200. This was not hidden by the passing full-library run.

Evidence and hypothesis test:

1. `baseline-protocol-matrix.log`: 1 passed, 1 failed, 1 existing live test ignored.
2. The exact failing test failed in three fresh processes: `baseline-matrix-isolated-{1,2,3}.log`.
3. A control including the preceding `ordinary_redact_keeps_new_protocol_bridge_available` test passed both tests: `baseline-matrix-initialization-control.log`.
4. `restoration_policy.rs:91-122` starts the policy gate unpublished/unavailable; the default configuration returned by `get_security_runtime` is not a published admission. `runtime.rs:131-167` keeps these paths distinct. The failing fixture omitted explicit initialization and depended on earlier tests publishing policy.
5. The only repair is `set_security_runtime_for_test(Default::default())` inside that fixture. The production admission gate remains fail-closed. Two initial matrix reruns passed: `m0-matrix-green-{1,2}.log`; final focused results are below.

Reproduction command for the original isolated failure (before the fixture repair):

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml \
  proxy::protocol_matrix_tests::claude_converted_tool_calls_reach_history_from_native_responses \
  -- --exact --test-threads=1
```

The fresh-process failure, initialization control, and fixture repair form behavioral RED/GREEN evidence for the test prerequisite only. The 13 added characterization tests describe unchanged production behavior and may pass initially; they are not claimed as RED/GREEN for a new core or native capability.

During fixture development, the Block characterization initially failed because its custom rule lacked the compiled regex and expected the wrong HTTP status. The corrected fixture uses the compiled regex and the existing HTTP 400 `dlp_blocked` contract. This was a test-authoring correction, not a production bug or a new capability RED. An intermediate formatting check also failed on module order and was corrected before final checks.

## 4. Characterization and inherited coverage

Each new test holds the repository serial-test lock. `ResponsesFixture` owns a separate real `DlpTokenStore` and 64 MiB `PayloadBudget`, publishes a test runtime, and restores the prior configuration on drop. No network is contacted by the new harness; existing integration fixtures use dynamic loopback ports, temporary/in-memory storage and nonexecuting tools.

| Requirement                                                    | New characterization / inherited regression                                                                                                                                                                                                                                 |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Current-request restoration without identity                   | `responses_current_request_restores_without_continuity_identity`; restored client output and denied anonymous continuation are separate assertions                                                                                                                          |
| Missing cursor, mandatory current redaction                    | `missing_cursor_degrades_history_but_keeps_current_redaction_and_restoration`                                                                                                                                                                                               |
| Mixed media, unknown fields and fabricated pairing             | `mixed_media_missing_history_preserves_request_without_granting_authority`; exact request preservation is checked separately from absent mapping privilege                                                                                                                  |
| Explicit Block, no partial mutation                            | `explicit_block_is_not_context_degradation_or_partial_redaction`                                                                                                                                                                                                            |
| Literal syntax is not authority                                | `fresh_leases_and_literal_syntax_do_not_supply_completion_authority`; its fresh-lease negatives are not presented as independent cross-session/policy proof                                                                                                                 |
| Completed-reference isolation                                  | `completed_reference_resumption_requires_matching_session_and_published_policy`: real successful completion, same-session/same-policy positive resumption, cross-session negative, semantic policy revision negative, and unchanged old-reference literal with no privilege |
| Native bytes, JSON types, unknown fields and headers           | `unmodified_json_keeps_wire_bytes_unknown_fields_and_headers`                                                                                                                                                                                                               |
| Compact readable versus opaque content                         | `compact_restores_readable_output_without_mutating_opaque_compaction`                                                                                                                                                                                                       |
| Actual attempt and retries, including zero-token attempts      | `retries_bind_each_actual_attempt_including_zero_token_requests`; real transport/failover fixtures retained in `reversible_integration_tests.rs`                                                                                                                            |
| Genuine upstream failures                                      | `genuine_upstream_failure_bypasses_success_restoration_unchanged`; sensitive-value redaction remains covered by `upstream_error_tests.rs`                                                                                                                                   |
| Original versus converted completion proof                     | `converted_client_completion_is_not_original_upstream_proof`; no context is published from converted-only success                                                                                                                                                           |
| Publication and historical provider binding                    | `completed_history_requires_client_restoration_and_matching_provider_binding`: no authority before restoration, restored output, then matching-provider positive and foreign-provider negative                                                                              |
| Fixed Codex local error projection                             | `local_failures_have_fixed_codex_projection_not_internal_diagnostics`; exact fixed code/message/param checked without printing payloads                                                                                                                                     |
| Cache authority and forged history                             | Existing HTTP cache-authorization fixtures in `reversible_integration_tests.rs:4093-4253`; broader literal/history regressions remain registered                                                                                                                            |
| Stale generation                                               | Existing `reversible_context_tests.rs:1498-1551`                                                                                                                                                                                                                            |
| Redirect rejection at actual sends                             | Existing `reasoning_history_redirect_{reqwest,hyper}_{active,empty}_{307,308}` fixtures in `reversible_integration_tests.rs:13344-13408`; assertions also require no redirected body transmission                                                                           |
| Invalid/interrupted completion, cancellation and tool barriers | Existing `reversible_completion_tests.rs` and security regressions; no production verifier or release barrier was changed                                                                                                                                                   |

Inherited regressions ran in the final security and full Rust suites. They are not claimed as new tests in this patch or as native-client acceptance. The existing Responses-client conversion cells remain baseline behavior, not authorization for new native cells.

## 5. Final verification of the test patch

Each focused filter was listed first with `cargo test --lib --manifest-path src-tauri/Cargo.toml "$filter" -- --list`; a nonzero `: test` match was required. Runs then used the same command with `-- --test-threads=1`. Listing logs use the corresponding `-refined-list.log` suffix.

| Filter                                  | Result                                           | Log                               |
| --------------------------------------- | ------------------------------------------------ | --------------------------------- |
| `proxy::security::unified_core_tests::` | 13 passed; 0 failed; 0 ignored                   | `m0-unified-refined.log`          |
| `proxy::security::`                     | 1264 passed; 0 failed; 0 ignored                 | `m0-security-refined.log`         |
| `proxy::protocol_matrix_tests::`        | 2 passed; 0 failed; 1 existing live test ignored | `m0-protocol-matrix-refined.log`  |
| `proxy::session::tests::`               | 17 passed                                        | `m0-session-refined.log`          |
| `proxy::session_identity_tests::`       | 19 passed                                        | `m0-session-identity-refined.log` |

Full Gate on the final Rust test patch:

| Command, from repository root                                               | Result                                                                         | Log                             |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | ------------------------------- |
| `pnpm typecheck`                                                            | Exit 0                                                                         | `m0-refined-typecheck.log`      |
| `pnpm build:renderer`                                                       | Exit 0                                                                         | `m0-refined-build-renderer.log` |
| `pnpm test:unit`                                                            | 254 files; 3064 tests passed                                                   | `m0-refined-test-unit.log`      |
| `pnpm format:check`                                                         | Exit 0; renderer source formatting only, not Markdown                          | `m0-refined-format-check.log`   |
| `cargo check --manifest-path src-tauri/Cargo.toml`                          | Exit 0                                                                         | `m0-refined-cargo-check.log`    |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                    | Exit 0                                                                         | `m0-refined-cargo-fmt.log`      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`          | Exit 0                                                                         | `m0-refined-cargo-clippy.log`   |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 5734 passed; 0 failed; 15 existing ignored                                     | `m0-refined-cargo-test-lib.log` |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`       | 5734 library tests passed; 15 existing ignored; all integration targets passed | `m0-refined-cargo-test.log`     |

The two edited Markdown documents were checked separately after the final documentation corrections with the following command (exit 0; `m0-docs-format-check.log`). The repository's `pnpm format:check` does not cover Markdown.

```bash
pnpm exec prettier --check \
  docs/design/security-dlp-unified-core-implementation-evidence.md \
  docs/design/security-dlp-unified-core-parallel-implementation-plan.md
```

One library run was stopped by the tool's 120-second timeout without a final result (`m0-refined-cargo-test-lib-timeout.log`). It is not passing evidence. Rerunning the library and full test commands with a 600-second limit completed successfully; library execution took approximately 124 seconds in each completed run.

The linker still reports the nonblocking `__eh_frame` size warning. No ignored/live checks, provider acceptance, installer, signing, notarization, or release acceptance are claimed. Renderer sources/dependencies were unchanged; renderer checks were nevertheless rerun. Subsequent edits in this handoff are documentation only and require formatting/accuracy checks, not a claim of additional production coverage.

## 6. Independent review and M0 decision

Independent reviewer `876ccc6ed0264c539b3612842543ea1e` returned **PASS** for the final M0 test-only patch: 520-line / 13-test characterization module, 148-line support module, test-only registrations, and the matrix prerequisite. The reviewer read code and local refined logs without editing files or running builds/tests. Review verified the root-cause/control chain, meaningful completed-lineage controls, fixed-label diagnostics, test isolation, and inherited cache/redirect/stale-generation coverage. There were no blocking findings in that limited patch.

Review scope explicitly excludes CORE-1, native implementation/enablement, and automatic approval of the entire M0 documentation gate. The integrating agent owns this evidence record and the final Full Gate accounting.

Harness limitation: its drop guard restores configuration, not an earlier unpublished/closed gate state or exact historical revision. Independent stores and the serial lock are appropriate for these tests; do not reuse it to assert the global gate's process-initial state or precise global revision numbers.

**M0 exit decision:** B1 source/environment/evidence recorded; B2 initial failure explained and isolated prerequisite repaired without production relaxation; B3 facade equivalence plus inherited regression coverage green. M0 is ready for the planned M1 core work, not for parallel lanes.

## 7. Next gate and forbidden carry-forward assumptions

At the M0 handoff, M1 C1-C5 were unchecked; C1's later completion is recorded in Sections 8-12. C2-C5 and CORE-1 remain unchecked. There is still no frozen contract signature, protocol-separated namespace change, non-Responses executable harness, trusted native-ingress classification, or authorized CORE-1 Git base. No lane agent may implement adapters until those capabilities and the independent boundary review pass. Obtain explicit authorization before creating a commit/base for parallel work; this record grants no Git write permission.

For M1, retain the compatibility facades, require behavioral RED/GREEN for each new boundary/capability, and rerun the plan's named core/Responses/security filters after extraction changes. Production paths outside the ownership table require a scoped amendment. Preserve strict actual-attempt binding, zero-token redirect refusal, native bytes/unknown fields, original failures, tool barriers and cancellation semantics. Do not derive authority from syntax, UUIDs, pairing or handler booleans; do not add unredacted fallback or broad `ContextUnavailable` degradation.

A source inspection also found the preexisting Claude/Gemini local envelopes in `http_restoration.rs:502-529` use the error code as the message rather than the architecture's fixed generic message. M0 neither changes nor accepts those envelopes as native-gate compliant. Resolve them within the later scoped client-projection work and its own tests before claiming the relevant native acceptance gate.

## 8. C1 foundations and current source identity

C1 is complete as an **unconnected internal foundation**, not as core extraction or an adapter-ready contract freeze. `restoration_contract.rs` provides distinct client/upstream protocol wrappers using the existing `protocol_bridge::Protocol` mapping, operation/framing/source/edit classifications, borrowed native-string observations, owner-bound opaque sites, and budget-owned atomic text-edit plans. No structured native tool arguments are serialized into a common IR.

`TextSites::observe` records an exact borrowed native JSON string node and rejects duplicate nodes and non-text values. `TextSites::plan` validates owner, immutability, decoded-string UTF-8 ranges, and duplicate/overlapping proposals before returning owned candidates. The input remains immutable; failure returns no plan and releases staged reservations. Final text equal to the original emits no edit, allowing a later adapter to preserve original wire bytes. Site and patch counts are each bounded at 4096, separately from byte budgets. Site lookup/deduplication is bounded linear work per observation; this is not a new constant-time index.

`protocol_adapters::registration` is descriptive only: Responses JSON/SSE and Compact JSON defer to the existing HTTP facade; native entries are disabled; mismatched operations and Compact SSE are unsupported. There is no enabled state or admission capability. `restoration_core.rs` and `protocol_adapters/responses.rs` contain comments only; the three lane directories contain empty registered modules/test slots. A source caller inventory found no new contract use in HTTP admission, forwarding, or response processing. Existing Responses/Compact behavior and conversion support remain owned by the old facade. This slice adds no namespace, completion proof, history-publication API, native implementation, or route enablement.

HEAD and branch remain `2b00c4573098f4da31345c788aacf321ef685060` / `main`; all changes remain unstaged. M0 files and the initially untracked architecture/plan were preserved. Current C1-tested Rust identities are below; the M0 table in Section 1 is historical.

| Path under `src-tauri/src/proxy/`                                                               | SHA-256                                                            |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `protocol_matrix_tests.rs`                                                                      | `2a80f6f393d977397dbf4fdd5492b11e7da0805cbd51c0b73e7ee3bf207a1c8f` |
| `security/mod.rs`                                                                               | `f73e01726f2e98b312c6c303f781f8d1201f5861f0da692f9a36dd46356e533e` |
| `security/restoration_contract.rs`                                                              | `eb2d934aea50c3c32f5701656854c60cdb8a883fb302561af3f626cfd30ccf7c` |
| `security/restoration_core.rs`                                                                  | `39c59ecf409c0c27117ff59077d303fa78441eab76be609a8cdb0ace50b91ca0` |
| `security/protocol_adapters/mod.rs`                                                             | `537702ce6c983ad30a953b7d17560db9dcf916faaf8e4bcecaaf5b91d40bc122` |
| `security/protocol_adapters/responses.rs`                                                       | `9363bd1cecaa20b487f0328863818090ad94eff102482aa38d5dd304fc726282` |
| `security/protocol_adapters/{chat,anthropic,gemini}/mod.rs` (each)                              | `9e9dc36b03b229ad4ceff9d7fd9f376fe205828a659c923cbc9e609fdcf91c00` |
| `security/protocol_adapters/{chat,anthropic,gemini}/{request,response,history,tests}.rs` (each) | `6eb677136cf7991506e81ea0f138d82d91feaf3d82252bd483f0cd2100840a74` |
| `security/tests/unified_core_tests.rs`                                                          | `fe549864b379c7a3af968e6b0091c56b2e2abc78dc98d2c5fa97b724ed39a357` |
| `security/tests/unified_test_support.rs`                                                        | `cb12e335c1f6e418e72cb29da700e140e857927cdfea9f024bf88bdd562b9721` |

The expanded 23-file manifest is `c1-final-source-sha256.txt` in the local evidence directory from Section 2; its SHA-256 is `d255132c5db101fe02ff5b8d05ff97e499277c9ee4a9089d0bd31f24b2d17cea`. It contains sorted `SHA-256  path` lines with a final newline. The architecture remains unchanged at `13a1ea25967d570679cc94a0f61cc6257ddd0bd06243cef4ffeda8728e3a6166`. Documentation-only edits after the C1 Rust freeze do not change these source identities.

## 9. C1 behavioral RED/GREEN and controls

These are new boundary behaviors, not initially passing characterizations:

| Stage                                             | Observed result and intended behavior                                                                  | Local log                |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ------------------------ |
| Compileable rejecting skeletons                   | 0 passed / 6 failed; protocol roles, registration, observation and planning assertions reached runtime | `c1-initial-red.log`     |
| Protocol/observation/registration implemented     | 3 passed / 3 failed; missing atomic planning isolated                                                  | `c1-patch-red.log`       |
| Initial planner                                   | 19 passed, including 13 unchanged M0 tests                                                             | `c1-unified-green.log`   |
| No-edit passthrough control                       | 9 passed / 1 failed; `unchanged_proposal_requires_native_reencoding`                                   | `c1-unchanged-red.log`   |
| No-edit correction plus additional controls       | 26 passed, including 13 M0 tests                                                                       | `c1-controls-green.log`  |
| Explicit site work limit                          | 0 passed / 1 failed; `site_work_limit_missing`                                                         | `c1-work-limit-red.log`  |
| Site limit implemented; patch limit still absent  | 0 passed / 1 failed; `patch_work_limit_missing`                                                        | `c1-patch-limit-red.log` |
| Both work limits implemented, final frozen source | 27 passed: 14 C1 controls plus 13 M0 tests                                                             | `c1-final-unified.log`   |

Historical RED evidence records the test names, assertion labels and counts from those source stages; it is not a claim that the final source still reproduces RED. The exact work-limit test is `proxy::security::unified_core_tests::c1_site_and_patch_counts_have_explicit_work_limits`; the final unified run executes it successfully. Other C1 tests are selected by the `proxy::security::unified_core_tests::c1_` filter; the broader final unified filter is recorded below.

The 14 C1 controls cover independent client/upstream identity; descriptive disabled registration, operation mismatches and Compact framing; exact native-node identity and equal-text separation; immutable/foreign sites; invalid UTF-8/ranges; duplicate/overlapping edits; empty/adjacent/insertion edits; no-op output suppression; byte and item limits; staged allocation/copy failures; peak simultaneously live text/metadata reservations; independent foreign/local budgets; and a plan retaining owned text after observation metadata drops. Assertions use fixed labels rather than payload diagnostics.

The first build/list attempt exceeded its 180-second command limit while waiting for the existing Cargo lock/compilation (`c1-initial-list-timeout.log`). It is neither behavioral RED nor passing evidence. Subsequent 600-second commands compiled and reached the recorded runtime failures. No timeout or compile error is counted as a behavioral failure.

## 10. C1 final verification on the frozen Rust source

Commands ran from the repository root on macOS arm64 with the existing target cache, serial Rust suites, and no worktree builds. Environment was reconfirmed: Rust/Cargo `1.98.1`, Node `v25.2.1`, pnpm `10.12.3`, 24 GiB memory, 12 CPUs, approximately 21 GiB free disk after checks. No unrelated editor process was terminated. The source identities in Section 8 remained unchanged across final verification and documentation updates.

Each focused filter was listed and required a nonzero `: test` match, then run serially:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml "$filter" -- --list
# Require a nonzero ': test' match in the listing before running:
cargo test --lib --manifest-path src-tauri/Cargo.toml "$filter" -- --test-threads=1
```

| Filter                                  | Final result                                     | Log; listing uses the corresponding `-list.log` suffix |
| --------------------------------------- | ------------------------------------------------ | ------------------------------------------------------ |
| `proxy::security::unified_core_tests::` | 27 passed; 0 failed; 0 ignored                   | `c1-final-unified.log`                                 |
| `proxy::security::`                     | 1278 passed; 0 failed; 0 ignored                 | `c1-final-security.log`                                |
| `proxy::protocol_matrix_tests::`        | 2 passed; 0 failed; 1 existing live test ignored | `c1-final-protocol-matrix.log`                         |
| `proxy::session::tests::`               | 17 passed                                        | `c1-final-session.log`                                 |
| `proxy::session_identity_tests::`       | 19 passed                                        | `c1-final-session-identity.log`                        |

Full Gate was rerun on this C1 state; M0 results are not used as proof of changed Rust code:

| Command, from repository root                                               | Final result                                                                   | Log                           |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | ----------------------------- |
| `pnpm typecheck`                                                            | Exit 0                                                                         | `c1-final-typecheck.log`      |
| `pnpm build:renderer`                                                       | Exit 0                                                                         | `c1-final-build-renderer.log` |
| `pnpm test:unit`                                                            | 254 files / 3064 tests passed                                                  | `c1-final-test-unit.log`      |
| `pnpm format:check`                                                         | Exit 0; renderer source formatting, not Markdown                               | `c1-final-format-check.log`   |
| `cargo check --manifest-path src-tauri/Cargo.toml`                          | Exit 0                                                                         | `c1-final-cargo-check.log`    |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                    | Exit 0                                                                         | `c1-final-cargo-fmt.log`      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`          | Exit 0                                                                         | `c1-final-cargo-clippy.log`   |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1` | 5748 passed; 0 failed; 15 existing ignored                                     | `c1-final-cargo-test-lib.log` |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`       | 5748 library tests passed; 15 existing ignored; all integration targets passed | `c1-final-cargo-test.log`     |

The library test phase took approximately 125 seconds in each completed library/full command; the full command also ran the integration targets. Commands used a 600-second limit. The existing nonblocking linker `__eh_frame` size warning persists. Ignored/live tests were not executed. Synthetic fixtures, temporary/in-memory stores, nonexecuting tools and dynamic loopback ports were retained; no application was launched/installed. These results do not establish native-client, packaging, signing, notarization or release acceptance.

The two updated Markdown files passed the explicit Prettier check from Section 5 (exit 0; `c1-final-docs-format-check.log`). Document accuracy/source-identity checks passed (`c1-final-evidence-audit.log`); these are not behavioral tests. A replayable, read-only source check is also saved outside the repository: from the repository root, run `bash ~/.voidx/verification/varyai-unified-core-20261105/c1-source-identity-check.sh`. It verifies the 23 current Rust file hashes, manifest identity, unchanged architecture and absence of staged paths, and prints HEAD for comparison; its completed exit-0 output is `c1-final-source-identity-check.log`.

## 11. Independent C1 implementation review and decision

Independent reviewer `dc2a41f9ecdc40ceb549a8e67ac820ce` returned **PASS** for the actual unconnected C1 slice. The reviewer read the final 288-line contract, registrations, empty slots/lanes, C1 tests, and local behavioral RED/final GREEN evidence without editing or running builds/tests. Owner/range/immutability validation, failure atomicity, budget retention and lack of new authority grants were checked. The reviewer also read final unified/security/check/Clippy/library logs. The full Rust run was still in progress when that review concluded; the integrating agent, not the reviewer, establishes its completed result in Section 10.

Review findings resolved before final verification:

- Unchanged final text initially produced an edit candidate. A behavioral RED exposed the native-reencoding risk; the planner now omits that edit.
- A byte limit alone did not bound observation deduplication work. Separate site/patch runtime REDs preceded the 4096-item guards.

Nonblocking coverage limitations remain explicit: exactly 4096 valid nonoverlapping patches succeeding is not a direct C1 test, and new tests do not directly inject `PayloadVec` metadata-growth failures or observe its double-allocation peak. Existing owner code was inspected and reused; text allocation/copy phases and simultaneously live site/scratch/staged reservations are tested. These limitations are not evidence of a production defect, but should be revisited before wiring adapters.

**C1 exit decision:** required contract foundations, compileable disabled slots, 14 new controls, current-source Full Gate and independent C1 review pass. Mark C1 complete only. This PASS does not freeze the adapter API or approve C2-C5, CORE-1, lane implementation, native route enablement, or a Git base. No staging, commit, push, implicit worktree or user-change discard occurred.

Independent documentation reviewer `14c06a98d2b44ff8af4c87678bfd79a0` returned **PASS** for the mixed-audience evidence/plan update: fresh-reader clarity, source/log accuracy, stage boundaries and C2 execution readiness were checked read-only. The reviewer ran no shell, builds or tests; hash validation relies on the integrating agent's recorded read-only script/output. Manifest count, framing wording and the script's print-only HEAD behavior were corrected before this verdict. This is a documentation gate only, not C2, CORE-1 or native acceptance.

## 12. Historical C1 handoff: C2 request and authority extraction

At the C1 handoff, C2 had not started. The following handoff constraints remain historical context; Sections 13-16 supersede its progress status. Work stays within the approved integrator ownership table: `security/restoration_core.rs`, `restoration_contract.rs`, `protocol_adapters/responses.rs`, `http_restoration.rs`, `request_preparation.rs`, `request_transaction.rs`, `tokenization.rs`, `completed_context.rs`, and the shared test/support files. No native lane implementation is part of C2; production changes outside the ownership table need a scoped amendment.

Current extraction anchors are `admit_http_request` / `DlpPreparedAttempt` / `prepare_attempt` in `http_restoration.rs`, private history classification and preparation in `request_preparation.rs`, budgeted field transactions in `request_transaction.rs`, and lease/store/actual-attempt ownership in `tokenization.rs` / `completed_context.rs`. Preserve one real store and existing strict owners; the C1 site plan alone is neither a lease nor mapping authority.

Before adding a new boundary, write and run a behavioral failing test for protocol-separated bindings/evidence. Preserve Responses/Compact operation transfer only within its existing operation family; do not give Gemini a Responses cursor or operation-transfer capability. Retain separate positive/negative controls for no-session current-request restoration, independently proven references, and first client result versus a previously delivered call. Source class, placeholder syntax, UUIDs, pairing, conversion or handler completion cannot mint privilege.

C2 must retain the public compatibility facade and existing Responses behavior, narrowly whitelisted HTTP context degradation, strict per-actual-attempt target/principal checks, zero-token redirect rejection, budget ownership and atomic request preparation. No unredacted fallback or new native cell is permitted. The existing `CompletionWitness::from_verified_completion(bool)` remains a known legacy visibility risk at `completed_context.rs:75-85`; C1 adds no use. Do not export or wrap it for adapters. A sealed verifier/finalization boundary belongs to C3 and must pass CORE-1 before any lane can consume it.

Required C2 checks, each listed first with a nonzero match as in Section 10:

```bash
check_tests proxy::security::unified_core_tests::
check_tests proxy::security::reversible_context_tests::
check_tests proxy::security::
```

`check_tests` is the shell convention defined in execution-plan Section 5, not a repository executable. C3 still owns response mechanics/strict completion; C4 owns orchestration/ingress and route negatives; C5 owns the executable non-Responses harness and independent CORE-1 boundary freeze. Do not mark any of those gates complete from the C1 foundation tests. An explicitly authorized identical Git base is still required before later parallel lane work; no such authorization has been granted.

## 13. C2 request/authority extraction and source identity

C2 is complete for its approved request/authority scope, not for CORE-1. The changes remain unstaged on `main` at HEAD `2b00c4573098f4da31345c788aacf321ef685060`. M0/C1 work is preserved and the authoritative architecture is unchanged (SHA-256 `13a1ea25967d570679cc94a0f61cc6257ddd0bd06243cef4ffeda8728e3a6166`). No native handler or route acquires reversible capability.

Implemented boundaries, with paths relative to `src-tauri/src/proxy/`:

- `security/restoration_core.rs`: `RequestAuthority` owns the existing real-store lease and shared payload budget. Admission uses the published Responses policy; each attempt forks current authority and binds its actual provider, upstream protocol, target and principal. Fixed fork/binding checkpoints are preserved, and independently held reservations and attempts outlive request-owner drop.
- `security/http_restoration.rs`: the HTTP request owns `RequestAuthority` rather than separate lease/budget fields. Existing `admit_http_request`, `DlpPreparedAttempt`, narrowly whitelisted context degradation, upstream failure handling, preparation atomicity and actual-send checks remain compatible.
- `security/protocol_adapters/responses.rs` and `request_preparation.rs`: Responses/Compact request classification/preparation is routed through a deliberately thin facade; the core delegates to the existing atomic, budgeted field transaction. `request_transaction.rs` itself is unchanged. Native-scoped leases are rejected before Responses preparation or final validation, even without dependencies.
- `security/restoration_contract.rs` and `tokenization.rs`: separate internal client/ingress continuity scopes use one store. Legacy Responses retains its original namespace identity digest. The native identity domain length-frames app, stable session, canonical client protocol and fixed trusted-ingress classification. This is namespace isolation only, not ingress wiring or route enablement; C4 owns trusted propagation.
- `security/completed_context.rs`: `CompletedEvidence::Responses(ResponsesEvidence)` explicitly owns existing response/call/opaque evidence; common binding and full dependency lineage are retained. No native evidence or permissive verifier is invented. Native scopes cannot publish/resume legacy Responses contexts, use Responses operations/result claims, or capture optional delivered Responses history.
- `security/tests/unified_core_tests.rs`: ten C2 controls supplement the 13 M0 and 14 C1 controls. Existing `tests/final_validation_tests.rs` fixtures use the moved owner; their assertions and coverage remain intact. The shared M0 harness is unchanged.

`c2-current-source-sha256.txt` records 28 implementation/test files, including all C1 manifest source files plus the five newly touched tracked C2 files. Manifest SHA-256: `d898cf04b6911f32b04f697b1fded492dbd67f9f7fe763e116e4b20891a9d201`. Documentation is excluded from that manifest. This is a local dirty-source identity, not a committed or authorized parallel-lane base. The earlier C1 source replay no longer matches intentionally changed C2 files.

## 14. C2 behavioral RED/GREEN and characterization

All RED results below are runtime failures of compileable, nonzero-match tests. Compiler errors, build/list timeouts and test-authoring corrections are excluded. Logs are relative to the local evidence directory in Section 2.

| Boundary                                                                           | Behavioral RED                                                                                                                                                                                   | Subsequent GREEN                                                                            |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| Scoped namespace admission                                                         | `c2-scope-initial-red.log`: 0 passed / 2 failed; `scoped_namespace_admission_missing`, `anonymous_scoped_request_missing`                                                                        | `c2-scope-first-green.log`: 29 unified tests passed                                         |
| Native exclusion from legacy Responses publication, operations, claims and history | `c2-responses-boundary-red.log`: 0 passed / 2 failed; follow-up `c2-result-api-red.log` and `c2-history-capture-red.log` isolate remaining result/history failures                               | `c2-scope-boundaries-green.log`: 31 unified tests passed                                    |
| Core request owner admission and per-attempt fork/binding                          | `c2-core-owner-red.log`: 0 passed / 1 failed, `core_request_owner_missing`; `c2-core-owner-fork-red.log`: one runtime `ContextUnavailable` after real admission, against rejecting fork skeleton | `c2-core-owner-green.log` and `c2-owner-http-extraction-green.log`: 32 unified tests passed |
| Native exclusion from Responses request preparation                                | `c2-native-request-boundary-red.log`: 0 passed / 1 failed, `native_scope_accepted_responses_preparation`                                                                                         | `c2-native-request-boundary-green.log`: 33 unified tests passed                             |

The four final reference/result controls characterize already-working Responses behavior; no artificial RED is claimed:

1. Independently completed references survive an unwitnessed pair/missing optional context; a separate same-namespace uncompleted mapping and unknown literal gain no authority. A changed provider loses the independent proof. Consumed output restores only the proven mapping.
2. Successful read-only call-ID verification grants no mapping. Exact completed call snapshots can import their own lineage; changed call payload/name/provider cannot. A first unseen client result body needs no previous result-body digest, but does not import uncompleted/literal mappings.
3. First and replayed/edited result bodies undergo current DLP inspection with exact or missing optional call context. Newly created current mappings are usable only by their request; dropping an uncompleted attempt does not attest its result body for a later request.
4. Explicit Block in result text is enforced at admission or preparation for exact and changed call snapshots. Neither valid path reaches conversion, partially mutates input, or adds mappings.

These tests drive the real HTTP/Responses facade and consume strict JSON completion delivery. They introduce no witness constructor/wrapper, inbound-result attestation store, tool execution or fake authority implementation. Final log: `c2-evidence-ownership-green.log`, 37 unified tests passed.

## 15. C2 current-source acceptance and independent review

Checks ran from the repository root using the Section 2 toolchain and existing target cache. Rust checks were serial. No source changes occurred between final targeted checks, broad checks and independent implementation review; only evidence/plan documentation changed afterward.

| Check                                                             | Result                                                                         | Local log                                                        |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------- |
| Listed unified filter                                             | 37 tests listed; 37 passed (13 M0, 14 C1, 10 C2)                               | `c2-current-unified-list.log`, `c2-evidence-ownership-green.log` |
| Listed reversible-context filter                                  | 22 listed; 22 passed                                                           | `c2-current-context{,-list}.log`                                 |
| Listed security filter                                            | 1288 listed; 1288 passed                                                       | `c2-current-security{,-list}.log`                                |
| Protocol matrix                                                   | 3 listed; 2 passed, 1 existing live test ignored, not executed                 | `c2-current-protocol-matrix{,-list}.log`                         |
| Session / session identity                                        | 17 / 19 listed and passed                                                      | `c2-current-{session,session-identity}{,-list}.log`              |
| `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings` | Exit 0                                                                         | `c2-current-cargo-{check,fmt,clippy}.log`                        |
| `cargo test --lib`                                                | 5758 passed, 15 existing ignored                                               | `c2-current-cargo-test-lib.log`                                  |
| `cargo test`                                                      | 5758 library tests passed, 15 existing ignored; all integration targets passed | `c2-current-cargo-test.log`                                      |
| `pnpm typecheck`, `pnpm build:renderer`                           | Exit 0                                                                         | `c2-current-{typecheck,build-renderer}.log`                      |
| `pnpm test:unit`                                                  | 254 files / 3064 tests passed                                                  | `c2-current-test-unit.log`                                       |
| `pnpm format:check`                                               | Exit 0                                                                         | `c2-current-format-check.log`                                    |

Rust commands use `--manifest-path src-tauri/Cargo.toml`; Clippy includes `-- -D warnings`. Listed filters use the exact names in the approved plan; session identity is `proxy::session_identity_tests::`. An initial overqualified session-identity listing matched zero and was excluded; the corrected listing and execution above are nonzero. No ignored/live tests were explicitly run. The existing linker unwind-size warning is nonblocking. Free disk was approximately 6.4 GiB after broad checks; no user caches/processes were deleted or terminated.

Independent reviewer `46a26cb451a549d1a26841e48ac87afa` inspected the full tracked dirty diff, untracked shared sources/native empty slots, tests, authoritative requirements and same-source logs: **PASS for C2 only**, with no required correction. The reviewer ran no builds/tests and made no edits. The verdict confirms substantive lease/budget ownership and fork/bind extraction, unchanged strict authority/dependency behavior, explicit Responses evidence, current-input result inspection, and native disablement. Read-only coverage analysis `51193c2647ce4cda8b80a2b63ff8df58` identified the final reference/result test gaps; those controls were added and passed before this review.

Independent documentation reviewer `ca308b1672b54a2d84efb15fd7c918fa` returned **PASS for C2 documentation only** after checking the human/LLM clarity and execution-readiness gates, primary counts, source identity, and separation from C3-C5/CORE-1/native acceptance. The final correction distinguishes no new production publisher use from the negative publisher test. The reviewer made no edits and ran no builds/tests. The explicit Markdown check passed after that correction; all 28 source manifest entries still matched, the authoritative architecture digest was unchanged, and staged paths remained zero.

## 16. Remaining gates and handoff after C2

C3 is next. The approved plan remains the executable task list; no scope expansion or new permission is granted by this evidence:

- C3: extract response mechanics and seal verifier-issued completion/finalization/publication. The legacy boolean `CompletionWitness` constructor and low-level publisher remain known pre-CORE-1 visibility risks. C2 adds no new production use or adapter wrapper and does not approve them for adapters; a negative publisher test directly exercises native-scope rejection. Cover splits, budgets, original-versus-converted evidence, contradictions, tool barriers, interrupted/aborted completion and delayed publication before accepting the boundary.
- C4: trusted ingress/client context and per-actual-attempt/final-client orchestration, disabled-cell/redirect/error tests. Do not infer ingress trust from client headers or capability from module registration/conversion.
- C5 / CORE-1: complete protocol-specific contracts/evidence, executable non-Responses real-core lifecycle/budget/abort harness, final Full Gate and independent boundary freeze. The thin Responses facade and single active evidence variant are intermediate extraction, not native readiness.

Native lanes and route enablement remain disabled until CORE-1 and their own acceptance gates. Later parallel work also requires an explicitly authorized identical Git base; no staging, commit, push or implementation worktree permission exists. All M0/C1/C2 source work remains unstaged. Nothing here claims installer, signing, notarization, application-launch or live-release acceptance.

## 17. Partial C3: original SSE provenance and fail-atomic reservation

C3 is **incomplete**. This section records one independently reviewed increment, not CORE-1 or native-adapter acceptance. M0/C1/C2 remain preserved; native Chat, Anthropic and Gemini remain disabled. Work remains unstaged on `main` at the HEAD recorded in Section 13; no commit, worktree, application launch or installation was performed.

The increment uses the existing store, lease and shared payload budget:

- `security/restoration_core.rs` adds budget-owned `AttemptProvenance`, `OriginalSseProtocol` and `SseEvidenceRequirement`. Allocation identity correlates one actual attempt; it grants no mapping or completion authority. The requirement survives zero-token attempts.
- `security/http_restoration.rs` issues original-evidence trackers from the prepared attempt and retains expected source/provenance in `security/response_restoration.rs`. The final wrapper requires both exact attempt identity and expected original protocol, plus existing strict protocol/transport completion, before releasing the tool barrier. Ordinary native Responses accepts no external tracker; converted Chat/Anthropic and the existing xAI normalization path require their corresponding original source.
- Provenance reservation occurs immediately after attempt fork/binding, before request preparation or transformation. The caller-body commit has no new fallible provenance allocation after it. Test-only thread-local fault injection has RAII restoration and is absent from production.
- Exact budget assertions in `http_restoration.rs` and `tests/reversible_integration_tests.rs` account for the new shared provenance allocation and its cancellation lifetime. Its arm64 charge is 32 bytes, derived from the reservation and shared-allocation header sizes; assertions were not relaxed.

Behavioral evidence, relative to the local directory in Section 2:

| Boundary                                                             | RED                                                                                                           | GREEN                                                                                                                   |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Missing, wrong-source and other-actual-attempt original SSE evidence | `c3-provenance-red.log`: one passing positive control and three runtime failures at the tool-release boundary | `c3-provenance-green.log`: four controls passed; current corrected replay `c3-corrected-provenance.log`: four passed    |
| Provenance capacity denial after a transformed-body candidate        | `c3-provenance-atomic-red.log`: one runtime failure, `provenance_failure_committed_request_mutation`          | `c3-provenance-atomic-green.log`: exact test passed; unchanged caller body, no added mapping, retry after fault removal |

Initial security replay had eight exact-accounting failures, each differing by the new provenance charge. Those were accounting regressions, not the behavioral RED for an authorization boundary. Explicit charge/owner equations were corrected and `c3-provenance-security-green.log` passed 1292 tests. Compiler/test-authoring failures and zero-match listings are not behavioral RED.

After the atomicity correction, `c3-corrected-command-manifest.txt` records commands, repository context and exits. Tests were listed first with nonzero matches and run serially through `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1`: provenance 4, unified 42, reversible completion 88, security 1293, all passing. `cargo check`, `cargo fmt --check` and `cargo clippy -- -D warnings`, each using the same manifest path, exited 0. The known linker unwind-size warning remained nonblocking. `c3-corrected-source-sha256.txt` and its identity-check log bind those runs to six scoped source/test files.

Independent reviewer `80ca1a26a2d543d59b097021e1c80a27` returned **FAIL** for the earlier post-commit reservation placement. After the behavioral atomicity RED/GREEN and corrected replay, independent reviewer `6a3f64e4947a40efa159061458076daa` returned **PASS for this provenance/atomicity increment only**. The reviewer inspected current source and supplied unchanged-source logs read-only, without rerunning commands. The wrong-protocol negative also changes attempt identity, so it does not independently isolate protocol discrimination. Cumulative tracked diff artifacts include earlier milestones; no complete pre-C3 source snapshot exists, only the earlier hash manifest. Neither limitation is hidden by this verdict.

## 18. Current C3 publication RED and rejected shortcut

The current tree intentionally has two unresolved behavioral failures in `security/tests/unified_core_tests.rs`:

- `c3_mandatory_sse_publication_waits_for_final_yield_resume`;
- `c3_mandatory_sse_publication_discards_unpolled_and_final_yield_drop`.

Both use the real HTTP restoration wrapper and token store, a nonexecuting synthetic tool and an opaque sibling, with mapped and zero-token cases planned by the helper. A probe uses the captured published policy and binding, not a different fixed-policy namespace. `c3-publication-list.log` lists exactly two tests. `c3-publication-red.log` records two runtime failures, `mandatory_record_published_before_delivery_resume`: the mandatory context is already resumable while the wrapper is suspended at the terminal yield. Execution stops at the first failing mapped branch; this RED does not establish later zero-token/success assertions. The earlier test-authoring compiler failure and incorrect fixed-policy probe are excluded from RED.

Root cause: `response_restoration.rs::finalize_and_flush_inner` publishes the mandatory snapshot before returning terminal frames; `http_restoration.rs::wrap_sse_stream_with_restoration_and_evidence` then yields those frames. Only optional delivered history currently waits for wrapper resumption. The delivery boundary under test is body-consumer resumption, not proof of remote receipt. Existing optional-history cancellation tests do not establish mandatory cursor/call/token/opaque absence.

Independent concurrency reviewer `4ede5d46468d431f8da27a726ab23e19` returned **FAIL for the proposed minimal saved-table stage/commit design**, not for an implemented replacement. Saving a prebuilt index and delaying its swap is insufficient: ordinary mapping/context writes change its version, and another namespace can exhaust logical context-entry capacity after the successful terminal has escaped. Forcing a stale swap, retrying from raw inputs, or appending a failure after that terminal is forbidden. Pending reservations also need namespace pinning and explicit destruction order. The preliminary stage-split sketch was removed; `tokenization.rs` still matches its C2 hash. No publication fix is claimed.

The next implementation must stay inside the approved shared ownership table and the existing store: pre-release strict validation and budget/logical-capacity reservation, unpublished ownership, and allocation-free activation after release. It must preserve generation/scope/actual-attempt/policy semantics, cancel without publication, and never hold a database/store mutex across yields. Behavioral controls still need same-namespace writes, two pending publications, cross-namespace capacity contention, invalidation, duplicate records, and cancellation at every release yield, checking visibility and exact reservation/allocation baselines. A consumer cannot mint or activate completion from a boolean; sealing the strict verifier-issued non-Clone proof and private finalized publisher remains required alongside this work.

Current-state evidence: `c3-publication-red-full-source-sha256.txt` identifies 28 implementation/test files, and `c3-publication-red-source-sha256.txt` identifies seven focused files. `c3-publication-preserved-unified-list.log` lists 42 preserved tests; `c3-publication-preserved-unified.log` passes all 42 with an explicit `--skip c3_mandatory_sse_publication_`. This exclusion is diagnostic only, **not** a passing unified/security gate. Current `cargo check` and `cargo fmt --check` exit 0 in `c3-publication-red-cargo-{check,fmt}.log`. The corrected security/completion/Clippy evidence in Section 17 predates these two new tests; no current Full Gate or C3 completion is asserted.

C3 response-mechanics extraction, sealed proof/finalization/publication and complete split/budget/abort coverage remain open. C4 trusted ingress/actual-send/final-client orchestration and C5 executable non-Responses harness/Full Gate/independent CORE-1 review have not started. The authoritative architecture remains unchanged. Native implementation, route enablement and parallel-lane bases remain unauthorized by these partial results.

## 19. C3 documentation gate and publication refinement review

Independent documentation reviewer `8cfb87e8bc1248668e162b028b4ec058` returned **PASS for Sections 17-18 only**, covering fresh-reader clarity, mixed-audience execution readiness and source/log accuracy. The reviewer ran no builds/tests. Current source identity was confirmed by the integrating agent's read-only repository-root `shasum -a 256 -c` checks: seven focused and 28 implementation/test manifest entries all matched. Current `tokenization.rs` retains its C2 hash, and the authoritative architecture retains the Section 13 hash. Historical review conclusions and command flag/exit attribution were retrieved from the execution/agent records, not independently replayed by the documentation reviewer.

[Execution-plan Section 9](security-dlp-unified-core-parallel-implementation-plan.md#9-c3-publication-transaction-refinement--approved-execution-spec) proposed the bounded hidden-slot publication/proof refinement at this historical checkpoint. It was **not yet implemented or approved then**. Independent candidate reviewer `250986c4025a485897701c7da09506fb` returned **FAIL** for record-reference cancellation races, live duplicate admission, incomplete release sealing and exceptional cleanup details. The specification removes record references from pending owners, requires final-lock joins, explicit retirement-before-refund and namespace pinning, and defines sealed consuming verifier/finalization/delivery ownership.

Independent specification reviewer `88225e517d5846b4ad35ec1606bb3131` then returned **FAIL** with three required corrections: a Rust-legal opaque batch/delivery bridge, accurate metadata limits and cycle-free reservation transfer, and preserving unchanged JSON's mandatory charged final-encoding validation separately from original-byte delivery. Section 9 incorporates those corrections. Focused independent reviewer `821e66476ee24d7fbcce27259a0d7b0e` returned **PASS for corrected design/specification readiness only**, with no required correction. Both reviews were read-only, with no builds/tests or implementation changes.

The proposed P1-P4 tasks require behavioral controls for all authority readers, independent mapped/zero-token branches, concurrent writes/capacity/growth, duplicate cancellation/reuse, exact normal cleanup and retained backing, strict proof/batch identity, unchanged JSON encoding denial and exceptional post-terminal failure. Slow-consumer resource retention and poisoned-store reconciliation limits remain explicit. User approval is required before executing this refinement; its future implementation still needs source-bound GREEN and independent implementation review. The two publication tests remain RED, and C3-C5/CORE-1/native gates remain unchanged.

## 20. Approved publication refinement: P1/P2 checkpoint

**Historical P1/P2 checkpoint, superseded by Section 21:** P2 store controls were green while P3 sealing and the HTTP delivery boundary remained open. The user approved execution of plan Section 9 after the historical review in Section 19. Those approval-pending statements and Section 18's two-test/C2-source snapshot describe their earlier state. This checkpoint did not complete C3, C4-C5, CORE-1, or native-adapter acceptance.

P1 replaced the two compound publication probes with 96 independent cases in `src-tauri/src/proxy/security/tests/unified_core_tests.rs`: mapped and zero-token attempts; cursor, call, opaque, output-call-ID and strict/permissive mapping probes; unpolled cancellation; cancellation at each of five release yields; and completed positives. `c3-p1-publication-red-final.log` records 36 passing controls and 60 runtime failures, with none ignored. Independent read-only reviewer `0701de7b4fbc48d6885efd666b086d59` returned PASS for the P1 tests/evidence, not the implementation boundary.

P2 changes `src-tauri/src/proxy/security/tokenization.rs` to budget-owned hidden/visible slots in the existing store. Pending owners retain a slot serial, not a record reference. Final-lock duplicate joins, reserved logical capacity, per-owner metadata and expiry pins precede admission. Every authority reader filters visibility under the mutex. Activation transfers record charges once without a version/capacity retry; cancellation detaches the exact last-owner hidden record, destroys it outside the lock and then refunds its reservation. Visible records survive cancellation. Unavailable but unpoisoned state permits cleanup only; poisoned state stays closed and conservatively retains unreconciled record charges until store destruction. Retained slot capacity is physical ledger ownership, not logical record metadata.

The production publisher still immediately prepares and activates; only the test facade exposes pending ownership. The raw publication entry points and existing completion witness are not yet sealed. Therefore P2 does not fix the real HTTP wrapper's early publication, and does not satisfy the full Section 9 refinement.

### Behavioral provenance

All log names below are relative to the local verification directory in Section 2.

| Boundary                                                                | Runtime RED                                     | Subsequent GREEN                                    |
| ----------------------------------------------------------------------- | ----------------------------------------------- | --------------------------------------------------- |
| Hidden authority and owner lifecycle                                    | `c3-p2-visibility-red.log`: 2 passed, 16 failed | `c3-p2-visibility-green-first.log`: 18 passed       |
| Staged vectors destroyed before record reservation refund               | `c3-p2-refund-red.log`: 1 failed                | `c3-p2-refund-green.log`: 26 scoped controls passed |
| Hidden/visible duplicate admission needs only owner backing near budget | `c3-p2-duplicate-red.log`: 26 passed, 2 failed  | `c3-p2-duplicate-green.log`: 28 passed              |
| Growth retry releases obsolete replacement backing before reallocating  | `c3-p2-growth-red.log`: 1 failed                | `c3-p2-growth-green.log`: 29 passed                 |

A further 27 P2 controls are first-green characterization, not additional demonstrated RED/GREEN fixes. They cover deterministic overlapping cancellation and detached-slot reuse, payload-before-refund/pin ordering, unavailable cleanup, poisoned-state retention, removal/equal/conflicting insertion during candidate staging, serial exhaustion, owner-budget denial/recovery, overlapping index charges, nonempty final destruction, migration of visible authority/hidden owners, local pending-capacity recovery and allocation-denial cleanup. `c3-p2-bounds-first.log` has one test-authoring failure: it incorrectly expected physical slot bytes in logical `stats()` metadata. Reading `stats()` and `StoreVec` ownership confirmed the mismatch; only that assertion was corrected. It is not production-fix RED.

### Final-source verification

Repository-root commands and exact filter/exit attribution are recorded in `c3-p2-final-command-manifest.txt`. Each test group was listed first with nonzero matches, then run using `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1`, with no `--skip` or `--ignored` flags.

| Filter suffix                                                      | Listed | Result                                                      |
| ------------------------------------------------------------------ | -----: | ----------------------------------------------------------- |
| `proxy::security::unified_core_tests::c3_publication_transaction_` |     56 | 56 passed, 0 failed, 0 ignored; exit 0                      |
| `proxy::security::reversible_context_tests::`                      |     22 | 22 passed; exit 0                                           |
| `proxy::security::reversible_completion_tests::`                   |     88 | 88 passed; exit 0                                           |
| `proxy::protocol_matrix_tests::`                                   |      3 | 2 passed, 1 pre-existing installed-CLI test ignored; exit 0 |
| `proxy::session::tests::`                                          |     17 | 17 passed; exit 0                                           |
| `proxy::session_identity_tests::`                                  |     19 | 19 passed; exit 0                                           |
| `proxy::security::unified_core_tests::`                            |    194 | 134 passed, 60 failed, 0 ignored; exit 101                  |
| `proxy::security::`                                                |   1445 | 1385 passed, 60 failed, 0 ignored; exit 101                 |

`cargo check --manifest-path src-tauri/Cargo.toml`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` each exited 0. The known linker unwind-size warning remains nonblocking. The ignored matrix case requires an installed Gemini CLI; no installed-CLI acceptance is claimed. The security and unified groups are **not passing gates**: `c3-p2-final-failure-comparison.txt` confirms their 60 failures are the same P1 mandatory-SSE-publication failures as the earlier P2 diagnostic, with no additional failed test names.

`c3-p2-final-source-sha256.txt` covers 204 tracked/untracked Rust files under `src-tauri/src/proxy/`; its identity-check log confirms all entries match after the final runs. No source edits occurred between those runs and hashing. Full Gate, renderer checks, installation and live release checks were not rerun or claimed for this checkpoint.

Independent reviewer `c2b5146aa85e435b85b9d6c43c748f96` returned **PASS for read-only P2 production inspection**: no blocking transaction defect was found. The reviewer ran no commands and did not independently reproduce counts. Tests/helpers changed during review; the final 56-control state and integrated results above were verified by the parent, not independently accepted. Remaining useful stress coverage includes nonempty dependency/delivered-snapshot equality conflicts during final-lock races.

**Next:** execute approved P3 consuming proof/finalization/delivery sealing and turn the 60 real-wrapper publication failures green, then P4 integrated review. Preserve JSON's charged final-encoding boundary and unchanged bytes. Do not bypass the stream continuation, expose a raw pending publisher, enable native routes, or claim C3/CORE-1 from this checkpoint. C3 shared-mechanics extraction, C4-C5 and independent CORE-1 freeze remain required afterward. The tree remains unstaged at HEAD `2b00c4573098f4da31345c788aacf321ef685060`; no staging, commit, worktree, application launch or installation was performed.

## 21. P3 sealed proof and delivery: current checkpoint

**Historical checkpoint:** P3 was integrated with targeted GREEN and a read-only implementation PASS; P4 and overall acceptance were open. Section 22 supersedes this section's test counts, changed test-file hash and Full Gate/privacy status. The 60 real HTTP publication-boundary failures in Section 20 were no longer failures at this checkpoint. This is neither C3 shared-mechanics completion nor CORE-1 freeze. No native adapter is enabled.

### Implementation and review scope

Paths below are relative to `src-tauri/src/proxy/security/`:

- `http_restoration/completion_verifier.rs` owns real final-client Native/transport verification and consumes the original tracker state through the stable shell's `Option::take`. Observer `Arc` clones cannot prevent transfer. `VerifiedCompletion` is non-Clone, has private fields, and requires a budget-owned exact `AttemptProvenance` plus SHA-256 of length-prefixed raw input blocks, including comments, whitespace and DONE. Original protocol checks run even with zero mappings. There is no production boolean issuer.
- `response_restoration/finalization.rs` consumes state and proof, checks attempt/framing and strict snapshot/tool consistency, and alone constructs sealed publication specifications. The store immediately consumes the short borrowed specification while its backing is live, returning an owned pending obligation. The opaque batch owns charged final frames, metadata, lease, history and release identity. Only its stream continuation after the last yield can issue matching `DeliveryResume`; unpolled/drop paths cancel rather than publish. No mutex is held across yields, and HTTP cannot extract/reassemble frames with pending ownership.
- `tokenization.rs` uses the P2 hidden-slot transaction for production sealed publication. `completed_context.rs`/`tokenization.rs` raw publishers, boolean witnesses, legacy nonconsuming finalization and manual history-release compatibility paths are `cfg(test)` only. The release identity is charged correlation, not independent authority.
- JSON original-body verification and restoration precede charged final serialization for both changed and unchanged output. `StagedJsonResponse::encode_and_publish` validates the same staged source/binding, encodes, then prepares and activates the sealed slot synchronously. Unchanged delivery drops the validation encoding and keeps the original input bytes owner. No SSE-only delay is introduced for JSON.
- `completion_json.rs` preserves raw header/JSON observation pairing before classifying valid data-only Native SSE by its JSON `type`. xAI original-argument comparison, Anthropic fully-read JSON fallback and non-authoritative legacy projections remain in place. Post-terminal activation failure returns only `io::Error::other("Response stream failed.")`, without a second protocol terminal.

Reviewer `510d26e3ab6b4c47a8ff177230b1f34a` (Helix) returned **PASS for current P3 implementation inspection only**. The reviewer read source without edits, builds, tests, generated-log reads or process inspection; execution evidence belongs to the parent. The confirmed medium-severity finding P3-R1 was a production sibling seam: a bare state plus wrapper could accept real Native completion with no attempt (`None/None`). There was no identified remote trigger through the existing HTTP entry, which already bound attempts. The fix rejects missing attempt at verifier finish and makes proof ownership require an actual `AttemptProvenance`; consumption also requires a matching actual identity. The reviewer re-read that fix and found no remaining P3 blocker. JSON same-binding/same-source donor replacement was investigated, but no production proof extraction/substitution path was found; it is not a confirmed vulnerability.

### Behavioral provenance and test integrity

All named controls are in `tests/unified_core_tests.rs`; shared fixtures are in `tests/unified_test_support.rs`. The test-only bridge into the private verifier observes real raw/native blocks and EOF; it never constructs proof fields.

| Boundary                                        | Demonstrated runtime RED                                                                                                     | Current result                                                                                                                 |
| ----------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Publication must wait for delivery continuation | `c3_mandatory_sse_publication_mapped_wait_cursor`: `mandatory_authority_published_before_delivery_resume`                    | All 96 mandatory publication controls pass, including mapped/zero-token absence probes and cancellation at each release yield. |
| Activation unavailable after terminal release   | `c3_sealed_delivery_unavailable_activation_returns_only_sanitized_stream_error`: `unavailable_activation_silently_completed` | Fixed sanitized body-stream error; no second terminal.                                                                         |
| Valid data-only Native SSE compatibility        | `c3_sealed_client_sse_accepts_valid_data_only_native_frames`: `data_only_native_completion_rejected`                         | Raw observation pairing and valid completion preserved.                                                                        |
| Missing exact attempt, including zero mappings  | `c3_sealed_real_native_completion_rejects_missing_attempt_even_without_mappings`: `missing_attempt_real_completion_accepted` | Missing attempt rejected. Both mapped and zero-mapping branches pass.                                                          |

The additional donor, same-attempt wrong-protocol, wrong-attempt/batch/framing, consumed-tracker reuse, repeated Native terminal and exhausted-delivery controls are **first-green characterization of the sealing already implemented**, not additional claimed RED/GREEN fixes. Proof/state ownership prevents literal reuse after a consuming call; runtime controls exercise real tracker transfer, mismatched real donors and repeated polling instead of substituting a fabricated proof or compiler failure.

For P3-R1, an initial test-authoring compilation error (`RestoreError` is not `StdError`) was corrected before obtaining RED. The valid run listed one matching test and failed its behavioral assertion. After the fix, `c3_sealed_` listed seven tests and passed all seven serially. The first broad security rerun had 1449 passing and three failures: old reasoning fixtures lacked attempt identity. Only their shared test fixture was updated to create a charged correlation identity and install a requirement; no production fallback was added. The subsequent full security rerun passed. An erroneous `session_identity::` filter listed zero tests and stopped its command sequence; it is not evidence. The correct `proxy::session_identity_tests::` filter was then listed and passed.

### Current-source commands and results

Repository root, macOS arm64, existing Cargo cache; heavy Cargo commands were run serially by the parent. Every executed test group below was listed first with nonzero matches, then run with `--test-threads=1`. There were no `--skip` or `--ignored` flags. The command pair is:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
```

| FILTER                                           | Listed | Current result                                          |
| ------------------------------------------------ | -----: | ------------------------------------------------------- |
| `c3_sealed_`                                     |      7 | 7 passed; exit 0                                        |
| `proxy::security::unified_core_tests::`          |    201 | 201 passed, 0 ignored; exit 0                           |
| `proxy::security::`                              |   1452 | 1452 passed, 0 ignored; exit 0                          |
| `proxy::security::reversible_completion_tests::` |     88 | 88 passed; exit 0                                       |
| `proxy::protocol_matrix_tests::`                 |      3 | 2 passed, 1 existing installed-CLI test ignored; exit 0 |
| `session::`                                      |     17 | 17 passed; exit 0                                       |
| `proxy::session_identity_tests::`                |     19 | 19 passed; exit 0                                       |

The current security run includes the eight JSON restoration controls, unchanged wire-byte/header preservation, 96 mandatory publication controls, P2 lifecycle controls and prior C1/C2 behavior. The matrix covers existing local Responses/conversion JSON/SSE behavior, not new native adapter acceptance. `installed_gemini_cli_uses_chat_bridge` requires an installed CLI; its ignored result grants no acceptance for that cell.

The following current-source commands all exited 0:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
git diff --check
```

The known linker `__eh_frame` size warning persists. No source edits occurred after these checks and the hashes below; subsequent edits only update these evidence/plan documents. `pnpm exec prettier --check docs/design/security-dlp-unified-core-parallel-implementation-plan.md docs/design/security-dlp-unified-core-implementation-evidence.md` and post-document `git diff --check` also exited 0. This scoped document check is not the denied repository-wide `pnpm format:check`. Local tool records under `.voidx/tool-results/16843d10dd78/` contain sanitized command attribution: `call_cf57a52436b04f04a7ed3e8376451cad.txt` (current security, protocol matrix, session; stops at the invalid zero-match filter), `call_5b9bb706eeac4b76973e899c17d6e67e.txt` (correct session identity, unified/completion and Rust check/fmt/Clippy), and `call_f7c2ab660e854568acf55116a15da87c.txt` (superseded three-fixture-failure run). These local records are not portable CI artifacts.

### Source identity

HEAD remains `2b00c4573098f4da31345c788aacf321ef685060` on `main`, with the existing unstaged tracked/untracked work preserved. SHA-256 values from repository-root `shasum -a 256`, after the final Rust checks, identify the scoped P3 files rather than all repository inputs. The architecture document was not changed.

| Path below `src-tauri/src/proxy/security/` | SHA-256                                                            |
| ------------------------------------------ | ------------------------------------------------------------------ |
| `http_restoration.rs`                      | `7095756d513057b7e3ff8e7e1f06fd7d4b3ccefd57562f682d0eff7e9678cbc8` |
| `http_restoration/completion_verifier.rs`  | `18c6bd991c6296d446638b36f683cba1849c37f492389d7aac39fdd536b1a73d` |
| `response_restoration.rs`                  | `d4770afadb8f87ca5b44aa7877c437fbbab10865e4624728a6fba256b79c1f6f` |
| `response_restoration/finalization.rs`     | `252bb87a94c033543660ee65e974a50dcaf836d5e48fb5d2f180b83c2447743f` |
| `tokenization.rs`                          | `cc6c4949b8d4645864b2e727db44a9b096e2c1e119e7a042b339fd1d859c6961` |
| `completed_context.rs`                     | `5527a4e2936c2a289b314d6db11e76c500134ab79c24682e8fd37bb5c3b17d33` |
| `restoration_core.rs`                      | `e2f894364a4da5fe7b3aba17e41d271ee3c1bd1f02ca841e090d78de3bde2a51` |
| `completion_json.rs`                       | `2ccf0fafad10facf09aefdbf18f40d41fd6699ae01b991800a18b27fd53185df` |
| `tests/unified_core_tests.rs`              | `755fcd838b16876c17d5c6b95482d0882c22bd1781e256d3e6c732b83623197c` |
| `tests/unified_test_support.rs`            | `db2b94dee4ad1d48624467d5b678db3b11cdbd78ef1ae895baabbb1b870359eb` |
| `tests/json_restoration_tests.rs`          | `28ed21b2930953070bb46d9032ba0af9918eb5a0488774c2ec06283b58f560db` |
| `tests/reversible_response_tests.rs`       | `33a6610d9f49dea86c3d6b8912913271e328f35a8d2f580e53dad0ccc22a40ec` |

### Historical outstanding gates and safe continuation

- The attempted Full Gate batch at this checkpoint was denied (`User denied: bash`) before execution and was not retried by another route. After the user's subsequent instruction to continue, the parent executed the authorized Full Gate; Section 22 records the final results. The earlier denial is not a current blocker or evidence of execution.
- Supplemental production compile-time privacy checks had not run at this checkpoint. Section 22 records their subsequent positive control, 35 rejected probes and exact file restoration. Static checks supplement, not replace, real runtime donor controls.
- P4 evidence reconciliation was open here. Section 22 contains the updated requirement-to-control inventory, integrated-source checks and independent-review status. C3 shared-mechanics extraction, C4-C5 and independent CORE-1 freeze remain incomplete; do not start native adapters.
- Local continuation proves that the stream resumed after yielding, not remote receipt. Slow-consumer charged retention and poisoned-store conservative retention remain explicit limitations; no new second authority store or policy relaxation was introduced.

No staging, commit, push, worktree creation, application installation/launch, process inspection or credential reading was performed. Continue from these sources and existing queues/slots; preserve M0/C1/C2/P2 work rather than reverting the dirty tree.

## 22. P3/P4 integrated verification and publication-proof closure

**Current status (2026-11-06):** final integrated-source checks, the production privacy supplement, the P4 control inventory and independent scoped P4 PASS close **only the approved Section 9 publication/proof increment**. This section supersedes Section 21's outstanding Full Gate/privacy status and old test counts. It does not complete remaining C3 shared-mechanics extraction, C4-C5, CORE-1 freeze, native adapter enablement or overall feature acceptance.

### Change scope and behavioral provenance

The only permanent Rust change after Section 21 is `tests/unified_core_tests.rs::c3_sealed_allocation_denial_before_http_tool_release_cleans_up_and_recovers`. It exercises the actual HTTP SSE wrapper, real Native verifier, consuming finalizer and delivery path. The eight denial branches are mapped/zero-mapping × record `Shared`/index `Vector` allocation × `BeforeAllocation`/`AfterAllocation`. Each branch confirms the allocation hook was reached, no privileged tool/success terminal was released, the fixed failure projection was used, and cursor/call/opaque/output-call-ID/mapping authority remained unavailable. Request payload usage and logical/physical store usage return exactly to their stable baselines. Each denial is followed by a successful retry using the same admitted request and store, with authority restored only on success.

This is **characterization of existing fail-atomic behavior**, not a new production RED/GREEN claim. Two test-authoring failures were corrected without changing production code: `sealed_allocation_store_charge_not_refunded` compared against a baseline containing an attempt lease that was legitimately destroyed; `sealed_allocation_request_owners_retained` wrongly expected zero payload charge while the admitted request remained alive. The final test warms up mapping backing, drops the warmup attempt, then records stable logical, physical and payload baselines. Existing retained index capacity is not called a leak or expected to shrink. The valid runtime RED/GREEN boundaries remain those in Section 21's provenance table.

### P4 requirement-to-control inventory

All paths in this table are below `src-tauri/src/proxy/`. Control names without a file prefix are in `security/tests/unified_core_tests.rs`; grouped transaction controls use the indicated module prefix. These controls ran in the final 202-test unified / 1453-test security groups, except the explicitly separate protocol matrix.

| P4 requirement                                                         | Current runtime controls                                                                                                                                                                                                                                                                                                                                                                                                                                  | Production boundary inspected                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Allocation denial before privileged release; mapped and zero mappings  | `c3_sealed_allocation_denial_before_http_tool_release_cleans_up_and_recovers`: eight denials and eight same-request/store positive retries; existing `c3_publication_transaction_rejection_destroys_staged_vectors_before_record_refund`                                                                                                                                                                                                                  | `security/response_restoration.rs::finalize_and_flush_inner` completes strict validation, charged frame preparation and publication admission before returning the sealed batch; `security/tokenization.rs` cleans rejected candidates before refund.                                                                                                                |
| Retained and overlapping index backing                                 | `c3_publication_transaction_bounds::slot_growth_charges_both_indices_and_cancellation_retains_only_new_high_water`; `c3_publication_transaction_growth_retry_drops_obsolete_backing_before_reallocation`                                                                                                                                                                                                                                                  | Budget-owned replacement allocation charges old and new backing together; migrate current slots under lock, destroy displaced backing outside the lock. Cancellation retains reusable physical high-water backing.                                                                                                                                                   |
| Positive owner bookkeeping, owner-count and serial bounds              | `c3_publication_transaction_bounds::{hidden_duplicate_owner_count_is_bounded_by_metadata_budget,visible_duplicate_owner_count_is_bounded_by_metadata_budget,serial_exhaustion_rejects_new_record_but_preserves_existing_duplicate_owner}`                                                                                                                                                                                                                 | Hidden and visible joins charge each owner under the finite global metadata budget; checked owner/pin/serial arithmetic rejects overflow before release.                                                                                                                                                                                                             |
| Cycle-free destruction and ordered retirement                          | `c3_publication_transaction_capacity::final_store_destruction_releases_retained_backing_without_a_cycle`; `c3_publication_transaction_lifetime::mapped_nonempty_hidden_record_destruction_has_no_store_ownership_cycle`; `c3_publication_transaction_retirement::record_destruction_keeps_reservation_and_last_pin_until_payload_is_gone`                                                                                                                 | Slots hold plain charges, not store-owning reservations; pending owners hold no record reference. Detached payload destruction precedes refund and last-pin release outside the mutex.                                                                                                                                                                               |
| No privileged partial release or early authority                       | All 96 `c3_mandatory_sse_publication_*` controls: mapped/zero mappings, unpolled drop, cancellation at every release yield and completed positives; new sealed allocation control above                                                                                                                                                                                                                                                                   | `security/response_restoration/finalization.rs` owns complete frames/pending/history/lease; private continuation issues resume only after the last yield. Visible-only readers grant no hidden cursor/call/opaque/output-call-ID/mapping authority. No mutex spans a yield.                                                                                          |
| Fixed failure, headers and native byte/owner preservation              | `c3_sealed_delivery_unavailable_activation_returns_only_sanitized_stream_error`; `c3_sealed_client_sse_accepts_valid_data_only_native_frames`; all eight `security/tests/json_restoration_tests.rs` controls, including `json_final_serialization_capacity_failure_does_not_publish_cursor`, `reasoning_history_json_final_encoding_failure_never_publishes` and `json_http_success_publishes_only_prepared_output_and_retains_charge_through_last_clone` | Post-terminal activation failure emits only `Response stream failed.` as a body-stream error, never another protocol terminal. Changed and unchanged JSON perform charged final encoding before synchronous activation; unchanged discards that encoding and delivers the original input bytes owner. Unknown headers and immutable/opaque content remain preserved. |
| Current Responses / Compact conversion cells                           | `security/tests/reversible_integration_tests.rs::reversible_http_*` local native/chat/Anthropic/xAI JSON/SSE and Compact fixtures; `proxy::protocol_matrix_tests::` separately listed three, passed two, ignored one installed-CLI cell                                                                                                                                                                                                                   | Existing `handlers.rs` conversion/restoration exits and `response_processor.rs::process_codex_response`; no new native client route or adapter is enabled. The installed Gemini CLI cell has no acceptance from this run.                                                                                                                                            |
| Consuming completion/finalization/activation and exact attempt/framing | `c3_sealed_real_native_completion_rejects_missing_attempt_even_without_mappings`; `c3_sealed_real_donor_proof_rejects_wrong_attempt_and_framing`; `c3_sealed_real_original_verifier_rejects_same_attempt_wrong_protocol_and_reuse`; `c3_sealed_real_verifier_completion_and_delivery_are_one_shot`; `c3_sealed_real_verifier_rejects_repeated_native_terminal`                                                                                            | Private real verifier `finish(self)`, state `finalize(self, proof)`, batch `into_delivery_stream(self)` and pending `activate(self, resume)` consume their owners. Tracker transfer uses `Option::take`; no production raw publisher, boolean proof, or frames/pending extraction/reassembly is exposed.                                                             |

The JSON controls include changed/unchanged final-encoding denial, successful publication before body polling, original bytes/header preservation and temporary/last-clone owner cleanup. Runtime real donors and the four behavioral REDs are not replaced by the compile-time checks below.

### Production-only privacy supplement

The parent ran `.voidx/verification/p3-p4-20261106/privacy-probes.py` serially from the repository root. For each group, it temporarily appended a genuine sibling module to `security/mod.rs` and invoked:

```bash
cargo check --lib --manifest-path src-tauri/Cargo.toml --message-format=json
```

This is production configuration, not `cfg(test)`. A positive group using legal opaque consuming APIs exited 0. Eight negative groups exited 101 and all **35 distinct probes** were attributed to their expected source lines and error codes; repeated diagnostics do not increase the probe count.

| Negative group                                                  | Distinct probes | Required error |
| --------------------------------------------------------------- | --------------: | -------------- |
| Private verifier/finalizer issuer modules                       |               2 | `E0603`        |
| Private proof/spec/resume/pending/batch fields                  |               6 | `E0616`        |
| Absent production raw/fixture/legacy APIs                       |               7 | `E0599`        |
| Private raw prepare / inner finalization                        |               2 | `E0624`        |
| Absent boolean witness                                          |               1 | `E0433`        |
| Non-Clone opaque proof/spec/resume/pending/batch                |               6 | `E0277`        |
| Reuse after consuming proof/JSON proof/state/spec/pending/batch |               6 | `E0382`        |
| Absent boolean/constructor/assembly/extraction APIs             |               5 | `E0599`        |

The initial `raw_absence` authoring attempt referenced methods as fields and produced `E0609`; strict attribution stopped the script. It is not accepted privacy evidence or runtime RED. After switching to associated method lookup, all nine groups were rerun successfully. Each group and the outer `finally` restored `security/mod.rs` byte-for-byte; the restored SHA-256 is `f73e01726f2e98b312c6c303f781f8d1201f5861f0da692f9a36dd46356e533e`. No probe or production entry point remains. Compilation failures establish only the specified static boundary, never behavioral RED.

Local reproducibility: `python3 .voidx/verification/p3-p4-20261106/privacy-probes.py`; do not run Cargo, fmt or edits to `security/mod.rs` concurrently with this temporary-injection script. Its `privacy/` directory contains group `.rs.txt`, `.jsonl`, `.stderr.log` and `summary.json`. These local files are not portable CI artifacts.

### Final commands, counts and evidence reuse

Execution context: repository root `/Users/chikham/workspace/varyai`, macOS arm64, branch `main`, unchanged HEAD `2b00c4573098f4da31345c788aacf321ef685060`, existing Cargo cache. The parent serialized heavy Cargo commands. Every test filter and full test command was listed first with nonzero matches and then run with `--test-threads=1`. No `--skip`, `--ignored`, live-test flags or installed-CLI execution was used.

For every FILTER row below, the exact command pair was:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
```

| FILTER                                            | Listed | Final result                                            |
| ------------------------------------------------- | -----: | ------------------------------------------------------- |
| `proxy::security::unified_core_tests::`           |    202 | 202 passed; 0 ignored; exit 0                           |
| `proxy::security::unified_core_tests::c3_`        |    165 | 165 passed; 0 ignored; exit 0                           |
| `proxy::security::unified_core_tests::c3_sealed_` |      8 | 8 passed; 0 ignored; exit 0                             |
| `proxy::security::reversible_completion_tests::`  |     88 | 88 passed; 0 ignored; exit 0                            |
| `proxy::security::`                               |   1453 | 1453 passed; 0 ignored; exit 0                          |
| `proxy::protocol_matrix_tests::`                  |      3 | 2 passed; 1 existing installed-CLI test ignored; exit 0 |
| `proxy::session::tests::`                         |     17 | 17 passed; 0 ignored; exit 0                            |
| `proxy::session_identity_tests::`                 |     19 | 19 passed; 0 ignored; exit 0                            |

The full command pairs were:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml -- --list
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

The lib listing contained 5938 tests; final execution passed **5923**, with **15 existing ignored**, 0 failed, exit 0. The full listing contained 6101 tests across targets; execution repeated that lib result and passed **163 integration tests across 13 nonempty targets**, exit 0. Main/support targets contained zero tests and grant no extra coverage. The nonempty integration target counts were `app_config_load` 4, `app_type_parse` 2, `deeplink_import` 2, `hermes_roundtrip` 2, `import_export_sync` 26, `mcp_commands` 23, `profile_bundle` 32, `profile_roundtrip` 7, `prompt_live_sync` 1, `provider_commands` 11, `provider_service` 44, `proxy_commands` 2 and `skill_sync` 7. `prompt_live_sync` is an ordinary local fixture target, not an enabled external live-test mode. The ignored installed-CLI test is not accepted or counted as passed.

The following Full Gate commands exited 0 in this continuation:

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
pnpm format:check
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Renderer unit results were **254 files / 3064 tests passed**. These frontend checks preceded only the added Rust characterization test and temporary privacy injection/restoration; frontend source, dependencies and build inputs were unchanged. The final Rust commands and all final Rust test groups ran after that test and after probe restoration. Earlier non-`final-` Cargo logs with 5922 passing lib tests are superseded, not final-state evidence. Existing frontend/Node warnings and the linker `__eh_frame section too large` warning persist; the listed commands exited 0.

Sanitized local evidence is in `.voidx/verification/p3-p4-20261106/`: `final-cargo-{check,fmt,clippy}.log`; `final-{unified,c3,c3-sealed,completion}-{list,test}.log`; `final-proxy--{security,protocol_matrix_tests}---{list,test}.log`; `final-proxy--session--tests---{list,test}.log`; `final-proxy--session_identity_tests---{list,test}.log`; `final-cargo-{lib,all}-list.log`; and `final-cargo-test-{lib,all}.log`. Frontend execution is recorded in `.voidx/tool-results/16843d10dd78/call_99cb2c81f1cd4146adad93bd902baeff.txt`. Parent exit-code checking stopped on failure; later success was not used to hide an earlier failing command.

### Final source identity

The final source-manifest snapshot `.voidx/verification/p3-p4-20261106/final-source-manifest.json` contains 1332 file hashes covering `src/`, `src-tauri/src/`, `src-tauri/tests/`, `tests/` and the present package/lock/TypeScript/Vite/Vitest/Cargo/build entry files. Manifest SHA-256: `b51a05697bb1b483a30183d629407a0838e647cc131d0a2464e0d9cc3c3afe52`. This is a scoped source snapshot, not a hermetic environment/build attestation. It was taken after the final broad Rust runs; the later exact-filter reruns made no source changes.

Relative to Section 21's source table, every listed production file and test-support/JSON/reasoning file retains its recorded hash. Only `tests/unified_core_tests.rs` changed, to `58f51235041448315a7f6b1a0eb1e70d8b599bb174eb6049ca3c42eae4a0ec56`. Restored `security/mod.rs` has the hash above. The unchanged authoritative architecture `docs/design/security-dlp-unified-core-protocol-adapters.md` hashes to `13a1ea25967d570679cc94a0f61cc6257ddd0bd06243cef4ffeda8728e3a6166`. The dirty tracked/untracked tree and unstaged state are preserved; these hashes do not identify HEAD as the implementation.

### Independent review and acceptance boundary

P3 reviewer `510d26e3ab6b4c47a8ff177230b1f34a` (Helix) returned the scoped P3 implementation PASS recorded in Section 21. P4 reviewer `35fbb9e3218c410688f09f8888227ec8` (Onyx) inspected the integrated implementation and revised characterization test read-only, found no remaining runtime blocker, and returned **NEEDS_CHANGE for the then-incomplete evidence handoff**, not a request to alter production behavior. That historical result was not treated as PASS.

Final independent reviewer `fcbce0cff2784db480bb5938d5b006db` (Onyx, a separate isolated review run) re-read Section 9, Sections 21-22 and the current verifier/finalization/store/JSON/test paths after receiving the final results and source identities. It returned **PASS only for Section 9 P4's publication/proof increment**, with no blocking findings. It confirmed the consuming/private boundaries, last-yield activation, charged changed/unchanged JSON boundary, fixed post-terminal error, eight allocation denials/eight same-request retries and correct characterization provenance. The reviewer did not edit, run builds/tests, inspect processes, read credentials or generated logs, or independently recompute hashes. Commands/results/hash validation remain parent-owned evidence; this is independent source/evidence review, not an independent execution claim.

**Increment exit decision:** P1/P2 are preserved, P3 consuming proof/sealing is integrated, and P4 verification plus independent implementation/evidence review pass. Close this publication/proof increment only. After recording that verdict, the parent reran scoped document Prettier, `pnpm format:check`, `git diff --check` and the 1332-file source-manifest comparison; all passed without source changes. The fresh-reader clarity and execution-readiness checks pass: current/historical status, exact source paths, commands, counts, provenance and forbidden expansions are explicit.

Known limits remain: the continuation proves local stream resumption, not remote receipt; a slow consumer retains charged capacity; a poisoned store conservatively retains uncertain charges until destruction. No installation, signing, notarization, live-provider or installed-CLI acceptance is established. C3 shared-mechanics extraction, C4-C5, CORE-1 and overall feature acceptance remain open; native adapters stay disabled. No staging, commit, push, worktree creation, application installation/launch, process inspection or credential reading was performed.

## 23. C3/C4/C5 layered checkpoint and evidence cutoff

This documentation-only design-evidence handoff is for human maintainers and implementing/reviewing agents. It appends to, rather than rewrites, Sections 1-22. Section 22's P3/P4 publication/proof closure remains intact; its Full Gate is historical evidence for that increment, not a Full Gate for the later C3/C4/C5 changes.

**Cutoff:** the last accepted run in this appendix is `c3-barrier-adopt-foreign-budget-red`, recorded on 2026-11-06. All log and manifest names in Sections 23-28 are relative to `.voidx/verification/core1-20261106/`. Later edits or logs are outside this handoff, even if present when it is read. The directory name is only an evidence locator, not an acceptance verdict. No final source freeze is claimed.

**Decision at this cutoff:** record bounded C3/C4/C5 progress only. Responses lifecycle-facade integration is still runtime RED; the barrier's foreign-budget guard is still runtime RED; typed barrier work is in flight. Native SSE staging, finalization and publication remain incomplete. CORE-1 and overall feature acceptance remain open. Production native routing stays `NativeDisabled`.

The interpretation order is **observation -> staging -> finalization -> publication -> replay -> import**. These are distinct authority boundaries, not synonyms for a completed native lane:

| Layer        | Evidence available at the cutoff                                                                                       | What it does not establish                                                                                     |
| ------------ | ---------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Observation  | Exact borrowed JSON sites and SSE frames/entities, attempt/client correlation, bounded inspection and ownership checks | Protocol completion, proof issuance, delivery, mappings or publication                                         |
| Staging      | Native JSON text/dependency preparation with rollback and unchanged native siblings                                    | Delivery or completed-context visibility; no completed native SSE staging path                                 |
| Finalization | Private completion verification, async driver checks and private JSON sealing/encoding controls                        | A production native route, native SSE finalization, or publication merely from possessing an observation/stage |
| Publication  | Controlled native JSON publication/replay tests and preserved P3/P4 Responses publication/proof evidence               | Native SSE last-yield activation or remote receipt; P3/P4 is not reaccepted for later source changes           |
| Replay       | Exact visible native evidence, binding/client/policy/generation checks and Responses-authority denial                  | Automatic mapping import, authority from staging, or positive coverage for every native protocol               |
| Import       | Explicit controlled import of immutable dependencies from a visible record under the same native authority             | Responses cursor/call/opaque authority, implicit import, or production enablement                              |

## 24. C3 shared mechanics, lifecycle and unresolved runtime boundaries

### Extraction and characterization are not new production capability

Shared-mechanics work is located in `src-tauri/src/proxy/security/restoration_core.rs` and its `lifecycle.rs` submodule; the existing response-restoration facade remains a separate integration boundary. The `c3-{text,stream,barrier,snapshot}-extracted-{list,test}.log` records characterize extraction, including text ownership, UTF-8 splits, stream tails/snapshots and held-tool state. They are not RED/GREEN proof of a new native runtime. Separate `c3-{text,stream,barrier,snapshot}-atomic-{red,green}-{list,test}.log` records exercise failure atomicity and recovery. For stream atomicity, the executable RED record is `c3-stream-atomic-red-{list,test}-retry.log`; the earlier test log has no completed runtime result. These behavioral fixes must not be conflated with the pure extraction checkpoints.

Earlier passing unified/security/completion groups in `c3-c4-*-integrated-{list,test}.log` are intermediate regressions only. Neither those groups nor Section 22's historical Full Gate accepts the later lifecycle, facade or barrier changes.

### Passing lifecycle primitives versus failing facade integration

| Log stem                             | FILTER                                                                 | Listed / executed result           | Interpretation                                                                                     |
| ------------------------------------ | ---------------------------------------------------------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------- |
| `c3-responses-original-green`        | `proxy::security::c3_lifecycle_tests::c3_lifecycle_responses_original` | 4 / 4 passed, exit 0               | Original Responses observation controls at the lifecycle boundary                                  |
| `c3-lifecycle-original-green`        | `proxy::security::c3_lifecycle_tests::`                                | 30 / 30 passed, exit 0             | Lifecycle checkpoint, including the original-observation controls; not 30 plus 4 independent tests |
| `c3-lifecycle-hook-characterization` | `c3_lifecycle_rejection_hook`                                          | 4 / 4 passed, exit 0               | Synthetic rejection-hook characterization only                                                     |
| `c3-responses-integration-control`   | `c3_responses_integration_control`                                     | 4 / 4 passed, exit 0               | Mapped/zero-mapping JSON/SSE controls through the existing facade                                  |
| `c3-responses-integration-red`       | `c3_responses_integration_reject`                                      | 12 / 0 passed, 12 failed, exit 101 | Runtime failure of the required lifecycle rejection boundaries through the production facade       |

The 12 failures cover JSON/SSE mapped and zero-mapping rejection cases for text open/push and SSE hold boundaries. Passing private lifecycle controls or a functioning test hook does not establish that the facade consumes those boundaries. This is an executable production-integration RED, not a compile failure or a missing-test result. The lifecycle-to-Responses-facade integration is **not accepted** at this cutoff; no later GREEN is asserted here.

### Barrier adoption: narrowed but still RED

Both barrier checkpoints use FILTER `c3_tool_barrier_adopt_`:

| Log stem                              | Listed | Result                       | Boundary                                                                                                                                |
| ------------------------------------- | -----: | ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `c3-barrier-adopt-red`                |      4 | 1 passed, 3 failed; exit 101 | Historical RED: excess bytes, excess units and nonempty destination were rejected by the tests, not yet correctly by the implementation |
| `c3-barrier-adopt-foreign-budget-red` |      5 | 4 passed, 1 failed; exit 101 | Latest stable run at this cutoff: only foreign-budget adoption remains failing                                                          |

The latest four passing controls are the empty active destination/charge-preservation characterization and rejection of excess bytes, excess units and a nonempty destination without displacing owners. They are the four passing members of the five-test run, **not** an independently all-green barrier suite. The remaining failure is `c3_tool_barrier_adopt_rejects_foreign_budget_without_moving_owners`, with synthetic assertion label `c3_barrier_adopt_accepted_foreign_budget_owner`. No sensitive payload is needed to identify this failure.

The latest list exits 0 with five matches; execution exits 101; its full Rust/Cargo pre/post manifest is unchanged. Thus the residual RED is attributable to that stable run. Typed ownership/budget implementation remains in progress, not a recorded GREEN. The same log also contains unused-import and ignored-`Result` warnings; it is not evidence of warning-free Clippy or safe facade adoption of the new fallible interface.

## 25. C4 identity, dispatch and actual-route checkpoint

| Log stem                       | FILTER / listing-derived scope                | Listed / passed | Scoped coverage                                                                                                        |
| ------------------------------ | --------------------------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `c4-native-identity-green`     | `proxy::session::tests::native_restoration_`  | 17 / 17         | Strict explicit-native identity parsing, source order, duplicates, malformed values and no generated/fallback identity |
| `c4-session-green`             | `proxy::session::tests::`                     | 34 / 34         | Full session module at that checkpoint, including the strict-native subset                                             |
| `c4-dispatch-green`            | `proxy::security::native_route_scope_tests::` | 10 / 10         | Gemini registration/classification and direct/gateway dispatch controls; not ten new enabled native routes             |
| `c4-route-failover-integrated` | `proxy::security::native_route_scope_tests::` | 22 / 22         | Route-scope group including two actual-cell failover characterizations, active and zero mappings                       |

The 22-test result supersedes the earlier 20-test route checkpoint for this scoped group; these overlapping counts must not be summed. The actual-cell characterizations preserve supported Responses cells and reject the unsupported Gemini backup cell. Classification, explicit identity and direct/gateway scope separation do not grant native restoration admission or positive native-protocol completion.

The parent reports reviewer Quill (`63b8`) returned **scoped PASS** for the C4 checkpoint. Only the parent's historical review summary is available to this appendix; the full review artifact was not recovered. This is not represented as an independently rerun suite, a full review transcript, or acceptance of C3/C5 or production native enablement. The nine-file `c4-source-checkpoint.json` is a scoped historical identity, not an all-source pre/post freeze; its SHA-256 is `17b4fff71da600adbc3761cf03461aebac409dde859418b2a5ab4b11c2a690fc`.

## 26. C5 request, private verification, JSON and SSE progress

### Layer-specific passing evidence

| Log stem                                             | FILTER                                                                                                       | Listed / passed | Scope and limit                                                                                                                                                     |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `c5-request-green`                                   | `proxy::security::c5_request_contract_tests::`                                                               | 15 / 15         | Borrowed request contract, immutable ownership and bounded preparation; not a native runtime route                                                                  |
| `c5-native-primitive-private-characterization-v2`    | `proxy::security::http_restoration::completion_verifier::c5_native_completion_tests::`                       | 20 / 20         | Private verifier: original/client completion, actual EOF, exact digest/attempt/binding/client, sticky failure and budget cleanup; approved Chat fixtures only       |
| `c5-native-response-green-v3`                        | `proxy::security::c5_native_response_tests::`                                                                | 13 / 13         | JSON observation/staging and rollback, not publication merely from staging                                                                                          |
| `c5-native-finalization-private-characterization-v2` | `proxy::security::response_restoration::finalization::c5_native_finalization_tests::c5_native_finalization_` | 16 / 16         | Private JSON finalization: real proof/source checks, mandatory final encoding, unchanged original owner, changed charged owner and unpublished sealed specification |
| `c5-native-driver-current-characterization-v2`       | `proxy::security::http_restoration::native_driver::tests::`                                                  | 28 / 28         | Private async JSON/SSE driver: real EOF, terminal-not-EOF, stream errors, cancellation/drop, backpressure and source/client/attempt binding                         |
| `c5-native-sse-observation-green-v2`                 | `proxy::security::c5_native_sse_tests::`                                                                     | 21 / 21         | Borrowed SSE frame/entity observation, exact raw ownership/order, strict parsing, bounded sites/work and atomic refunds; explicitly not a completion validator      |

The private driver rejects unapproved pairs without polling. A private Chat JSON/SSE verifier or driver success is not positive Anthropic/Gemini coverage, an adapter completion issuer, or a production-lane acceptance. The observation suite checks finite frame/entity/site/work limits but does not establish protocol terminal semantics, proof authority, staging or delivery. Native SSE **staging -> finalization -> publication remains incomplete**, notwithstanding the 21 passing observation controls and 28 passing driver controls.

The parent reports reviewer Flux (`045ab`) returned **scoped PASS** for the request/private-verifier/JSON-staging/private-finalization checkpoint. The full artifact is unavailable to this appendix; this attribution is a historical parent summary, not a new complete review or execution claim. The scope excludes the later import review, unfinished native SSE stages, production enablement and overall acceptance.

### Privacy tightening, extraction and invalid attempts

Moving verifier/finalization controls behind their private module boundaries and re-running characterization records privacy tightening and retained behavior. It is not itself RED/GREEN for a newly enabled production runtime. Separate executable failure records must carry any new-behavior claim; filename suffixes or test names alone do not establish a result.

The first primitive characterization filter (`completion_verifier::native_tests::`) listed zero tests and is invalid coverage; `c5-native-primitive-private-characterization-v2` replaces it with the 20-test nonempty private scope. The first `c5-native-response-green` attempt did not compile, and `c5-native-response-green-v2` executed with 8 passed / 5 failed; only `c5-native-response-green-v3` records the 13-test passing JSON result. The later 16-test private-finalization characterization is distinct from the earlier 13-test private group. Compile/privacy checks, pure extraction and passing characterization are not substituted for the unresolved production-facade RED in Section 24.

### Native JSON publication, replay and explicit controlled import

The accepted replacement runs are:

| Log stem                                       | FILTER                                        | Listed / result        |
| ---------------------------------------------- | --------------------------------------------- | ---------------------- |
| `c5-native-import-current-review-v2`           | `proxy::security::c5_native_evidence_tests::` | 19 / 19 passed, exit 0 |
| `c5-native-import-current-characterization-v2` | `proxy::security::c5_native_import_tests::`   | 4 / 4 passed, exit 0   |

The 19-test evidence group distinguishes staged data from delivery/import, checks JSON replay visibility before the first body poll, preserves that visibility on unpolled JSON-body drop, rejects ordered-snapshot mutations and foreign scope, and denies Responses cursor/token/output-call authority even for zero mappings. The four import characterizations cover complete dependencies with overlap/idempotence, exact handle fields and evidence variant, expired-generation rejection and late allocation failure with atomic retry. These are controlled explicit-native tests, not a production native route or native SSE publication proof.

The v1 `c5-native-import-current-{review,characterization}` attempts encountered build-directory lock waits and lifecycle-symbol compilation errors. The review listing matched 19, but its execution did not compile; the characterization listing also failed compilation. They are invalid behavioral evidence, not a runtime RED. The v2 pairs above replace them, with complete unchanged pre/post source manifests.

The parent recovered the complete result from reviewer Rune (`f7027`): **PASS only for explicit-native controlled import**, with no blocking finding. The review confirmed the v2 19+4 results, unchanged manifests and matching four scoped file hashes below. It checked full binding, visible record and live immutable dependency owner; staging outside the lock followed by locked final revalidation before swap; no lock across a yield; and allocation-free zero-mapping/idempotent imports. Nonblocking runtime gaps remain: forced final-revalidation failure, budget-ceiling exhaustion and successful-import Drop/refund. The review does **not** establish positive Anthropic/Gemini/SSE coverage or CORE-1 acceptance. The complete review result was relayed by the parent; a local standalone review artifact or verbatim transcript is not claimed here.

| Path below `src-tauri/src/proxy/security/` | SHA-256 in both v2 import manifests                                |
| ------------------------------------------ | ------------------------------------------------------------------ |
| `completed_context.rs`                     | `4bf17536032aeb660837fa7065cc896ca119c28c39e3689cd104157c6fec57fc` |
| `tokenization.rs`                          | `42300123358ee3bc8d54870322d6250b104b582e5cb8633eefdc849df674f581` |
| `tests/c5_native_evidence_tests.rs`        | `221312f715f65442b6c12806cef5a0bc61cd345c70ed954e5ede385b919a160b` |
| `tests/c5_native_import_tests.rs`          | `823c1791d93644f73a020de89068ed9c3b2bddbe881ee2cf101ac7054fade4b8` |

## 27. Commands, source provenance and non-transferable results

### Recorded execution protocol, not commands run by this documentation pass

The root-relative Cargo list/run protocol is:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
```

Use the FILTER in the corresponding table. Accepted evidence pairs have a successful nonzero listing before execution, a single test thread and no `--skip`. Header-bearing logs record the root working directory, complete command and exit code. Older C4/request/JSON logs lack those command headers: their table scopes are reconstructed from the listed names, with nonzero listing and runtime result directly observable; no missing shell transcript is invented. Overlapping subsets and successive checkpoints are not additive test totals. Failed/zero-match/compile-blocked attempts are retained as failures or invalid evidence, never silently counted as passing tests.

`run-focused.py` documents the later runner's exact scope: every `src-tauri/src/**/*.rs` file plus `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock`, hashed before the listing and after execution; zero matches or source drift prevent acceptance. It was read, not executed, for this handoff. Each full-scope manifest below contains **495 identities: 493 Rust source files plus the Cargo manifest and lockfile**, with identical pre/post maps. All recorded counts and exits were checked against their actual list/test logs.

| Source manifest                                            | Manifest SHA-256                                                   |
| ---------------------------------------------------------- | ------------------------------------------------------------------ |
| `c5-native-import-current-review-v2-source.json`           | `af45e5d062b585a42ec67b454b4ab86c1058c9a98b675de2ad27af35ea81806f` |
| `c5-native-import-current-characterization-v2-source.json` | `24dccf92733daa9b3113924225e750c18eaf14abac4b8ab9b8df2e168a3799f7` |
| `c5-native-driver-current-characterization-v2-source.json` | `80d6ff664a206048e4f03af7923c2413318e5a961dfe35252cbd972be6aec895` |
| `c5-native-sse-observation-green-v2-source.json`           | `7ccf2db618d1281ed7543b01ace5ae7bcb9449de758279d480c7eb270ee22417` |
| `c3-lifecycle-hook-characterization-source.json`           | `1f906366ddd0b1c5b69cb72239ec34eccf031f510c8bfaa1610687a6b53c0264` |
| `c3-responses-integration-control-source.json`             | `f3297c9b01227b2849affe5d73df4b9b27414dabd83363f30cdb77c8ecf20a48` |
| `c3-responses-integration-red-source.json`                 | `548b9e0721bfdd814c3f5d33c17c95e5d1600a4f33d2c3fdc60adfa70e0c80cb` |
| `c3-barrier-adopt-red-source.json`                         | `380290259983edda16f6bdcce40697d17afd5d619fad86e7de326054a00b55e7` |
| `c3-barrier-adopt-foreign-budget-red-source.json`          | `f08436762e6d792ad952267bf911f73dd9a6ae68668a6d0c0c04f0a4db539e31` |

The two v2 import maps are identical to each other; the driver, SSE observation, hook and facade control/RED manifests share that same map. Barrier adoption has its own later stable maps. Stability within a run is not stability between checkpoints or a final integrated freeze.

Earlier artifacts are intentionally weaker: `c3-{responses,lifecycle}-original-green-source.json` have 107 scoped identities and equal pre/post maps, not all Rust sources; the private primitive/finalization v2 `*-source.json` artifacts are single 216-file proxy-source snapshots, not complete pre/post manifests. The nine-file C4 snapshot likewise cannot attest unrelated inputs. Their results remain attributed to those checkpoints; later full-scope manifests do not retroactively turn them into full-scope or current-tree verification.

Read-only comparison during this documentation pass already found Rust additions and changes after the accepted full-scope snapshots, including lifecycle/native-response/core and unified-test work. The four import hashes above still matched when checked, but that limited match does not make the whole tree unchanged or rerun any test. Therefore **neither checkpoint GREEN nor checkpoint RED is asserted as a fresh execution result for the live tree**. The latest accepted recorded runtime outcomes remain the 12-failure facade RED and one-failure barrier RED; no inferred or anticipated GREEN is admitted. The parent owns the subsequent all-source pre/post comparison, reruns, final freeze and any new append.

## 28. Fresh-reader and execution-readiness handoff

The documentation review uses the mixed human+LLM design-evidence checks below; this is a documentation-only TDD exception, not an implementation or runtime-verification cycle.

- **Fresh-reader scope:** Section 23 identifies the cutoff and non-goals before the detailed tables. Readers can distinguish inherited P3/P4 closure, C3 extraction/characterization, privacy tightening, scoped C4/C5 progress, invalid attempts and executable production RED. Review summaries and the recovered Rune result have explicit provenance and limits.
- **Execution readiness:** Each counted group has a log stem and FILTER, commands are root-relative, failing exits and source-manifest coverage are explicit, and unfinished authority transitions are named. A future implementer must not skip the facade/foreign-budget failures, substitute private characterization for integration, or enable native routing from these results.
- **Source integrity:** Full-scope pre/post identities were checked for the later runs; earlier partial snapshots and live-tree drift are disclosed. This appendix deliberately supplies no final all-source freeze or current Full Gate. Existing plan, architecture, source and historical evidence sections are not revised.
- **Privacy and operations:** Only synthetic test labels and source identities are recorded. This documentation pass runs no Cargo, renderer build, tests, Prettier, process inspection, credential access, installation or Git writes. The previously recorded commands are evidence descriptions, not operations performed by the document editor.

**Documentation-quality self-review: PASS for fresh-reader clarity; PASS for execution readiness of this evidence handoff.** These verdicts cover documentation scope, provenance and actionable boundaries only, not implementation, runtime, release or whole-feature acceptance.

**Continuation boundary:** keep production native adapters `NativeDisabled`. Complete and verify Responses-facade lifecycle integration and foreign-budget/typed barrier adoption first; complete native SSE staging/finalization/publication with distinct authority and delivery evidence; retain Rune's explicit import limits; then obtain the parent-owned integrated source freeze, relevant regressions/Full Gate and independent acceptance. None of those future results is preapproved by this appendix. After append-only preservation, log/hash/link checks and the two document-quality checks, this editor freezes its document changes; any later evidence belongs in a new parent-owned append.

## 29. Current C3 buffered-to-SSE ownership and cancellation increment

**Status at this checkpoint: scoped PASS for unique buffered-to-SSE ownership and pre-handoff cancellation cleanup only. C3, C4, C5 and CORE-1 remain open.** This appendix adds fresh parent-executed evidence; it does not rewrite the historical cutoffs in Sections 23–28 or extend Section 22's P3/P4 acceptance. Full Gate was explicitly not authorized for this turn and was not run.

### Implementation and authority boundary

- `src-tauri/src/proxy/handlers.rs` creates converted SSE evidence only for a streaming request or an actual SSE response. Buffered Anthropic JSON and unmarked SSE that will produce SSE use `observe_fully_read_anthropic_body` on the one SSE inspection before conversion. Final JSON still uses `verify_upstream_json_completion` / `verify_upstream_sse_completion`; it does not create an unused SSE lifecycle.
- `src-tauri/src/proxy/security/http_restoration.rs` caches one observer tracker per prepared attempt. `PreparedSseEvidence` owns cancellation cleanup, not restoration or publication authority. Attempt/handler drop and early wrapper exits destroy both inspection and original completion state even when an external tracker `Arc` survives. Successful SSE handoff transfers cleanup into the returned stream. Poisoned state is recovered only for destruction, not as evidence. Existing `LentSseInspection` handles cancellation while the sole lifecycle is lent across the upstream poll.
- Buffered SSE shares the existing frame delimiter, strict JSON parsing, completion and EOF checks through `observe_buffered_sse`. The actual inspection observes original frames/JSON, never converted values. `response_restoration.rs` retains full binding, pair and attempt checks on handoff. There is no new authority store, raw-authority access, mutable dependency ledger or publication capability. Native route registration remains Disabled.
- `restoration_core/lifecycle.rs` adds only a `cfg(test)` thread-local successful-begin counter. Real router controls assert exactly one begin without weakening existing tool, type, redaction, completion, publication or budget assertions. `tests/json_restoration_tests.rs` adds the new private cleanup field to its existing fixture; its assertions are unchanged.

### Test-first evidence and invalid attempts

All stems below are relative to `.voidx/verification/core1-20261106/`; each accepted test pair has `-list.log`, `-test.log` and `-source.json`.

| Accepted RED stem                   | Nonzero runtime result                       | Target boundary                                                                                                                                                   |
| ----------------------------------- | -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `c3-fallback-owner-red`             | Listed 2; 0 passed / 2 failed; test exit 101 | Real mapped/zero Anthropic JSON-to-SSE routes pass their prior behavior assertions, then fail `c3_stream_created_shadow_lifecycle`: 2 begins instead of 1         |
| `c3-early-owner-red-corrected`      | Listed 5; 0 passed / 5 failed; test exit 101 | Attempt drop, unpolled wrapper future, poisoned attempt drop, encoded response and upstream-error exits strand the inspection while the external tracker survives |
| `c3-buffered-sse-owner-red-runtime` | Listed 2; 0 passed / 2 failed; test exit 101 | Real valid mapped/zero unmarked Anthropic SSE-to-SSE routes fail `c3_stream_valid_original_rejected` before the same-owner buffered observation is connected      |

Each accepted RED has list exit 0 and unchanged covered sources. The earlier `c3-early-owner-red` tuple-shape compilation failure and `c3-early-owner-red-runtime` admission-fixture failures are **not** behavioral RED. The initial `c3-buffered-sse-owner-red` attempt did not compile because an existing fixture lacked the new private field; it is also invalid RED. The initial `c3-owner-current-transaction` filter listed zero tests and ran none; only the corrected transaction scope below counts. These failed attempts remain preserved.

### Current frozen-source verification

The parent ran Cargo serially from the repository root with `CARGO_BUILD_JOBS=2`, `rustc 1.98.1 (48a229cea 2026-09-01)` and `cargo 1.98.1 (797e8a9bc 2026-08-05)`, at HEAD `2b00c4573098f4da31345c788aacf321ef685060` with the existing dirty tree preserved. No ignored/live tests or application launch/install was used. The exact command protocol is:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER
# The runner first lists, then executes the nonempty scope:
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
# Production check, separately logged:
CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml
```

For the table, each stem starts with `c3-owner-current-`. Passing scopes have list/test exit 0, nonzero listing and 0 ignored; completion has list exit 0 / test exit 101. Counts overlap and must not be summed.

| Stem suffix             | FILTER                           | Listed / runtime result                                 |
| ----------------------- | -------------------------------- | ------------------------------------------------------- |
| `cleanup`               | `c3_early_inspection_`           | 9 / 9 passed                                            |
| `fallback`              | `c3_stream_fallback_router_`     | 2 / 2 passed                                            |
| `buffered`              | `c3_stream_buffered_sse_router_` | 2 / 2 passed                                            |
| `stream-router`         | `c3_stream_router_`              | 8 / 8 passed                                            |
| `json-router`           | `c3_json_router_`                | 32 / 32 passed                                          |
| `preconversion`         | `c3_json_preconversion_`         | 28 / 28 passed                                          |
| `facade`                | `c3_responses_integration_`      | 48 / 48 passed, including the 28 preconversion controls |
| `lifecycle`             | `c3_lifecycle_tests::`           | 34 / 34 passed                                          |
| `transaction-corrected` | `lifecycle::transaction_tests::` | 6 / 6 passed                                            |
| `deferred`              | `c3_deferred_units_tests::`      | 10 / 10 passed                                          |
| `native-stage`          | `c5_native_sse_staging_tests::`  | 13 / 13 passed; staging only, not finalization          |
| `unified`               | `unified_core_tests::`           | 216 / 216 passed                                        |
| `http`                  | `reversible_http_`               | 65 / 65 passed                                          |
| `diagnostic`            | `reversible_restore_diagnostic_` | 5 / 5 passed; overlapping completion/response coverage  |
| `completion`            | `reversible_completion_`         | 88 / 87 passed, 1 failed                                |

`c3-owner-current-check.log` and `c3-owner-current-check-source.json` record production check exit 0. Existing production unused-import warnings and the test linker/unwind warning remain; this is not warning-free Clippy acceptance.

The nine cleanup controls cover attempt drop, an unpolled wrapper future, an unpolled returned body, Pending stream cancellation, repeat tracker acquisition, poisoned attempt/wrapper, encoded response and unchanged upstream-error status/body. They assert state destruction, no completed/pending publication and payload budget zero after real owners drop while the external tracker remains alive. They are direct attempt/Response tests, not nine additional real-router cancellation tests.

### Source identity and review

Every current accepted runner manifest and the production check has identical before/after maps, and those maps equal each other: **498 identities**, comprising 496 Rust files under `src-tauri/src/` plus the Cargo manifest/lockfile. The canonical sorted compact-JSON map SHA-256 is `49a011ccecd3d4dac9ea1989577a95bd9127c1160d204b8ec6246d13b7d7ddf3`.

`c3-owner-current-freeze.json` additionally records all 513 project Rust/Cargo identities, the per-run results and the delta from `c3-json-current-scoped/source-before.json`. All 513 paths are common; eight changed, with no added/missing path: `handlers.rs`, `security/http_restoration.rs`, `security/http_restoration/completion_verifier.rs`, `security/protocol_adapters/responses.rs`, `security/response_restoration.rs`, `security/restoration_core/lifecycle.rs`, `security/tests/json_restoration_tests.rs` and `security/tests/reversible_integration_tests.rs` (all below `src-tauri/src/proxy/`). The 15 files outside the runner are `src-tauri/build.rs` and 14 Rust integration-test/support files; their present hashes equal the earlier 513-source snapshot, but they were **not** hashed around each current test run. Source-count differences alone do not imply deletion or change. Freeze-manifest SHA-256: `3ce2a77ef62effc7a5fac7ab81032bb1c73f80fc7ad99715249f775dcbe7cd97`.

Independent reviewer Cipher (`aef0233d3f09487e954830fed7110343`) returned **scoped PASS** after reading current code and frozen-source execution evidence. The complete returned result is preserved as an explicitly labeled English translation in `.voidx/verification/core1-20261106/c3-owner-cipher-aef0233-review.json`, with its source identity and exclusions. It checked the actual buffered output branch, single cached tracker, cleanup transfer, poisoned/external-Arc destruction and retained identity isolation. It used only read/search APIs: no commands, edits or delegation. This addresses Prism's prior duplicate-owner and pre-handoff cleanup findings **only in this increment's scope**; it does not promote the older whole-streaming review to overall C3 PASS.

### Remaining blockers and non-goals

- `text_fallback_review_transient_kind_copy_is_reserved_before_parse` still fails at `src-tauri/src/proxy/security/tests/reversible_completion_tests.rs:5008`: left 0 / right 96. The baseline is taken after synthetic stream-owned identities exist, while the final assertion follows their destruction. Neither the assertion nor identity charges were weakened. The synthetic projection initialization contract still needs a separate test-first resolution; completion is not green.
- The complete converted-SSE identity, sticky rejection, multi-poll, budget and final-yield continuation matrix is not accepted by this narrow review. Existing JSON pre-poll publication and SSE tests do not stand in for the missing cases.
- xAI rewrite-before-restoration original observation, sealed consuming native SSE finalization/restored-wire revalidation and multiline/per-yield coverage remain incomplete. Native staging success grants no route enablement.
- No Full Gate, renderer verification, Clippy, installer or release acceptance is claimed. P3/P4 and existing unrelated changes remain preserved. Close C3/C4/C5 and CORE-1 only after their own current-source validation and corresponding independent PASS.

**Documentation self-check:** mixed human/LLM audience; scope and failure are stated before acceptance tables, every accepted scope has a reproducible command/filter and source-coverage limit, and unresolved implementation boundaries are explicit. This appendix is an execution-evidence record, not a new implementation or route-enablement authorization.

## 30. Converted SSE reclaim failure is terminal (scoped acceptance)

**Scoped PASS only: the reclaim failure transition and 36 owner-level controls. C3/C4/C5/CORE-1 remain open.** This record follows Section 29 without widening its earlier acceptance. Audience: maintainers and implementing/reviewing agents. No Full Gate was authorized or executed.

### Defect, behavioral RED and minimal change

`SseRestorationState::reclaim_original_inspection` previously returned identity/observation errors without retaining failure in the stream, and did not reject an already-failed receiving state. The parent added `src-tauri/src/proxy/security/tests/c3_converted_sse_owner_tests.rs`, registered privately under `response_restoration.rs`, before changing implementation. `c3-sse-owner-red` listed 36 tests (list exit 0), then ran 4 passing valid controls and 32 runtime failures (test exit 101): 30 failed `c3_sse_owner_handoff_rejection_not_sticky`, and two failed `c3_sse_owner_invalid_handoff_accepted`. Compilation succeeded, the fixtures reached the intended boundary, and covered sources stayed unchanged; these are accepted behavioral RED, not donor or compilation failures.

`src-tauri/src/proxy/security/response_restoration.rs:1848–1872` now rejects existing failure/finalization and conflicting owners before validating the full binding, protocol pair, attempt and successful original observation. Any rejection clears pending delivered history/publication and records sticky failure. The consumed invalid inspection is destroyed; no replacement lifecycle, authority store, raw authority, mutable dependency ledger or publication capability is introduced. Legal multi-frame observation and subsequent polls without new original input remain valid. The direct-native Responses guard and native Disabled registration are unchanged.

The 36 controls use real prepared bindings with nonempty operation lineage, but synthetic owner-level attempt provenance and handoff. Four Chat/Anthropic × mapped/zero controls exercise repeated lending/reclaiming, including polls without new original frames. Other cases cover client/upstream/attempt, store/namespace/generation/policy/provider/principal/operation mismatches, unobserved handoff, foreign original attempt/upstream, later observation rejection, capacity rejection and already-failed state. Existing assertions were not weakened. Tests additionally assert one lifecycle, payload budget zero after real owners drop, and unchanged publication counts.

### Current-source execution and provenance

The parent executed Cargo serially from the repository root with `CARGO_BUILD_JOBS=2`, on the same HEAD/toolchain as Section 29, preserving the dirty tree. Evidence is under `.voidx/verification/core1-20261106/`. The accepted RED stem is `c3-sse-owner-red`; initial GREEN is `c3-sse-owner-green` (36/36). After formatting only the new test file, current evidence uses the following stems. Run each with `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER`; the runner saves list/test logs, actual exit codes and before/after source hashes. Counts overlap and must not be summed.

| Current stem                         | FILTER                           | Result                                 |
| ------------------------------------ | -------------------------------- | -------------------------------------- |
| `c3-sse-owner-current-owner`         | `c3_sse_owner_`                  | 36/36                                  |
| `c3-sse-owner-current-stream-router` | `c3_stream_router_`              | 8/8                                    |
| `c3-sse-owner-current-fallback`      | `c3_stream_fallback_router_`     | 2/2                                    |
| `c3-sse-owner-current-buffered`      | `c3_stream_buffered_sse_router_` | 2/2                                    |
| `c3-sse-owner-current-json-router`   | `c3_json_router_`                | 32/32                                  |
| `c3-sse-owner-current-cleanup`       | `c3_early_inspection_`           | 9/9                                    |
| `c3-sse-owner-current-facade`        | `c3_responses_integration_`      | 48/48                                  |
| `c3-sse-owner-current-unified`       | `unified_core_tests::`           | 216/216                                |
| `c3-sse-owner-current-http`          | `reversible_http_`               | 65/65                                  |
| `c3-sse-owner-current-completion`    | `reversible_completion_`         | 88 listed; 87 passed / 1 known failure |

All scopes have nonzero listings, list exit 0, unchanged covered sources and 0 ignored; passing test exits are 0, completion exits 101. `c3-sse-owner-current-check.log` / `-check-source.json` record `CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml`, exit 0 with five existing warnings. The multi-scope shell's final exit 0 did **not** mean completion passed; per-run manifests/logs retain its exit 101.

`c3-sse-owner-current-freeze.json` SHA-256 is `2c261172ce814f0eada9f4253b75ec92fc7844a0112ba473be6f8b499fe8bd28`. Its 514 inputs add only `tests/c3_converted_sse_owner_tests.rs` and change only `response_restoration.rs` relative to Section 29's freeze; no source path is missing. Each current runner and production check covers the same 499 inputs (497 Rust files under `src-tauri/src/` plus Cargo manifest/lock), with equal before/after maps. Canonical sorted compact-JSON map SHA-256: `a1026860796eb91814d8cdf43427caf056ae4b9f7673dfff0ba4710557849d05`. The other 15 files remain freeze-only identities, not per-run coverage. A fresh parent hash check before acceptance found no drift among all 514 inputs.

### Independent verdict and limits

Helix (`627dbbf3136140fa936321ab7e9cbd95`) returned **scoped PASS**, with no mandatory change in scope, after read/search inspection of current code and the RED/GREEN/current logs. The complete returned result is archived as a labeled English translation in `.voidx/verification/core1-20261106/c3-sse-owner-helix-627dbbf-review.json`. The reviewer ran no commands, edits or delegation. An earlier Flux run (`2807a05164be4df089ecb5d5c122f9cd`) could not be retrieved (`Unknown run`); no acceptance is attributed to it.

Limits remain explicit: nonempty pending cleanup, finalized-state rejection and owner-conflict branches have code-review evidence but not separate dynamic cases in these 36 tests. Owner controls do not establish real HTTP final-yield activation or the full converted-SSE continuation/capacity matrix. Completion still fails the unchanged `text_fallback_review_transient_kind_copy_is_reserved_before_parse` assertion at `reversible_completion_tests.rs:5008`, left 0 / right 96. xAI pre-rewrite same-owner observation, synthetic projection lazy initialization, sealed native SSE finalization and the remaining C4/C5/CORE-1 requirements are not closed. No native route enablement, warning-free Clippy, Full Gate or release acceptance is claimed.

**Documentation quality checks:** fresh-reader clarity PASS (scope, defect, commands and blockers are explicit); execution-readiness PASS (exact paths/filters, source identities, unchanged authority boundaries, prohibited acceptance expansion and per-run limitations checked against saved evidence). These are documentation verdicts only.

## 31. xAI pre-rewrite same-owner observation (scoped acceptance)

**Scoped PASS only: original-frame observation through the existing mechanics owner and four real-router controls. C3/C4/C5/CORE-1 remain open.** Audience: maintainers and implementing/reviewing agents. This increment does not close the broader continuation/cancellation matrix, enable native routes, or establish Full Gate acceptance.

### Behavioral RED, minimal integration and unchanged boundaries

Before the production changes, `c3-xai-router-red` listed four tests and ran two passing controls plus two runtime failures at `c3_xai_original_rejection_did_not_stop_completion` (list exit 0, test exit 101). Both mapped and zero-mapping streams still completed despite rejecting `ObserveOriginal`. Compilation and the two controls succeeded; this is behavioral RED, not a fixture/compiler failure.

The production change has only two sites under `src-tauri/src/proxy/security/`: `http_restoration.rs:3239–3244` calls `observe_inspection_frame(raw)` before xAI original completion validation; `response_restoration.rs:1826–1831` includes `Responses + XaiWholeFloat` in the existing poll-scoped mechanics-owner lending condition when original evidence exists. Original frames are observed before legacy xAI framing/JSON repairs. The owner is reclaimed through the existing identity/sticky-failure checks. There is no second lifecycle or authority store. Direct-native input observation, attempt provenance, conversion behavior, sealed finalization/publication and native Disabled registration are unchanged.

`tests/reversible_integration_tests.rs:15227–15407` adds mapped/zero × control/original-rejection cases through the actual `ProxyServer` router, an in-memory database and a dynamic-port loopback upstream. Seven-byte chunks, CRLF framing and synthetic UTF-8 exercise the existing rewrite. Assertions cover whole-float-to-integer arguments, restored tool identity/content, no reference exposure, one lifecycle, no publication before body polling, expected publication after collection, rejection without tool/completion/private-text release, and payload budget zero after owners drop. Existing assertions were not weakened. Initial `c3-xai-router-green` and formatted current-router runs both passed 4/4.

### Current-source evidence

All artifacts below are under `.voidx/verification/core1-20261106/`. Reproduce a table row from the repository root using `python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER`. The runner lists first, requires nonzero selection, runs `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1`, and records actual exits plus before/after source hashes. The second correction/check batch used `CARGO_BUILD_JOBS=2`; earlier invocations did not explicitly set it. Counts overlap and must not be summed.

| Current stem suffix (`c3-xai-current-`) | FILTER                                            | Result                                   |
| --------------------------------------- | ------------------------------------------------- | ---------------------------------------- |
| `router`                                | `c3_xai_stream_router_`                           | 4 passed                                 |
| `xai-raw`                               | `p1_acceptance_xai_raw_completion`                | 1 passed                                 |
| `owner-v2`                              | `c3_sse_owner_`                                   | 36 passed                                |
| `stream`                                | `c3_stream_router_`                               | 8 passed                                 |
| `fallback`                              | `c3_stream_fallback_router_`                      | 2 passed                                 |
| `buffered`                              | `c3_stream_buffered_sse_router_`                  | 2 passed                                 |
| `json`                                  | `c3_json_router_`                                 | 32 passed                                |
| `cleanup-v2`                            | `c3_early_inspection_`                            | 9 passed                                 |
| `facade-v2`                             | `c3_responses_integration_`                       | 48 passed                                |
| `unified`                               | `proxy::security::unified_core_tests::`           | 216 passed                               |
| `http`                                  | `reversible_http_`                                | 65 passed                                |
| `json-restoration`                      | `json_restoration_tests::`                        | 8 passed                                 |
| `xai-rewrite`                           | `transform_codex_responses_xai_sanitize::tests::` | 37 passed                                |
| `protocol`                              | `proxy::protocol_matrix_tests::`                  | 3 listed: **2 passed, 1 ignored**        |
| `session`                               | `proxy::session::tests::`                         | 34 passed                                |
| `completion`                            | `reversible_completion_`                          | 88 listed: 87 passed, 1 failed; exit 101 |
| `response`                              | `reversible_response_`                            | 44 listed: 41 passed, 3 failed; exit 101 |

All table rows listed successfully and retained identical before/after covered sources. Passing rows have test exit 0; only protocol has an ignored installed-CLI test, which was not enabled or counted as passed. Initial stems `c3-xai-current-owner`, `-cleanup` and `-facade` used incorrect filters: zero selections, no test execution, runner exit 2. They are invalid acceptance attempts, not passes; the nonzero `-v2` rows replace them. Multi-scope runners exited 1 when a selection or test failed. `c3-xai-current-check.log` / `-check-source.json` record `CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml`, exit 0 with five existing unused-import warnings. `c3-xai-current-diff-check.log` records `git diff --check`, exit 0. No warning-free Clippy or full formatting claim follows.

`c3-xai-current-freeze.json` SHA-256: `75905d0dca8378eedabab6602e24ab6bfa618a5943d67c68acab29e93af1d6c0`. All 514 frozen input identities were rechecked by the parent after independent review and match current files. Relative to Section 30, only `http_restoration.rs`, `response_restoration.rs` and `tests/reversible_integration_tests.rs` changed; no frozen input is missing. Every scoped run and production check covers the same 499 inputs with equal before/after maps; canonical sorted compact-JSON map SHA-256: `368935c3180fc076e7196df592821deaee66163871f79181b7e4e7e51c6d1a6c`. The other 15 inputs remain freeze-only identities, not executed per-run coverage. Archived log hashes were also checked. This freeze does not include this evidence document.

### Independent review and remaining blockers

Iris (`d55259dd1e6943dca84f91233fa255bf`) returned **scoped PASS** for the two production integration sites and four router RED/GREEN controls, with no blocking implementation defect in that scope. The complete result is archived as a labeled English translation in `c3-xai-iris-d55259d-review.json` (SHA-256 `b4ae5ad1aaa025dfa8871e4eddbaa7b78cb7f99365be1780ee854bcbb8a4c6d8`). The reviewer used read/search only, ran no commands, edits or delegation, and did not independently recompute hashes; parent checks supply the identity verification. The review corrected the protocol result to 2 passed/1 ignored; it is not 3/3 passed.

The four new tests do not directly establish xAI Pending cancellation after owner lending or publication after each individual terminal-delivery continuation. Shared controls and the existing xAI raw-completion test do not replace those dedicated cases. Synthetic projection lazy initialization, sealed native SSE finalization and remaining C3/C4/C5/CORE-1 requirements remain open.

Completion still fails the previously recorded `text_fallback_review_transient_kind_copy_is_reserved_before_parse` assertion at `tests/reversible_completion_tests.rs:5008`, left 0/right 96. The response regression additionally exposed three failing reasoning controls: `reasoning_pair_sse_releases_exact_content_through_all_snapshots`, `reasoning_pair_sse_rejects_unsupported_parts_and_strict_siblings` and `reasoning_pair_sse_double_arrays_preserve_tool_commit_barrier`. They are **not established baseline failures**, and negative cases sharing their helper cannot yet establish their intended rejection boundaries.

Cipher (`3a017c76428f4671acb13d7624d6f417`) inspected the latter failures read-only. `reasoning_pair_delivered` creates one provenance for the evidence requirement while test-only `SseRestorationState::new` creates another for the lifecycle. Static inspection shows the first Exact original observation would fail identity comparison before reasoning processing. The two new xAI production branches are unreachable in these Exact/no-tracker fixtures. This is a static diagnosis, not runtime attribution or acceptance: the child ended `contract_unsatisfied` because a minimal runtime comparison was still missing. Its returned result is archived in `c3-xai-reasoning-cipher-3a017c7-diagnosis.json` (SHA-256 `53402f145dfe681042f24c09c1f59e2cfcea90b130fa3315dcdba5bd98cec26f`). Next, confirm shared-versus-foreign provenance behavior using synthetic first-frame controls before changing the helper; do not weaken production identity checks or attribute all reasoning failures to that hypothesis without rerunning them.

No Full Gate, native enablement, installation, application launch or release acceptance was performed. The scoped PASS does not turn either failing regression scope into a passing gate.

**Documentation quality checks:** fresh-reader clarity PASS: the first paragraph states the accepted scope, later sections distinguish passing evidence from failed/ignored/zero-match attempts, and blockers are explicit. Execution-readiness PASS: paths, filters, command context, current identities, forbidden authority changes and the next bounded diagnostic step were checked against the actual code and saved artifacts. `c3-xai-doc-current.log` records the document-format check; Markdown whitespace/table-separator normalization does not change historical evidence content. These are documentation verdicts only.

## 32. Reasoning fixture shares actual-attempt provenance (scoped acceptance)

**Scoped PASS only: the reasoning test-fixture identity repair and its positive/negative controls. No production code changed in this increment. C3/C4/C5/CORE-1 remain open.** Audience: maintainers and implementing/reviewing agents. This section resolves Section 31's reasoning diagnosis with runtime evidence; it does not erase that historical failure or accept the still-failing completion scope.

### Confirmed cause, RED and minimal repair

`tests/reversible_response_tests.rs::reasoning_pair_delivered` created provenance A for `SseEvidenceRequirement`, while test-only `SseRestorationState::new` independently created provenance B for the lifecycle. The first Exact original-frame observation rejected the mismatch before reasoning processing. Original shared/foreign controls (`reasoning-provenance-original-controls`, `-same-control`, `-foreign-control`) passed 4, 1 and 1 tests respectively. The original five reasoning tests then reproduced 2 passed/3 failed in `reasoning-provenance-red` (list exit 0; test exit 101).

Before repairing the helper, two new tests at `tests/reversible_response_tests.rs:2661–2739` isolated the hypothesis: `c3_reasoning_pair_provenance_first_frame_shared_and_foreign` accepted shared identity, rejected foreign identity, retained sticky failure and verified budget zero after owners dropped; `c3_reasoning_pair_provenance_helper_reaches_completion` required the normal helper to deliver completion. `reasoning-provenance-minimal-red` listed two tests: the first-frame comparison passed, and helper completion failed at `c3_reasoning_provenance_helper_rejected_valid_original` (test exit 101). This is behavioral RED after successful compilation, not an assumed baseline or a zero-match result.

The helper at lines 2741–2778 now uses `new_bound(Responses/Responses, provenance.clone())`, sharing identity with its evidence requirement. The normal SSE wrapper, original-byte observation and completion verification remain in place. No production identity check, budget, completion proof, publication permission, lifecycle or authority store was changed or bypassed. Existing byte-exact, snapshot-mismatch, unsupported-shape and tool-commit-barrier assertions remain intact. Minimal GREEN passed 2/2 (`reasoning-provenance-minimal-green`), followed by all five reasoning controls (`reasoning-provenance-green`), including the negative cases that could previously fail prematurely on identity.

### Current-source execution and identity

From the repository root on the same HEAD/toolchain, the parent ran each scope serially with `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER`. The runner lists first, requires nonzero selection, then executes with `--test-threads=1`; it records actual exit codes and before/after source hashes. Artifacts are under `.voidx/verification/core1-20261106/`. Counts overlap and must not be summed.

| Current stem suffix (`reasoning-provenance-current-`) | FILTER                          | Result                                   |
| ----------------------------------------------------- | ------------------------------- | ---------------------------------------- |
| `minimal`                                             | `c3_reasoning_pair_provenance_` | 2 passed                                 |
| `reasoning`                                           | `reasoning_pair_sse_`           | 5 passed                                 |
| `response`                                            | `reversible_response_tests::`   | 46 passed                                |
| `unified`                                             | `unified_core_tests::`          | 216 passed                               |
| `lifecycle`                                           | `c3_lifecycle_`                 | 34 passed                                |
| `xai-router`                                          | `c3_xai_stream_router_`         | 4 passed                                 |
| `owner`                                               | `c3_sse_owner_`                 | 36 passed                                |
| `facade`                                              | `c3_responses_integration_`     | 48 passed                                |
| `completion`                                          | `reversible_completion_`        | 88 listed: 87 passed, 1 failed; exit 101 |

All listings exited 0 and were nonempty. All scopes had 0 ignored and identical before/after covered-source maps; passing scopes exited 0. The failed completion scope is not counted as a passing regression gate. Existing unused-import/linker warnings remain; no warning-free claim is made.

`reasoning-provenance-current-freeze.json` SHA-256: `2267e74d0b526f0a6cd8147341f7ece95ed25a8c79c00b58fedeaffe1886838c`. After independent review, the parent rehashed all 514 frozen inputs against current files, verified every run's 499-input before/after map, and confirmed canonical sorted compact-JSON map SHA-256 `30652f1445e32eac6288dbd1dd824c885536138df1c0c70eff614b5f3c51c33b`. Relative to Section 31's freeze, only `tests/reversible_response_tests.rs` changed; no path is missing and no production source changed. The remaining 15 inputs are freeze-only identities, not per-run coverage. This evidence document is outside the source freeze.

### Independent verdict and unresolved work

Rune (`63d73a7fe1b0433a94b2ed3aeb3578e0`) returned **scoped PASS**, with no mandatory change in this fixture scope. The complete result is archived as a labeled English translation in `reasoning-provenance-rune-63d73a7-review.json` (SHA-256 `a03d05769e1c1ba30e27ad46070dfc98191478ac3e625526b1b68e1f17f8910a`). The reviewer used read/search/find only, with no commands, edits or delegation. The review checked the minimal RED/GREEN logs, current minimal/reasoning logs and preserved assertions; other regression logs were not individually re-reviewed. The parent supplied the source/hash verification above.

Completion still fails `text_fallback_review_transient_kind_copy_is_reserved_before_parse` at `tests/reversible_completion_tests.rs:5008`, actual 0/expected 96. Kai (`e17f53cd3e5446519ae11f5ab308739b`) returned a read-only static diagnosis, archived in `completion-budget-kai-e17f53c-diagnosis.json`: the fixture records its baseline while stream-owned identity reservations are alive, then compares against it after dropping the stream. The hook assertions precede the failure. A pre-construction/live/post-drop runtime comparison is still required before changing the assertion. Separately, the existing test-only wrapper and first-buffer hook do not establish complete production kind-copy accounting; that concern must not be silently closed by a fixture repair.

Dedicated xAI Pending-cancellation and per-terminal-delivery continuation controls, synthetic projection lazy initialization, sealed native SSE finalization and remaining C3/C4/C5/CORE-1 work remain open. No Full Gate, native enablement, staging, commit, worktree, installation, application launch or release acceptance was performed. Neither this PASS nor earlier scoped passes can be accumulated into overall acceptance.

**Documentation quality checks:** fresh-reader clarity PASS: the accepted test-only scope, confirmed cause, current results and remaining blockers are explicit. Execution-readiness PASS: source paths, runner commands/filters, actual exits, freeze identities, preserved invariants and the next bounded runtime diagnostic were checked against code and saved artifacts. These are documentation verdicts only.

## 33. Completion fixture releases stream-owned identity budget (scoped acceptance)

**Scoped PASS only: the completion test-fixture baseline repair, not production kind-copy accounting or the broader projection scope. C3/C4/C5/CORE-1 remain open.** Audience: maintainers and implementing/reviewing agents. This section resolves Section 32's `0 != 96` failure using ownership controls and preserves the newly exposed HTTP failure as unresolved.

### Root cause, RED and test-only correction

The original `text_fallback_review_transient_kind_copy_is_reserved_before_parse` recorded `baseline = budget.used()` after constructing `TextProjectionStream`. Its final assertion compared that live-stream baseline against usage after `drop(stream)`. Attempt provenance, lifecycle identity and deferred-owner identity reservations belong to the stream; their release legitimately lowers usage. Kai's static diagnosis is archived in `completion-budget-kai-e17f53c-diagnosis.json` (SHA-256 `543e3515eac39611c758c40ba72338b9669bfcc125ecc569ec99bd6118a6a3e9`). The parent confirmed the relevant current source identities before and after runtime checks.

`completion-budget-original-red` listed one test and reproduced the final comparison failure, actual 0/expected 96 (test exit 101). The same fixture's existing hard-failure release control passed (`completion-budget-release-control`, one listed). Before changing the failing assertion, the parent extracted `prepared_fixture` at `tests/reversible_completion_tests.rs:4381–4409`, preserving store/admission/preparation, and added `text_fallback_review_stream_identity_budget_returns_to_preconstruction_baseline` at lines 4411–4428. This control passed: construction starts at zero, the live stream has a positive reservation, and destruction returns to the pre-construction baseline. It does not hardcode 96 or manufacture an external owner. `completion-budget-with-control-red` then reproduced the original final-assertion failure with the added control present. Both RED runs compiled, selected one test, retained stable covered sources and exited 101 only after the earlier hook/rejection assertions succeeded.

The existing transient-kind test at lines 5014–5040 now captures the pre-construction baseline, constructs the same stream, and retains the original live baseline for `event.used >= baseline + reserve + event.requested`. Hook-hit and parse-rejection checks are unchanged. Only the post-drop assertion compares against the pre-construction baseline; that baseline is independently asserted zero. `completion-budget-minimal-green` passed one listed test. No production source, reservation rule, provenance check, original observation, completion proof, publication authority or lifecycle was changed.

### Current execution, frozen inputs and independent review

All artifacts are under `.voidx/verification/core1-20261106/`. From the repository root, run `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER`. The runner requires a successful nonempty listing before execution with `--test-threads=1`, records actual exits and hashes covered sources before/after. The original two-check shell accepted the expected RED exit 101 explicitly; its aggregate exit 0 was not a passing test result. The first wider batch stopped with exit 1 on projection failure; a separate batch ran the other regression scopes serially. Counts overlap.

| Current stem suffix (`completion-budget-current-`) | FILTER                         | Result                                   |
| -------------------------------------------------- | ------------------------------ | ---------------------------------------- |
| `completion`                                       | `reversible_completion_`       | 89 passed                                |
| `projection-stream`                                | `text_fallback_stream_tests::` | 17 passed                                |
| `response`                                         | `reversible_response_tests::`  | 46 passed                                |
| `unified`                                          | `unified_core_tests::`         | 216 passed                               |
| `lifecycle`                                        | `c3_lifecycle_`                | 34 passed                                |
| `xai-router`                                       | `c3_xai_stream_router_`        | 4 passed                                 |
| `owner`                                            | `c3_sse_owner_`                | 36 passed                                |
| `facade`                                           | `c3_responses_integration_`    | 48 passed                                |
| `http`                                             | `reversible_http_`             | 65 passed                                |
| `projection`                                       | `text_fallback_`               | 58 listed: 57 passed, 1 failed; exit 101 |

All rows have successful nonzero listings, 0 ignored and unchanged covered sources; passing test exits are 0. Existing compiler/linker warnings remain. `rustfmt --check --edition 2021 src-tauri/src/proxy/security/tests/reversible_completion_tests.rs` and `git diff --check` exited 0. No Full Gate or new production `cargo check` is claimed for this test-only increment.

`completion-budget-current-freeze.json` SHA-256: `e75fc9e9ca801208535168d2c0cb32b625cd622d61ca32dca17e5d0a695a1878`. The parent rechecked all 514 current frozen inputs, all ten 499-input before/after runner maps and all 30 archived current artifacts after independent review. Canonical sorted compact-JSON runner-map SHA-256: `4165ee44b72af9e4f837d05d47eb6cbbbb14301a84f1407e98ec6a7c1f5b8a67`. Relative to Section 32, only `tests/reversible_completion_tests.rs` changed (SHA-256 `d5c1537d65a5f04a9398ddf5c217165a20d648bfaf89525cd8888819aee8ca97`); production files did not change. The other 15 frozen inputs are not per-run coverage, and this document is outside the freeze.

Prism (`44e16080f61046469c913fa9114a5e53`) returned **scoped PASS**, with no mandatory change in the fixture scope. The complete result is archived as a labeled English translation in `completion-budget-prism-44e1608-review.json` (SHA-256 `bbde6141d893d4d111e1f37a27a4ed517307da01b39432410fb86792fdc7e9cf`). The reviewer used read/search/find only, with no commands, edits, hash recomputation or delegation. It directly checked RED/control/GREEN and current completion/projection-stream logs, confirmed preserved reservation checks and required the broader failure and production-accounting gap to stay open. Parent hash checks supply identity verification, not a broader acceptance verdict.

### Unresolved boundaries and next action

The broader projection run exposed `text_fallback_http_capacity_cancel_truncation_io_and_utf8_refuse` at `tests/reversible_integration_tests.rs:9546`, actual 0/expected 1. It is not established as a baseline failure, and 57 passing tests do not accept that scope. Cipher (`e35e65754859450ebf3612374952524f`) returned read/search/find-only diagnosis, archived in `projection-http-cipher-e35e657-diagnosis.json`: SSE case 6 has a one-byte budget, insufficient for the mandatory attempt identity before body polling; the JSON path can instead read once before append rejects. This static explanation is not runtime attribution because the failure log lacks mode/case, and the panic prevented subsequent cleanup/cancellation assertions. Next, isolate the failing HTTP mode with a nonempty serial run, confirm fixed error classification and pre/post budget, compare adequate-budget and post-construction exhaustion controls, then use behavioral RED/GREEN for any repair. Do not lower the assertion or relax reservations based only on the numerical mismatch.

Production kind-copy accounting remains independently unverified: `TextProjectionStream` is test-only, and its first buffer hook does not directly observe the production kind allocation. This fixture repair does not establish that accounting or authorize removal of reservations. Dedicated xAI Pending cancellation/per-terminal-delivery controls, synthetic projection lazy initialization, sealed native SSE finalization and remaining C3/C4/C5/CORE-1 gates remain open. All prior dirty/untracked work and P3/P4 boundaries are preserved; no staging, commit, worktree, native enablement, installation, application launch, Full Gate or release acceptance occurred.

**Documentation quality checks:** fresh-reader clarity PASS (test-only conclusion, causal controls, current failure and exclusions are explicit). Execution-readiness PASS (paths, exact filters, command/exit interpretation, source/artifact identities, forbidden weakening and the next diagnostic boundary checked against current code and saved evidence). These are documentation verdicts only.


## 34. HTTP projection capacity stages (test-only scoped acceptance)

**Scoped PASS only: HTTP projection capacity-phase controls and the SSE case-6 read expectation.** This records the previously reviewed test-only increment after Section 33; it is not a new production change or C3/C4/C5/CORE-1 closure. Audience: maintainers and implementing/reviewing agents. Section 35 records the newer production-source state.

The parent reproduced Section 33's failure in `projection-http-original-red` (one listed, runtime exit 101). An adequate-budget control passed. Before changing the numerical expectation, `tests/reversible_integration_tests.rs:9502–9567` added six controls: JSON/SSE times insufficient initialization budget, sufficient initialization followed by exhaustion inside the first body poll, and adequate-budget success. `projection-http-phase-control` passed one listed test containing all six cases. The reservation explicitly survives the yielded chunk; a failed reservation would become a non-capacity error, so the fixed-capacity classification cannot accidentally accept that fixture failure. These controls assert exact reads, the live initialization charge at body poll, zero final budget and no authority.

`projection-http-diagnostic-red` retained the original count assertion, selected one test and failed with `mode=true case=6`, actual 0/expected 1. Only SSE case 6 now expects zero reads: its one-byte budget cannot create the mandatory stream identity before polling. Other read counts, rejection, cleanup, no-authority and cancellation assertions remain unchanged. `projection-http-minimal-green` passed one listed test. No production file changed in this increment.

Artifacts are under `.voidx/verification/core1-20261106/`. Reproduction uses the Section 29 runner command from the repository root with `CARGO_BUILD_JOBS=2`, nonempty listing followed by serial execution. At this historical source state, `projection-http-current-` stems passed: `projection` (`text_fallback_`, 59), `completion` (`reversible_completion_`, 89), `response` (`reversible_response_`, 46), `unified` (`unified_core_tests::`, 216), `lifecycle` (`c3_lifecycle_`, 34), `xai-router` (`c3_xai_stream_router_`, 4), `owner` (`c3_sse_owner_`, 36), `facade` (`c3_responses_integration_`, 48), and `http` (`reversible_http_`, 65). Every accepted row has list/test exit 0, nonzero listing, 0 ignored and stable covered sources; counts overlap.

The parent confirmed freeze `projection-http-current-freeze.json` SHA-256 `4b58fb35ec472a6eac196bf81d4b4b7f54ecc60ec7be7c2b5405894a08bddc92`: 514 frozen Rust/Cargo inputs, 499 per-run identities, canonical runner-map SHA-256 `237ae5145d9c1a58bfc4fdd38b88511250b7dd9c5f6c9d45e29d2a61d5391359`. Only `tests/reversible_integration_tests.rs` differed from Section 33's freeze. Helix (`d8199fa800744518a8ed61ffb7db5f3a`) returned **scoped PASS** with no mandatory change; complete translated result: `projection-http-helix-d8199fa-review.json`, SHA-256 `c5c063a1bbf014d92f62aedef88f6849f7672d76e475abefc386a39bb08eafbd`. Review used read/search/find only, not test execution or hash recomputation. This closes the stale test expectation, not production initialization design or the then-unverified production kind copy. Section 35 reruns affected scopes after that production change.

**Documentation checks:** mixed-audience clarity and execution-readiness PASS: test-only scope, causal controls, commands, source cutoff and exclusions are explicit. No Full Gate is claimed.

## 35. Production SSE event-kind copy accounting (scoped acceptance)

**Scoped PASS only: the transient event-kind copy in the production Responses SSE parser and twelve direct-parser controls. C3/C4/C5/CORE-1 remain open; native adapters remain Disabled.** This resolves the production accounting gap explicitly excluded by Sections 33–34, not the remaining continuation, synthetic initialization or native finalization work. Audience: maintainers and implementing/reviewing agents.

### Root cause, RED and minimal implementation

`response_restoration.rs::process_sse_block_inner` copied the decoded JSON `type` with `.to_owned()` after `PayloadJson` parsing. The retained JSON reservation covers its own strings, not this distinct allocation. An independent kind owner is needed when the parsed JSON moves into a held/deferred event. The synthetic-only `TextProjectionStream` outer reservation and its buffer hook did not prove production-copy accounting.

Before modifying production code, the parent appended twelve `c3_sse_kind_*` tests at `tests/c3_converted_sse_owner_tests.rs:308–445`. They construct the production `new_bound` state and call `process_sse_block` directly, without `TextProjectionStream`: mapped/zero times explicit header/data-only times success/allocation refusal/copy refusal. The fixture uses an escaped Unicode kind and an unknown native field. Existing `PayloadJson` and `PayloadText` hooks assert the exact concurrent charge: state baseline + data buffer + retained JSON + decoded kind bytes. Success checks native output preservation and transient cleanup; refusal checks sticky failure, no publication and final zero after real owners drop.

`kind-copy-red` listed twelve tests and ran 0 passed/12 failed, exit 101, with unchanged covered sources. Eight refusal cases failed `c3_kind_unaccounted_copy_accepted`; four successful-output cases failed `c3_kind_missing_allocation_control` after their output checks. This was runtime RED, not compilation or admission failure.

The sole production change at `response_restoration.rs:2226–2240` replaces the uncharged String with existing `PayloadText::try_from_str` under the same budget, `MAX_FRAME_BYTES` bound and fixed `RestoreError::capacity` mapping. Equality still compares decoded strings. `PayloadText` reserves before allocation/copy and drops its allocation before its reservation. No new dependency, authority surface, proof/publication path, lifecycle, route enablement or error plaintext is introduced. `kind-copy-green` passed all twelve cases before test formatting; existing assertions were not weakened.

### Current-source serial verification

Commands ran from the repository root on macOS arm64, HEAD `2b00c4573098f4da31345c788aacf321ef685060`, Rust/Cargo 1.98.1, with the dirty tree preserved and no concurrent Cargo process, ignored/live test, application launch or installation. For each row:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER
# Production compile, separately logged with before/after source identities:
CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml
```

| Current stem suffix (`kind-copy-current-`) | FILTER                          | Listed / passed |
| ----------------------------------------- | ------------------------------- | --------------- |
| `kind`                                    | `c3_sse_kind_`                  | 12 / 12         |
| `completion`                              | `reversible_completion_`       | 89 / 89         |
| `response`                                | `reversible_response_`         | 46 / 46         |
| `projection`                              | `text_fallback_`               | 59 / 59         |
| `unified`                                 | `unified_core_tests::`         | 216 / 216       |
| `lifecycle`                               | `c3_lifecycle_tests::`         | 34 / 34         |
| `owner`                                   | `c3_sse_owner_`                | 36 / 36         |
| `facade`                                  | `c3_responses_integration_`    | 48 / 48         |
| `http`                                    | `reversible_http_`             | 65 / 65         |
| `xai-corrected`                            | `c3_xai_stream_router_`        | 4 / 4           |

All rows have list/test exit 0, 0 ignored and identical unchanged 499-input before/after maps. Counts overlap. The initial `kind-copy-current-xai` used the incorrect filter `c3_stream_xai_router_`, listed zero and did not execute; it is excluded, and its batch stopped before production check. The corrected filter and separately recorded `kind-copy-current-check` both pass. Production unused-import warnings and the test linker/unwind warning remain; this is not warning-free Clippy acceptance. The new test file passes `rustfmt --check --edition 2021`; `git diff --check` passes. No Full Gate was run.

### Source identity, independent review and limits

`kind-copy-current-freeze.json` SHA-256: `2cdcd96574bd2f73c10c3715fc564694dc49f68791cf23761a7bea161142b914`. All 514 current frozen inputs match after independent review; ten per-run source manifests and the production check match the canonical 499-input map, SHA-256 `7213f3d70250d14c5093c35400420be3300cbe93edc9a767f36dd61d7f90c3ea`. The other 15 frozen inputs are not per-run coverage; documentation is outside this freeze. Only two Rust files differ from `kind-copy-before.json`:

- `src-tauri/src/proxy/security/response_restoration.rs`: `5ff3d30d81dc21c994b7b3188303b123dc9a2f7b8f11a02b7a8dc62a75131918`.
- `src-tauri/src/proxy/security/tests/c3_converted_sse_owner_tests.rs`: `c6a10af8ed350c532c658a77d19f4408933851679eb84b535a7afb82be8d61f2`.

Nova (`890e5ffc470648758f297c97e2026dc8`) returned **scoped PASS**, with no blocker after read/search/find-only inspection of code and RED/GREEN/current evidence. Complete translated result: `kind-copy-nova-890e5ff-review.json`, SHA-256 `9564b2e661edcddb61e66c02dea37a300ff6383da000bd296b7aeb34aa7458fc`. The reviewer confirmed direct production coverage, independent decoded-byte charging, owner destruction order, preserved output and sticky failure. Parent recomputation supplies final identity verification; the reviewer did not rerun tests or hashes.

The hooks inject `Unavailable`; they do **not** establish actual remaining-budget `CapacityExceeded` behavior at the kind reservation. The reviewer considers that an optional additional negative control, not a blocker for this exact accounting increment. Direct parser controls are not HTTP delivery acceptance. Dedicated xAI Pending cancellation/per-terminal delivery continuations, lazy synthetic projection initialization, sealed native SSE finalization/restored-wire validation, remaining C3/C4/C5 acceptance and the current Full Gate/independent CORE-1 freeze are still open. Preserve P3/P4 and all unrelated work; do not use this PASS to enable native routes or close those gates.

**Documentation checks:** fresh-reader clarity PASS: acceptance is narrow and stated first, RED versus GREEN versus zero-match attempts are distinguished, and residual gaps are explicit. Execution-readiness PASS: source paths, exact commands/filters, current hashes, verification scope and forbidden acceptance/enablement expansion are specified.

## 36. xAI Pending cancellation and terminal delivery continuation (test-only scoped acceptance)

**Acceptance:** the twelve new mapped/zero xAI real-router cancellation controls and four strengthened existing controls have current-source focused regression evidence and independent **scoped PASS**. This increment changes tests and `cfg(test)` instrumentation only, not production behavior. It does not close C3, C4, C5, CORE-1, native registration, or Full Gate. Sections 34–35 remain historical scoped checkpoints; this section records the newer source state.

**Audience/scenario:** mixed human/LLM post-implementation verification record. Follow the approved plan and authoritative architecture linked above; this record grants no new implementation or routing permission.

### Scope and observable boundaries

- `src-tauri/src/proxy/security/http_restoration.rs:3413–3487`: test-only lending probe plus two test-gated recording calls, after lending inspection and after successfully observing an original xAI frame. The probe retains the tracker shell so cancellation cannot pass merely because the shell disappeared. Only counts and inspection occupancy escape; no mechanics owner, proof, lease, binding, or publisher is returned. The parent reconstructed the exact pre-increment HTTP file by removing only these additions and matched its prior SHA-256.
- `src-tauri/src/proxy/security/tests/reversible_integration_tests.rs:15484–15839`: extend the existing xAI upstream/router fixture, preserving seven-byte CRLF chunks, UTF-8 text, whole-float integer repair, actual-send capture, in-memory storage and dynamic loopback binding.
- Two `c3_xai_delivery_pending_cancel_*` controls wait until four complete original frames have been observed, the client stream is actually Pending, and the inspection is lent. No client output or publication is allowed. Dropping the stream clears inspection while the retained tracker is still alive.
- Ten `c3_xai_delivery_cancel_{mapped,zero}_after_{1..5}` controls drop the final client stream after each separate yield, including arguments-done, item-done and response-completed. Each successful yield has exactly `(published, pending, pins) = (0, 1, 1)`; cancellation returns all three counts to zero. These held pending records are not completed publication.
- Successful existing mapped/zero controls receive all five frames and then poll through EOF: only that resumed delivery changes counts to `(1, 0, 0)`. The completed frame alone is insufficient. Existing original-observation rejection controls retain failure/no-tool/no-private-output assertions. All controls preserve one actual send, redacted upstream input, placeholder exclusion, tool type/call/name identity, restored argument values and integer type, a single lifecycle, and zero payload budget after all real owners, Probe and TLS guard are dropped.

### Test development and excluded evidence

No production behavioral RED/GREEN is claimed. The unchanged production implementation already satisfies these added controls after fixture corrections:

1. `xai-delivery-initial` listed 16 tests successfully but the outer command timed out while the test invocation waited for the build-directory lock. It has no completed source manifest or accepted test result; it is excluded. The parent checked that no Cargo/test processes remained before retrying.
2. `xai-delivery-initial-retry` listed 16, passed 4 and failed 12, exit 101, stable source. The new assertion wrongly expected pending/pins zero during valid yields; `publication_test_support::counts` defines them separately from published entries. Correcting only that new expectation to `(0, 1, 1)` produced `xai-delivery-green`: 16/16.
3. Strengthening the probe from a weak reference to a retained tracker produced `xai-delivery-retained-observer`: 16 listed, 12 passed and four budget assertions failed because the TLS probe guard still retained original evidence at the zero-budget assertion. Explicitly dropping both Probe and guard before that assertion produced `xai-delivery-final-green`: 16/16. Inspection-empty and all publication/no-authority assertions remain checked before these observer drops. This was fixture-owned retention, not a production leak correction.

### Current-source verification

Repository root, macOS arm64, unchanged HEAD `2b00c4573098f4da31345c788aacf321ef685060`; source was stable across every accepted run. Cargo work was serialized with `CARGO_BUILD_JOBS=2`; each scope was first listed successfully with nonzero test count, then run with `--test-threads=1`:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py xai-delivery-current-STEM FILTER
CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml
```

| STEM | FILTER | Listed / passed |
| --- | --- | ---: |
| xai | `c3_xai_` | 16 / 16 |
| kind | `c3_sse_kind_` | 12 / 12 |
| completion | `reversible_completion_` | 89 / 89 |
| response | `reversible_response_` | 46 / 46 |
| projection | `text_fallback_` | 59 / 59 |
| unified | `unified_core_tests::` | 216 / 216 |
| lifecycle | `c3_lifecycle_tests::` | 34 / 34 |
| owner | `c3_sse_owner_` | 36 / 36 |
| facade | `c3_responses_integration_` | 48 / 48 |
| http | `reversible_http_` | 65 / 65 |
| early | `c3_early_inspection_` | 9 / 9 |

All listing/test exits are zero, no test was ignored, and all eleven pre/post runner maps match the same current 499 Rust/Cargo input map. Counts overlap and must not be added. Production check also exited zero on that same map, with five existing unused-import warnings; test linking retains the existing large `__eh_frame` warning. `git diff --check` exited zero. New probe and xAI fixture blocks are rustfmt fixed points. Whole-file HTTP rustfmt is **not passing**: both the current input and exact reconstructed pre-increment input fail on pre-existing formatting. This increment does not establish whole-file formatting, warning-free Clippy, renderer, Full Gate, installer or release acceptance.

### Frozen identity and independent verdict

Artifacts remain under `.voidx/verification/core1-20261106/`:

- `xai-delivery-current-freeze.json`: SHA-256 `c09f8be822bb36c1a735dc40bad2fedfb4e27358455f4c8952e6559e9ec86b6e`; 514 frozen Rust/Cargo inputs, including 499 covered by per-run source maps. The other 15 are not additional per-run coverage. Documentation is outside that source freeze. After review, the parent corrected the freeze's `runs[].filter` metadata from runner stem names to the actual filters read from listing commands; no source, logs, counts, source-map identity or verdict changed. The earlier metadata-only freeze SHA supplied to the reviewer was `bafde2475bd0704086ea236c2342482971bb06c3928d10a8b178125ff9697f0c`.
- Canonical 499-input runner map: SHA-256 `98afd5ec1f082dfc2ae50df5163bcab809343675df6754aab0e6e0e699223b39`.
- HTTP source: SHA-256 `ee4afa04fab54a8ee405214f5dc9b6cf64f4c6f77da19dc0140cfbfde250d323`; integration fixture: SHA-256 `f9db374e5dd8af3daf6d606f7a53f18fbe7dd310921f5d95ed8d8ab06c2e4ba1`. Only these two frozen inputs differ from `xai-delivery-before.json`; no input was added/deleted and all prior dirty/untracked work remains.
- Vesper (`24f8d9769b4447058cc1108fa9152460`) returned **scoped PASS**, no blocking findings or required changes. Complete English review: `xai-delivery-vesper-24f8d97-review.json`, SHA-256 `1a2e83d6607faaaab8ef01343b09ebfa40e55003084c15768b42e44d65c9c14e`. Review used read/search/find only, with no commands, edits or further delegation. It inspected archived logs; execution and hash/reconstruction facts remain parent-supplied and parent-rechecked evidence, not independently rerun commands.
- Section 34's embedded canonical runner-map value was explicitly checked against `projection-http-current-freeze.json` and matches `237ae5145d9c1a58bfc4fdd38b88511250b7dd9c5f6c9d45e29d2a61d5391359`. This confirms that historical value, not current source identity.

**Open boundary:** synthetic projection lazy initialization, sealed native SSE finalization/restored-wire validation, remaining shared C3 mechanics, C4/C5 contracts/orchestration and current Full Gate/independent CORE-1 acceptance remain open. Do not use this test-only PASS to enable a native route, relax authority/budgets, close the core, or supersede P3/P4's deliberately limited publication/proof acceptance. No staging, commit, push, worktree, application launch/installation, real credential, ignored/live test, or fixed proxy port was used.

**Documentation quality checks:** fresh-reader clarity PASS: acceptance is stated first; pending versus published, fixture failures versus production behavior, and historical versus current identities are distinguished. Execution-readiness PASS: exact paths, filters, commands, source identities, ownership assertions, limitations and forbidden closure/enablement are specified and cross-checked against current files/artifacts. These are documentation self-checks, not independent whole-core acceptance.

## 37. Synthetic projection lazy initialization (scoped acceptance)

**Scoped PASS only: the synthetic-only lazy-initialization increment and its controls. C3/C4/C5/CORE-1 remain open.** This post-implementation record is for maintainers and implementing/reviewing agents. It supersedes Section 36 only as the newest focused source checkpoint; it does not expand P3/P4 acceptance or enable native routes.

### Behavior and test-first evidence

`src-tauri/src/proxy/security/response_restoration.rs:3491–3567` now defers synthetic `TextProjectionStream` mechanics until explicit initialization or its first frame. Construction retains the existing lease/budget without creating attempt/lifecycle identities. Successful initialization is idempotent; initialization failure, frame failure and finish stay closed. Empty EOF/abort does not initialize mechanics or mint completion evidence. The existing inner transaction, budget-owned identities, hold-all barrier and extra synthetic kind reservation are unchanged; there is no new owner, proof or publication accessor.

`http_restoration.rs:5780–5785` explicitly initializes the synthetic SSE fixture before polling the upstream body. This preserves the existing one-byte-budget zero-read rejection rather than trading it for lazy direct construction. JSON collection is unchanged. All implementation changes are under `cfg(test)`; there is no production behavior or routing change.

Four new `text_fallback_lazy_*` controls were written first. `projection-lazy-red` listed 4 successfully, then failed all 4 at runtime (test exit 101): eager construction charged 96 bytes, or refused the one-byte budget during construction rather than first use. `projection-lazy-green-retry` passed all 4 after the minimal implementation. The first `projection-lazy-green` attempt reached successful listing after slow linking but the outer 120-second limit interrupted the test stage; it has no complete source manifest/result and is **excluded from acceptance**. The parent checked that no Cargo/test process remained before retrying.

Two supplementary direct characterizations cover explicit initialization idempotence and partial-initialization refunds at capacities 1/32/64/95. These were added after the first GREEN and are not claimed as RED. Existing identity-charge and transient-kind fixtures explicitly initialize before their live-charge baselines, retaining the original charge/release assertions. The new `text_fallback_http_lazy_unpolled_and_pending_cancel_preserve_boundaries` characterization at `tests/reversible_integration_tests.rs:9502–9553` deterministically polls JSON/SSE fixtures in three phases: unpolled, Pending before bytes, and Pending after a complete synthetic entity but before EOF. It checks read counts, retained charges, denied historical authority, `(published, pending, pins) = (0, 0, 0)` and cancellation cleanup. Existing HTTP capacity-phase, cancellation and original-safety assertions are unchanged.

### Current-source verification

From the repository root on macOS arm64, at HEAD `2b00c4573098f4da31345c788aacf321ef685060`, with the dirty tree preserved, each row used:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py projection-lazy-current-SUFFIX FILTER
# The runner requires a successful nonempty listing before execution:
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
```

All twelve rows have list/test exit 0, nonzero listing, 0 ignored and identical before/after source maps. Counts overlap and must not be summed.

| SUFFIX | FILTER | Listed / passed |
| --- | --- | --- |
| `lazy` | `text_fallback_lazy_` | 6 / 6 |
| `projection` | `text_fallback_` | 66 / 66 |
| `completion` | `reversible_completion_` | 95 / 95 |
| `response` | `reversible_response_` | 46 / 46 |
| `unified` | `unified_core_tests::` | 216 / 216 |
| `lifecycle` | `c3_lifecycle_tests::` | 34 / 34 |
| `owner` | `c3_sse_owner_` | 36 / 36 |
| `facade` | `c3_responses_integration_` | 48 / 48 |
| `http` | `reversible_http_` | 65 / 65 |
| `xai` | `c3_xai_` | 16 / 16 |
| `kind` | `c3_sse_kind_` | 12 / 12 |
| `early` | `c3_early_inspection_` | 9 / 9 |

`CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml` exited 0, with the same stable 499-input runner map (`projection-lazy-current-check.log` and `-check-source.json`). `git diff --check` exited 0. New wrapper/test blocks are rustfmt fixed points; no whole-file formatting PASS is claimed. Existing five production unused-import warnings, test unused import and linker/unwind warning remain. No ignored/live tests, real credentials, fixed application port, application launch/install, staging, commit, push or worktree was used.

### Identity, independent review and exclusions

Artifacts are under `.voidx/verification/core1-20261106/`. `projection-lazy-current-freeze.json` SHA-256 is `ed5561d3575e5f5a7847b072a1dd3d054ce331b97ed0ed05f8ed47597cace9c9`. It records 514 project Rust/Cargo inputs and the current 499-input canonical runner map SHA-256 `d91d3155598ebe307d36f637a0ba4a82eb94d199fadec5774e6344c342c0ce54`. The additional 15 inputs are not per-run covered. Compared with `projection-lazy-before.json`, exactly four inputs changed, with no additions/deletions: `security/response_restoration.rs`, `security/http_restoration.rs`, `security/tests/reversible_completion_tests.rs`, and `security/tests/reversible_integration_tests.rs` under `src-tauri/src/proxy/`.

The parent reconstructed `response_restoration.rs` by removing only the new wrapper and renaming the unchanged inner struct/impl, and reconstructed `http_restoration.rs` by removing only the new initialize call. Both exactly match their saved increment-before files. Scoped diffs are retained as `projection-lazy-diff-*.rs.txt`; this is a synthetic test-path change, not a production parser fix. All 514 frozen input identities were rechecked after review.

Onyx (`0367c1efe79b42e9b6c2c374c41ad06c`) returned **scoped PASS**, with no mandatory changes, using read/search/find only: no commands, edits, delegation or independent hash computation. It confirmed lazy/sticky ownership, retained charged mechanics, HTTP pre-read initialization and cancellation controls, while relying on the parent's saved identity/execution evidence. The English result is preserved in `projection-lazy-onyx-0367c1e-review.json`, SHA-256 `3ff2ded921ab3db73ea15ea0e9dc4bef8fba05bcec32379f6efaf399c09cdfb6`.

This closes only synthetic projection lazy initialization. Sealed native SSE finalization/restored-wire validation, other C3 boundaries, C4 orchestration, C5 contract/harness freeze and CORE-1 remain incomplete. Current Full Gate and corresponding broader independent PASS are still required. Native registration remains Disabled. No Full Gate, whole-file rustfmt, Clippy, renderer, installer or release acceptance is claimed; P3/P4 and all prior uncommitted work remain preserved.

**Documentation quality checks:** mixed-audience fresh-reader clarity PASS (accepted scope, behavior, evidence and open gates are explicit); execution-readiness PASS (exact paths, commands, filters, counts, identities, invariants and exclusions are present and cross-checked against source/manifests).

## 38. Native Chat SSE consuming finalization and restored-wire validation (scoped acceptance)

**Scoped PASS only: the sealed native Chat SSE finalizer and its ten controls. C3/C4/C5/CORE-1 remain open; native registration remains Disabled.** This mixed-audience post-implementation record supersedes Section 37 only as the newest focused source checkpoint. Unlike Section 37's synthetic-only change, this increment adds an unconnected production core capability; it does not alter the existing Responses execution path or expand P3/P4 acceptance.

### Accepted capability and ownership

- `src-tauri/src/proxy/security/response_restoration/finalization.rs:530–607` is the sole issuer of the private-field `NativeSseFinalizationKey` and opaque `FinalizedNativeSse`. `finalize_native_sse` consumes the stage and HTTP proof, requires the lease/client/binding, checks the exact original entity and actual attempt, and validates the actual restored encodings before preparing publication. No raw-wire/pending pair or activation capability is returned to an adapter.
- `security/http_restoration/completion_verifier.rs:983–1012` consumes `VerifiedNativeSse` for restored-wire validation. The original digest/binding/attempt/client checks precede allocation for restored-wire verification. The existing `NativeSseEvidence` strict parser checks the restored stream and complete tool arguments independently of the valid original proof; it does not issue a new proof or manufacture network EOF.
- `security/restoration_core/native_response.rs:1041–1104` permits bounded reading and consuming charged-frame transfer only with the finalizer's key. It reuses `PayloadVec` and `PayloadBuffer`, reverses existing encodings for ordered delivery, and exposes no bare uncharged wire. A metadata failure after pending creation drops that pending slot.
- Each delivery yield uses `Bytes::from_owner` to retain its charged allocation, including while cloned bytes remain held. The pending owner survives every yield, including DONE. Only polling after the final yield issues the existing `DeliveryResume`; cancellation retires pending authority. Activation failure yields only the fixed `io::Error::other("Response stream failed.")`, not a new successful terminal.

The facade changes in `security/response_restoration.rs` and `security/restoration_core.rs` only expose these sealed capabilities at their existing internal visibility. Existing publication storage, budget owners, identity/authority checks, completion parser and P3/P4 continuation are reused. There is no new dependency, schema, UI, persisted setting, credential source, routing activation or unredacted fallback.

### Test-first, mutation and characterization evidence

The ten controls are in `src-tauri/src/proxy/security/tests/c5_native_sse_finalization_tests.rs:198–635`, registered inside the finalizer. They cover mapped/zero full delivery and cancellation when unpolled or after every yield; invalid restored tool JSON; foreign binding/attempt, exact digest and size rejection; actual budget exhaustion and verifier allocation/copy faults; late metadata failure while pending; activation failure after DONE; yielded-byte/clone accounting; and tool identity, unknown fields, numeric types and raw metadata preservation.

Evidence classification is deliberately narrow:

1. `native-sse-finalization-red` timed out after 120 seconds while waiting for the Cargo build lock. It did not reach a complete compilation/test result. A subsequent process/lock-inspection command was denied by the user; that denied command was not retried and no other process was terminated.
2. `native-sse-finalization-red-retry` has list exit 101, zero listed tests and no test run: interfaces/reexports were missing and the new counts fixture initially passed a lease instead of its required store. These compile failures are **not behavioral RED**.
3. A temporary test-only reject scaffold allowed `native-sse-finalization-runtime-red` to list 4 and execute: 1 passed / 3 failed (test exit 101). This demonstrates the missing-interface behavior, not an existing production vulnerability. The scaffold was removed. The initial consuming implementation passed 4/4 in `native-sse-finalization-green`.
4. Six supplementary characterization controls were added after initial GREEN; intermediate 8/8 and 10/10 runs were successful. No RED is claimed for these six additions or for all ten tests collectively.
5. A controlled temporary mutation omitted restored-wire strict revalidation. `native-sse-finalization-revalidation-mutation` listed 1, then failed 0 passed / 1 failed (test exit 101), with the sanitized assertion `native_sse_invalid_restored_tool_released`. This independently shows the restored-tool control is sensitive to the required check. `native-sse-finalization-mutation-restoration.json` records byte-exact restoration of `completion_verifier.rs`: both hashes are `561554c1a2309fe068fd7b921b1098e4227df21045fee58410bbe11405184db9`. All accepted current-source runs below follow restoration.

### Current-source verification

Repository root, macOS arm64, unchanged HEAD `2b00c4573098f4da31345c788aacf321ef685060`; Rust `1.98.1 (48a229cea 2026-09-01)` and Cargo `1.98.1 (797e8a9bc 2026-08-05)`. Heavy Cargo checks ran serially with `CARGO_BUILD_JOBS=2`:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py native-sse-finalization-current-SUFFIX FILTER
# Each runner first requires a successful nonempty listing, then executes:
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list
cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --test-threads=1
CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml
```

| SUFFIX | Actual Cargo FILTER | Listed / passed |
| --- | --- | ---: |
| `native` | `c5_native_` | 153 / 153 |
| `completion` | `reversible_completion_` | 95 / 95 |
| `response` | `reversible_response_` | 46 / 46 |
| `projection` | `text_fallback_` | 66 / 66 |
| `core` | `unified_core_tests::` | 216 / 216 |
| `lifecycle` | `c3_lifecycle_tests::` | 34 / 34 |
| `owner` | `c3_sse_owner_` | 36 / 36 |
| `facade` | `c3_responses_integration_` | 48 / 48 |
| `http` | `reversible_http_` | 65 / 65 |
| `xai` | `c3_xai_` | 16 / 16 |
| `kind` | `c3_sse_kind_` | 12 / 12 |
| `early` | `c3_early_inspection_` | 9 / 9 |
| `driver` | `native_driver::tests::` | 28 / 28 |
| `route` | `native_route_scope_tests::` | 22 / 22 |

All fourteen listing/test exits are zero, with nonzero counts, 0 ignored, and identical pre/post 500-input maps. The broad native selection includes the ten new tests. Counts overlap and must not be added. Production check exited 0 with the same stable map; the parent rechecked current inputs, recorded commands, filters, listing counts, result counts and exit codes after independent review. `git diff --check` exited 0. `native-sse-finalization-format.json` records rustfmt fixed points for the three new implementation blocks and new test file only; it is not whole-file formatting evidence. Production/test unused-import warnings and the existing linker `__eh_frame` warning remain; warning-free Clippy is not claimed.

### Frozen identity and independent review

Artifacts are under `.voidx/verification/core1-20261106/`:

- `native-sse-finalization-current-freeze.json`: SHA-256 `48e8f004466725685c31fa597f674fbcfd1a26adc45f9f0abfbc8fd89720524e`; 515 frozen Rust/Cargo inputs. Its 500-input canonical runner map SHA-256 is `701d72d669a1a1d69634ff904a7740607cad454025b3acd1668389977d04789c`. The other 15 frozen inputs are not additional per-run coverage; documentation is outside the source freeze.
- `native-sse-finalization-filter-correction.json` records correction of `runs[].filter` from runner scope labels to actual Cargo filters read from the listing commands and cross-checked with execution commands. The earlier metadata-only SHA `4af4e169c7cde39353637f23846d93b984fd43a2d04c93f2884f9e292fc2066b` is retained in `native-sse-finalization-freeze-before-filter-correction.json`. Source, logs, counts and runner-map identity did not change.
- Compared with `native-sse-finalization-before.json`, five existing frozen inputs changed: `security/http_restoration/completion_verifier.rs`, `security/response_restoration/finalization.rs`, `security/response_restoration.rs`, `security/restoration_core/native_response.rs`, and `security/restoration_core.rs`, all under `src-tauri/src/proxy/`. The sole added Rust input is `security/tests/c5_native_sse_finalization_tests.rs`; none was removed. Increment-before snapshots and bounded `native-sse-finalization-diff-*.rs.txt` preserve the exact review scope, without replacing prior dirty/untracked work.
- Kai (`9d6526e04483437e8754103c5ee0d110`) returned **scoped PASS**, with no mandatory changes. English result: `native-sse-finalization-kai-9d6526e-review.json`, SHA-256 `06beb227452d2737ba3d3c4e0d0708759acfe6d21b6a9a44b917a2217fc34a0f`. Review used only read/search/find: no commands, edits, delegation or independent hash recomputation. Execution/hash facts remain parent-supplied and parent-rechecked evidence.

**Remaining boundaries:** this closes only the unconnected sealed native Chat SSE finalization/restored-wire increment. Native adapter lanes/enablement, live tee, actual native HTTP final-send integration, progressive per-delta release, Anthropic/Gemini behavior, broader snapshot selection/aggregation semantics, remaining C3 mechanics, C4 orchestration, C5 contract/non-Responses harness freeze, and CORE-1 remain unaccepted. Delivery continuation is not remote-receipt proof. Current Full Gate and corresponding independent broader PASS remain required. Existing whole-file formatting failures are not repaired by expanding formatting scope. No Full Gate, whole-file rustfmt, Clippy, renderer, installer or release acceptance is claimed. No staging, commit, push, worktree, application launch/install, real credentials, ignored/live tests or binding of the application proxy port was used.

**Documentation quality checks:** mixed-audience fresh-reader clarity PASS: the limited accepted production capability, synthetic history, test evidence classes and open gates are distinguished. Execution-readiness PASS: exact source/artifact paths, current behavior, authority/budget invariants, commands, real filters, counts, identities, forbidden enablement and remaining verification are explicit and cross-checked against code/manifests. These self-checks do not substitute for broader implementation acceptance.

## 39. Existing C4 identity and disabled-dispatch bounded review

**Outcome: historical bounded PASS, not C4 closure.** Juno (`30582b7dd2634fc580eaa7b733d03b8c`) reviewed the existing strict native identity parsers, trusted ingress classification, disabled native admission/dispatch, and specified final-client/final-send regressions. No Rust modification was requested. The review used only read/search/find; command results and hashes were parent-supplied, not independently executed or computed.

Source anchors are `session.rs:85–165`, `handler_context.rs:17–146`, `handlers.rs:136–168,891–917,2867–2875`, `gateway/ingress.rs:193–225`, `security/http_restoration.rs:1056–1107,1163–1183,1277–1299`, and `security/protocol_adapters/mod.rs:1–33`, all under `src-tauri/src/proxy/`. Invalid higher-priority identity does not fall through; a client header cannot select trusted ingress; registration grants no restoration capability. Existing actual-attempt and real Reqwest/Hyper final-send controls preserve zero-send rejection and prepared-attempt redirect prohibition. These are not native final-send integration acceptance.

Local review artifact: `.voidx/verification/core1-20261106/c4-followup-juno-30582b7d-review.json`. Its source was frozen **before** the three-file JSON increment in section 40: `c4-followup-current-freeze.json`, SHA-256 `db2435741676a20f46ccbaea345497b26c6f3e4599eff27bc60029915dbdd0a2`; historical 500-input map SHA-256 `701d72d669a1a1d69634ff904a7740607cad454025b3acd1668389977d04789c`. Seven historical runs recorded session 34, final validation 73, final-send 10, redirect 8, broad matrix 3 listed/2 passed/1 existing live test ignored, and two exact synthetic matrix filters with 1 passed each. Earlier same-source native route controls passed 22. Counts overlap and must not be summed. Section 40 records current-source reruns; those reruns are not a new independent whole-C4 review.

Athena's historical `NEEDS_CHANGE` gap assessment (`2459cbb2043b4eeb95a3e0faa5e5bbfa`) is preserved in `.voidx/verification/core1-20261106/core1-gap-athena-2459cbb2-review.json`. Its source-established JSON restored-inner-arguments gap was subsequently reproduced and addressed in section 40. Its remaining reusable native harness, shared lifecycle-to-native handoff, and contract-freeze findings remain open. Full Chat indexing, Anthropic signed-unit semantics, Gemini Part semantics, live tee, and progressive delivery remain later lane/integration work, not claimed here.

## 40. Native JSON consuming restored-wire validation: scoped acceptance

**Outcome: independent scoped PASS for the three-file increment only.** Native registration remains `Disabled`; C3/C4/C5 and CORE-1 are not closed. This supersedes section 38's current-source identity for overlapping Rust inputs, not its historical SSE acceptance.

### Behavior and authority boundary

The original HTTP-owned proof establishes completion of the original response entity, not validity after authorized text replacement. A replacement can make an originally valid tool-argument string invalid JSON. The finalizer now reuses the strict native JSON observer on the actual selected delivery bytes, including inner arguments, before any publication preparation or activation.

- `src-tauri/src/proxy/security/http_restoration/completion_verifier.rs:632–656,787–825`: private borrowed proof validation and `VerifiedNativeJson::check_source` reject foreign source, binding, client, and actual attempt before finalizer allocation. Consuming `validate_restored` then consumes the original proof and uses existing `NativeJsonEvidence::observe`; it issues no new proof or authority.
- `src-tauri/src/proxy/security/response_restoration/finalization.rs:410–426,477–535`: early source check → strict exact-original gate → mandatory final encoding → select encoded changed bytes or original unchanged `Bytes` → consuming restored-wire validation → release identity/sealed specification → prepare/activate. No candidate reserialization is added. Unchanged delivery preserves the original owner. JSON activation remains before body polling; SSE continuation is untouched.
- `src-tauri/src/proxy/security/tests/c5_native_finalization_tests.rs:809–1170`: four appended controls cover invalid restored inner arguments; safe-tool and zero-mapping wire/type/owner preservation and pre-poll activation; restored outer/inner/digest parser faults at `BeforeParse`/`AfterParse`; and real budget denial after encoding and after outer parse. Failures assert no publication, pending entry, or pin and clean owner/budget release.

No new store, budget owner, completion issuer, finalizer, dependency, schema, UI, setting, credential source, or enabled route is introduced. Token/store/session/generation/provider/operation/actual-attempt boundaries and generic failure projection are unchanged.

### Behavioral RED/GREEN and characterization

Local artifacts are under `.voidx/verification/core1-20261106/`:

- `native-json-revalidation-runtime-red-*`: successful listing of 2 tests, then 1 passed/1 failed at the intended assertion `native_json_invalid_restored_tool_released`. A preceding rejected unknown-carrier fixture is not behavioral RED; the synthetic carrier was corrected to null without relaxing the parser.
- `native-json-revalidation-green-*`: 2 listed/2 passed after minimal implementation.
- `native-json-revalidation-characterization-*`: additional fault/budget controls initially had a tuple-arity compilation failure, then an incorrect budget-boundary fixture expectation. Neither is a security RED. Reading scratch-lifetime accounting showed outer scratch is released before inner parsing; synthetic bulk makes the inner peak exceed that released capacity.
- `native-json-revalidation-formatted-*`: final 4 listed/4 passed/0 ignored. Additional characterization is not misrepresented as behavioral RED.
- `native-json-revalidation-format.json`: only the newly added methods/call block/appended tests are rustfmt fixed points. The pre-existing whole-file `http_restoration.rs` formatting failure is not repaired or accepted by expanding formatting scope.

### Frozen current-source execution

HEAD remains `2b00c4573098f4da31345c788aacf321ef685060`, on macOS arm64 with the previously recorded Rust/Cargo 1.98.1 toolchain. The freeze `native-json-revalidation-current-freeze.json` has SHA-256 `dce6aba010a0c222671a12cc6be40cfebf18c87037dd49245479bdafbf65df53`. It freezes 515 inputs; the canonical runner map contains 500 inputs and has SHA-256 `669caeb418f053e1c6a691f1b92c6b4acbc8c6da992ad38e4d5c1f43b7697359`. Only the three Rust files above changed relative to `native-json-revalidation-before.json`; no Rust input was added. The additional 15 frozen inputs are not claimed to be covered by every focused runner.

Each scope below used `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER` from the repository root. The runner first successfully listed a nonzero set with `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, then ran the same filter with `-- --test-threads=1`. All 22 list/test exits were 0, with equal pre/post maps and `unchanged_during_check: true`. Exact stems, commands, source-manifest hashes and counts are frozen in `runs` in the freeze file. Counts overlap; do not sum them.

| Filter | Listed | Passed | Ignored |
| --- | ---: | ---: | ---: |
| `c5_native_json_revalidation_` | 4 | 4 | 0 |
| `reversible_http_final_send_binding_` | 10 | 10 | 0 |
| `proxy::protocol_matrix_tests::claude_converted_tool_calls_reach_history_from_native_responses` | 1 | 1 | 0 |
| `proxy::protocol_matrix_tests::` | 3 | 2 | 1 |
| `proxy::protocol_matrix_tests::four_protocol_matrix_preserves_text_and_usage_in_json_and_sse` | 1 | 1 | 0 |
| `reasoning_history_redirect_` | 8 | 8 | 0 |
| `proxy::session::tests::` | 34 | 34 | 0 |
| `final_validation_tests::` | 73 | 73 | 0 |
| `reversible_completion_` | 95 | 95 | 0 |
| `unified_core_tests::` | 216 | 216 | 0 |
| `native_driver::tests::` | 28 | 28 | 0 |
| `c3_early_inspection_` | 9 | 9 | 0 |
| `c3_responses_integration_` | 48 | 48 | 0 |
| `reversible_http_` | 65 | 65 | 0 |
| `c3_sse_kind_` | 12 | 12 | 0 |
| `c3_lifecycle_tests::` | 34 | 34 | 0 |
| `c5_native_` | 157 | 157 | 0 |
| `c3_sse_owner_` | 36 | 36 | 0 |
| `text_fallback_` | 66 | 66 | 0 |
| `reversible_response_` | 46 | 46 | 0 |
| `native_route_scope_tests::` | 22 | 22 | 0 |
| `c3_xai_` | 16 | 16 | 0 |

The broad matrix's single ignored fixture is an existing live test and was not executed; both exact synthetic filters also ran separately. The production `CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml` exited 0 with the same stable 500-input pre/post map (`native-json-current-production-check-{source.json,log}`). `git diff --check` exited 0 (`native-json-current-diff-check.log`). Existing unused-import and linker `__eh_frame section too large (max 16MB)` warnings remain; no warning-free or Full Gate claim is made.

### Independent review and post-review identity

Vesper (`6d756a68b5de4da58308ec707a449554`) returned **PASS**: “No mandatory changes found in the three-file increment and four appended tests.” The review accepts consuming proof use, early borrowed identity rejection, actual delivery-byte validation before publication, unchanged owner preservation, and the four controls. Artifact: `native-json-revalidation-vesper-6d756a68-review.json`, SHA-256 `43816af424c3a5dd71278d7ca0b4a6ec0cf5a1e37e87bdd4919ca3badeff8674`. The reviewer used only read/search/find; all execution/hash evidence remains parent-provided.

`native-json-revalidation-post-review-identity.json` records a parent recomputation of all 515 frozen input hashes and the current 500-input runner map, exact equality with all 22 runs' pre/post maps, and equality with the production check's maps. HEAD and freeze SHA also match. Thus the existing checks are reused only for their unchanged recorded Rust scopes. Review/evidence metadata writes do not imply new test coverage. The three review artifacts distinguish historical Juno/Athena source from the current JSON increment; no historical freeze is relabeled as current.

**Unaccepted boundaries:** reusable observed-native-request-through-delivery harness; shared streaming decisions/dependencies/held-unit handoff; adapter-facing contract freeze; whole C3/C4/C5/CORE-1; current Full Gate and corresponding independent review; whole-file formatting; warning-free Clippy; native actual HTTP final-send; live tee; native lanes/enablement; progressive release; remote receipt; renderer/installer/signing/notarization/release acceptance. No staging, commit, push, worktree, application launch/install, real credentials, ignored/live execution, or application-port binding is part of this increment.

**Documentation quality checks:** mixed-audience fresh-reader clarity checked: historical C4 acceptance, current JSON increment, reproduced RED versus characterization, parent execution versus independent review, and still-open gates are explicit. Execution-readiness checked against local manifests and source: exact paths, real filters/commands, counts, source identities, authority/budget invariants, no-enable constraints, and remaining acceptance work are stated. These documentation checks do not close implementation gates.

## 41. Native execution harness and JSON lifecycle handoff: frozen scoped verification

**Outcome: independent scoped PASS for the seven-file increment, with current-source verification and post-review identity closure.** This section covers only the reusable synthetic native execution harness and Chat/Chat JSON lifecycle-to-staging handoff. It does not close C3/C4/C5 or CORE-1. Section 40 remains a historical accepted checkpoint, not the current source identity after these seven-file changes.

### Implemented boundary

- `src-tauri/src/proxy/security/http_restoration/test_transport.rs`: appended generic synthetic JSON/SSE stream entry points call the existing HTTP-private `NativeCompletionDriver`; the harness cannot construct a completion proof.
- `src-tauri/src/proxy/security/tests/native_test_support.rs` and `native_execution_tests.rs`: reusable observed-request preparation, actual-attempt binding, scripted transport and delivery recording. Mapping comes from `RequestObservation` → `prepare_observed_request`, never direct lease seeding. Scripts cover byte boundaries, empty chunks, Pending/wake, errors, parked cancellation, EOF and drop recording. These test-only facilities are registered in `security/mod.rs`.
- `src-tauri/src/proxy/security/restoration_core/native_response.rs`: `stage_native_json_with_lifecycle` composes one `ResponseLifecycle` through Begin/OpenText/Done/Hold/Finish. Its `FinishedInspection` retains the exact native edits, deduplicated dependencies and occurrence count used by staging. The existing charged `fork_attempt_extracted` supplies the lease capability; no second matcher, ledger, budget, store or proof authority is added. The legacy staging entry remains compatible. The entry supports only the existing Chat/Chat verifier and rejects a mismatched client pair.
- `src-tauri/src/proxy/security/restoration_core.rs` reexports the composition entry. `response_restoration/finalization.rs` rejects foreign lifecycle actual-attempt lineage before proof precheck or finalizer allocation. The finalizer still consumes a genuine HTTP proof and validates the final actual wire. JSON activates before body polling; the SSE staging/finalizer and last-yield continuation are unchanged in this increment.

`FinishedInspection` remains an inspection/mechanics owner, not completion, release or publication authority. Unchanged JSON uses the original `Bytes` owner. Zero mapping performs Begin/OpenText/Done/Finish but creates no Patch/Hold; tests do not require an absent Hold hook. Native registration remains `Disabled`.

### RED, GREEN and characterization are separate

Artifacts below are under `.voidx/verification/core1-20261106/`:

- `native-execution-json-red-*`: 1 listed, 0 passed, 1 failed at `native_execution_configurable_json_transport_missing`; `native-execution-json-green-*`: 1/1. `native-execution-sse-red-*`: 2 listed, 1 passed, 1 failed at `native_execution_configurable_sse_transport_missing`; corresponding GREEN: 2/2. These are runtime failures of test-only capability stubs returning `RestoreUnsupported`, not evidence of an enabled production vulnerability.
- `native-execution-lifecycle-characterization-*`: 6 listed/6 passed initially. Partial-reference Hold, charged held units, dependencies/occurrences, abort, sticky injected Delta failure and cleanup characterize existing mechanics; no RED is claimed.
- `native-json-handoff-runtime-red-*`: 1 listed, 0 passed, 1 failed at `native_json_lifecycle_gate_bypassed`. `native-json-handoff-identity-runtime-red-*`: 2 listed, 0 passed, 2 failed, including `native_json_lifecycle_foreign_attempt_or_pair_released`. The temporary entry delegated to legacy staging and ignored lifecycle pair/attempt; these failures establish the missing composition, not an enabled native HTTP vulnerability. `native-json-handoff-runtime-green-*`: 2/2 after implementation.
- The third JSON lifecycle control initially passed as characterization. It covers content and nonexecuting tool-argument dependencies, exact site identity versus same-valued siblings, types, immutable conflict, real budget denial, stage drop, finalizer BeforeParse/AfterParse faults, single-chunk tool delivery and zero-map raw owner preservation. `native-json-handoff-formatted-harness-*`: final 9 listed/9 passed/0 ignored.
- `native-json-handoff-first-green-*` failed compilation because the lease is not Clone; `fork_attempt_extracted` is the correct existing charged capability. `native-json-handoff-tool-characterization-*` initially used the nonexistent `into_stream` instead of `into_delivery_stream`. Neither compile failure counts as behavioral RED.

### Current frozen commands and identity

From the repository root, each test scope uses `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER`: successful nonzero `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, then the same filter with `-- --test-threads=1`. All Cargo work was serialized. The 21 successful scopes below have list/test exits 0, equal pre/post maps, and `unchanged_during_check: true`; counts overlap and must not be summed.

| Filter | Listed / passed | Ignored |
| --- | ---: | ---: |
| `c5_native_execution_` | 9 / 9 | 0 |
| `c5_native_` | 166 / 166 | 0 |
| `unified_core_tests::` | 216 / 216 | 0 |
| `c3_lifecycle_tests::` | 34 / 34 | 0 |
| `reversible_completion_` | 95 / 95 | 0 |
| `reversible_response_` | 46 / 46 | 0 |
| `text_fallback_` | 66 / 66 | 0 |
| `c3_sse_owner_` | 36 / 36 | 0 |
| `c3_responses_integration_` | 48 / 48 | 0 |
| `reversible_http_` | 65 / 65 | 0 |
| `c3_xai_` | 16 / 16 | 0 |
| `c3_sse_kind_` | 12 / 12 | 0 |
| `c3_early_inspection_` | 9 / 9 | 0 |
| `native_driver::tests::` | 28 / 28 | 0 |
| `native_route_scope_tests::` | 22 / 22 | 0 |
| `proxy::session::tests::` | 34 / 34 | 0 |
| `final_validation_tests::` | 73 / 73 | 0 |
| `reasoning_history_redirect_` | 8 / 8 | 0 |
| `reversible_http_final_send_binding_` | 10 / 10 | 0 |
| `proxy::protocol_matrix_tests::claude_converted_tool_calls_reach_history_from_native_responses` | 1 / 1 | 0 |
| `proxy::protocol_matrix_tests::four_protocol_matrix_preserves_text_and_usage_in_json_and_sse` | 1 / 1 | 0 |

An earlier nonexistent matrix filter listed 0 tests (`native-json-handoff-regression-matrix-history-*`); the runner correctly did not execute it. It is excluded from successful scopes and replaced by the two exact synthetic filters above. No ignored/live test was executed.

`native-json-handoff-current-freeze.json` SHA-256: `502edb9902d7f31609d7442b509ba1b4a2d243e1f717daecb33cb7e6f9640ff6`. It freezes 517 Rust/manifest/lock inputs at HEAD `2b00c4573098f4da31345c788aacf321ef685060`; its canonical 502-input runner map has SHA-256 `13ff9ebebf9fc3966e9b57a52659eaf9201f2b7d416ba7a03215912529236159`. The additional 15 build/integration Rust inputs are not claimed as covered by every focused runner. Exactly the seven files identified above differ from section 40's checkpoint. Freeze `runs` records manifests, counts and manifest hashes; each manifest has exact commands in its list/test logs. The parent checked every successful pre/post map against current source.

`CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml` exited 0 with the same stable runner map (`native-json-handoff-production-check.{json,log}`). `git diff --check` exited 0 (`native-json-handoff-diff-check.log`). `native-json-handoff-format.json` limits formatting to the two new harness files and new JSON staging/reexport/transport blocks; JSON prefix/SSE suffix and the transport snapshot prefix remain unchanged. No whole-file format acceptance is claimed. Existing unused-import and linker unwind warnings remain.

### Review and unfinished boundary

Lyra (`f31b41c7ae3d4ec0b68900ae02be38d6`) returned **scoped PASS**, with no mandatory issues in the seven-file increment. References: real request preparation and HTTP-private proof (`native_test_support.rs:55–144`, `test_transport.rs:148–195`); one charged lifecycle owner (`native_response.rs:125–158,310–372`); early lineage check and retained sealed finalizer authority (`finalization.rs:477–535`). The read/search/find-only review ran no commands and recomputed no hashes. Artifact: `native-json-handoff-lyra-f31b41c7-review.json`, SHA-256 `e4507c0e51d41b279d8196bf50b9b5294b0c7b9be6b41ae0708a46ac7adb71d1`.

The parent recomputed all 517 frozen inputs and the 502-input runner map after review; all 21 successful runs' pre/post maps and the production check's maps match this unchanged state. `native-json-handoff-post-review-identity.json` SHA-256: `78166f939c9439b31c32f863acfb5a0f3c945c4e7ac8abc460ecb445f1111411`. This closes only the bounded checkpoint's identity loop, not the open gates. Legacy JSON callers are not all migrated to lifecycle enforcement.

Mixed-audience documentation quality: fresh-reader clarity PASS for the bounded outcome, historical-versus-current source, independent review versus parent execution, and unfinished work; execution-readiness PASS for source/artifact paths, exact commands/filters/counts, hashes and preserved invariants. `native-json-handoff-document-quality.json` records the checks; documentation quality is not implementation acceptance.

Shared streaming lifecycle decisions/dependencies/held units have not yet been handed to native SSE staging/sealed finalization; JSON handoff and separate lifecycle inspection do not establish that requirement. Adapter-facing contract freeze, three disabled empty lane modules compiling against that contract, current Full Gate and independent CORE-1 acceptance remain open. No native route, actual native HTTP final-send, live tee, progressive release, full lane semantics, renderer, Clippy, installer or release acceptance is claimed. All dirty/untracked work is retained; no staging, commit, push, worktree, real credential, application launch or default-port binding occurred.

## 42. Native SSE lifecycle handoff: frozen scoped acceptance

**Outcome: independent scoped PASS for the five-file Chat/Chat SSE increment only.** Section 41 is the accepted historical harness/JSON checkpoint; its source map is not the current complete Rust identity. This section closes the bounded SSE lifecycle-to-staging handoff, not C3/C4/C5, CORE-1 or the adapter-facing contract freeze. Native registration remains `Disabled`.

### Implemented boundary

- `src-tauri/src/proxy/security/restoration_core/native_response.rs` adds `NativeSseTextStream` and `stage_native_sse_with_lifecycle`. Grouping is an adapter declaration of decoded fragments, not completion evidence. Before matcher work, coverage must be complete, unique, exact and ordered within each declared stream; missing, duplicate, reversed or equal-valued foreign sites fail closed.
- A single charged `ResponseLifecycle` performs Begin/OpenText/Delta/Done/Hold/Finish. `FinishedInspection<NativeSseEdit>` retains the edits, dependencies and occurrence ledger consumed by staging; the lifecycle path does not restore a second time through legacy staging. Checked Delta-emitted lengths project restored output back to exact fragments, with the last fragment receiving the remainder. Authorized restoration affecting a stream containing an immutable site is conservatively rejected, not partially released. Zero mapping performs inspection without Patch/Hold.
- `src-tauri/src/proxy/security/restoration_core.rs` reexports the entry and grouping type. `response_restoration/finalization.rs` checks lifecycle actual-attempt lineage before genuine HTTP-proof validation. Existing strict restored-wire validation, sealed keyed access, consuming transfer, charged frame ownership and activation only after the final yield continuation remain intact. `FinishedInspection` is not completion, release or publication authority.
- `src-tauri/src/proxy/security/tests/native_test_support.rs` composes the new entry with observed-request preparation and the genuine HTTP-private completion driver. `native_execution_tests.rs` covers cross-frame restoration, lifecycle rejection, identity, exact coverage, all reference splits, immutable failure, parser/budget denial, cancellation and cleanup. No matcher, proof issuer, store or budget owner is added.

The legacy JSON/SSE compatibility entries remain. This entry supports only Chat/Chat and does not implement complete Chat choice/tool indexing, Anthropic signature/block association or Gemini candidate/Part semantics. SSE sites are capped at 128 and decoded stream/entity size at 1 MiB, using existing charged owners. Fixtures use synthetic values, in-memory storage, nonexecuting tools and scripted streams; no real network or credentials are required.

### Runtime RED versus characterization

All artifact names in this section are relative to `.voidx/verification/core1-20261106/`.

- `native-sse-handoff-runtime-red-*`: successful nonzero listing of 2 tests, then 0 passed/2 failed at the intended runtime assertions `native_sse_lifecycle_cross_frame_reference_not_restored` and `native_sse_lifecycle_gate_bypassed`. The corresponding GREEN passes both after implementation. This establishes the missing handoff, not an enabled native HTTP vulnerability.
- Additional identity, coverage, immutable, all-reference-split, budget, parser and zero-map controls initially passed as characterization; they are not additional RED evidence.
- `native-sse-handoff-interleaved-characterization-*` initially passes: interleaved content and nonexecuting tool fragments form two declared streams, with 3 occurrences/1 dependency. Every yield, including DONE, retains publication state `(published, pending, pins) = (0, 1, 1)`; only the resumed final continuation reaches `(1, 0, 0)`.
- `native-sse-handoff-finalizer-characterization-*` initially passes: BeforeParse/AfterParse fault hooks are installed only after successful staging, and assertions prove the finalizer reached each hook. Separate real budget exhaustion before finalization also fails without publication and cleans up atomically. Earlier parser failures are not counted as finalizer-stage evidence. Failure/cancellation leaves `(0, 0, 0)`.
- Compilation errors and fixture failures are not behavioral RED. `native-sse-handoff-final-harness-*`, after scoped formatting, lists and passes all 16 tests with 0 ignored.

### Current-source verification

Commands run from the repository root on macOS arm64, HEAD `2b00c4573098f4da31345c788aacf321ef685060`, Rust/Cargo 1.98.1. Each scope uses `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py STEM FILTER`: successful nonzero `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, then the same filter with `-- --test-threads=1`. Cargo work was serialized. All 21 scopes have list/test exits 0, unchanged pre/post maps matching the frozen source, and 0 ignored. Counts overlap and must not be summed.

| Filter | Listed / passed |
| --- | ---: |
| `c5_native_execution_` | 16 / 16 |
| `c5_native_` | 173 / 173 |
| `unified_core_tests::` | 216 / 216 |
| `c3_lifecycle_tests::` | 34 / 34 |
| `reversible_completion_` | 95 / 95 |
| `reversible_response_` | 46 / 46 |
| `text_fallback_` | 66 / 66 |
| `c3_sse_owner_` | 36 / 36 |
| `c3_responses_integration_` | 48 / 48 |
| `reversible_http_` | 65 / 65 |
| `c3_xai_` | 16 / 16 |
| `c3_sse_kind_` | 12 / 12 |
| `c3_early_inspection_` | 9 / 9 |
| `native_driver::tests::` | 28 / 28 |
| `native_route_scope_tests::` | 22 / 22 |
| `proxy::session::tests::` | 34 / 34 |
| `final_validation_tests::` | 73 / 73 |
| `reasoning_history_redirect_` | 8 / 8 |
| `reversible_http_final_send_binding_` | 10 / 10 |
| `proxy::protocol_matrix_tests::claude_converted_tool_calls_reach_history_from_native_responses` | 1 / 1 |
| `proxy::protocol_matrix_tests::four_protocol_matrix_preserves_text_and_usage_in_json_and_sse` | 1 / 1 |

`native-sse-handoff-current-freeze.json` SHA-256 is `9530be7813b24b67899b688c123c2439464627bb73c0e15f8048a06268d2e282`. It records 517 frozen Rust/manifest/lock inputs and the five changed Rust inputs above relative to section 41. The canonical 502-input runner map SHA-256 is `0c74b426511f26216486eef9d25e121a500f91db5f0faccbfef5d06950e47375`; the extra 15 build/integration inputs are not claimed as exercised by every focused runner. Its `runs` array records the 21 `native-sse-handoff-final-*-source.json` manifests and hashes; associated list/test logs provide exact commands.

`CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml` exits 0 with stable matching maps (`native-sse-handoff-final-cargo-check-source.json` and `native-sse-handoff-final-cargo-check.log`). `git diff --check` exits 0 (`native-sse-handoff-final-diff-check.log`). `native-sse-handoff-final-format.json` proves a formatting fixed point only for the new SSE staging/reexport blocks and the two harness files; the accepted JSON prefix and sealed keyed finalization suffix remain byte-identical. It does not establish whole-file formatting. The preexisting `http_restoration.rs` whole-file formatting failure, unused-import warning and linker unwind warning remain; no warning-free claim is made.

### Independent review, identity closure and remaining gates

Nexus (`c96de8fcebb54c80802e4433358182fe`) returned **scoped PASS**, with no mandatory findings, for exactly this frozen five-file increment. `native-sse-handoff-nexus-c96de8fc-review.json` SHA-256 is `f263954849a33cea7c5bb4ef52919dc8e5122d62a3a7b76e7e96c266ef1e7fe4`. The reviewer used only read/search/find: no commands, edits, LSP, delegation, independent execution or hash computation. The review accepts exact coverage, the single lifecycle ledger/edits, checked projection, conservative immutable failure and the early actual-attempt gate while retaining genuine proof, sealed transfer and final continuation.

The parent's `native-sse-handoff-post-review-identity.json` SHA-256 is `c9d1d7b202a94120cab0ea7ef2d29d5cbdccd2698ecd972a9bba1b9249636a05`. It confirms all 517 frozen inputs, 502 runner inputs, 21 run artifacts/maps, production-check artifact/maps and diff check after review. The documentation update does not change those Rust inputs; the parent rechecked the 517 frozen inputs and all 21 manifest hashes before writing this section.

**Still open:** whole C3/C4/C5 and CORE-1 acceptance; adapter-facing signatures/visibility/ownership and deliberate unsupported-capability freeze; three disabled empty lanes compiling against the same contract; current Full Gate; separate independent CORE-1 boundary PASS. Legacy callers are not all migrated to lifecycle enforcement. The bounded PASS does not cover full lane semantics, native actual-HTTP final-send, live tee, progressive release or native enablement. No renderer, Clippy, installer, signing, application launch or release acceptance is established here. Existing Responses behavior and P3/P4 work are preserved; no checkbox is closed by this section, and all dirty/untracked work is retained without staging, commit, push or worktree creation.

## 43. Full Gate failure, fixture repair and scoped lint follow-up

**Status: the historical complete Full Gate failed; subsequent focused passes do not close C3/C4/C5 or CORE-1.** This section is for maintainers and implementing/reviewing agents. Artifacts are relative to `.voidx/verification/core1-20261106/`. All execution is parent execution from the repository root on macOS arm64, HEAD `2b00c4573098f4da31345c788aacf321ef685060`, Rust/Cargo 1.98.1. Cargo checks are serialized with `CARGO_BUILD_JOBS=2`; Rust tests require successful nonzero listing followed by `--test-threads=1`. No ignored/live tests, real credentials, installed application, or fixed application port are used.

### Historical complete gate and confirmed causes

`core1-current-full-gate.json` retains the complete pre-repair run with identical pre/post input maps. Typecheck, renderer-only build, unit tests (254 files/3064 tests), Prettier, production Cargo check and diff check exit 0. Rust fmt exits 1 (485 diff hunks); Clippy exits 101. Lib listing succeeds with 6405 tests; execution reports 6388 passed/2 failed/15 ignored. All-target listing succeeds with 6568 tests; execution stops at the same two lib failures, so later integration targets were not executed. These are FAIL results, not a baseline waiver.

Both failures were independently reproduced by the parent on unchanged input maps (`core1-fullgate-repro-{pending,xai}-source.json`: each 1 listed, list exit 0, test exit 101). Read-only comparison with production ownership identified:

- `http_restoration.rs::private_sse_wrapper_charges_pending_frame_and_releases_on_cancel` assumed only attempt provenance remained (32 bytes on arm64). The real baseline is three charged owners: attempt provenance, lifecycle identity, and deferred-owner identity. Each reserves `size_of::<Reservation>() + 2 * size_of::<usize>()`, totaling 96 bytes. The fixture now states that exact accounting; it still requires the pending frame charge (`>= baseline + 18`) and cancellation cleanup to zero.
- `reversible_integration_tests.rs::p1_xai_original_frame_limits_budget_and_cancel` drained the original observer before constructing an unrelated static body. The final wrapper's lent inspection therefore had no original observation and strict reclaim correctly refused it. The fixture now composes original observer -> existing xAI sanitizer -> streaming Body -> final wrapper, matching the single-owner production poll path. `take(0)`/`take(1)` mean original-source non-polling/truncation, not genuine original EOF. Budget denial occurs after wrapper lifecycle setup and before body polling. Exact 1 MiB, over-by-one, budget, cancel, unpolled, fail-closed/no-reference-leak, and unrelated 59-byte reservation cleanup assertions remain.

Only these two fixture functions changed for those failures. No production reclaim, EOF, frame limit, identity, or completion gate was relaxed. Final focused artifacts `core1-fixture-pending-pass-source.json` and `core1-fixture-xai-pass-source.json` each record 1 listed/1 passed, exits 0, stable maps and 0 ignored. This is existing regression reproduction and fixture correction, not new-feature behavioral RED. The intermediate `NamespaceLookup` compile failure and `core1-fixture-pending-green` build-failure artifact are not RED or PASS evidence.

Tess (`aa609cb3eb814c0d8c41ab2bc594f863`) returned **scoped PASS only for the two fixture functions**, with no mandatory issues. Parent transcription: `core1-tess-aa609cb3-fixture-review.json`. Review used only read/search/find, did not inspect the parent execution JSON and performed no execution/hash computation. It excludes overall C3/C4/C5, CORE-1, Full Gate, production behavior and native enablement; original-source cancellation controls do not establish every downstream cancellation point.

### Lint and formatting boundaries

The scope-local lint follow-up moves `ClientProtocol` and Responses serialization/result compatibility imports behind existing `cfg(test)`, removes the unused `LifecycleRejectionGuard` reexport, and retains intentionally frozen native API reexports with an explicit `unused_imports` allowance. Narrow `too_many_arguments` allowances preserve independent authority inputs in `observe_json`, `stage_json_response_bound` and `stage_native_sse_with_lifecycle`. Narrow `large_enum_variant` allowances keep charged lifecycle owners inline; no uncharged Box or API redesign is introduced. No runtime logic, visibility, dependency, schema or budget cap changes.

`CARGO_BUILD_JOBS=2 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0 (`core1-scoped-lint-clippy.log`). This earlier command has no canonical pre/post record and is a scoped diagnostic, not complete-source acceptance; the next Full Gate must reproduce it against the frozen candidate. Whole-file/repository formatting has not been repaired. The historical 485-hunk fmt failure is not permission to expand formatting scope. Linker unwind warnings are not resolved or relabeled warning-free.

## 44. Candidate common adapter contract: signatures, ownership and lane compile proof

**Status: candidate frozen mechanics baseline, not permission to start a native lane.** The common contract below addresses Rune's two historical readiness blockers (missing nested lane compile proof and explicit freeze record). Rune (`d46d872ee0494b8db0b9b4e2d0b77692`) returned **NEEDS_CHANGE for contract readiness only**, not an identified shared API redesign; parent transcription is `core1-rune-d46d872e-readiness-review.json`. Current complete Full Gate and fresh independent boundary acceptance remain required before CORE-1 closure. Section 4 and C3/C4/C5 checkboxes remain open.

### Exact candidate identity and visibility

`core1-contract-candidate-freeze.json` records 518 Rust/build/integration/manifest/lock inputs. Artifact SHA-256: `ac02ca65416149720cafe4d51338cb5b42a3740afb8a98f67bb90d0fbb473111`. Content identity (SHA-256 of the sorted path/hash map serialized with Python `json.dumps(..., sort_keys=True)`): `2eb8bcbdb056097113d60a14b8f1a9a08812f5dc4bb4f407f83bd7130cd4c270`. The focused runner's 503-input canonical map identity is `953854ad7952af2ceb229ddbf3e25b408345f3703d39c8a2a8c6bdf4820ca4e6`. The extra 15 inputs are not claimed exercised by each lib filter. The artifact lists all 11 changed/new Rust inputs since section 42. Evidence-document updates are outside this Rust identity and must be checked separately.

Paths below are relative to `src-tauri/src/proxy/security/`. Contract/preparation `pub(super)` items are visible to descendants of `security`; lifecycle/native types and core reexports use `pub(in crate::proxy::security)` or security-level `pub(super)`. This is crate-internal effective visibility, not a public API grant. Private fields, proof issuers, publication constructors and release keys stay private. No visibility expansion was needed.

### Frozen adapter-facing surface

The signatures below omit module qualification where the named source defines the type; lifetimes and ownership are intentional. Source remains authoritative for generic bounds and method bodies.

| Source | Concrete surface | Ownership and invariants |
| --- | --- | --- |
| `restoration_contract.rs` | `ClientProtocol::new(Protocol)`, `UpstreamProtocol::new(Protocol)`, `ProtocolPair::new(ClientProtocol, UpstreamProtocol)`; `IngressNamespace`, `ContinuityScope`, `Operation`, `Framing`, `SourceClass`, `EditConstraint`, `TextPolicy` | Client/upstream, application/ingress, operation and retry binding are independent roles. A protocol pair is not route authority. Native Gemini acquires no Responses cursor/Compact transfer. |
| `restoration_contract.rs` | `RequestObservation::new(&'native Value, ClientProtocol, PayloadBudget) -> Result<Self, ContractError>`; `observe_text(&mut self, &'native Value, SourceClass, EditConstraint) -> Result<(), ContractError>`; `claim_history(&mut self, HistoryClaim<'native>) -> Result<(), ContractError>` | Exact borrowed native owner, charged metadata, declared source/editability before planning. History/call-result pairing is an untrusted claim, not delivered-call or execution proof. |
| `restoration_contract.rs` | `TextSites::site(usize) -> Option<TextSite<'_, 'native>>`; `TextPatch::new(TextSite<'sites, 'native>, Range<usize>, &'replacement str) -> Self`; `TextSites::plan(&[TextPatch<'_, 'native, '_>]) -> Result<AtomicPatchPlan<'native>, ContractError>`; `AtomicPatchPlan::edits() -> &[NativeTextEdit<'native>]` | Owner-bound opaque native node, not diagnostic path; ranges are decoded UTF-8 byte boundaries. Validate the whole proposal before exposing charged edits. Adapters apply edits to native leaves, never a common response IR; foreign/immutable/overlapping/invalid proposals fail atomically. |
| `request_preparation.rs` | `prepare_observed_request<T>(&mut DlpRequestLease, &RequestObservation<'_>, &SecurityRuntimeConfig, &[&str], &[&str], impl FnMut(&Value, &[u8], &[&str]) -> Result<T, TokenError>) -> Result<DlpPreparedRequest<T>, PlanError>` | Require native request scope and exact node membership. Rebuild, residual inspection, mandatory encoding and validation precede token commit. Existing lease/store/budget own authority; no cloned lease or plaintext fallback. |
| `restoration_contract.rs` | `OriginalResponseObservation::json(&'native Value, UpstreamProtocol, AttemptProvenance, PayloadBudget)` / `frame(&'native [u8], UpstreamProtocol, AttemptProvenance, PayloadBudget) -> Result<Self, RestoreError>`; `claim_node(&mut self, &'native Value) -> Result<(), RestoreError>` | Borrow original entities/frames, including arbitrary UTF-8 splits; charge retained claims. Exact pointer membership only. Neither terminal/EOF, successful conversion, delivery nor completion is inferred. |
| `restoration_core/lifecycle.rs` (core reexports) | `ResponseLifecycle::<T>::begin_native(DlpRequestLease, ActualAttemptBinding, PayloadBudget, ProtocolPair, AttemptProvenance, LifecycleLimits) -> Result<Self, RestoreError>`; `ResponseLifecycle::begin(...)` for `PayloadBuffer` | One non-Clone mechanics owner, charged stream identity, existing matcher/barrier/ledger/budget. `LifecycleLimits` explicitly bounds text bytes/streams, held bytes/units. Binding and attempt lineage remain independent checks, including zero mapping. |
| `restoration_core/lifecycle.rs` | `observe_original(&mut self, &OriginalResponseObservation<'_>) -> Result<(), RestoreError>`; `open_text(&mut self, TextPolicy) -> Result<TextStreamId, RestoreError>`; `delta/done/snapshot(&mut self, &TextStreamId, &str) -> Result<RestoreDecision, RestoreError>`; `arguments(&mut self, &str) -> Result<PayloadText, RestoreError>` | Policy fixed before output. Delta is decoded text; Done is a reconciled full snapshot. `RestoreDecision::{Unchanged, Patch(PayloadText), Hold}` grants no completion/release authority. Shared dependency/occurrence accounting is transactional. Adapter retains native indexing and terminal semantics. |
| `restoration_core/lifecycle.rs` | `hold(&mut self, PayloadBuffer)`; `hold_native(&mut self, T, usize)`; `hold_all(&mut self)`; `deferred_owner(&mut self)` / `adopt_deferred(&mut self, &mut DeferredUnits<T>)`; `finish(self) -> Result<FinishedInspection<T>, RestoreError>`; `abort(self) -> Result<(), RestoreError>` | Hold methods return `Result<_, RestoreError>` and retain charged owners under the same limits. Finish consumes into inspection only, retaining edits/dependencies/occurrences; abort consumes and cleans up, including sticky failure. Inspection does not expose a lease, binding, proof, publisher, resume or release capability. |
| `restoration_core/native_response.rs` | `stage_native_json_with_lifecycle(&NativeJsonObservation<'native>, &DlpRequestLease, &ActualAttemptBinding, ProtocolPair, &AttemptProvenance) -> Result<StagedNativeJson<'native>, RestoreError>` | Exact native sites/snapshot ownership; consumes a single lifecycle inspection ledger for edits. Current executable pair is Chat/Chat. Compatibility `stage_native_json` remains separately named and is not universal lifecycle enforcement. |
| `restoration_core/native_response.rs` | `NativeSseTextStream::new(&'native [&'native Value])`; `stage_native_sse_with_lifecycle(&NativeSseObservation<'native>, &[NativeSseSnapshotSelection<'native>], &[NativeSseTextStream<'native>], &DlpRequestLease, &ActualAttemptBinding, PayloadBudget, ProtocolPair, &AttemptProvenance) -> Result<StagedNativeSse<'native>, RestoreError>` | Adapter-declared grouping is not proof. Complete unique exact per-stream ordered coverage precedes matching. Cross-frame restored fragments use one lifecycle ledger. Charged staging holds required units; immutable restoration conflicts fail closed without partial release. Current executable pair is Chat/Chat. |
| `response_restoration/finalization.rs` (security-scoped reexports) | `finalize_native_json(StagedNativeJson<'_>, &DlpRequestLease, VerifiedNativeJson, &AttemptProvenance, Bytes, usize) -> Result<FinalizedNativeJson, RestoreError>`; `finalize_native_sse(StagedNativeSse<'_>, &DlpRequestLease, VerifiedNativeSse, &AttemptProvenance, usize) -> Result<FinalizedNativeSse, RestoreError>` | Consume genuine HTTP-private proof after early actual-attempt check; validate actual restored delivery bytes using strict parser. JSON activates after encoding/validation before body poll. SSE transfers sealed charged frames and activates only when the final yield continuation resumes. Errors/cancel/drop do not publish. No adapter/handler boolean proof constructor or direct publisher. |

### Common nested-lane compilation proof and executable harness

`protocol_adapters/contract_probe.rs` is included independently inside each nested lane test module:

- `protocol_adapters/chat/tests.rs`: Chat / ChatCompletions.
- `protocol_adapters/anthropic/tests.rs`: Anthropic / Messages.
- `protocol_adapters/gemini/tests.rs`: Gemini / GenerateContent.

The common bodies typecheck request observation/atomic edits/preparation, JSON/frame original observations, lifecycle Delta/Done/arguments/Hold, and separate consuming Finish/Abort paths. They consume caller-supplied authority inputs and only drop `FinishedInspection`. The runtime tests reference these already-typechecked functions without executing them, and assert both JSON/SSE registrations remain Disabled. Local `ProbeError` preserves existing typed errors without requiring new production error traits. `core1-lane-contract-probe-pass-source.json` records successful listing of 3 tests then 3 passed, exits 0, 0 ignored and stable maps. This is compile-scaffold evidence (the plan's nonbehavioral exemption), not runtime native mechanics, protocol verifier acceptance, or feature RED. Intermediate error-trait/build failures are neither behavioral RED nor PASS.

Runtime mechanics remain separately exercised through `tests/native_test_support.rs` and `native_execution_tests.rs`: observed request -> real prepare -> genuine HTTP-private driver -> JSON/SSE lifecycle staging -> sealed finalizer, with scripted arbitrary byte splits, Pending/wake, empty chunks, source errors, Park/cancel, delivery recording, publication absence and budget cleanup. Sections 41–42 are historical accepted checkpoints for that harness; current complete-source verification must be recorded separately.

Reproduce the nested compilation proof from repository root with `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py NEW_STEM disabled_lane_contract_compiles_without_registration`. Require list exit 0/nonzero 3, then test exit 0 with `--test-threads=1`, 0 ignored and matching current canonical map. For full acceptance run the plan section 5 Full Gate, not this probe alone. Shared contract changes after acceptance follow section 4's integrator-owned amendment/review/revalidation rule.

### Deliberately unsupported capabilities and remaining acceptance

No unresolved shared API redesign was identified by the historical readiness review; this does not substitute for fresh boundary acceptance. This candidate deliberately does not implement full Chat choice/tool indexing and terminal rules, Anthropic blocks/signature association, Gemini candidate/Part/signature parsing, or their sealed protocol verifier variants. Those are post-CORE-1 lane responsibilities, with their own behavioral RED/GREEN and ordered integration gates. Current proof composition/lifecycle staging is Chat/Chat only. All native routes remain Disabled; no live native final-send/progressive-release or release/signing acceptance is claimed. Legacy JSON/SSE compatibility callers remain; not all callers are lifecycle-enforced. Unsupported or immutable required restoration fails closed; no unredacted fallback, partial privileged tool release or relaxed budget/identity is authorized.

The candidate freezes shared mechanics and obligations, not every internal function, and does not authorize lanes while the complete Full Gate is failing or fresh independent review is missing. Whole-repository fmt failure cannot be waived by scoped formatting. C3/C4/C5 and every CORE-1 checkbox remain unchanged until current required evidence and corresponding independent PASS justify each closure.

## 45. Current complete execution: tests and lint pass; Rust formatting still blocks acceptance

**Outcome: Full Gate FAIL solely on `cargo fmt --check`; C3/C4/C5 and CORE-1 remain open.** This is current-source execution evidence, not a native-route, release, signing or installer acceptance. It supersedes neither the historical FAIL record in section 43 nor the bounded reviews in sections 35–42.

### Execution identity and results

The parent ran `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-post-fixture-full-gate.py` from the repository root on macOS arm64, HEAD `2b00c4573098f4da31345c788aacf321ef685060`, Rust/Cargo 1.98.1. The runner preserves the earlier failed gate under its original artifact names. `core1-post-fixture-full-gate.json` records every exact command, exit, duration and log, with identical pre/post tracked-and-untracked input maps (`unchanged_during_gate: true`). All 518 section 44 frozen inputs match both maps and remain unchanged after execution. Cargo work was serialized with two build jobs; both Rust test selections first successfully listed nonzero tests, then ran with `--test-threads=1`. Ignored tests were not run.

| Repository-root command | Exit / observed result |
| --- | --- |
| `pnpm typecheck` | 0 |
| `pnpm build:renderer` | 0; renderer only, no app installation or launch |
| `pnpm test:unit` | 0; 254 test files / 3064 tests passed |
| `pnpm format:check` | 0 |
| `CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml` | 0 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | **1; not waived** |
| `CARGO_BUILD_JOBS=2 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 0 |
| `CARGO_BUILD_JOBS=2 cargo test --lib --manifest-path src-tauri/Cargo.toml -- --list` | 0; 6408 listed |
| Same lib command with `-- --test-threads=1` | 0; 6393 passed / 0 failed / 15 ignored |
| `CARGO_BUILD_JOBS=2 cargo test --manifest-path src-tauri/Cargo.toml -- --list` | 0; 6571 listed |
| Same all-target command with `-- --test-threads=1` | 0; lib 6393 passed / 15 ignored, then 163 integration tests passed across 13 nonempty targets; binary/doc targets with no tests are not nonzero acceptance scopes |
| `git diff --check` | 0 |

The all-target run includes the same lib tests again; counts must not be added to the separate lib run. The linker `__eh_frame section too large (max 16MB)` warning persists. No warning-free build, ignored/live execution, actual native-route support, installer, signing or release claim is made.

### Affected controls established by this complete lib execution

`core1-post-fixture-current-scope-inventory.json` is a read-only inventory matching exact names from the current successful lib listing to the current lib execution outcomes. It is **not** a second set of focused executions. Every scope below has nonzero listed names, all matching outcomes `ok`, no missing outcomes and 0 ignored. Counts overlap and must not be summed.

- Harness/native/core/lifecycle: `c5_native_execution_` 16, `c5_native_` 173, `unified_core_tests::` 216, `c3_lifecycle_tests::` 34.
- Responses/failure/ownership: `reversible_completion_` 95, `reversible_response_` 46, `text_fallback_` 66, `c3_sse_owner_` 36, `c3_responses_integration_` 48, `reversible_http_` 65.
- xAI/kind/early/driver: `c3_xai_` 16, `c3_sse_kind_` 12, `c3_early_inspection_` 9, `native_driver::tests::` 28.
- Route/session/final-send: `native_route_scope_tests::` 22, `proxy::session::tests::` 34, `final_validation_tests::` 73, `reasoning_history_redirect_` 8, `reversible_http_final_send_binding_` 10.
- Exact synthetic matrix names `proxy::protocol_matrix_tests::claude_converted_tool_calls_reach_history_from_native_responses` and `proxy::protocol_matrix_tests::four_protocol_matrix_preserves_text_and_usage_in_json_and_sse`: 1 each.
- `disabled_lane_contract_compiles_without_registration`: 3, one in each nested lane context. This re-establishes the common compile-probe evidence on the current post-lint source without claiming the probe mechanics functions executed.

These outcomes cover the affected Responses SSE, xAI delivery, ownership/budget and final-send regressions on the same current source. They do not turn historical behavioral RED, characterization or compile failures into new evidence categories.

### Formatting diagnosis and permission boundary

The current fmt log reports 486 hunks across 29 distinct files. `core1-format-scope-diagnosis.json` records a parent read-only comparison: each affected file was passed to `rustfmt --edition 2021 --config skip_children=true --emit stdout`; no formatted output was written to source. For tracked files, the corresponding HEAD content is formatted, and all current formatting blocks overlap dirty-to-HEAD regions within two context lines. Other affected files are new/untracked in this dirty series. This is a scope locator, not proof that every difference belongs to the latest increment, nor authorization to rewrite accepted historical or user work. Its SequenceMatcher block counts differ from Cargo fmt's hunk count and are not test counts.

The formatting failure therefore cannot be labeled an unrelated clean-HEAD baseline problem, but it also cannot be repaired by silently expanding the latest increment to whole-file formatting across the accumulated dirty series. No source formatting was applied and no formatting waiver was granted. A permitted bounded formatting treatment, followed by applicable current-source verification and independent identity closure, is still required before Full Gate/CORE-1 acceptance. All native registrations remain Disabled, all dirty/untracked work remains preserved, and no plan checkbox is changed by this section.

## 46. C3/C4 review findings, behavioral regressions and minimal amendments

**Outcome: the bounded C3 JSON and C4 local-error amendments have independent scoped PASS; whole C3/C4/C5 and CORE-1 remain open.** Evidence artifacts in sections 46–48 are relative to `.voidx/verification/core1-20261106/`. Historical source identities and gate counts above are retained, not relabeled as current evidence.

### Findings and behavioral test-first evidence

Kai (`0a51c0ae264e4738be59d62ac87953ed`) returned **NEEDS_CHANGE** for the shared C3 response boundary: original completion plus a digest of final JSON did not prove final-client completion semantics, and a genuine completion proof from a different exact attempt could match the same source and complete binding. `core1-kai-0a51c0ae-c3-review.json` preserves the findings. The foreign-donor control is an internal sealed-boundary test, not a claim of an externally triggerable exploit.

Echo (`bfe32db217be42aea313492e4d09e0a4`) returned **NEEDS_CHANGE** for C4: local Claude/ClaudeDesktop and Gemini error envelopes used a code as their message instead of the architecture's fixed sentence. Existing tests encoded that wrong expectation. `core1-echo-bfe32db2-c4-review.json` preserves this finding; that review found no other mandatory trusted-ingress, strict-identity, actual-send or Disabled-registry defect within its inspected boundary, but was not overall C4 acceptance.

The parent added runtime controls before changing implementation:

| Artifact stem (`-source.json`, `-list.log`, `-test.log`) | Listed / runtime outcome            | Evidence category                                                                                                  |
| -------------------------------------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `core1-c3-json-review-red-retry`                         | 7 / 0 passed, 7 failed, 0 ignored   | Intended behavioral RED: `c3_json_invalid_final_completion_published` and `c3_json_foreign_attempt_proof_accepted` |
| `core1-c3-json-review-green`                             | 7 / 7 passed, 0 ignored             | GREEN after exact-attempt and final-wire validation                                                                |
| `core1-c4-fixed-message-red`                             | 22 / 15 passed, 7 failed, 0 ignored | Intended behavioral RED for the fixed local sentence                                                               |
| `core1-c4-fixed-message-green`                           | 22 / 22 passed, 0 ignored           | GREEN after native local-message projection repair                                                                 |

Each recorded listing exited 0 with nonzero tests; runtime RED exited 101 and GREEN exited 0. All four artifacts record unchanged source during their own execution. The earlier `core1-c3-json-review-red` invocation timed out after 120 seconds during compilation and is **not** behavioral RED. Compilation/fixture failures and compile-only probes elsewhere are not recategorized as RED.

The six production-wrapper controls in `http_restoration.rs::private_correctness_tests` start from genuine Chat original completion, then independently corrupt final Responses `status`, `error` or `incomplete_details`, with mapped and zero-map cases. They require no publication, no privileged tool output and no synthetic sensitive-output disclosure. The donor control performs two genuine `ResponsesFixture::admit` / `prepare_attempt("openai_chat")` preparations with identical source and every binding field (`store_id`, namespace, generation, policy revision, provider fingerprint, reusable principal, operation lineage), but different `AttemptProvenance`; recipient staging is independent. It requires denial before encoding, no publication and cleanup of staging charge. The eighth test, `private_c3_json_valid_final_control_preserves_delivery_and_publication`, checks successful mapped/zero-map restored arguments and publication; it is initially passing **characterization**, not a RED claim.

### Minimal production changes and compatibility recovery

- `http_restoration/completion_verifier.rs:494–549` retains a charged exact-attempt clone from genuine inspection in `VerifiedJsonCompletion`. Validation compares full binding, source digest and exact attempt. The read-only accessor in `response_restoration.rs` provides provenance, not a new proof issuer.
- `response_restoration/finalization.rs:163–195` compares the proof with `FinishedInspection::attempt()` **before** mandatory charged encoding. The encoded final bytes then pass `JsonCompletion::observe_raw_json_scoped` and strict protocol completion validation before release permission, publication preparation or activation. JSON `ReleaseIdentity` retains the exact attempt, including zero-map output. Serialization alone is not completion evidence.
- `response_restoration.rs:850–858` derives `StagedJsonResponse.completion_protocol` from the checked client/upstream pair and endpoint. Native Compact remains strict Compact. Converted Chat/Anthropic Compact uses ordinary Responses validation because the existing converters emit that envelope (`transform_codex_chat.rs:2032–2046`, `transform_codex_anthropic.rs:1361–1374`). No response-field-based selection or relaxation of `validate_compact_json_response` was added.
- `http_restoration.rs:471–555` uses `client_message()` for local Claude/ClaudeDesktop/Gemini failures: `The local proxy could not process the request. Please retry.` Status, code and native envelope remain intact. Genuine `UpstreamStatus(_)` retains its existing native code/message projection. The corresponding local assertions in `native_route_scope_tests.rs` and `reversible_completion_tests.rs` were updated.

The first broader lib execution is retained as `core1-post-c3c4-lib-source.json`: 6416 listed, 6396 passed, 5 failed, 15 ignored, stable source. Three failures exposed endpoint-only Compact validator selection against converted ordinary Responses; two exposed stale local-message expectations and an unintended change to genuine upstream projection. Source inspection confirmed converter shape before the minimal pair-based correction; upstream projection was restored. The subsequent `core1-post-c3c4-compat-lib-source.json` records 6416 listed, 6401 passed, 0 failed, 15 ignored and stable source. Neither intermediate failures nor the original finding records were overwritten. Only this increment's new HTTP test block was formatted via rustfmt stdin/stdout; no whole-file formatting of accumulated dirty work occurred.

### Independent amendment re-reviews

Both reviewers used **read/search/find only**: no commands, modifications, LSP, hashes or further delegation. Execution and frozen identities are parent-supplied, not independently reproduced.

- Echo (`57b4e6664e47495095955cef3e452496`), `core1-echo-57b4e666-c3-json-amendment-review.json`: **PASS only for the C3 JSON strict-completion/exact-attempt amendment and shared finalization composition**. Both Kai findings are resolved; no mandatory amendment-specific changes. Inspected actual controls, charged authority, final encoding/validation order, failure cleanup and converted Compact selection.
- Tess (`9b4c111189c34236a344b53af274e3ae`), `core1-tess-9b4c1111-c4-message-amendment-review.json`: **PASS only for the C4 fixed-local-error projection amendment and synthetic controls**. The fixed sentence and genuine upstream compatibility are preserved. Claude/ClaudeDesktop upstream tests lack direct message assertions; production branch inspection supports preservation, but the missing explicit assertions remain an optional coverage limitation, not a mandatory finding.

These PASS verdicts are bound to section 48's parent-recorded source identity. They do not close whole C3/C4/C5, CORE-1, Full Gate, native enablement, release, universal lifecycle enforcement or remote-receipt guarantees. Prior P3/P4 and SSE continuation boundaries remain unchanged.

## 47. Historical common-contract and executable-harness scoped reviews

Pixel (`42e6b1254a5347c2a0685d9a7d8d3743`) returned **PASS only for the common compile probes and sections 43–44 contract readiness**; `core1-pixel-42e6b125-contract-review.json` preserves that result. Rune's nested-lane compile-proof and explicit freeze-record blockers were resolved within that scope. Each lane independently typechecks the common supplied-authority mechanics probe; runtime tests reference those functions and assert Disabled registration, without executing probe mechanics or granting proof/publication authority. There is no inferred verdict from the unavailable historical Vesper run.

Mira (`bba0597865b94c56a6d78da28a60c023`) returned **PASS only for C5 test-only non-Responses executable-harness mechanics/readiness**; `core1-mira-bba05978-harness-review.json` preserves that result (artifact SHA-256 `ab4c7d75dc5d62b3861255ddce3a2de6df0833ea1d533e985d486edc2329f0ab`). The inspected harness creates mappings through real observation/preparation, delegates original-byte proof to the HTTP-private driver, and composes JSON/SSE lifecycle staging with one ledger. Controls cover splits, Pending/wake, transport failure, cancellation at every yield, finalizer hooks, budgets, actual attempt and cleanup. No shared-core blocking redesign stub was identified within that harness scope.

Both reviews were read/search/find only and used the historical section 44 candidate identity. Section 48 re-executes their affected controls, but does not silently promote the historical reviews to a fresh overall verdict after the six-file amendment. Probe source and executable-harness-specific source are unchanged; shared JSON proof/finalization changes are covered by section 46's separate scoped review. Full Chat indexing/terminals, Anthropic block/signature handling and Gemini candidate/Part/verifiers remain later-lane work. Legacy compatibility entries/callers remain; no universal lifecycle-enforcement claim is made. Native registrations remain Disabled. Neither review waives formatting or closes C5/CORE-1.

## 48. Post-amendment current-source Full Gate: formatting still blocks acceptance

**Outcome: Full Gate FAIL solely on Rust formatting; whole C3/C4/C5 and CORE-1 remain open.** This section supersedes section 45 only as current-source execution evidence, not as a waiver or overall boundary acceptance.

### Frozen source and execution

`core1-post-c3c4-candidate-freeze.json` preserves a new 518-input Rust/build/integration/manifest/lock candidate; the historical `core1-contract-candidate-freeze.json` is untouched. Artifact SHA-256: `bdbb02ba6a565f4ba72c0b5815192c42eee9bdb93c543cd1c1202eb47aa96c9e`. Sorted-map content identity: `368935b81984667e64de3ea671ae6bf4bdd9a40f3c652c49605fb602ed7c8100`; focused runner 503-input identity: `dcaebcb0b2a2af7c92ab02f156baf12dee50f1a92e40445b1575f9f4a0886d24`. Both identities use SHA-256 over Python `json.dumps(map, sort_keys=True)`. Six files differ from the historical candidate: `http_restoration.rs`, `http_restoration/completion_verifier.rs`, `response_restoration.rs`, `response_restoration/finalization.rs`, `tests/native_route_scope_tests.rs` and `tests/reversible_completion_tests.rs`, all under `src-tauri/src/proxy/security/`. No authority visibility, budget owner, dependency, schema or route registration was added.

From repository root on macOS arm64, HEAD `2b00c4573098f4da31345c788aacf321ef685060`, Rust/Cargo 1.98.1, the parent ran:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-post-c3c4-full-gate.py
```

`core1-post-c3c4-full-gate.json` records each exact command, exit, duration and log. Its pre/post maps agree on all 1536 recorded repository paths; all 518 frozen inputs match both maps and the post-run source. Rust heavy checks were serialized with two build jobs. Both test selections first successfully listed nonzero items and then executed with `--test-threads=1`; ignored/live tests were not run. Runner process exit 0 means evidence collection finished, **not** Full Gate PASS: individual fmt exit 1 remains decisive.

| Repository-root command                                                               | Exit / observed result                                                                 |
| ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                      | 0                                                                                      |
| `pnpm build:renderer`                                                                 | 0; renderer only, no installation or launch                                            |
| `pnpm test:unit`                                                                      | 0; 254 files / 3064 tests passed                                                       |
| `pnpm format:check`                                                                   | 0                                                                                      |
| `CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml`                 | 0                                                                                      |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                              | **1; 487 hunks / 29 files; no waiver**                                                 |
| `CARGO_BUILD_JOBS=2 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 0                                                                                      |
| `CARGO_BUILD_JOBS=2 cargo test --lib --manifest-path src-tauri/Cargo.toml -- --list`  | 0; 6416 listed                                                                         |
| Same lib command with `-- --test-threads=1`                                           | 0; 6401 passed / 0 failed / 15 ignored                                                 |
| `CARGO_BUILD_JOBS=2 cargo test --manifest-path src-tauri/Cargo.toml -- --list`        | 0; 6579 listed                                                                         |
| Same all-target command with `-- --test-threads=1`                                    | 0; lib 6401 passed / 15 ignored, plus 163 integration tests across 13 nonempty targets |
| `git diff --check`                                                                    | 0                                                                                      |

The all-target execution repeats lib tests; counts must not be added to the separate lib run. Empty binary/doc targets are not nonzero acceptance scopes. The linker `__eh_frame section too large (max 16MB)` warning remains; no warning-free, installer, signing, release or live-route claim is made.

`core1-post-c3c4-current-scope-inventory.json` matches 23 overlapping scopes from the successful lib listing to execution outcomes, not new focused executions. The 22 section 45 filters retain their nonzero counts and all outcomes `ok`, including 16 executable-harness tests, 173 `c5_native_` tests and three lane compile probes. The added `private_c3_json_` scope has eight listed / eight passed. All scopes have 0 ignored and 0 missing outcomes; overlapping counts must not be summed.

### Remaining blockers and allowed next work

The current fmt result is **487 hunks / 29 files**, not the historical 486-hunk result. Section 45's dirty-region diagnosis remains historical context, not a fresh per-hunk classification. Accumulated dirty/untracked formatting differences are not automatically authorized as this amendment's scope. No whole-file formatting, waiver, staging, commit, push, worktree, native-route activation, app installation/launch or real-credential use occurred.

Before closure: obtain an explicitly bounded formatting authorization; preserve all dirty work; make only permitted formatting changes; refresh source identity and affected complete verification; then obtain independent overall CORE-1 boundary acceptance. The overall boundary review has **not** occurred. Contract/lane work and native enablement remain gated. JSON publishes only after charged encoding and strict validation; SSE publishes only after the last yield continuation resumes; invalid completion, failure or cancellation must never publish. No fallback, partial privileged release, weakened identity/budget assertion or new proof authority is permitted. Evidence-document updates after this execution are outside the Rust freeze and require their own documentation checks. All plan checkboxes remain unchanged.

Documentation-only follow-up: repository `pnpm format:check` passes but checks `src/`, not Markdown. The explicit `pnpm exec prettier --check docs/design/security-dlp-unified-core-implementation-evidence.md` reports whole-document style differences. A read-only formatter comparison located ten pre-section-46 blocks and two new table blocks. Only the two new table blocks were formatted; historical sections were not rewritten. A subsequent incremental formatter comparison must show no differences in the amended status line or sections 46–48; full-document Markdown style remains a separate non-waived limitation, not a Rust source change or a passing Markdown check.

## 49. Approved bounded formatting and current complete Full Gate

**Outcome: current-source Full Gate PASS, including Rust formatting. Independent task-wide C3/C4 and C5/CORE-1 boundary verdicts are still required before checklist closure.** Sections 45 and 48 retain their historical FAIL results; they are not overwritten or waived.

### Authorized mechanical change

The user explicitly approved the bounded-format plan after its approval window had timed out. The parent applied exactly **690 formatter difference blocks across the 29 named Rust files**, not a write-capable whole-tree `cargo fmt`. `core1-bounded-format-manifest.json` records original/formatted ranges and bytes, pre/post hashes and the formatter command (`rustfmt --edition 2021 --config skip_children=true --emit stdout PATH`). `core1-bounded-format-original/` retains all 29 original files; `core1-bounded-format-applied.json` records successful range reconstruction, unchanged equal intervals and the fact that only the 29 approved inputs changed among the 518 frozen inputs. No other production logic, test expectation, authority capability or route registration was changed. This mechanical change is not new behavior and makes **no new behavioral RED claim**; prior RED/GREEN and characterization remain separate categories.

The **487** Cargo display hunks in the old log and **690** per-file formatter comparison blocks are different representations, not test counts. The affected eight tracked files were already dirty; the other 21 were untracked. Authorization covered these accumulated-series format blocks, not arbitrary future dirty work or historical Markdown style. Existing dirty/untracked work, P3/P4 and all old artifacts were preserved; there was no staging, commit, push, worktree creation, dependency/schema/UI/settings change or native enablement.

### Current exact identity and reproduction

`core1-post-bounded-format-candidate-freeze.json` records all **518 Rust/build/integration/manifest/lock inputs**. Artifact SHA-256: `38be61bfdd8b0999208518bd7bd073fe44016efa4da2a376728bdbf735640e78`. Sorted-map content identity: `1d7cc08f2bc80c2fae588293c214e966cab7611056c6e4d2bc8a1c610e490f24`. The focused runner's 503-input identity is `6d8f5b308ccca044765ddc7d8f6eef4333d189c41416f151b042da2162405916`. Map identities use SHA-256 over Python `json.dumps(map, sort_keys=True)`; the extra 15 inputs are not attributed to each lib filter. The exact previous freeze remains intact. Evidence/plan documentation updates are outside this Rust identity and require separate checks.

From repository root on macOS arm64, HEAD `2b00c4573098f4da31345c788aacf321ef685060`, Rust/Cargo 1.98.1, the parent ran:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-post-bounded-format-full-gate.py
```

`core1-post-bounded-format-full-gate.json` (artifact SHA-256 `7ebea6ba8398239d939bda4fee1a114aab5dea2b2db45873cc6f1c4cfece294d`) records all commands, exits, durations and logs. All command exits are **0**. All 1536 recorded repository input paths agree before/after the gate; every one of the 518 frozen inputs matches both maps and the current post-run source. Heavy Rust checks were serialized with `CARGO_BUILD_JOBS=2`. Both test selections first listed nonzero items successfully, then ran with `--test-threads=1`. No ignored/live test was run.

| Repository-root command                                                               | Exit / observed result                                                     |
| ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                      | 0                                                                          |
| `pnpm build:renderer`                                                                 | 0; renderer only, no installation or launch                                |
| `pnpm test:unit`                                                                      | 0; 254 files / 3064 tests passed                                           |
| `pnpm format:check`                                                                   | 0; repository-defined `src/` check, not whole-document Markdown acceptance |
| `CARGO_BUILD_JOBS=2 cargo check --manifest-path src-tauri/Cargo.toml`                 | 0                                                                          |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                              | **0; previous Rust fmt blocker resolved within the approved bounds**       |
| `CARGO_BUILD_JOBS=2 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 0                                                                          |
| `CARGO_BUILD_JOBS=2 cargo test --lib --manifest-path src-tauri/Cargo.toml -- --list`  | 0; 6416 listed                                                             |
| Same lib command with `-- --test-threads=1`                                           | 0; 6401 passed / 0 failed / 15 ignored                                     |
| `CARGO_BUILD_JOBS=2 cargo test --manifest-path src-tauri/Cargo.toml -- --list`        | 0; 6579 listed                                                             |
| Same all-target command with `-- --test-threads=1`                                    | 0; same lib outcomes plus 163 integration tests across 13 nonempty targets |
| `git diff --check`                                                                    | 0                                                                          |

Lib tests execute again under all-target; these counts must not be added together. Empty binary/doc targets do not become nonzero scopes. The linker `__eh_frame section too large (max 16MB)` warning persists; passing Clippy is not a warning-free linker/build claim. No installer, signing, release, live-route or remote-receipt acceptance is implied.

`core1-post-bounded-format-current-scope-inventory.json` maps **27 overlapping scopes** from the current successful lib listing to runtime outcomes; this is not 27 separately executed focused commands. Every scope has nonzero listed tests, no failed/missing outcomes. The previous 23 scope counts remain unchanged, including 216 unified controls, 173 `c5_native_` controls, 16 executable-harness controls, eight C3 JSON controls and three lane compile probes. The explicitly named task suites have 95 reversible-completion, 22 reversible-context and 1914 security tests passed. Protocol matrix has three listed / two passed / one existing ignored test, not three executed tests. Ignored tests remain excluded; counts overlap and must not be summed.

### Frozen contract and deliberately unsupported capabilities

Section 44's symbolic signatures, security-scoped effective visibility, ownership and lane paths remain the adapter-facing contract; mechanical formatting changes line locations, not signatures or authority. Current JSON attempt capture, mandatory charged encoding, strict final-wire completion and checked-pair Compact validator selection are the section 46 amendments. The lifecycle continues to own mechanics and charged inspection only, not completion/publication authority. Genuine original-byte completion verification remains HTTP-private; sealed finalizers own release and activation. JSON activates after strict final validation before body poll; SSE only after the final yield continuation resumes. No fallback, privileged partial tool release or publication after invalid completion/cancellation is allowed.

The three compile-only probes independently compile in `protocol_adapters/{chat,anthropic,gemini}/tests.rs`; mechanics bodies typecheck but are not executed. The real non-Responses harness composes request observation/preparation, original HTTP-private proof, JSON/SSE lifecycle staging, finalization, transport/Pending/failure/cancel and publication/refund observations. Executable native proof/staging composition remains **Chat/Chat only**. Full Chat indexing and terminal semantics, Anthropic blocks/signatures, Gemini candidates/Parts/verifiers and production native integration belong to later lanes; native registry entries remain **Disabled**. Legacy JSON/SSE compatibility entries and callers remain; no universal lifecycle-enforcement claim is made. A new shared-contract change needs a new identity, affected checks and independent acceptance; native route enablement requires its later lane/integration gates.

The plan's section 6 also requires a user-approved Git base and isolated worktrees before implementation lanes start. The current user prohibits staging, commits and worktrees. A tested content freeze does not fabricate such a Git base: even a later technical CORE-1 boundary PASS does not authorize starting lane implementation here. Keep lanes operationally paused until that separate workspace/base requirement and explicit authorization are satisfied.

## 50. Whole-task reviews and approved F1 evidence amendment

**Current outcome: C3/C4/C5 and all ten CORE-1 technical items PASS on the post-F1 identity below. Native lane implementation remains operationally paused.** The following three bullets record the pre-F1 review checkpoint; Orion's F1 is resolved by the new characterization evidence and Kai's current-source boundary PASS below, not by rewriting the old verdict. This mixed human/execution record supplements, rather than replaces, the historical reviews and Full Gate.

- Iris (`6a81f1796c8a4ed8bfc24979bc337204`) returned **whole-C3 PASS**, with no mandatory finding or identified shared lane-blocking API/security issue. Record: `core1-iris-6a81f179-c3-full-review.json`. The runner ended with `step_limit` after an explicit task-wide verdict. This is not merely the earlier JSON-amendment review.
- Quill (`f501fbda7de74204a0a1e847f79c4d50`) returned **whole-C4 PASS**, with no mandatory finding. Record: `core1-quill-f501fbda-c4-full-review.json`. Optional limitation: Claude/ClaudeDesktop upstream tests do not directly assert the message field; the production branches were inspected for compatibility.
- Orion (`f6720b3e44f74a14acc8ddeaf669e3ed`) returned **C5/CORE-1 NEEDS_CHANGE**: item 7 lacks non-Responses runtime evidence of an allowed optional-context miss that withholds historical privilege while mandatory current-input processing succeeds. Item 10's overall boundary PASS is withheld. Record: `core1-orion-f6720b3e-core1-full-review.json`. This is an acceptance gap, not a demonstrated forwarding vulnerability. The runner ended with `context_limit`; unseen truncated output is not inferred. Iris's whole-C3 verdict arrived after Orion's cutoff.

All three reviewers used read/search/find only; execution and identity were parent-supplied. Their scope excludes native route enablement, later protocol semantics, installer/signing/release/live acceptance and remote receipt.

### Approved bounded F1 work

The user explicitly approved continuation after the F1 checkpoint timed out. Approval covers only an equivalent extraction of the two existing HTTP-private optional-context decisions in `security/http_restoration.rs`, a test-only entry in `http_restoration/test_transport.rs`, and fixture/controls in `tests/native_test_support.rs` and `tests/native_execution_tests.rs`. No production native degradation operation is added. Mandatory failures, explicit Block, capacity/encoding failures and strict authority checks remain fail-closed. No new store, budget owner, proof issuer, dependency, schema, setting or route is permitted.

The fixture must obtain genuine delivered native history, expire its optional in-memory context, observe a miss under a legitimate current binding, withhold historical authority and still perform current mandatory inspection/redaction/restoration through the existing JSON/SSE proof/finalization composition. It must also exercise non-degradable failures without releasing an unredacted fallback or publishing invalid completion. These are characterizations of existing behavior; first-run success is not behavioral RED. A necessary new production behavior requires a separate genuine runtime RED before implementation.

Verification starts with `CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py core1-f1-before proxy::security::unified_core_tests::`. New controls use the `c5_native_execution_context_degradation` prefix; the same runner must list nonzero tests before single-threaded execution. After edits, create a new 518-input freeze, run a serialized `run-post-f1-full-gate.py`, inspect each command result, then obtain independent current-source C5/CORE-1 review restricted to read/search/find. Old identities and artifacts remain intact as historical checkpoints. Checkboxes close only with current verification and corresponding independent PASS. Section 49's Git-base/worktree operational hold remains unchanged.

### Executed F1 scope and authority controls

Only four frozen inputs changed after Section 49: `security/http_restoration.rs`, `http_restoration/test_transport.rs`, `tests/native_test_support.rs`, and `tests/native_execution_tests.rs` (relative to `src-tauri/src/proxy/`). The private `optional_history_miss(DlpForwardError) -> Result<(), DlpForwardError>` accepts only `ContextUnavailable` and propagates every other error unchanged. Its production callers are the same two existing HTTP optional-history boundaries with retained diagnostics. The native composition is test-only: it validates the current native binding first, performs genuine exact-delivery verification/import, and invokes that same private decision. There is no new production native degradation operation or global downgrade.

`ExpiredNativeHistory` prepares a genuinely observed request, obtains HTTP-private original-response proof, finalizes and records delivery, and positively replays/imports that exact history while live. It then expires the real in-memory namespace (`idle_ttl` one second; `expire_idle_at` sixty seconds ahead), asserts expiration, and admits a legitimate current binding using `admit_security_runtime()?.restoration_policy_revision`. No completion, authority mapping or policy revision is fabricated.

The three `c5_native_execution_context_degradation` characterizations establish these separate outcomes:

- Actual optional-history loss withholds the old token's authority before and after a current mapping is created; current mandatory redaction and restoration still succeed through JSON and SSE. The current binding remains independently valid.
- Wrong generation is rejected before the optional-history decision. Explicit Block, capacity exhaustion, encoding allocation failure and mandatory preparation-validation failure preserve their typed errors and precommit atomicity; mappings and budget do not leak.
- Mandatory response rebuild failure neither releases a fallback nor publishes completion. JSON activates only after strict final validation and encoding, before body polling. SSE retains `(published, pending, pins) = (0, 1, 1)` through all three yields and reaches `(1, 0, 0)` only when the final continuation resumes. Final charged budget is zero.

The first run listed three tests successfully but failed all three with `native_history_current_binding_invalid` **before reaching the optional-miss decision**. This is a retained fixture failure, not behavioral RED: the fixture's fixed revision `1` disagreed with the actually published policy revision. Capturing the real admission revision repaired that prerequisite without weakening the identity assertion. `core1-f1-characterization-{source.json,test.log}` retains the failure; `core1-f1-characterization-policy-admission-{source.json,test.log}` records the three passing controls. F1 is unchanged-behavior characterization plus equivalent extraction, not a newly repaired forwarding vulnerability. The earlier C3/C4 genuine runtime RED/GREEN evidence remains separate.

From the repository root, focused checks used this existing runner with `CARGO_BUILD_JOBS=2`; each successful run first listed a nonzero selection and then executed it with `--test-threads=1`:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py core1-f1-before proxy::security::unified_core_tests::
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py core1-f1-formatted-characterization c5_native_execution_context_degradation
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py core1-f1-native-regression c5_native_execution_
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-focused.py core1-f1-unified-regression proxy::security::unified_core_tests::
```

Results, recorded in the corresponding `*-source.json` and list/test logs: baseline **216/216**, formatted F1 **3/3**, native harness **19/19**, unified regression **216/216**. Pre/post input maps were stable for each run. The baseline precedes the four edits and is not current-source acceptance; the other three match the current 503-input focused identity `28014c0aa698740efd4c76552ca5413ff62345a209e13069a270a1cf224a9d75`. `core1-f1-local-format.json` limits formatting to the four edited files' new/changed ranges: no HTTP-file difference, two test-transport blocks, seven support blocks and 33 execution-test blocks. The old 29-file/690-block authorization was not expanded to arbitrary later dirty work.

### Current post-F1 freeze and complete Full Gate

All artifacts in this section are relative to `.voidx/verification/core1-20261106/`. `core1-post-f1-candidate-freeze.json` records **518 inputs** at sorted-map content identity `ce68663772ada37be5a34db78c54bcdbc7e99062a657d21621a6bfac4582c9aa`; artifact SHA-256 `bbd996021d66149017e59c789c606a599af13943bc88c27f05cd9a8ac3834d4c`. Exactly the four approved inputs differ from Section 49's freeze. HEAD remains `2b00c4573098f4da31345c788aacf321ef685060`; execution context remains repository-root macOS arm64, Rust/Cargo 1.98.1. This dirty-source content freeze is not a new Git revision or an approved lane base.

The parent ran:

```bash
CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-post-f1-full-gate.py
```

The foreground tool timed out after the four frontend checks had exited 0; Cargo check had not completed. The timeout was not a passing command or completed gate. After a successful process check found no residual runner/Cargo process (`pgrep` exit 1), the parent confirmed the source map still equaled the original gate pre-map, preserved `core1-post-f1-full-gate-interrupted.json` and `core1-post-f1-full-gate-interrupted-check.log`, and resumed using `nohup env CARGO_BUILD_JOBS=2 python3 .voidx/verification/core1-20261106/run-post-f1-full-gate-resume.py`. That runner reused only successful frontend entries, reran the incomplete Cargo check and serialized all remaining checks. Driver output is retained in `core1-post-f1-full-gate-resume-driver.log`.

The resulting `core1-post-f1-full-gate.json` (SHA-256 `756a34e4056ccd16e7363b5eb0d73ec822ab7c54d026d9b2bbe12c0fdbccd442`) contains **12 individual exit-0 results**. All 1536 recorded repository paths were identical across gate pre/post; every frozen input matched both maps. Heavy Rust checks were serial with `CARGO_BUILD_JOBS=2`. Both Cargo test selections first listed nonzero tests successfully, then executed single-threaded; ignored/live tests were not run.

| Repository-root command                                            | Exit / observed result                                                 |
| ------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| `pnpm typecheck`                                                   | 0                                                                      |
| `pnpm build:renderer`                                              | 0; renderer only, no installation or launch                            |
| `pnpm test:unit`                                                   | 0; 254 files / 3064 tests passed                                       |
| `pnpm format:check`                                                | 0; repository-defined `src/` check, not whole-document Markdown format |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | 0                                                                      |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | 0                                                                      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | 0                                                                      |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml -- --list`  | 0; 6419 listed                                                         |
| Same lib command with `-- --test-threads=1`                        | 0; 6404 passed / 0 failed / 15 ignored                                 |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --list`        | 0; 6582 listed                                                         |
| Same all-target command with `-- --test-threads=1`                 | 0; same lib outcomes plus 163 integration tests / 13 nonempty targets  |
| `git diff --check`                                                 | 0                                                                      |

All-target execution reruns the same lib tests; do not add those counts. `core1-post-f1-current-scope-inventory.json` records **28 overlapping inventories**, not 28 separately executed focused commands. All have nonzero selections with no failed/missing outcomes: native harness 19, `c5_native_` 176, F1 three, unified 216, C3 lifecycle 34, C3 JSON eight, shared lane probes three, security 1917, reversible-completion 95, context 22, routes 22, session 34, final validation 73, redirect eight and final-send ten. Protocol matrix is three listed / two passed / one existing ignored, not three executed. The linker `__eh_frame section too large (max 16MB)` warning persists; Clippy PASS is not a warning-free linker/build claim.

### Independent current-source acceptance and remaining boundaries

Kai (`b6b19faaa977489bbb56212766e5aecb`) returned explicit **PASS** for the approved four-file F1 increment, preservation of prior whole-C3/C4 PASS, C5's technical contract freeze and all ten CORE-1 technical items. No mandatory technical finding remains in that reviewed scope. Record: `core1-kai-b6b19faa-post-f1-review.json`. It is a parent transcription of the visible return: runner `completed; finish_reason=step_limit`, with a tool-reported 3522-character truncated tail. The explicit overall verdict, scoped verdict table, findings and all ten item verdicts were visible; omitted text is not inferred. The independent reviewer used only read/search/find; commands, edits, LSP, hashing and further delegation were prohibited. Execution evidence and content identities were supplied by the parent, not independently reproduced or hashed.

| CORE-1 item | Current independent technical decision and evidence boundary                                                                                                                                                                |
| ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1           | PASS: prior whole-C3/C4 acceptance preserved; current affected Responses/Compact and security regressions pass. Client results and withheld authority are checked separately.                                               |
| 2           | PASS: concrete contracts, ownership and request/response/finish/abort operations; Section 44 records signatures and effective visibility.                                                                                   |
| 3           | PASS: owner-bound opaque native nodes, exact membership and decoded UTF-8 range validation; no value-equality authority.                                                                                                    |
| 4           | PASS: immutable conflicts, atomic rebuild/inspection/encoding before commit and retained charged staging ownership; later protocol-specific signature association remains deferred.                                         |
| 5           | PASS: independent client/upstream, namespace, identity, policy, provider/principal, operation and exact-attempt roles; no inherited Responses transfer to native operations.                                                |
| 6           | PASS: ordered protocol evidence, no client-result attestation, HTTP-private proof construction and sealed release/activation; claims and pairing grant no authority.                                                        |
| 7           | PASS: real non-Responses shared API execution now includes optional-history loss, absent historical privilege and continuing mandatory current processing.                                                                  |
| 8           | PASS: executable native preparation, genuine original proof, JSON/SSE staging/finalization, splits, Pending, transport/cancel/fault and publication/refund checks.                                                          |
| 9           | PASS: native registration remains Disabled; existing Responses admission and both transports' final-target/history/redirect checks remain intact, including zero-map cases.                                                 |
| 10          | PASS, technical scope: three disabled lanes compile the same contract probe; signatures, visibility, lane paths and unsupported capabilities are recorded, with no identified unresolved shared lane-blocking API decision. |

Section 44's frozen contract and lane paths, Section 46's JSON proof/final-wire/Compact amendments and this section's F1 evidence define the accepted technical boundary. `FinishedInspection` remains mechanics/inspection only, not completion or publication authority. Exact binding/attempt, mandatory charged encoding, independent strict original/final completion validation and sealed activation are preserved. There is no plaintext fallback, privileged partial tool release, budget/identity relaxation or publication after invalid completion/cancellation.

**Closure:** the plan's C3/C4/C5 rows and CORE-1 items 1–10 are checked only after current verification and the corresponding independent PASS. Earlier amendment-only reviews are not substituted for whole-task reviews. Historical failures, Orion's NEEDS_CHANGE and all old freezes/logs remain preserved. Final document/source checks are recorded separately in `core1-post-f1-doc-closure.json`; documentation is outside the 518-input Rust identity.

**Still not accepted or authorized:** native executable proof/staging remains Chat/Chat only; full Chat indexing/terminal semantics, Anthropic blocks/signatures, Gemini candidates/Parts/verifiers and production native integration are later-lane work. Legacy JSON/SSE compatibility entries/callers remain, so this is not universal lifecycle enforcement. Native registrations stay **Disabled**. Kai explicitly classified operational lane-start prerequisites **NEEDS_CHANGE — implementation remains paused**: plan Section 6 still requires a user-approved Git base and isolated worktrees, while the current user prohibits staging/commits/worktrees. A content identity cannot stand in for that base; no lane is started. No installer, signing, notarization, live provider, remote receipt or release acceptance is implied. All dirty/untracked work and P3/P4 remain preserved; no dependency, schema, UI, persistent setting or credential source was added.

## 51. Approved baseline commit, isolated workspaces and request-lane RED

### Authorization and immutable starting point

The user's later explicit instruction to commit completed work first and then follow the plan superseded the earlier staging/commit/worktree hold for this baseline and the planned isolated workspaces. Section 50's technical PASS and historical operational NEEDS_CHANGE remain unchanged; the operational prerequisite was satisfied afterward, not waived.

- Accepted CORE-1 commit: `ca7f883c742df07abe9bcaa9b0eade02aae5e119`; parent `2b00c4573098f4da31345c788aacf321ef685060`.
- English message: `refactor(security): freeze verified unified DLP core contracts`.
- Exactly 65 intended files (62 Rust, three design documents); every committed blob matched its accepted snapshot. Main was clean immediately afterward. Local verification artifacts were excluded; no push occurred.
- The accepted 518-input identity remains `ce68663772ada37be5a34db78c54bcdbc7e99062a657d21621a6bfac4582c9aa`. Its Full Gate is baseline evidence, not evidence for later lane modifications.
- Local records under `.voidx/verification/core1-20261106/`: `core1-approved-baseline-commit-manifest.json`, `core1-approved-baseline-commit.json`, `core1-native-lane-workspaces.json` and `core1-native-lane-cache-clones.json`. Workspace/cache records are creation-time snapshots, not test results.

Each initially clean worktree starts at the exact accepted commit:

- `.voidx/native-lanes/chat`: `feat/dlp-native-chat`, exclusive target `.voidx/cargo-targets/chat`.
- `.voidx/native-lanes/anthropic`: `feat/dlp-native-anthropic`, exclusive target `.voidx/cargo-targets/anthropic`.
- `.voidx/native-lanes/gemini`: `feat/dlp-native-gemini`, exclusive target `.voidx/cargo-targets/gemini`.

The targets are independent APFS copy-on-write cache clones, not symlinks or shared hardlink caches. Cache reuse establishes no execution PASS. Unrelated existing worktrees remain untouched. Lane owners modify only their own `src-tauri/src/proxy/security/protocol_adapters/<lane>/` files; shared amendments remain integrator-only.

### First request-task executions

The parent runs heavy Cargo checks globally serial with `CARGO_BUILD_JOBS=2`, one target per lane, and a global `fcntl.flock`. The local `run-native-lane-focused.py` runner records complete repository input maps before/after each attempt. It executes `cargo test --lib --manifest-path src-tauri/Cargo.toml <filter> -- --list`, requires successful nonzero discovery and unchanged inputs, then runs the same filter with `-- --test-threads=1`. All attempts below had stable inputs. They used synthetic fixtures and in-memory stores, no live provider, ignored tests, application launch/install or production port.

- **AN1 prerequisite failures, not complete behavioral RED:** `an1-request-red-01` listed 11; one probe passed, ten tests failed. Four reached `an1_native_observation_missing`; six failed earlier with `Unavailable` because runtime admission was not initialized. `an1-request-red-02-fixture` did not compile due to a test-helper delimiter error; runtime execution was not attempted. Both records remain preserved and are not misreported as full RED.
- **AN1 genuine runtime RED:** `an1-request-red-03-fixture` listed 11, then one probe passed and all ten new tests failed exactly at `an1_native_observation_missing` (exit 101). A tests-only RAII guard publishes a synthetic runtime, uses the actual admitted restoration-policy revision, and restores the previous runtime on drop. The production observation seam still returned a fixed error during RED.
- **GE1 genuine runtime RED:** `ge1-request-red-01` listed 16; three passed and thirteen failed (exit 101). Two failures reached generation/framing assertions and eleven reached `ge1_request_observation_missing`. The passes are the unchanged Disabled probe and two unsupported-route/framing characterizations, not new RED. The tests-only admission guard uses the actual published revision before execution; both production seams remained fixed-error.
- **CH1 genuine runtime RED:** `ch1-request-red-01` listed 14; one probe passed and all thirteen new tests failed (exit 101). Eleven reached `chat_ch1_observation_missing`; two reached precise metadata-byte/site-count capacity assertions instead of accepting the seam's generic error. The fixture reuses the existing synthetic runtime owner, actual admitted revision and real scoped native lease.

Each stem has separate `-source.json`, `-list.log` and, when executed, `-test.log` artifacts in the local evidence directory. The linker still reports `__eh_frame section too large (max 16MB)`; no warning-free claim is made.

### Current scope and next gates

These RED results authorize only the matching minimal request task, not an independent claim that deeper preparation assertions have already run. CH1 and AN1 minimum implementations are being prepared for parent-controlled GREEN execution. GE1 adds resource-boundary tests before implementing the corresponding new behavior; no new RED is claimed until parent execution. Depth/work exhaustion coverage and other reported omissions must be resolved or explicitly bounded before handoff.

All three protocol-state tables and original compile probes remain in lane `tests.rs` files. `HistoryClaim`, source classes and call/result pairing remain inspection metadata, never authority. Native registry entries remain **Disabled**. No shared API workaround, new dependency, schema, UI, persistent setting or credential source is authorized. The earlier 29-file/690-block formatter approval applies only to the committed baseline, not these lane diffs; only bounded newly changed lane files may be formatted.

Next evidence must record current GREEN results, resource negatives, security regressions and independent protocol review before lane handoff. Use `proxy::security::protocol_adapters::chat::tests::`, `proxy::security::protocol_adapters::anthropic::tests::` or `proxy::security::protocol_adapters::gemini::tests::` for the matching task, then `proxy::security::` at handoff, always through nonzero listing and single-thread execution in that lane's worktree/target. Complete CH2/CH3, AN2/AN3 and GE2/GE3 without enabling routes; follow M2 Chat, M3 Anthropic, M4 Gemini sequential integration and acceptance, then M5. Current native proof/staging remains Chat/Chat only; any required shared extension needs an integrator amendment, TDD, current-state verification and independent review. No native lane, integration, release or overall-plan completion verdict is recorded here.

## 52. Request-lane GREEN, characterizations and scoped review findings

This mixed human/execution checkpoint supersedes only Section 51's then-current pending-execution status. It preserves the accepted baseline, historical failures and all later gates. The three isolated worktrees still start at `ca7f883c742df07abe9bcaa9b0eade02aae5e119`; request changes have not been integrated. Native registrations remain **Disabled**.

### Parent execution evidence

The parent used Section 51's globally serialized runner with `CARGO_BUILD_JOBS=2`, independent lane targets, successful nonzero listing, and only then single-thread runtime execution. Every record below reports `unchanged_during_check: true`. At this checkpoint the parent compared each lane's current inputs with its latest security-regression post-run map: zero mismatches in all three worktrees. Artifacts are under `.voidx/verification/core1-20261106/`; each stem retains separate source, listing and runtime records.

| Lane / scope                                    | Evidence stem                         | Listed / runtime result          | Interpretation                                                                            |
| ----------------------------------------------- | ------------------------------------- | -------------------------------- | ----------------------------------------------------------------------------------------- |
| CH1 added boundaries                            | `ch1-boundary-characterization-01`    | 5 / 5 passed                     | Initial-pass characterization, **not RED**                                                |
| CH1 current request suite                       | `ch1-request-green-02-boundary`       | 19 / 19 passed                   | Original request RED plus five later characterizations                                    |
| Chat shared security                            | `ch1-security-regression-02-boundary` | 1935 / 1935 passed               | Isolated Chat worktree only                                                               |
| AN1 added resources                             | `an1-resource-characterization-01`    | 4 / 4 passed                     | Initial-pass characterization, **not RED**                                                |
| AN1 current request suite                       | `an1-request-green-02-resource`       | 15 / 15 passed                   | Original request RED plus four later characterizations                                    |
| Anthropic shared security                       | `an1-security-regression-01`          | 1931 / 1931 passed               | Isolated Anthropic worktree only                                                          |
| GE1 resources before implementation             | `ge1-resource-red-01`                 | 4 / 0 passed, 4 failed; exit 101 | Genuine runtime RED: one excessive-depth assertion and three missing-observation failures |
| GE1 request suite before review correction      | `ge1-request-green-01`                | 20 / 20 passed                   | Does not establish signed-field alias completeness                                        |
| Gemini shared security before review correction | `ge1-security-regression-01`          | 1936 / 1936 passed               | Predates the mandatory review correction below                                            |

All GREEN/characterization rows have exit 0 and zero failed/ignored tests. Counts overlap; do not add request and security scopes or treat them as integrated-source acceptance. CH1's earlier GREEN was 14/14 and security 1930/1930; AN1's earlier GREEN was 11/11. Those historical records remain intact. The baseline Full Gate does not cover these lane diffs. The linker still reports `__eh_frame section too large (max 16MB)`.

Request production scope is limited to each lane's `request.rs` and `mod.rs`, with tests in that lane's `tests.rs`. Chat borrows supported text and modern function-argument strings, preserves role/position source classes and untrusted results, and delegates atomic preparation to the existing shared owner. Anthropic borrows system/message text and recursive tool-input string values; readable thinking is immutable from the outset. Gemini classifies only the approved generation URI/query framings and observes native text and Struct string values without serialization. None of these observations, history claims or successful preparations grants completion, release or publication authority.

### Independent read-only decisions

Reviewers were restricted to read/search/find: no commands, modifications, LSP, hashing or further delegation. Execution and source-stability evidence came from the parent. Local parent-transcription records preserve the visible verdicts and runner boundaries; no omitted output is inferred.

- **Orion `ca22eea9bd0e42788c56532d7263456e`: PASS FOR CH1 REQUEST-ONLY CURRENT REVIEWED SCOPE.** No mandatory finding. Runner: `completed; finish_reason=contract_unsatisfied`; the complete visible scoped verdict was returned. The reviewed source had the initial 14-test suite, not the later five boundary characterizations. Production code is unchanged since that review; the old PASS is not extended to the new test increment. Record: `ch1-orion-ca22eea9-request-review.json`.
- **Sol `7db277b482e84689b5d9212c719589de`: PASS FOR AN1 REQUEST-ONLY CURRENT REVIEWED SCOPE.** No mandatory finding; current 15-test request suite and 1931-test security records were inspected. Runner: `completed`. Record: `an1-sol-7db277b4-request-review.json`.
- **Nexus `c966302ce6db42cca96dbecb2d11a882`: FAIL FOR GE1 REQUEST-ONLY CURRENT REVIEWED SCOPE.** Mandatory F1: `gemini/request.rs` determines whole-Part immutability from `thoughtSignature` alone. The recognized original protobuf spelling `thought_signature` is missed, so supported readable text/call/result leaves can be classified editable while their signature remains. The parent confirmed the single-spelling predicate before any correction. Runner: `completed; finish_reason=contract_unsatisfied`; the visible result contains this finding and explicit incomplete boundaries. Record: `ge1-nexus-c966302c-request-review.json`. No additional mandatory flaw was identified by this review; that is not a GE1 PASS.

F1 follow-up is test-first: add original-spelling signed text, call-argument and result coverage, including mandatory-patch and authorized-reference atomic failures; obtain parent-controlled genuine runtime RED before touching production; then recognize both spellings without inspecting, renaming or discarding the signature. Rerun Gemini request and shared-security selections and obtain focused independent re-review. Historical GREEN does not close F1.

### Remaining coverage and handoff boundaries

CH1's five characterizations cover malformed/unknown carriers, exact metadata capacity, 4096/4097 sites, and observation success followed by shared deep-tree preparation failure. AN1's four cover exact metadata capacity, 4096/4097 observation sites, generous interior/excessive recursive depth, refunds and unchanged store/lease state. These initial passes are not retroactive behavioral RED.

The 4 Mi visited-node exhaustion boundary for AN1/GE1 and shared aggregate work for CH1 remain untested; exact native depths 64/65, AN1 end-to-end preparation at 4096 sites, and exhaustive malformed/source-class combinations are also not established. No million-object fixture or test-only substitute is claimed as production-path evidence. Observation is not schema validation, and synthetic signatures do not prove live-provider acceptance. These limitations need explicit disposition before full lane handoff.

CH2/CH3, AN2/AN3 and GE2/GE3 remain unimplemented. Executable shared native proof/staging is still **Chat/Chat only**; any Anthropic/Gemini extension is an integrator-owned bounded amendment requiring TDD, affected verification and independent review. There is no full native-lane protocol PASS, HTTP enablement, M2–M4 integration, M5, new Full Gate, live-provider or release acceptance. Integrate only in order **M2 Chat → M3 Anthropic → M4 Gemini**, after each lane's own handoff and integration gates. Do not enable registry entries or weaken exact binding/attempt, strict completion, continuation, budgets or failure-closed policy on the strength of this checkpoint.

## 53. GE1 signature-alias correction and request-review closure

**Current request checkpoint: CH1 19/19, AN1 15/15 and GE1 33/33 pass; isolated shared-security selections pass 1935/1931/1949.** These are separate worktree results, not integrated-source or full-lane acceptance. Section 52's open F1 is now resolved by the following TDD increment and independent re-review; its historical FAIL remains preserved.

### GE1 F1: runtime RED before the predicate correction

The [ProtoJSON field-name rule](https://protobuf.dev/programming-guides/json/#format-description) accepts both lowerCamelCase and original proto names. `thought_signature` therefore cannot be treated as arbitrary unknown metadata when determining whether a Gemini Part is signed. Only signature presence is used; contents remain opaque.

Echo `4a81b8397cb241bdac2e7d962d95c856` appended 13 tests in Gemini `tests.rs`, preserving the original 1030-line prefix. The parent ran the existing serialized runner against `proxy::security::protocol_adapters::gemini::tests::ge1_signature_alias_` before changing production:

- `ge1-signature-alias-red-01`: successful listing of **13**; **3 controls passed / 10 genuine runtime failures**, exit 101. One failed initial immutable classification, six reached actual shared preparation for mandatory Redact/Tokenize in text/call/result carriers, and three reached actual preparation for exact current-authorized references. No compile or fixture failure is counted as RED.
- The parent then changed only `gemini/request.rs` relative to that RED snapshot: whole-Part immutability now checks `thoughtSignature` **or** `thought_signature` before observing any leaf. No signature decoding, renaming, fallback or shared API change was introduced. Only this owned file was formatted.
- `ge1-signature-alias-green-01`: **13 listed / 13 passed**, exit 0.
- `ge1-request-green-02-signature-alias`: **33 listed / 33 passed**, exit 0.
- `ge1-security-regression-02-signature-alias`: **1949 listed / 1949 passed**, exit 0.

All four manifests report stable inputs, successful nonzero discovery before single-thread runtime, `CARGO_BUILD_JOBS=2` and the independent Gemini target. The parent confirmed that the RED-to-GREEN source-map difference is only `request.rs`, the old test prefix is identical, and current inputs match the latest 1949-test post-run map. `ge1-signature-alias-parent-check.json` records these checks plus bounded Rust formatting and diff checks. Every stem's source/list/test artifacts remains separate and immutable.

Tests exercise synthetic real scoped admission, current exact mapping ownership, original input/store preservation, an earlier editable sibling, validator-not-called failures, existing typed `PlanError::Unsupported`, and complete budget refunds. Unsigned carriers continue redaction/tokenization; unchanged signed units preserve both original signature spellings. These request failures neither publish completion nor grant release authority.

### Independent decisions on the current increments

- **Cipher `9a131a8d30f74fd5a3ae6d78b267e3b7`: PASS FOR GE1 REQUEST-ONLY CURRENT REVIEWED SCOPE — F1 RESOLVED.** No remaining mandatory finding in this bounded review. The reviewer inspected the current predicate, appended tests and parent RED/GREEN/security evidence. Record: `ge1-cipher-9a131a8d-request-rereview.json`; runner `completed`.
- **Athena `5b33bf100b5d4381a64dce0c0c088a20`: PASS FOR CH1 FIVE-TEST CHARACTERIZATION INCREMENT ONLY.** No mandatory finding. This closes the test-increment review gap without expanding Orion's old 14-test review. Record: `ch1-athena-5b33bf10-characterization-review.json`; runner `completed`.

Both reviewers used only read/search/find, without execution, edits, LSP, hashing or delegation. Source-map equivalence and execution results are parent evidence, not independently reproduced. Sol's Section 52 AN1 request-only PASS remains unchanged. No full native protocol, HTTP, M2–M5, Full Gate or live-provider verdict follows from these scoped PASS decisions.

Remaining limits are explicit: local 4 Mi work exhaustion and exact depths 64/65 remain untested; 4096-site observation is not end-to-end preparation acceptance; deep membership rejection is before transaction preparation, not rollback after provisional mapping creation. Synthetic signatures establish preservation only. The linker warning persists. No new commit or push was made after the baseline.

### CH2 readiness: integrator amendment required, not lane workarounds

Helix `f837c8b4fb974c7d94b8f554df97e666` returned **NEEDS_CHANGE for CH2 shared-contract readiness**, not a failed CH2 implementation or protocol verdict. Record: `ch2-helix-f837c8b4-readiness.json`; runner `completed; finish_reason=contract_unsatisfied`, with the visible bounded findings retained. Read/search/find only; no execution or modifications. The parent inspected the cited predicates and staging loops:

1. Strict shared Chat JSON/SSE verification currently accepts a single choice (`completion_json.rs:569–575`, `completion_evidence.rs:410–430`), whereas CH2 requires independent choice indexing. Native support must not silently relax the conversion-era Responses checks.
2. Native staging selects `TextPolicy::Output` for sites/streams and does not compose decoded argument-JSON restoration (`restoration_core/native_response.rs:346–353,1015–1037`); lifecycle `arguments` exists separately. Tool-argument semantics cannot be replaced with ordinary output text.
3. SSE snapshot selections project frame nodes rather than complete accumulated delivered assistant/call snapshots (`native_response.rs:793–807,1387–1426`). Arbitrary lane-attested snapshots would not be a safe substitute.
4. Current frame staging rejects no-data/comment-only frames, and conversion-era carrier restrictions reject native extensions/streaming refusal (`native_response.rs:510–540`, `completion_json.rs:504–550`, `completion_tools.rs:12–33`). Preserve native bytes/unknown fields without weakening strict completion.

The next integrator scope is a bounded, test-first native capability amendment: protocol-specific strict verification, typed argument/policy staging, deterministic delivered-snapshot composition, and no-data/native-carrier handling. It must retain exact binding/attempt, charged encoding, original/final strict validation, real HTTP EOF and sealed publication/continuation ownership. Original proof constructors remain HTTP-private; lifecycle finish remains inspection only. The amendment needs affected Responses/security regressions, an updated contract identity and independent review before a lane consumes it. No implementation of that amendment, expanded lane ownership or registry enablement is recorded here. CH2/CH3, AN2/AN3, GE2/GE3, sequential M2–M4 and M5 remain outstanding.

## 54. Authorized Chat-only shared amendment: execution checkpoint

**The amendment is not frozen or accepted.** The latest parent executions pass **24/24 CH2-focused tests** and **1941/1941 shared-security tests**. These are shared Chat/Chat capability checks, not a Chat lane, HTTP route, integration, Full Gate or live-provider acceptance. Independent re-review closes snapshot F1 only. Explicit typed refusal/reasoning policy selection and evolving opaque-field snapshot semantics remain open.

### Authorization, ownership and invariants

The user explicitly approved the integrator-only, Chat-only bounded amendment after Section 53's readiness review, then explicitly approved one additional C5 test-file change. A checkpoint timeout was not approval or a user rejection; the later explicit user assent is the authorization. The three isolated request lanes remain unmodified by this amendment and are not integrated. Native registrations remain **Disabled**. There is no post-baseline commit or push.

Paths below are relative to `src-tauri/src/proxy/security/`. Current changed Rust files are `completion_json.rs`, `completion_evidence.rs`, `completion_tools.rs`, `http_restoration/completion_verifier.rs`, `restoration_core.rs`, `restoration_core/native_response.rs`, `response_restoration/finalization.rs`, `tests/unified_core_tests.rs`, and the newly authorized `tests/c5_native_sse_tests.rs`. The original owned scope also permits `tests/native_test_support.rs`; it is unchanged. The C5 extension permits only its single no-data/DONE test, not other tests or production files. The two design documents record execution; the authoritative architecture is unchanged.

No conversion-matrix, ingress, identity-source, existing route or provider enablement change is authorized. No new dependency, database schema, setting, credential source or token store is introduced. Exact binding/attempt, one dependency ledger, existing payload owners, charged final encoding, independent final strict validation, sealed publication and SSE last-yield continuation remain mandatory. Original completion constructors stay HTTP-private; observation, staging, inspection finish and snapshot composition confer no completion/release/publication authority.

### Implemented mechanics, not a distributed frozen contract

- `JsonCompletion::native_chat()` selects explicit native choice handling: 1..=128 choices with distinct integer indices below 128. Default conversion mode remains single-choice. Native JSON proof and final restored-wire checks select this constructor.
- `SseCompletion::native_chat()` retains independently terminal choice states in the existing charged `PayloadVec`; native `ChatTools::native` supports sparse, choice-local indices. Duplicate delta indices, conflicting identity, invalid arguments, incomplete tools and invalid terminal reasons still fail. Conversion mode retains its original single-choice/contiguous-tool restrictions.
- Lifecycle JSON/SSE staging dispatches `SourceClass::ToolArguments` through existing strict object decoding in `ResponseLifecycle::arguments`, not ordinary text matching. SSE mixed-source streams fail before matching; tool output remains fully held. Changed fragmented arguments are retained to the final fragment, using the same ledger. Ordinary sites still select `TextPolicy::Output`: explicit Refusal/Reasoning selection is **not implemented** here.
- `NativeSseSnapshotSelection::chat_choice(frame_index, native)` selects exact borrowed choice nodes in full original frame/choice order. Mixed modes, missing/reordered/duplicate/nonchoice selections fail. Legacy `new` frame-node projection remains separate. The internal `compose_chat_snapshots` deterministically groups choices/tools in first-seen order, produces complete assistant messages followed by their call snapshots, and uses existing charged JSON/buffer/vector owners. This mechanics function is visible only within security, not a proof issuer.
- `VerifiedNativeSse::validate_restored_chat_snapshots` reparses actual final candidate bytes, reruns strict native completion, independently recollects choices and rebuilds charged snapshots before byte comparison. The finalizer consumes the real HTTP proof and does not publish before all yields resume. Legacy snapshot validation is retained separately.
- Explicit native carrier checking preserves unknown message/tool/function members, unknown typed JSON content parts and opaque reasoning details. Known content/refusal/reasoning-content types remain checked. Conversion's closed-world carrier checks remain unchanged. No-data frames preserve borrowed wire bytes only when source is absent and sites are empty; empty `data:` remains data, not a keepalive/completion fact.

### Parent execution method and immutable artifacts

Repository root: `/Users/chikham/workspace/varyai`; base/HEAD: `ca7f883c742df07abe9bcaa9b0eade02aae5e119`; main target: `src-tauri/target`, separate from lane targets. The parent-only runner `.voidx/verification/core1-20261106/run-native-core-focused.py` has SHA-256 `2183fe4d952857891e713e66bd694679422a661f4e0e4ab5234d0149e2015bd8`. It acquires the common `native-lane-cargo.lock`, sets `CARGO_BUILD_JOBS=2`, records full tracked/untracked input maps, requires successful nonzero listing, then executes the same filter with `--test-threads=1`. Each attempt has a unique source/list/test stem; a compile failure has no runtime log. No artifact is overwritten.

Reproduce a focused check from the repository root with a **new** stem:

```bash
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  python3 .voidx/verification/core1-20261106/run-native-core-focused.py \
  UNIQUE-STEM 'proxy::security::unified_core_tests::ch2_core_'
```

Equivalent Cargo commands are `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, then, only after nonzero successful listing, the same command with `-- --test-threads=1`. The lock, main target and jobs bound apply to both. All executed fixtures are synthetic, use in-memory stores and genuine test transport EOF; no ignored/live tests, app launch/install, real credentials or production proxy port were used. The linker `__eh_frame section too large (max 16MB)` warning persists.

Artifact stems below are under `.voidx/verification/core1-20261106/`. Each completed execution reports source stability. Results belong to their exact recorded input maps; earlier regression counts do not cover later production changes.

| Increment                  | Runtime RED / excluded attempts                                                                                      | GREEN / bounded result                                                                                        |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Native JSON choices        | `ch2-core-json-red-01`: 4 listed, 2 passed / 2 genuine failures                                                      | `ch2-core-json-green-01`: 4/4; `ch2-core-json-security-01`: 1921/1921                                         |
| Native SSE choices/tools   | `ch2-core-sse-red-01`: 4 listed, 2 passed / 2 genuine failures                                                       | `ch2-core-sse-green-01`: all then-current CH2 8/8                                                             |
| Decoded argument staging   | `ch2-core-typed-red-01`: 4 listed / 4 genuine failures                                                               | `ch2-core-typed-green-02-delivery-owner`: all then-current CH2 12/12; `ch2-core-typed-security-01`: 1929/1929 |
| Complete ordered snapshots | `ch2-core-snapshot-red-01`: 3 listed / 3 failed; cancellation test stopped at the earlier missing-snapshot assertion | `ch2-core-snapshot-green-02-visibility`: all then-current CH2 15/15                                           |
| Native carriers / no-data  | `ch2-core-carrier-red-01`: 4 listed, 1 control passed / 3 genuine failures                                           | `ch2-core-carrier-green-01`: 4/4; `ch2-core-carrier-all-green-01`: all then-current CH2 19/19                 |

`ch2-core-typed-green-01` passed 3 and failed 1 because the test asserted a budget refund while still retaining charged delivery bytes. Only the test dropped that owner before checking refund; production lifetime semantics were not changed. `ch2-core-snapshot-green-01` failed compilation with private re-export errors, listed zero tests and is **not** behavioral RED. Visibility was narrowed to security-internal before the next successful run.

### Authorized C5 single-test coordination and interrupted check

`ch2-core-carrier-security-01` ran 1936 tests: 1935 passed, one failed in the old `c5_native_sse_frame_only_done_can_be_opaque_and_never_has_sites`. `ch2-core-carrier-c5-isolated-01` independently reproduced the failure. That old test rejected comment/empty frames, conflicting with the approved no-data behavior. The parent stopped before editing its unowned file and obtained explicit user approval to update only this test.

The renamed `c5_native_sse_frame_no_data_and_done_have_no_source_or_sites` preserves rejection of source-less JSON/invalid data and DONE-with-source/sites. It adds empty `data:` rejection and checks original borrowed bytes, absent source/empty sites, forged source/sites rejection and full budget refunds for empty/comment/CRLF/event-id-retry no-data frames and DONE. This is a contract characterization update, **not a new behavioral RED**.

- `ch2-core-carrier-c5-coordinated-01`: 1 listed / 1 passed.
- `ch2-core-carrier-c5-all-ch2-01`: 19 listed / 19 passed.
- `ch2-core-carrier-c5-security-01`: nonzero listing succeeded, but a combined tool command hit its 120-second timeout during runtime. No full runtime conclusion or stability footer exists; this is an **interrupted attempt, not PASS or FAIL**. The parent checked no Cargo/test process remained and all recorded input maps still matched before resuming.
- `ch2-core-carrier-c5-security-resume-01`: 1936 listed / 1936 passed, stable inputs.
- `ch2-core-carrier-refusal-characterization-01`: 1 listed / 1 passed on first execution. This is **characterization, not RED**. Mapped/zero-map sealed JSON delivery restores selected refusal/text carriers, retains opaque siblings and source JSON, rejects edited opaque history, and refunds after delivery owner drop. It does not prove explicit reasoning policy selection.
- `ch2-core-carrier-protocol-matrix-01`: 3 listed, **2 passed / 1 ignored**; ignored tests were not run.
- `ch2-core-carrier-session-01`: 34 listed / 34 passed.

The protocol/session checks precede the subsequent F1 production correction and must be rerun before claiming current complete-amendment verification.

### Independent snapshot FAIL, verified feedback and F1 runtime repair

Flux `42035b28f1fe441f8f29018785897e74` returned **FAIL FOR CURRENT SNAPSHOT COMPOSITION REVIEWED SCOPE ONLY**. The parent verified F1 against `completion_tools.rs`: strict SSE ignores later null `id`, `type` and function `name`, whereas the snapshot composer selected the last value including null. This could publish invalid reconstructed identities even when the original strict wire passed. Reusing the same incorrect composer during final comparison did not establish strict assembled-call validity. The historical FAIL remains open until independent re-review.

The parent appended four tests before changing production:

- Three separate null ID/type/name cases obtain a real HTTP proof, use exact choice selections and decoded argument staging, perform sealed SSE delivery and resume the final continuation, then require exact published history with the established identity.
- One six-mode malformed assembled-call test requires failure for null/missing identity, non-object/invalid argument JSON and duplicate call IDs, without retained charges or publication. It exercises structural mechanics, not a completion issuer.

`ch2-core-snapshot-f1-red-01` successfully listed 7 tests: the three prior controls passed and all four new tests genuinely failed at runtime. Each null case reached fully delivered publication and failed `ch2_core_snapshot_null_identity_erased_in_published_history`; malformed calls failed `ch2_core_snapshot_malformed_assembled_call_accepted`. No compiler/fixture failure is counted.

The production correction affects only `completion_json.rs` and `restoration_core/native_response.rs` relative to this RED snapshot. Non-null tool identity selection now ignores nulls and rejects conflicting values. The existing strict JSON message/tool structure logic is extracted into security-private `validate_chat_message`; the original JSON choice still checks terminal reason before calling it. After charged composition/parsing, every complete assistant message is checked by this shared strict validator, and each following standalone call must match its validated message call. This adds structural validation, not completion authority. Final actual-wire strict validation and independent reconstruction remain required.

- `ch2-core-snapshot-f1-green-01`: **24 listed / 24 passed**, all current CH2-focused tests.
- `ch2-core-snapshot-f1-security-01`: **1941 listed / 1941 passed**, current shared-security selection.

Both report stable inputs, the common lock, jobs bound and single-thread runtime. Lyra `b91a1cfddf624fae8197e3f5c5861609` returned **PASS FOR SNAPSHOT F1 CURRENT REVIEWED SCOPE ONLY — F1 RESOLVED**, with no blocking finding in that scope. The reviewer confirmed null-ignore/conflict rejection, strict assembled-call checks, retained JSON completion checks, actual-wire validation and delayed activation. Flux's earlier FAIL remains historical. This does not grant whole-amendment, full-protocol, typed-policy, evolving opaque-composition, M2–M5, Full Gate or live acceptance. Review used only read/search/find, without commands, LSP, modifications, hashing or delegation. Parent test results were not independently executed or raw-log-verified; JSON preservation was assessed from current source without a historical diff, and final reconstruction shares the composer rather than an implementation-diverse oracle.

### Remaining stop conditions

Do not distribute or consume this shared contract yet. Required work includes explicit typed policy selection and its genuine runtime RED/GREEN, evolving opaque/structured-field snapshot semantics, current protocol/session regressions, bounded owned-file Rust formatting (`--config skip_children=true`), Cargo check and Clippy, source identity/freeze, document audience gates and independent whole-amendment review. New capabilities must not be backdated into the baseline Full Gate. Preserve all historical formatter blocks outside newly approved changes.

The lane resource handoff gaps in Section 53 remain open. Shared executable proof/staging remains Chat/Chat only; no Anthropic/Gemini proof expansion is authorized here. CH2/CH3, AN2/AN3, GE2/GE3, full-protocol lane reviews, sequential M2–M4 integration, M5 and release/live acceptance remain incomplete. No workflow completion or post-baseline commit/push is recorded.

## 55. Approved typed-policy increment: parent verification, not amendment acceptance

**The Chat-only shared amendment remains unfrozen and unaccepted.** This checkpoint supersedes Section 54's open typed-policy item only for the increment below. Current parent checks pass **38/38 CH2-focused tests, 1955/1955 shared-security tests, 34/34 session tests, and protocol matrix 2 passed / 1 ignored**. Cargo check, Clippy with warnings denied, bounded formatting and diff checks pass. Independent review returns **PASS for the typed-policy increment only**, with no mandatory finding. Evolving opaque/structured snapshot semantics, whole-amendment review and all lane/integration acceptance remain open.

### Authorized files, APIs and invariants

The user explicitly approved the four-file typed-policy plan after the approval prompt timed out. The timeout itself was not approval or a rejection. Relative to the pre-increment input map `ch2-core-policy-start-01.json`, implementation changes are limited to `src-tauri/src/proxy/security/restoration_core/native_response.rs` and `src-tauri/src/proxy/security/tests/unified_core_tests.rs`; documentation appends are limited to this evidence record and `docs/design/security-dlp-unified-core-parallel-implementation-plan.md`. Local source/list/test manifests and starting snapshots live under `.voidx/verification/core1-20261106/`; they are not release artifacts. The authoritative architecture, previous shared changes, C5 characterization and three isolated request lanes are preserved.

- `NativeJsonObservation::new_with_policies(source, client, sites, delivered_nodes, policies, budget)` requires one existing `TextPolicy` per site, in exact site order. Policies are retained in a budget-charged `PayloadVec`; the constructor rejects missing/extra policies and disagreement between `SourceClass::ToolArguments` and `TextPolicy::ToolArguments` before staging. Its old `new` constructor retains default Output/ToolArguments behavior without adding a policy vector.
- `NativeSseTextStream::with_policy(fragments, policy)` selects one policy for the entire decoded stream. Source/policy compatibility is checked for every fragment before matcher work. Existing exact, unique, ordered site coverage and homogeneous source requirements remain. The old `new` retains its default behavior. Stream metadata remains borrowed; no new retained owner or token store is introduced.
- JSON/SSE lifecycle staging passes Output/Refusal/Reasoning to the existing lifecycle. Reasoning preserves readable text and adds no restoration occurrence or token dependency. Output/Refusal restore under existing scope. ToolArguments still takes the dedicated complete-object decoding/holding path, never ordinary text matching. One lifecycle and dependency ledger serve mixed policies.
- Non-lifecycle JSON staging rejects explicitly policy-bearing observations rather than silently falling back to Output. SSE explicit policy exists on stream claims accepted only by the lifecycle entry; the old entry has no stream/policy parameter to ignore. This is an API-shape constraint, not a new tested rejection on the old SSE entry.

No conversion-mode weakening, protocol/site-kind inference, proof issuer, completed-context owner, release permission, early publication or snapshot-composition change is introduced. Exact attempt binding, original node identity, immutable restrictions, charged encoding, independent final strict validation and last-yield continuation remain mandatory. Opaque content is not a readable reasoning policy merely because it contains a reference. Native registrations remain **Disabled**. No commit, push, app launch/install, ignored/live test or release acceptance is recorded.

### Actual RED/GREEN and characterization history

All stems below are under `.voidx/verification/core1-20261106/` and are immutable. The Section 54 parent runner, common Cargo lock, `CARGO_BUILD_JOBS=2`, main target, nonzero discovery and single-thread runtime apply. Fixtures are synthetic and use in-memory stores and genuine test transport EOF.

| Artifact stem                         | Observed result and boundary                                                                                                                                                                                                                                           |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ch2-core-policy-red-01`              | Test compilation failed because the new SSE helper omitted the `MAX_ENTITY` import; zero listed, no runtime. Not behavioral RED. Only the import was corrected.                                                                                                        |
| `ch2-core-policy-red-02-import`       | 4 listed; 2 controls passed, JSON/SSE reasoning genuinely failed at runtime on `policy_json_reasoning_restored_or_dependencies_granted` / `policy_sse_reasoning_restored_or_dependencies_granted`. Original API selected Output; production was unchanged.             |
| `ch2-core-policy-green-01`            | 4/4 passed after explicit policy carriers/dispatch. Tests switched construction to `new_with_policies` / `with_policy`; safety assertions and fixtures were not weakened. This is not an unchanged-callsite regression or proof that old constructors infer Reasoning. |
| `ch2-core-policy-guards-red-01`       | 13 listed; 5 passed / 8 genuine runtime failures: JSON missing/extra/source-policy mismatch and legacy fallback; SSE three source-policy mismatches. Existing mixed-source/immutable rejection passed as a control.                                                    |
| `ch2-core-policy-guards-green-01`     | 13/13 passed after compatibility/cardinality validation and rejection of explicit JSON policies in non-lifecycle staging. Guard assertions were unchanged.                                                                                                             |
| `ch2-core-policy-characterization-01` | 14/14 passed on first run of the added mixed-policy/metadata-allocation test. That last test is characterization, not RED.                                                                                                                                             |

Tests cover mapped/zero-map JSON and split-reference SSE, original-source preservation, no dependency grant for reasoning, restored Output/Refusal, exact sealed delivered history, unchanged borrowed wire bytes, cancellation without last continuation activation and full budget refunds. The mixed-policy test covers Output/Refusal/Reasoning/decoded ToolArguments sharing one dependency token, unchanged immutable reasoning, and failure before/after policy-vector allocation with complete refunds. Negative fixtures cannot publish. SSE immutable/mixed-source tests are existing-behavior controls, not new RED.

### Final-source checks and reproducibility

The first Cargo check (`ch2-core-policy-cargo-check-01`) returned zero but reported one unused `TextPolicy` import. Removing that import was the only subsequent Rust edit; it is not a behavioral fix. All checks below were executed after the removal, and each source manifest reports unchanged pre/post inputs. Earlier formatting-only checks and pre-removal executions remain historical.

| Final artifact stem                        | Command selection/result                                                                                |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| `ch2-core-policy-final-all-green-01`       | `proxy::security::unified_core_tests::ch2_core_`: 38 listed / 38 passed, including the 14 policy tests. |
| `ch2-core-policy-final-security-01`        | `proxy::security::`: 1955 listed / 1955 passed.                                                         |
| `ch2-core-policy-final-protocol-matrix-01` | `proxy::protocol_matrix_tests::`: 3 listed, 2 passed / 1 ignored; ignored not executed.                 |
| `ch2-core-policy-final-session-01`         | `proxy::session::tests::`: 34 listed / 34 passed.                                                       |
| `ch2-core-policy-cargo-check-02-import`    | `cargo check --manifest-path src-tauri/Cargo.toml`: exit 0, no Rust warning.                            |
| `ch2-core-policy-clippy-01`                | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: exit 0.                             |

From the repository root, reproduce any test selection with a new stem:

```bash
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  python3 .voidx/verification/core1-20261106/run-native-core-focused.py \
  UNIQUE-STEM 'proxy::security::unified_core_tests::ch2_core_policy_'
```

Use the four final filters in the table for regression runs. The underlying commands are `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, followed only after successful nonzero listing by the same command with `-- --test-threads=1`. Cargo check/Clippy used the same jobs bound, target and global `native-lane-cargo.lock`, saving new logs and full pre/post maps. No lane Cargo overlapped. Final bounded formatting command was `rustfmt --edition 2021 --config skip_children=true --check src-tauri/src/proxy/security/restoration_core/native_response.rs src-tauri/src/proxy/security/tests/unified_core_tests.rs`; `git diff --check` also returned zero. Formatting touched only these approved Rust files. The linker `__eh_frame section too large (max 16MB)` warning persists; it is not a Rust/Clippy warning.

Documentation appends do not alter executable inputs; source-map revalidation must explicitly distinguish the two appended documents from all other inputs. This is not a newly frozen contract identity or Full Gate. Whole-project renderer/unit/integration/format commands were not rerun for this bounded increment and no broader current-tree claim is made. Next: establish approved evolving opaque/structured snapshot semantics and contradiction/conflict/resource/cancellation tests before source freeze and independent whole-amendment review. Stop for design/scope approval rather than infer those semantics or touch unowned files. CH2/CH3, AN2/AN3, GE2/GE3, lane resource handoffs, sequential M2–M4 and M5 remain incomplete.

### Bounded independent decision and audience gates

Rune `9178bf23c6444ba481b6b7405a680f6b` returned **PASS FOR CH2 CHAT-ONLY TYPED-POLICY INCREMENT ONLY**, with no blocking finding; runner status `completed`. Parent transcription: `ch2-core-policy-rune-9178bf23-review-01.json`. The reviewer compared the pre-increment snapshots and incremental diff with current policy carriers/dispatch, existing lifecycle and tests, not the whole HEAD diff. Read/search/find only; no commands, LSP, edits or delegation. Parent execution results and raw logs were not independently reproduced or verified. Parallel documentation, whole amendment, full lane, dirty-tree-wide correctness, M2–M5 and release acceptance were not reviewed. A nonblocking coverage gap remains: no separate positive test selects explicit SSE ToolArguments; the unchanged dedicated argument branch and old-constructor regression tests support that path. The first API-construction switch and characterization boundaries remain explicit.

Lyra `050b28a1b95d41da8e2da4255e887bc2` returned **PASS for human fresh-reader clarity and PASS for LLM checkpoint execution readiness**, with no mandatory fixes, on this appended Section 55 and plan Section 11. Read/search/find only; no execution, LSP, edits, hashing or delegation. API descriptions and inspected parent counts matched; formatting/hash checks remain parent evidence. Parent transcription: `ch2-core-policy-lyra-050b28a1-doc-review-01.json`. The checkpoint puts the newest result/hold first and records exact paths, explicit policy versus defaults, invariants/forbidden edits, commands/filters/artifacts, RED/compile/control distinctions and next approval stop. Historical sections and headers are untouched. Parent final document-boundary/source-equivalence record: `ch2-core-policy-doc-boundary-01.json`; post-review result/checkbox bookkeeping does not change reviewed executable inputs. These local audience gates do not freeze or accept the shared contract.

## 56. Conservative Chat snapshot contract: approved design, implementation pending

**The design direction is approved; this section is not implementation evidence or a frozen shared contract.** The next execution plan is [plan Section 12](security-dlp-unified-core-parallel-implementation-plan.md#12-conservative-chat-snapshot-increment-execution-plan). Its implementation approval, runtime RED/GREEN, current-source regressions and independent **whole-amendment** PASS are still required. Section 55's typed-policy results remain historical evidence for that increment, not acceptance of these new rules. Native registrations remain **Disabled**; no lane may consume this candidate.

### Purpose, authority and current defect

This mixed human/LLM contract specifies a bounded, conservative projection of Chat SSE choices into reusable assistant/call history. It does not define new provider wire semantics. The authoritative [protocol-adapter architecture](security-dlp-unified-core-protocol-adapters.md) remains unchanged. The user approved concatenating known text deltas but rejecting unsupported evolution of opaque values rather than guessing how to combine them.

At this checkpoint, `compose_chat_snapshots` in `src-tauri/src/proxy/security/restoration_core/native_response.rs` uses `last(values, field)` for unknown message/tool/function members. Its `text_field` also falls back to the last value for non-string reasoning. Consequently two different opaque values can silently become a seemingly reusable history value. This is a source-inspected defect hypothesis awaiting behavioral RED, not a new runtime finding. The prior F1 fix for null identities and strict assembled messages/calls must be preserved.

Original wire preservation and reusable history are different contracts. Retaining every original field/frame on the wire does **not** prove a unique full assistant history exists. Conversely, a history snapshot is not a replacement response envelope. IDs, usage, finish reasons, no-data frames and other envelope metadata retain their existing wire/proof handling; they are not invented as assistant fields. Choice/tool `index` values are existing grouping keys, not opaque merge instructions.

### Supported projection and missing/null rules

Apply the following rules independently within each exact choice and choice-local tool. Preserve first-seen choice order, first-seen tool order and field order, with each complete assistant followed immediately by its standalone calls. Never merge across choices/calls. Final assembled structures must still pass existing strict validation.

| Field class                                              | Supported composition                                                                                                           | Missing, null and conflict behavior                                                                                                                                                                                                               |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `content`, `refusal`, `reasoning_content` string deltas  | Concatenate decoded strings in observed order; preserve empty strings and escaping.                                             | Missing contributes nothing. Existing text nulls contribute no string and cannot erase established text; if observed values are only null, retain null. Existing original validators reject invalid types; SSE content arrays remain unsupported. |
| Readable string `reasoning`                              | The same ordered string/null behavior, only when every non-null occurrence is a string.                                         | No automatic Reasoning policy inference: explicit `TextPolicy::Reasoning` still controls readable-site restoration.                                                                                                                               |
| Non-text/structured `reasoning`                          | Treat the entire value as opaque, including any null occurrences in that field's sequence.                                      | A single value or structurally equal repeats only. String/object/array/scalar type switches and null/non-null conflicts reject. This does not extend what original protocol validators accept.                                                    |
| Tool `id`, tool `type`, function `name`                  | Retain the established non-null identity; identical repeats are allowed.                                                        | Preserve F1: missing/null cannot erase identity, conflicting non-null identities reject, and final required identity/type checks remain strict. No name/ID concatenation.                                                                         |
| Function `arguments` string deltas                       | Concatenate in order, then retain the existing dedicated complete-object decoding, restoration and tool hold.                   | Missing/null contribute nothing; null cannot erase established arguments. Required final arguments remain a nonempty string decoding to an object. Wrong types, invalid/incomplete JSON and mixed-source/policy claims reject.                    |
| Unknown assistant-message, tool-call or function members | Preserve the first encountered value verbatim as a JSON value; accept later occurrences only after bounded structural equality. | Missing does not overwrite. Explicit null is a value: null/null is equal, but null/non-null in either order rejects. Different strings, booleans, numbers, object/array values or types reject.                                                   |
| Known containers and envelope facts                      | Keep existing `role`, `tool_calls`, `function`, index, terminal and completion handling.                                        | Do not reinterpret these as opaque values or relax their existing type/null/structural checks. Native complete-JSON content arrays retain existing support; no SSE array support is added.                                                        |

For example, an opaque object seen once and then omitted is preserved; the same object repeated is accepted; objects with different members reject rather than deep-merge. Unknown strings are never delta-concatenated. Arrays retain element order and must match element by element; no append, set union, sorting or reasoning-index inference is allowed. Opaque nested reference-shaped text is neither scanned nor restored and grants no new occurrence/dependency. Supported readable sites still use their explicit typed policy and the existing single dependency ledger.

### Equality, resource ownership and failure boundary

Use the existing bounded comparison pattern (`NativeSseFrameObservation::same_value`) inside the owned production file, not unbounded `Value` equality, cloning, canonicalization or serialized temporary comparisons. Define equality over JSON structure: identical scalar variants/values using existing `serde_json::Number` equality, exact decoded strings, equal ordered arrays, and equal object key/value membership irrespective of object insertion order. Preserve the **first value's** object field order and array order in history. This is not lexical equality of wire escapes or a numeric-normalization feature.

Each `compose_chat_snapshots` invocation has **one 4 _ 1024 _ 1024 work allowance** shared across all new opaque selections/comparisons, including assistant and repeated standalone-call emission; never reset per field, choice, tool or comparison. Charge one unit before each field-occurrence lookup and each visited value node, plus the inspected UTF-8 key/string byte lengths with checked subtraction. Apply depth **64**, root at depth zero; depth greater than 64 or exhausted work returns the existing capacity failure. Validate the first selected opaque value with the same bounded traversal even if there is no repeat, so a single oversized/deep opaque value cannot bypass the bound. Existing frame inspection retains its own allowance; stage and final-wire reconstruction each have a separate bounded composer invocation, not shared mutable authority.

The work allowance bounds CPU inspection; `PayloadBudget` still accounts for memory. Borrow selected values from already owned native inputs, keep existing `PayloadVec`, `PayloadBuffer`, charged serialization and `PayloadJson` owners, and introduce no retained opaque copy, cache, second ledger or new dependency. Comparison uses depth-bounded stack state only. Existing count/index limits (128) and 1 MiB composed-output limit remain. All allocations/copies must retain existing precharge/RAII refund behavior; work exhaustion, allocation denial, semantic conflicts and cancellation must release temporary owners and create no completed record.

Unsupported evolution is a reversible-processing boundary, not permission to discard metadata, emit last-value history, switch to ordinary forwarding or fail open. Fail through the existing sanitized restoration path **before client delivery, tool release or completed-record publication**. Do not echo field values, reference strings or tool inputs in errors/assertion reports. If this guarantee requires modifying HTTP proof/finalization or any unowned file, stop and seek separate approval.

### Verification oracle and closure boundary

`VerifiedNativeSse::validate_restored_chat_snapshots` in `src-tauri/src/proxy/security/http_restoration/completion_verifier.rs` consumes genuine transport proof, reparses final wire, invokes the same composer and compares charged encodings. `src-tauri/src/proxy/security/response_restoration/finalization.rs` performs that check before sealed release/publication. Both files are read-only here. The final verifier is separate from staging but **not an implementation-diverse composition oracle**.

Tests must therefore hand-author expected complete history without calling the composer, assert first-value field/array ordering via charged serialization where JSON structural assertions would hide it, and add final-wire/candidate contradictions. These negatives must show that valid original HTTP completion alone cannot authorize inconsistent final history, release tools or publish. Include immutable restoration conflicts, real HTTP EOF versus truncation, exact attempt mismatch, first-run passing controls, opaque-token noninspection, resource bounds/refunds, and dropping delivery after the last yielded frame but before its continuation. Add the disclosed explicit SSE `TextPolicy::ToolArguments` positive case using the dedicated object-decoding path. The detailed matrix and commands are in plan Section 12; passing controls or compilation failures are not RED.

Before closing the **whole Chat-only shared amendment**, record a new candidate contract identifier and exact source-input manifest covering all cumulative shared changes since CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`, not merely this four-file increment. Reconcile callable signatures/visibility with actual code; bind current regression manifests, parent command results and independent review scope to that candidate. A scoped increment PASS cannot close the amendment. Freeze/distribution requires independent whole-amendment PASS plus both documentation audience gates; any subsequent executable change invalidates affected checks and review. Do not label the candidate frozen before those gates pass.

The pre-increment boundary is `.voidx/verification/core1-20261106/ch2-core-opaque-start-01.json` and its four same-stem basename snapshots. Preserve all historical prefixes/status headings and all earlier evidence stems. Only the four files listed in plan Section 12 may change relative to that boundary; the existing 11-file dirty HEAD diff is not the increment. Verification artifacts are parent-local evidence, not extra production ownership. No implementation, runtime RED/GREEN, new freeze identity or whole-amendment review is recorded by this section. Lane resource handoff, CH2/CH3, AN2/AN3, GE2/GE3, sequential M2–M4, M5, Full Gate and live/release acceptance remain separate and incomplete.

### Document-gate result and implementation approval stop

Initial document review by Juno `2abae8cb5b2248a0860945433fc00af9` returned **NEEDS_CHANGE / human FAIL / LLM FAIL** on TDD order and resource RED/control classification. Both findings were checked against `same_value`, the composer and existing charged parser/output limits. Only the new plan section was corrected: conflict and missing aggregate-work runtime RED, plus supported first-value/order baselines, now precede any production edit; bounded selection and accounting are implemented together afterward. Already-correct depth/size/low-budget/refund cases remain controls. The original review is retained in parent transcription `ch2-core-opaque-juno-2abae8cb-doc-review-01.json`.

Independent re-review by Flux `f028ebe63de146acaec978cf15b0bf9a` returned **human fresh-reader PASS and LLM execution-readiness PASS**, explicitly resolving both findings with no mandatory additions. Scope: new plan Section 12 and this Section 56, not implementation or the cumulative amendment. The reviewer used read/search/find only; no command, test, hash, edit, LSP or delegation execution. Parent formatting, prefix/source checks and earlier runtime results are not reviewer execution evidence. Parent transcription: `ch2-core-opaque-flux-f028ebe6-doc-review-01.json`.

Parent checks from the repository root confirmed both historical document byte prefixes unchanged, both new Markdown suffixes Prettier-idempotent, only these two documents changed against `ch2-core-opaque-start-01.json`, and both owned Rust files/all other source inputs unchanged. The embedded serialized check/Clippy wrapper was parsed as Python, not executed. `git diff --check` passed. Initial suffix-format record: `ch2-core-opaque-doc-draft-01.json`; final boundary/source/hash record including this bookkeeping: `ch2-core-opaque-doc-boundary-01.json`. No Cargo/runtime test or production change was performed for this documentation increment. The parent also fresh-read the final result/approval bookkeeping; it does not change the reviewed semantics or execution order. Await explicit implementation-plan approval before TDD; this document PASS is not amendment acceptance, freeze, lane consumption or M2–M5 authorization.
## 57. Conservative snapshot implementation and whole-review F1 checkpoint

**Current checkpoint: candidate 02 is runtime-verified but remains unfrozen and unaccepted.** Whole-amendment independent re-review and the final mixed-audience documentation gates are pending. Native registrations remain **Disabled**. No lane may consume this candidate. This append supersedes only the current-state interpretation of Sections 55–56; their historical text, pending labels and verification records are unchanged.

### Approval, audience and ownership

This section serves human checkpoint readers and LLM executors. The user explicitly approved plan Section 12 with permission to continue before production implementation. The earlier checkpoint timeout was not a rejection. The authoritative [protocol-adapter architecture](security-dlp-unified-core-protocol-adapters.md) is unchanged.

The snapshot increment is measured against `ch2-core-opaque-start-01.json`; the entire amendment is measured against CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`. Only `restoration_core/native_response.rs`, `tests/unified_core_tests.rs` (under `src-tauri/src/proxy/security/`) and these two plan/evidence documents may change in this increment. The cumulative nine-Rust-file/eleven-file dirty HEAD diff includes earlier authorized work; it is not this increment's ownership list. No other shared production file, C5 fixture, lane, HTTP proof/finalizer, route, registration, conversion matrix or identity source was changed in this increment. No dependency, storage, schema, UI, setting or token store was added.

### Conservative projection implemented

`compose_chat_snapshots` no longer selects the last opaque value. It preserves the first value and accepts only bounded structural-equal repeats. Missing values do not overwrite; opaque null is a real value, so null/non-null in either direction rejects. Unknown strings are not concatenated. Object equality ignores insertion order but output retains the first object's field order; arrays are order-sensitive. There is no numeric normalization, deep merge, opaque cloning or opaque reference scanning. Structured/non-text `reasoning` follows the opaque rule. Known text/null deltas and complete structured-argument restoration retain their prior ordered semantics, and F1's earlier identity-null protection and strict assembled assistant/call validation remain intact.

Each first value is bounded by self-comparison using `NativeSseFrameObservation::same_value`; subsequent repeats use the same comparison. Depth is limited to 64. One composer invocation shares `4 * 1024 * 1024` work across fields, choices, tools and standalone-call emission, including `1 + field.len()` lookup charges. Existing `PayloadBudget`, `PayloadVec`, `PayloadBuffer`, `PayloadText` and `PayloadJson` retain allocation ownership and refund behavior. No new dependency authority is granted by opaque data.

The original wire and reusable assistant/call history remain distinct. The final verifier consumes real HTTP proof, revalidates final wire and compares charged serialized ordered history before release/publication. It and staging **share the composer**; this is not an implementation-diverse oracle. Handwritten full histories, charged serialized-order assertions and real-proof final-wire/candidate contradictions provide counterevidence instead.

### Snapshot TDD and controls

All artifacts below are under `.voidx/verification/core1-20261106/`; earlier artifacts are retained without overwrite.

| Artifact stem                                                                | Observation and classification                                                                                                                                                                                                                                                                                                                                          |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ch2-core-opaque-red-01`                                                     | Listing compilation failed with four `E0277` helper error conversions. Zero listing/runtime; **not RED**. Only helper `map_err` handling was corrected.                                                                                                                                                                                                                 |
| `ch2-core-opaque-red-02-mapping`                                             | 23 listed; 2 controls passed and **21 genuine runtime failures** before production changes. Tags: `opaque_conflicting_snapshot_accepted`, `opaque_aggregate_work_limit_missing`, `opaque_first_value_or_history_order_changed`. These cover 19 conflicts, missing aggregate work and first-value/order preservation.                                                    |
| `ch2-core-opaque-green-01`                                                   | 23/23 passed after the minimal composer change; safety assertions unchanged.                                                                                                                                                                                                                                                                                            |
| `ch2-core-opaque-controls-02`                                                | CH2 73/73, including 35 added snapshot/policy tests. First-passing parser/output/depth/low-budget/refund, final-wire/candidate contradictions, immutable, cancellation, equal-repeat/single-value, multi-choice and sparse-tool coverage are controls/characterization, **not additional RED**. Explicit SSE `TextPolicy::ToolArguments` positive coverage is included. |
| `ch2-core-opaque-final-security-01` / `final-session-01` / `final-matrix-01` | Historical candidate 01: 1990/1990; 34/34; 2 passed / 1 ignored respectively. Ignored not executed.                                                                                                                                                                                                                                                                     |
| `ch2-core-opaque-check-clippy-01`                                            | Historical candidate 01 Cargo check and Clippy `-D warnings`: both exit 0.                                                                                                                                                                                                                                                                                              |

### Whole-amendment review FAIL and verified stream-isolation feedback

Onyx `b10a987a8fa443f0a6dd31357c40decc` returned **whole-amendment FAIL** for `chat-native-shared-conservative-20261107-candidate-01`, not merely a scoped opaque/policy result. Its mandatory F1 finding was that `NativeSseTextStream` checked source class, exact site coverage and order but did not bind fragments to one native choice. Same-class fragments from parallel choices could form a complete reference and be restored into different choices; snapshot-selection coverage and final wire reconstruction could not detect that earlier grouping error. The parent verified the source path and reproduced it, including the analogous same-choice call/carrier isolation gaps, before changing production code. The review used **read/search/find only**, with no commands, tests, LSP, hashes, edits or delegation. Test results cited by Onyx were parent evidence, not independent execution. Onyx reported no other confirmed blocker but did not exhaustively audit all manifests/RED logs; its report is retained as `ch2-core-isolation-onyx-b10a987a-review-01.json`.

The correction stays private to `native_response.rs`: `chat_sse_stream_key` derives a key from exact original frame-node pointers, native choice index and either the exact supported text field or choice-local tool index. `ChatSseStreamCarrier` and the helper are private; no staging signature/API is extended. Charged key metadata and one bounded lookup allowance precede lifecycle/matcher work. Same-class/same-policy claims cannot cross choices, calls or content/refusal/reasoning carriers, even with legacy snapshot selections. Classification must agree with a real argument versus text carrier. Reordered choices and sparse reordered calls remain supported. Unsupported sites fail through the existing safe path, not ordinary forwarding or fail-open.

| Artifact stem                    | Observation and classification                                                                                                                                                                                                                               |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ch2-core-isolation-red-01`      | 7 listed, then **6 runtime failures / 1 positive control**. All six hit `isolation_cross_choice_call_or_carrier_stream_accepted`; zero-publication and complete-refund assertions passed before the intended failure. Production was unchanged for this run. |
| `ch2-core-isolation-green-01`    | Timed out after 120 seconds waiting for Cargo package-cache/build-directory locks during listing. No successful listing or runtime; **neither RED nor GREEN**. The incomplete record/log remain intact. No unknown process was terminated.                   |
| `ch2-core-isolation-green-02`    | After explicit permission for read-only process/lock inspection found no holder, new-stem listing and single-thread runtime passed 7/7. No earlier artifact was overwritten.                                                                                 |
| `ch2-core-isolation-controls-01` | Final CH2 **81/81** after adding a sparse-call positive control. Eight isolation tests total: the six RED-derived negatives and two first-passing controls, not eight REDs.                                                                                  |

The first attempt to format the new tests reported a mismatched delimiter before Cargo ran; it was repaired in test code only and is **not runtime RED**. The prior unused-import/typed-policy compile history remains in Section 55. The write-tool stale-read rejection was resolved by rereading, not by overwriting external work.

### Current candidate 02 evidence and identity

Candidate `chat-native-shared-conservative-20261108-candidate-02` is recorded in `ch2-core-isolation-candidate-02.json`, with the full input SHA-256 map, baseline, cumulative nine-file Rust list and evidence mapping. Its `-whole.diff` covers the amendment against CORE-1; its `-increment.diff` covers only the F1 change against `ch2-core-isolation-start-01`. Candidate 01's manifest/diffs and all earlier logs remain unchanged.

| Current parent artifact stem           | Listing/runtime or command result                                                                                                           |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `ch2-core-isolation-controls-01`       | `proxy::security::unified_core_tests::ch2_core_`: 81 listed / **81 passed**.                                                                |
| `ch2-core-isolation-final-security-01` | `proxy::security::`: 1998 listed / **1998 passed**.                                                                                         |
| `ch2-core-isolation-final-session-01`  | `proxy::session::tests::`: 34 listed / **34 passed**.                                                                                       |
| `ch2-core-isolation-final-matrix-01`   | `proxy::protocol_matrix_tests::`: 3 listed / **2 passed, 1 ignored**; ignored not executed.                                                 |
| `ch2-core-isolation-check-clippy-01`   | `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: both **exit 0**. |

All current command manifests have identical pre/post maps, matched by the parent to candidate 02. All Cargo runs used the common `native-lane-cargo.lock`, `CARGO_BUILD_JOBS=2` and main `src-tauri/target`; runtime followed successful nonzero listing and used one test thread. Two-file rustfmt and `git diff --check` passed. The linker warning persists: `__eh_frame section too large (max 16MB) ... compact unwind table`; it is not suppressed or treated as acceptance of release tooling.

Parent-recorded executable SHA-256 values:

- `native_response.rs`: `e494a9eb1bb3ff330c7e082bfcef6b89a89dfe6c91eb1f8b7558567bf9f554f3`.
- `unified_core_tests.rs`: `ec23c98c7082504b5b95530b40ef847f2d7345c921ba260b5bd5f5cf97cd7306`.
- Unchanged focused runner: `2183fe4d952857891e713e66bd694679422a661f4e0e4ab5234d0149e2015bd8`.

The parent API/visibility audit found the earlier amendment's security-scoped policy constructors, Chat choice selection/composer, native completion constructors/carrier validator and final snapshot validator; no pre-existing public staging signature was removed or changed. `validate_chat_message` was extracted as a security-internal helper. Native-only constructors are selected by the native verifier; conversion constructors/defaults retain strict behavior. F1 adds only private mechanics. Exact binding/attempt checks, one lifecycle ledger, immutable restrictions, original/final strict validation, real EOF, tool hold, sealed release/publication and activation only after the last SSE yield remain required and were inspected. Direct Chat/Anthropic/Gemini registrations remain `Registration::Disabled`.

### Reproduction and remaining gates

From the repository root, use a **new** evidence stem for each command; never overwrite stems:

```bash
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  python3 .voidx/verification/core1-20261106/run-native-core-focused.py \
  UNIQUE-STEM 'proxy::security::unified_core_tests::ch2_core_'

rustfmt --edition 2021 --config skip_children=true --check \
  src-tauri/src/proxy/security/restoration_core/native_response.rs \
  src-tauri/src/proxy/security/tests/unified_core_tests.rs
git diff --check
```

Use the other current-table filters for security/session/matrix. Cargo check/Clippy require the same global lock/jobs/target and saved pre/post inputs. Renderer/unit/all-library/integration/full-format checks were **not** rerun for this bounded amendment, so no current Full Gate claim is made. Ignored/live tests, credentials, app startup/install, commit/push, lane rebase/distribution/integration and registration changes remain forbidden in this scope.

Next: obtain independent **whole-amendment PASS** for candidate 02, final human fresh-reader and LLM execution-readiness PASS, then confirm source/contract identity and append the actual freeze decision. A documentation-only append may reuse executable evidence only after all executable/config/test inputs are confirmed unchanged; it does not retroactively change any historical result. A mandatory finding must be verified and fixed test-first within the four files, or execution must stop if it requires another file, API expansion or protocol-semantic change. Even a later shared-amendment freeze is not CH2/CH3, AN2/AN3, GE2/GE3, lane handoff, M2–M5, Full Gate or live/release acceptance.

## 58. Candidate 02 whole-review FAIL and candidate 03 continuity checkpoint

**Current state: candidate 03 is runtime-verified, unfrozen and unaccepted.** Kai `c49bac7c31c94a099aa2030b6fa39374` is independently reviewing the whole cumulative amendment; no result is assumed. Candidate 02 did not pass whole review. This mixed human/LLM checkpoint supersedes only the current-state interpretation of Section 57, preserving all earlier text, statuses, failures and evidence. Native registrations remain **Disabled**, and no candidate is available to lanes.

### Independent reports and parent verification boundary

Lumen `c5d465a1f869425e970aa64364cf8da6` returned **human fresh-reader PASS** and **LLM execution-readiness PASS** for plan Section 13/evidence Section 57, with no mandatory document finding. Its complete report is retained in `ch2-core-isolation-lumen-c5d465a1-review-01.json`. That PASS concerns the then-unfrozen checkpoint, not code acceptance or the new sections here.

Vesper `0cc25edd17e04a64b83e3f59fa71c167` returned **whole-amendment FAIL** for `chat-native-shared-conservative-20261108-candidate-02`, covering the cumulative nine-file scope rather than only the previous isolation fix. The agent completed with `finish_reason=time_limit`; its full returned report and coverage limits are preserved in `ch2-core-isolation-vesper-0cc25edd-review-01.json`. It is neither a PASS nor an exhaustive audit. Both reviewers used read/search/find only, without commands, tests, hashes, LSP, edits or delegation. Runtime reproduction, source identity, formatting and retained-artifact checks below were executed by the parent, not independently by these reviewers.

Vesper F1 identified a remaining partition-authority defect: same-choice/same-carrier fragments `[0,2]` could form one stream while fragment `[1]` formed another. The groups were internally ordered, disjoint and collectively complete, but could synthesize a valid reference absent from the original continuous carrier. Staging and final verification sharing the composer cannot prove that original matching provenance. The parent verified this path and also reproduced contiguous carrier partitioning and omission of an unobserved middle fragment before production changes.

Vesper F2 identified guard masking: several SSE policy/immutable negatives used `{"text":...}` rather than valid Chat choices/delta. The new carrier check rejected them before the intended guard. These historical green results remain true executions, but no longer establish the intended guard-specific coverage. The parent verified the masking and replaced only those fixtures in the owned test file with valid Chat sources/exact carrier nodes, paired with successful controls differing only at the intended policy, source-classification or immutable guard. Mixed-source coverage now uses HistoricalText/CurrentInput on the same content carrier, rather than an argument/text carrier mismatch.

### Test-first continuity correction and honest controls

All artifacts in this section are under `.voidx/verification/core1-20261106/`; unique stems preserve earlier records. `ch2-core-continuity-start-01.json` and its two Rust snapshots capture the correction start. No other production file or API was changed.

| Artifact stem                            | Result and classification                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ch2-core-continuity-red-01`             | 8 successfully listed, then **6 genuine runtime failures / 2 first-passing controls**, with unchanged pre/post inputs and unchanged production. Four default/explicit-policy × composed/legacy-selection hopping cases, contiguous partitioning and omitted middle-site acceptance fail at `continuity_partition_or_omission_accepted`. Hopping/omission cases first confirm one restoration and one dependency; all negatives first pass zero-publication, exact refund and original-input assertions. |
| `ch2-core-continuity-red-01` controls    | Complete original text remains unchanged with no restoration/dependency. Sparse argument hopping is already rejected elsewhere by strict processing; it is a control, not a seventh RED or proof that the new continuity guard alone rejected it.                                                                                                                                                                                                                                                       |
| `ch2-core-continuity-policy-controls-01` | 7 listed, 5 passed / 2 runtime test-expectation failures at `policy_sse_valid_carrier_control_not_restored`: the reused escaped argument fixture contains two reference occurrences but the new control expected one. This is **not production RED**. The mistaken count was corrected in tests only; this record remains intact.                                                                                                                                                                       |
| `ch2-core-continuity-policy-controls-02` | Corrected valid-carrier controls and negatives **7/7 before production modification**. First-passing coverage repair/control, not new RED.                                                                                                                                                                                                                                                                                                                                                              |
| `ch2-core-continuity-green-01`           | Continuity **8/8** after the private production correction. The intended six rejection assertions and original/refund checks are unchanged.                                                                                                                                                                                                                                                                                                                                                             |

`native_response.rs` now uses private `visit_chat_sse_carriers` traversal for both exact-node key lookup and validation of each selected carrier's complete original sequence. Before lifecycle creation or matcher work, every original frame with a source is examined; every string node of that carrier must match the next exact stream pointer, with no omission, jumping, partitioning, duplication or reordering. Other choices and sparse calls may still interleave. Missing/null fields do not invent fragments; source-less no-data/DONE frames do not invent carriers. One existing 4 Mi lookup-work allowance charges frames, choices, field lookups and tools across the inspection; charged site/key/selection ownership remains in the existing `PayloadVec` objects. No public staging signature, dependency authority, finalizer, proof or protocol semantics were extended.

### Current candidate 03 execution and identity

`chat-native-shared-conservative-20261108-candidate-03` is recorded in `ch2-core-continuity-candidate-03.json`, including all 1,536 tracked/untracked non-artifact inputs, the cumulative nine Rust paths, current evidence stems and failure classifications. `ch2-core-continuity-candidate-03-whole.diff` is the entire Rust amendment against CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`; `-increment.diff` is the two-file Vesper correction against `ch2-core-continuity-start-01`. All prior candidates, diffs, reviewer FAILs and execution records remain retained.

| Current parent artifact stem            | Result                                                                            |
| --------------------------------------- | --------------------------------------------------------------------------------- |
| `ch2-core-continuity-controls-01`       | CH2: **89 listed / 89 passed**. Eight added continuity tests, not eight new REDs. |
| `ch2-core-continuity-final-security-01` | Security: **2006 listed / 2006 passed**.                                          |
| `ch2-core-continuity-final-session-01`  | Session: **34 listed / 34 passed**.                                               |
| `ch2-core-continuity-final-matrix-01`   | Matrix: **3 listed / 2 passed, 1 ignored**; ignored was not executed.             |
| `ch2-core-continuity-check-clippy-01`   | Cargo check and Clippy `-D warnings`: both **exit 0**.                            |

Each current command manifest's complete pre/post input map equals candidate 03's map. Cargo remained globally serialized with `native-lane-cargo.lock`, `CARGO_BUILD_JOBS=2` and main `src-tauri/target`; focused runtime followed successful nonzero listing and used `--test-threads=1`. Two-file rustfmt and `git diff --check` exited 0. The known compact-unwind linker warning remains recorded. Exact reproduction uses Section 57's unchanged runner/commands with fresh stems and the current table's security/session/matrix filters; the continuity filter is `proxy::security::unified_core_tests::ch2_core_continuity_`. Cargo check/Clippy require the same global lock/jobs/target and saved pre/post maps.

Parent-recorded SHA-256 values:

- `restoration_core/native_response.rs`: `775627c4cd1b1efe006df6a648e941cff84a844362e5aa9a3cb26b33f45aa339`.
- `tests/unified_core_tests.rs`: `c06a79da144521377c8be070a0073f79d92e3f2ceae93332fece77f4d9a55de1`.
- Unchanged focused runner: `2183fe4d952857891e713e66bd694679422a661f4e0e4ab5234d0149e2015bd8`.

The parent previously saved `ch2-core-isolation-boundary-preclosure-01.json`: candidate 02's then-current 1,536 inputs differed only in appended documents; all five executable evidence maps and the whole diff matched candidate 02; original document prefixes were intact. It also established a baseline inventory of 1,836 retained artifacts. That inventory begins at its own checkpoint, not an invented earlier directory-wide hash history. Code changed afterward for the verified Vesper findings, so candidate 02 execution is historical, not evidence for candidate 03. `ch2-core-continuity-doc-prefix-01.json` captures exact document prefixes including Sections 13/57 before this append; those prefixes must remain byte-for-byte unchanged.

### Remaining holds and next execution

1. Retain Kai's full **whole-amendment PASS/FAIL** for candidate 03; a scoped continuity/F1/policy PASS is insufficient. Verify any mandatory finding against source and runtime before fixing, or stop if another file, API extension or protocol-semantic change is necessary.
2. Obtain human fresh-reader and LLM execution-readiness PASS for the latest plan/evidence append. Format only new suffixes via stdin Prettier, preserving all historical prefixes/statuses and failure records.
3. Recheck all tracked/untracked non-artifact inputs against `ch2-core-opaque-start-01.json` (only four owned paths changed), candidate 03 (only appended documents differ), unchanged runner, retained-artifact inventory, strict conversion/Disabled registration boundary and exact evidence mapping. Save a new parent manifest; changed/uncertain executable/config/test inputs require affected reruns and re-review.
4. Only after all gates pass, append the actual frozen source/contract identity and acceptance checkpoint. No freeze has occurred in this section.

Maintain exact binding/attempt, one dependency ledger, budgets/refunds, immutable restrictions, strict original/final validation, real HTTP EOF, complete tool hold, sealed publication and last-yield SSE continuation. The architecture, other shared files, C5, lanes, route/registration/conversion matrix and identity sources remain unchanged in this increment. No dependency, storage, schema, UI, setting or token store is authorized. No commit/push, app launch/install, ignored/live tests or credentials were used. Renderer/unit/all-library/integration/full-format checks were not rerun: there is no current Full Gate claim. Shared-amendment closure, if later achieved, does not accept CH2/CH3, AN2/AN3, GE2/GE3, lane resource handoffs, sequential M2 → M3 → M4, M5 or live/release; it never implicitly distributes, rebases, integrates or enables lanes.

## 59. Chat-only shared amendment frozen identity and acceptance

**Decision: freeze and accept only the bounded Chat-only shared amendment as `chat-native-shared-conservative-20261108-frozen-01`.** Candidate 03 has passed its required whole-amendment static review, latest pending-checkpoint document dual gate and parent source/contract/evidence checks. This is not lane, Full Gate, M2–M5 or live/release acceptance. Native registrations remain **Disabled**; no distribution, rebase, lane consumption, integration or activation follows. Plan Section15 records the same bounded decision without rewriting historical pending checkpoints.

### Frozen identity and evidence ownership

The source/contract identity SHA-256 is **`0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`**. The authoritative machine-readable mapping is `.voidx/verification/core1-20261106/ch2-core-continuity-closure-ready-01.json`. Its `identity_payload` binds CORE-1, all nine cumulative Rust file hashes, the unchanged architecture, reviewed Plan11–14/Evidence54–58 checkpoint hashes, runner and bounded scope. The digest uses UTF-8 `json.dumps(identity_payload, sort_keys=True, separators=(',', ':'))`. This acceptance suffix is excluded from that digest to avoid self-reference; the final boundary manifest records current document hashes separately.

Candidate 03's manifest SHA-256 is `263d408172fb247dd966dcac78066745638f987afe3269a005ca96893c9af520`; cumulative `-whole.diff` SHA-256 is `930d5a7aed3954a2b9a97987f820a4fa1840d935603c5fd184d2dc071c031eb1`. The two owned executable hashes remain Section58's `775627c4...aa339` and `c06a79da...5de1`, with complete values in the manifest. The runner remains `2183fe4d952857891e713e66bd694679422a661f4e0e4ab5234d0149e2015bd8`. All artifacts below are in the same verification directory.

| Gate or boundary                       | Retained record and result                                                                                                                                                                                                                                                                                                                                                                   |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whole candidate 03 code                | `ch2-core-continuity-kai-c49bac7c-review-01.json`: Kai `c49bac7c31c94a099aa2030b6fa39374`, completed **WHOLE-AMENDMENT PASS**, all nine cumulative Rust files and critical callchains/tests; no mandatory findings.                                                                                                                                                                          |
| Plan14/Evidence58 document gate        | `ch2-core-continuity-onyx-b2d29187-doc-review-01.json`: Onyx `b2d291875b97491fa3183d84915b2fd9`, completed **human fresh-reader PASS / LLM execution-readiness PASS**, no mandatory findings.                                                                                                                                                                                                |
| Preserved abnormal document run        | `ch2-core-continuity-nexus-6e20fa8d-review-01.json`: returned dual PASS but completed with `finish_reason=contract_unsatisfied`. Not treated as clean completion or sole document gate; the completed Onyx replacement does not depend on it. Earlier Nexus wait timeout is not a verdict. Kai/Onyx returned results did not supply a finish reason; none is invented.                       |
| Parent input/evidence/retention checks | `ch2-core-continuity-boundary-preclosure-01.json` and `ch2-core-continuity-closure-ready-01.json`: all 1,536 tracked/untracked non-artifact inputs accounted for; only four owned files differ from snapshot-start, only append-only docs from candidate 03; cumulative diff, runner, all five current complete pre/post maps and historical prefixes agree.                                 |
| Parent contract and format audit       | `ch2-core-continuity-contract-audit-01.json`: existing public declarations retained; additions security-internal; native constructors/conversion defaults and proof/release/last-yield ordering preserved. Current two-file rustfmt and `git diff --check` exit 0. `ch2-core-continuity-doc-draft-01.json`: Plan14/Evidence58 suffix-only Prettier/idempotence and exact-prefix checks pass. |
| Preserved parent tooling failure       | `ch2-core-continuity-boundary-attempt-01.json`: first inventory assertion failed because top-level enumeration omitted historical subdirectories. All 1,836 old entries already matched. Recursive enumeration passed with 1,877 retained files; closure-ready checks preserve that set and later artifacts. No production change or runtime RED arose from this verification-script error.  |

Both independent reviewers used only read/search/find, not commands, tests, hashes, edits, LSP or delegation. Their complete reports distinguish static inspection from parent execution and disclose coverage limits. Neither exhausted every historical log or input entry; Kai's PASS is whole-amendment static review, not an exhaustive proof. Parent runtime/hash/format/retention work must not be attributed to them. Staging and final verification still share the composer, not an implementation-diverse oracle. Onyx candidate01 and Vesper candidate02 whole FAILs, their step/time limits, all prior candidates and failed/control runs remain retained as history.

### Current executable evidence and the stopping boundary

Section58's five current command manifests still have complete equal pre/post maps matching candidate 03. Parent rechecked executable/config/test identity before reuse: **CH2 89/89; security 2006/2006; session 34/34; matrix 2 passed / 1 ignored; Cargo check and Clippy `-D warnings` exit 0**. Runtime followed successful nonzero listing, `--test-threads=1`, global `native-lane-cargo.lock`, `CARGO_BUILD_JOBS=2` and main `src-tauri/target`; ignored was not run. No new Cargo execution was necessary for append-only documentation. The retained 1,836-file baseline is a checkpoint inventory, not invented earlier full-directory history. Final acceptance-suffix format, prefix, current hash and report mapping checks are recorded in `ch2-core-continuity-final-boundary-01.json`; source/config/test drift invalidates reuse and requires affected reruns and whole re-review.

Keep exact binding/attempt, one dependency ledger, charged budgets/work/refunds, immutable restrictions, strict original/final validation, true HTTP EOF, complete tool hold, sealed publication and last-yield continuation. The four-file ownership remains `restoration_core/native_response.rs`, `tests/unified_core_tests.rs` under `src-tauri/src/proxy/security/`, this evidence file and the parallel plan. Architecture and all other shared/C5/lane/HTTP proof/finalizer/route/registry/conversion-matrix/identity-source inputs remain unchanged in this snapshot increment. No new API/protocol semantics, dependency, storage, schema, UI, setting or token store is authorized.

**Stop after this shared checkpoint.** CH2/CH3, AN2/AN3, GE2/GE3, resource handoffs, sequential M2 → M3 → M4, M5, current Full Gate and live/release remain unfinished independent gates. Section57 supplies exact reproduction commands for a later authorized consumer, using fresh stems and the existing lock/listing/runtime discipline. Renderer/unit/all-library/integration/full-format checks were not rerun; no full-project, installer, signing, notarization or release claim is made. No commit/push, app launch/install, ignored/live test, credential use, lane distribution/rebase/integration/enablement occurred. CORE-1 remains the committed baseline; this accepted shared amendment remains uncommitted and unpushed.

## 60. Approved Chat-only consumer delivery and execution boundary

**The user explicitly approved the prior seven-step Chat-only plan with "agree, continue" after the earlier approval request timed out.** The timeout was not approval and was not treated as a rejection. This new authorization permits only Chat consumption of the frozen shared amendment and independent CH2/CH3 closure, with the Chat-exclusive target exception. It does not authorize M2, consumption by Anthropic/Gemini, further shared edits, registration enablement, Git rebase, commit or push. [Plan Section16](security-dlp-unified-core-parallel-implementation-plan.md#16-approved-chat-only-consumer-delivery-and-independent-ch2ch3-closure) defines ownership, exact commands, TDD ordering, review and stopping gates. The documents are mixed human/LLM appendices; the pre-behavior dual document gate is not yet claimed.

### Parent-only snapshot and delivery facts

- The previous bounded shared acceptance is complete: `ch2-core-continuity-final-acceptance-01.json`, SHA-256 `c44631b9e66b9e1fbe37e608c0d9bdd2427baedbf5e01469c2aa39b9bd6424fe`, maps Kai whole-amendment PASS, Onyx Plan14/Evidence58 dual PASS and Pixel `e0a553daef3847bb818e2e0725755a7c` Plan15/Evidence59 dual PASS. Pixel returned `completed`, no mandatory findings; its result supplied no finish reason, which was not inferred. The parent rechecked 1,536 current inputs and 1,888 retained artifacts against that receipt, then two-file rustfmt and `git diff --check` exited 0. That was Chat-only shared acceptance, not lane acceptance. The final-boundary artifact's earlier pending-document text was preserved, not rewritten.
- Before consumer delivery, the parent reconfirmed the receipt hash, canonical source/contract digest `0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`, all 1,536 main inputs and all receipt-listed artifacts. `chat-lane-consume-20261108-start-01.json` records a new recursive inventory of **1,889** old artifacts (the prior 1,888 plus the final acceptance receipt), full main/three-lane input maps, document byte-prefix snapshots, frozen source hashes, and Chat CH1 snapshots/dirty diff. It does not invent older directory history.
- All three worktree HEADs were CORE-1 `ca7f883c742df07abe9bcaa9b0eade02aae5e119`, each with only its existing lane `mod.rs`, `request.rs`, `tests.rs` dirty. Every Chat shared destination to be copied exactly matched its CORE-1 version, so no extra target work was overwritten. The parent copied the nine Rust entries of closure-ready `identity_payload.source_sha256` from main to Chat, then checked each against its frozen hash. Exactly those nine Chat inputs changed; all five original Chat lane files, main inputs and other two lanes remained unchanged.
- `chat-lane-consume-20261108-delivery-01.json` records **9 exact copies / CH1 preserved / main and other lanes unchanged / no Cargo executed**. Consumer has 1,536 non-artifact inputs at delivery. No lane behavior, protocol review PASS, runtime GREEN, lane handoff, M2 or Full Gate is claimed by copying. Lane historical documents are not replaced with main-tree documents. The copied C5/finalizer/shared files are immutable consumer inputs, not newly editable ownership.
- The unchanged focused lane runner is `run-native-lane-focused.py`, SHA-256 `f0df435c652265d0084aead8627e136ddab5c730b9a70fff504124ecf2f17085`. Under the same `native-lane-cargo.lock`, jobs2 and successful nonzero listing before serial runtime, it uses `.voidx/cargo-targets/chat`. This exception was explicitly approved to keep distinct worktree builds isolated; main-tree verification still uses `src-tauri/target`. Full source/environment evidence must be recorded for new checks; pre-delivery baseline counts are historical, not consumer acceptance.

All artifacts named here reside under `.voidx/verification/core1-20261106/`. Parent snapshot/hash/copy/format/runtime checks are not independent reviewer executions. Reviewers may only read/search/find and must explicitly distinguish static review from these parent facts. The transient workflow `no_active_nodes` response on resumption is retained in tool history; the parent entered the document workflow rather than treating it as an approval or production RED.

### Remaining gates and unchanged boundaries

First obtain Plan16/Evidence60 human fresh-reader and LLM execution-readiness PASS, then run consumed Chat/security baseline, and only then start test-first CH2 followed by CH3. Preserve all earlier failures, controls, candidates, reports and artifacts. Every new behavior needs genuine production runtime RED before its implementation; compile/format/lock errors, wrong fixture expectations, parent verification errors and first-passing controls are not RED. Extend the English protocol-state table, use real core/finalizer/HTTP EOF fixtures, require whole-Chat protocol review and all four focused filters/static checks on the actual handoff state. Append actual results and final document dual gate later rather than predicting counts now.

Keep exact binding/attempt, one dependency ledger, budgets/work/refunds, immutable limits, strict original/final validation, genuine HTTP EOF, complete tool hold, sealed publication and SSE last-yield continuation. Staging and final validation share the composer; no implementation-diverse oracle is claimed. Stop for separate approval if the frozen API cannot support correct behavior. Native registration stays **Disabled**; no other lane change/distribution, main-tree integration, route/identity/conversion change, new dependency/store/schema/UI/setting, ordinary-forwarding fallback, commit/push, app launch/install, ignored/live test or credential use is permitted. CH2/CH3 and lane handoff remain unfinished; AN2/AN3, GE2/GE3, sequential M2 → M3 → M4, M5, current Full Gate and live/release acceptance remain independent and unauthorized here.

## 61. Chat CH2/CH3 consumer results and handoff evidence

**The Chat-only consumer candidate has passing bounded parent checks and independent whole-protocol review, with no mandatory source finding.** This is private Disabled adapter readiness, not M2, HTTP/router integration, shipped behavior, Full Gate or release acceptance. [Plan Section17](security-dlp-unified-core-parallel-implementation-plan.md#17-chat-ch2ch3-closeout-and-disabled-handoff-boundary) defines the final document/continuity gates and stop. These mixed human/LLM appendices deliberately retain Sections16/60 and older headers as historical pre-execution states. The final document verdict and handoff are not predicted here: after the actual dual PASS and exact-state verification, `chat-lane-ch23-final-handoff-01.json` is the authoritative closeout of this checkpoint without changing reviewed text again.

All artifact names below are relative to `.voidx/verification/core1-20261106/`. Hash/copy/build/test/format/retention checks were executed by the parent. Independent reviewers only read/search/find; none executed the parent commands or recomputed hashes.

### Authorization, source identity and consumed baseline

The user-approved Section16 scope was followed: exact delivery of the nine frozen Rust files to `.voidx/native-lanes/chat/`, then behavior edits only inside owned Chat files. The frozen amendment remains `chat-native-shared-conservative-20261108-frozen-01`, source/contract identity `0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`. `chat-lane-consume-20261108-{start,delivery,doc-draft}-01.json` preserve original inputs, CH1 snapshots, byte prefixes, old artifacts and nine exact copied hashes. Echo `427dda0eddf34dc7af147501dbed07b1` completed the pre-behavior Plan16/Evidence60 human fresh-reader and LLM execution-readiness reviews with dual PASS, no mandatory findings; the complete report is `chat-lane-consume-20261108-echo-427dda0e-doc-review-01.json`. That gate preceded baseline and implementation.

The consumed baseline used fresh stems `chat-lane-consume-20261108-baseline-chat-01` and `chat-lane-consume-20261108-baseline-security-01`: successful nonzero listings of **19** and **2024**, then **19/19** and **2024/2024** runtime passes, with identical complete pre/post inputs. These are first-passing baseline controls, not production RED.

### Test-first implementation record

Every stem in this table retains `-source.json`, `-list.log` and, after successful nonzero listing, `-test.log`. Only intended runtime failures of absent production behavior count as RED. Later control additions may pass on their first run.

| Work and evidence stem          | Listing and runtime             | Meaning                                                                                                              |
| ------------------------------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `chat-lane-ch2-json-red-01`     | 27 listed; 20 passed / 7 failed | Valid runtime RED for absent JSON native observation/staging.                                                        |
| `chat-lane-ch2-json-green-03`   | 27 listed; 27/27                | JSON implementation GREEN after correcting test-only delivery timing/refund expectations in retained GREEN01/02.     |
| `chat-lane-ch2-sse-red-01`      | 31 listed; 27 passed / 4 failed | Valid runtime RED for absent SSE native consumer.                                                                    |
| `chat-lane-ch2-sse-green-01`    | 31 listed; 31/31                | SSE implementation GREEN.                                                                                            |
| `chat-lane-ch2-sse-controls-02` | 37 listed; 37/37                | Expanded controls GREEN after the synthetic response-id fixture correction; not a new feature RED.                   |
| `chat-lane-ch3-red-02`          | 43 listed; 40 passed / 3 failed | Valid runtime RED for missing strict delivered import and structural pairing.                                        |
| `chat-lane-ch3-green-01`        | 43 listed; 43/43                | CH3 implementation GREEN.                                                                                            |
| `chat-lane-ch3-controls-01`     | 45 listed; 45/45                | Current candidate: actual SSE output replay plus ambiguous-ID/budget controls; first-passing additions are controls. |

**CH2 JSON:** `src-tauri/src/proxy/security/protocol_adapters/chat/response.rs` inside the consumer selects exact native message nodes in original choice-array order. The eight added JSON tests cover supported content/refusal, non-restoring readable reasoning, complete content arrays, strict object-decoded function argument strings, sparse multi-choice delivery, original-wire checks and invalid/budget atomic rejection. Text/output_text/refusal parts are supported; opaque media and unknown siblings are preserved. `JsonCompletion::native_chat()` validates the budgeted serialization for staging; it does not issue HTTP completion or release authority. Frozen lifecycle staging uses a single dependency ledger. Strict original wire and exact attempt/binding proof still come from the shared verifier.

**CH2 SSE:** `response.rs::finalize_sse` uses `PayloadJson`, `PayloadBuffer` and `PayloadVec` for retained parsing/metadata. It parses complete exact original frames and groups supported fragments by choice plus text field or choice-local tool index. Typed Output/Refusal/Reasoning/ToolArguments streams go to the frozen lifecycle; reasoning remains non-restoring. Every source frame's choices are selected in original order through `NativeSseSnapshotSelection::chat_choice`. Controls cover every split of the tested reference, sparse/interleaved choices/calls, cross-carrier isolation, opaque values, CRLF/no-data/usage frames, strict terminal/DONE handling, actual EOF/error/Pending cancellation, capacity refunds, entity/attempt/binding substitution and last-yield continuation. This does not claim every UTF-8/network byte boundary or add SSE content-array semantics. The frozen entity/frame/site/selection limits remain 1 MiB / 1024 / 128 / 128; exact slices must cover the ordered entity, and no-data/DONE observations have no source/sites. Unsupported evolution fails through the existing safe failure contract.

**CH3:** `history.rs::import_delivered` calls frozen `verify_native_delivered_extracted` before `import_native_completed_context_extracted`, without lane-local authority or optional degradation. Complete ordered assistant/call evidence is mandatory; JSON choices retain source order, and SSE evidence includes composed assistant plus separate call snapshots. `history.rs::observe_request` wraps preserved CH1 observation with borrowed `HistoryClaim::CallResultPair` metadata only for unique, unconsumed function IDs in the immediately adjacent assistant/tool group. First, replayed and edited tool-result bodies remain untrusted current DLP input, not historical permission or execution proof. Tests cover exact replay, repeated import, edited/reordered/truncated/unknown evidence, no-session, cross-session/provider/protocol/store, retry, eviction, first/edited result DLP and actual SSE output replay. SSE replay is rejected before the last-yield continuation and accepted only afterward with complete call evidence. Ambiguous IDs and budget controls pass.

### Preserved failures and non-RED classification

- JSON GREEN01's `chat_ch2_json_published_before_delivery` assertion incorrectly assumed SSE-like publication timing for JSON. Frozen JSON intentionally activates after strict proof/final encoding and before returning the body stream. GREEN02's `chat_ch2_json_delivery_charge_retained` assertion checked refunds while returned `Bytes` chunks still owned the charge; dropping `chunks` before checking refunds fixed only the fixture. Both failed runs remain; neither is production RED or a finalizer change.
- `chat-lane-ch2-sse-controls-01` listed 37, with 35 passed / 2 failed (`chat_ch2_sse_http_proof_rejected`). The helper omitted the response `id` required by strict completion at frozen `completion_evidence.rs:404–411`. Adding a synthetic id to the fixture produced controls02 37/37; no production relaxation was made.
- `chat-lane-ch3-red-01` failed compilation during listing with E0277 because `PlanError` is not `StdError`. The test mapped it to a static synthetic error; only red02 then obtained intended runtime RED. A pairing site-count expectation was corrected from seven to six (one argument, four tool bodies, one user body), without changing CH1 scanning.
- A parent candidate script used nonexistent `input_sha256`/`inputs_sha256` keys and raised `AssertionError: ['head', 'status', 'inputs']`. The real snapshot map is `start_manifest['lanes'][lane]['inputs']`; correcting the parent check produced candidate01 without an earlier candidate write. This is not a production failure. Formatter-triggered file-read guards were handled by rereading, not overwriting external work.
- Historical compile/format/lock/timeouts, incorrect fixture expectations, parent-script errors and first-passing controls are retained rather than relabeled RED. Earlier checkpoint timeout was not approval or user rejection. Read-offset/nonexistent-path failures and dot-worktree search misses were investigation/tool results, not proof that an API was absent. The prior shared FAIL/RED/candidates and final-boundary pending text were not rewritten.
- Nova's earlier 1024-second wait timeout meant running, not PASS/FAIL. Its eventual tool finish was `step_limit`; Cipher's later confirmation ended `contract_unsatisfied`. Their complete PASS reports and literal finish details are retained below. Neither tool finish is a production runtime RED, and neither is presented as normal completion.

### Current candidate and reproducible parent verification

`chat-lane-ch23-candidate-01.json`, SHA-256 `7d1768d0628bfec24125bba95a09089cc89181e46d1463f5679d7977ca0e2f72`, binds **1,536** Chat inputs and `chat-lane-ch23-candidate-01-whole.diff`, SHA-256 `b858dc992920653eeeced997784f940999339d82e8c1572ec15c3b227d6daa15`. Relative to frozen delivery, only `history.rs`, `response.rs`, `tests.rs` changed. `request.rs` and `mod.rs` remain unchanged; preserved CH1 tests remain included. The owned hashes under `src-tauri/src/proxy/security/protocol_adapters/chat/` are:

| File          | SHA-256                                                            |
| ------------- | ------------------------------------------------------------------ |
| `history.rs`  | `911b2c14f8dd8b9f85e320927810e4d84df67a6788d4959b34997825f8e4efe9` |
| `mod.rs`      | `17c4296d4534dbf9be0f6a8dcfb9289f3c78b70cfb7b272760f2cd236ff01528` |
| `response.rs` | `8eb09d3aacf9e07e26e32c068086bad528887b25839f42b2e7bf3a3e8b8158b0` |
| `tests.rs`    | `656a9c8176c55594ee3730291843cdeacff6dca0d20ae6265598bd84f96bd008` |

Environment: Darwin arm64; rustc `1.98.1 (48a229cea 2026-09-01)`; cargo `1.98.1 (797e8a9bc 2026-08-05)`; Node `v25.2.1`; pnpm `10.12.3`. The parent reconfirmed these versions before document closeout. The unchanged runner SHA-256 is `f0df435c652265d0084aead8627e136ddab5c730b9a70fff504124ecf2f17085`. Invocations ran from the main repository root, with actual Cargo cwd `.voidx/native-lanes/chat`, absolute `CARGO_TARGET_DIR=<main>/.voidx/cargo-targets/chat`, `CARGO_BUILD_JOBS=2`, and the exclusive global `native-lane-cargo.lock`. Main-tree target remains `src-tauri/target`.

```bash
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat chat-lane-ch3-controls-01 proxy::security::protocol_adapters::chat::tests::
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat chat-lane-ch23-security-01 proxy::security::
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat chat-lane-ch23-session-01 proxy::session::tests::
python3 .voidx/verification/core1-20261106/run-native-lane-focused.py chat chat-lane-ch23-matrix-01 proxy::protocol_matrix_tests::
```

These are recorded invocations, **not reusable stems**. A future run must replace each stem. The runner executes `cargo test --lib --manifest-path src-tauri/Cargo.toml FILTER -- --list`, requires exit 0/nonzero count and unchanged inputs, then executes the same filter with `--test-threads=1`. No ignored/live run was requested. Full commands, logs, exits, durations and complete pre/post input maps remain in the per-stem artifacts.

| Candidate-bound evidence                  | Listed | Current runtime/result                                  |
| ----------------------------------------- | -----: | ------------------------------------------------------- |
| `chat-lane-ch3-controls-01`               |     45 | 45 passed, zero failed/ignored                          |
| `chat-lane-ch23-security-01`              |   2050 | 2050 passed, zero failed/ignored                        |
| `chat-lane-ch23-session-01`               |     34 | 34 passed, zero failed/ignored                          |
| `chat-lane-ch23-matrix-01`                |      3 | 2 passed / 1 ignored; ignored not executed              |
| `chat-lane-ch23-check-clippy-01.json`     |    n/a | Cargo check and Clippy `-D warnings`, both exit 0       |
| `chat-lane-ch23-runtime-boundary-01.json` |    n/a | Four-file rustfmt and Chat/main diff checks, all exit 0 |

Check/Clippy ran sequentially with the same global Python `fcntl.flock(LOCK_EX | LOCK_NB)`, jobs2 and absolute Chat target, never unlocked. Commands were `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` with cwd Chat. Non-Cargo commands were the exact four-file `rustfmt --edition 2021 --config skip_children=true --check` and Chat/main `git diff --check` in Plan16. Complete pre/post maps for all four focused records and check/Clippy match candidate01. The runtime-boundary receipt SHA-256 is `f6973e18dfe5deac30794a1a6dc838b4bb87f1970caa820b96e69e31f9591165`; its original review-pending state is retained, not overwritten. Linker output retains the existing `__eh_frame` compact-unwind size warning; no warning-free linker claim is made.

### Independent protocol review and completion details

- **Nova `1cdb215839de44688daf6afaf40a7217`: whole-Chat protocol PASS, mandatory_findings=[]** for preserved CH1, CH2 JSON/SSE, CH3 and frozen consumption. Tool status was `completed; finish_reason=step_limit`. `chat-lane-ch23-nova-1cdb2158-review-01.json` retains the full report, literal finish reason, candidate binding and caveat. The report checks exact nodes/choice/tool isolation, single lifecycle, strict proof/EOF/final wire, sealed last-yield publication and verified ordered history; it read parent current checks but did not execute them or independently rehash inputs. Historical TDD logs were parent-provided history, not all individually re-audited by Nova.
- **Cipher `543bbab752ef48898cfa4a6f9c2fec1b`: bounded confirmation PASS, mandatory_findings=[]**, after actual lane source and frozen finalizer/history/budget checks. Tool status was `completed; finish_reason=contract_unsatisfied`. `chat-lane-ch23-cipher-543bbab7-review-01.json` preserves the full returned JSON and caveat. It is neither normal-completion evidence nor a replacement whole review.
- **Nexus `e581f04f0dca49c7a9719fb008d72f87`: terminal review-package/source confirmation PASS, mandatory_findings=[]**. Tool returned `completed` without a finish reason; none is inferred. `chat-lane-ch23-nexus-e581f04f-review-01.json` retains the full report. Nexus read both reports and actual lane `history.rs:14–115`, `response.rs:208–404`, frozen `finalization.rs:493–638`; it found no conflicting mandatory issue and explicitly accepted the bounded Disabled handoff package while preserving the earlier anomalies. Its scope is narrower than Nova's whole review.

All reviewers stayed within read/search/find. Independent static review is not independent runtime execution. No mandatory finding required code changes after candidate01. No claim is made that the anomalous finishes became normal retroactively.

### Final retention/document gate and limits

Before these appends, parent `chat-lane-ch23-prehandoff-boundary-01.json`, SHA-256 `fbcee367a66937c48cb3b086048388c14bfc71cbce01002c233264612aa522d4`, reconfirmed current candidate/runtime map equality, all three lane HEADs at CORE-1, nine shared hashes matching main/freeze, unchanged other-lane maps and exact reviewed whole diff. Main inputs differed from authorization start only in the two authorized documents; existing main shared dirty work was preserved, not reverted. All **1,889** start artifacts were unchanged; a recursive **1,961** then-current artifact map was saved before the subsequent confirmation records. It preserves the immediate plan/evidence prefix sizes **131,042 / 429,316** bytes with SHA-256 `314fe3a8c3b24ff50b42890018ff822e1814d16da608e68eaae8a8bd1bc0b99f` / `a8f7316466d8f966d1031ce7d31d9e94c8be9018a5f34854a1878f0c89dce98d`. Original pre-Section16/60 prefixes remain **121,857 / 423,409** bytes, with original hashes in the start manifest. Only the new suffix is formatted and checked for idempotence; historical prefixes are checked byte-for-byte, not reformatted.

The next and final action is the independent human fresh-reader plus LLM execution-readiness document gate, followed by current continuity/static checks and a non-circular final receipt binding source, contract, runtime, reviews, exact reviewed documents and retained artifacts. That receipt, not an anticipated PASS in this appendix, records the actual verdict, report/status details and final counts. Relevant runtime evidence is reused only with identical lane executable/config/test/environment inputs; source drift requires affected reruns/new candidate/whole re-review, and any required shared change requires a preserved reproducer and scoped decision.

Native registration remains **Disabled**. No other lane consumed this amendment; no main-tree source integration, rebase, commit/push, route/identity/conversion change, dependency/storage/schema/UI/settings/token-store addition, ignored/live test, credential use or app launch/install occurred in this Chat stage. CORE-1, Responses, P3/P4, the frozen shared amendment and the authority architecture remain intact. Handoff stops this authorization: AN2/AN3, GE2/GE3, sequential M2 → M3 → M4, M5, Full Gate and live/release acceptance stay separate and unauthorized.

Coverage is deliberately bounded. Synthetic transport exercises the real async EOF driver but not real-network tee/backpressure or latency. Placeholder splits do not establish all UTF-8/network-byte boundaries; first/edited tool-result tests are not full live client two-round integration. HTTP client-error projection, optional historical fallback and genuine upstream-error preservation remain HTTP-orchestration acceptance, not lane-local authority. Frozen conservative composer and finite budgets are unchanged, and staging/final validation share the composer. Renderer checks, complete all-library/integration gates, installer/signing/notarization and release are not established by this focused Chat evidence.

## 62. Approved direct-native shared amendment start and protocol evidence

**The user authorized full developer closure, then explicitly approved the minimum shared amendment.** The literal approval is retained in `native-direct-shared-approved-start-01.json`; in English, it means “Agreed, continue.” This supersedes the historical Chat-only stop, not any security invariant. Newly authorized work is integrator-only, versioned support for direct Anthropic/Anthropic and Gemini/Gemini lifecycle, strict HTTP-owned proof and complete delivered-history composition, followed by TDD, whole independent review, re-freeze and exact delivery to all three lanes. [Plan Section 18](security-dlp-unified-core-parallel-implementation-plan.md#18-approved-direct-native-shared-amendment-and-full-developer-closure) specifies ownership, tasks, commands and stop conditions. Remaining lanes and M2 Chat → M3 Anthropic → M4 Gemini → M5 are authorized but not yet accepted.

### Starting identity and approval

All records below are under `.voidx/verification/core1-20261106/`, use new stems and preserve previous receipts, failures and pending statements.

| Record                                              | SHA-256                                                            | Scope                                                                                                                                                                                 |
| --------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `chat-lane-ch23-final-handoff-01.json`              | `d3e05f55419b9396fc658f63396a1e74e56a858ef21b2206f087ba32d6ba24b5` | Completed Chat-only Disabled handoff; historical STOP was correct at that time.                                                                                                       |
| `full-closure-authorized-start-01.json`             | `61b0ab6c11451e1d947a7ea5f96b454e5f530f446a03930a86e0a755f8dc85bf` | Parent verified four CORE-1 HEADs and 1,536 inputs per tree unchanged from handoff, preserved 1,973 old artifact hashes, and inventoried 1,975 current artifacts before new receipts. |
| `full-closure-frozen-api-reproducer-01.json`        | `1aa407b5c5ea49ed1cbb52995d88eb457c380655e4b865ea3207bdeccd4e38be` | Exact guards and reused passing rejection control, not new feature RED.                                                                                                               |
| `full-closure-frozen-api-readiness-reviews-01.json` | `d19df212bc8d6092e104218fca9390cebd0e712d49f3419627c470f54c463d60` | Complete static readiness reports and literal anomalous tool finishes.                                                                                                                |
| `native-direct-shared-approved-start-01.json`       | `c9014fdcb81a7158a9f13ee6946b0576dc54c4997287f12bd2fde32fd3613c11` | Literal explicit approval, exact scope, pre-append prefix sizes/hashes and reconfirmed unchanged source/HEADs/artifacts.                                                              |
| `native-direct-shared-protocol-evidence-01.json`    | `64242ea318f95c0671f57ccd3a8d1a74fccf7aeb8cc4a9639c528196afa814a4` | Retained official fetched references, exact excerpts, conservative interpretation and unresolved Gemini association gate.                                                             |

All four HEADs remain `ca7f883c742df07abe9bcaa9b0eade02aae5e119`. Main/Chat still match the nine frozen sources and identity `0a2db1a78fdffae9952a09758ec110bfefd76fd8a30e9991ce4b957ad65c7c6c`. Anthropic/Gemini retain CORE-1 plus their existing AN1/GE1 owned dirty files, not the Chat amendment. No implementation/delivery occurred during this checkpoint. No new compilation/runtime was executed. Parent version commands reconfirmed Darwin arm64, rustc `1.98.1 (48a229cea 2026-09-01)`, cargo `1.98.1 (797e8a9bc 2026-08-05)`, Node `v25.2.1`, pnpm `10.12.3`; the Cargo version query held the global lock.

The preceding scoped checkpoint timed out: it was neither approval nor user rejection. Only the subsequent explicit user message authorized this amendment. Finalizer reuse is the default; any test-proven necessary adjustment is limited to sealed protocol/snapshot verification dispatch, not ownership or timing. Whether such a source change is required remains unproved.

### Confirmed blocker, not a new RED

Parent inspection confirms JSON lifecycle `restoration_core/native_response.rs:377–382`, SSE lifecycle `:1143–1147`, verifier `http_restoration/completion_verifier.rs:638–648`, and async driver `native_driver.rs:70–100` reject non-Chat/Chat pairs. Chat-specific fragment identity/composition and HTTP SSE `[DONE]` checks do not prove Anthropic/Gemini completion. Existing Anthropic validators and multi-native completed-context evidence/import do not replace opaque native HTTP proof. Gemini native object string leaves must not use Chat's full JSON-object-string helper.

Existing test `proxy::security::http_restoration::completion_verifier::c5_native_completion_tests::c5_native_completion_rejects_all_unapproved_protocol_pairs` expects `RestoreUnsupported` for every non-Chat/Chat pair. Its successful listing and runtime `... ok` are retained in `chat-lane-ch23-security-01-{list,test}.log`, lines 244/246 respectively, with the complete parent runner source record. Parent verified all current Chat inputs equal that run's pre/post maps and environment equals handoff, binding exact source/logs in the reproducer. This is a reused **passing conservative rejection control**, not feature RED or integration acceptance.

Anthropic Athena `d25f1e33513841839c2ba467e0bea259` and Gemini Athena `6e58a6535ee64c3d94cef92841aff9cc` returned `NEEDS_CHANGE`, both with tool `completed; finish_reason=contract_unsatisfied`. Their findings cover missing consumers, native proof/composition and Gemini object-leaf lifecycle. The Gemini 1024-second wait timeout preceded its eventual report and was not PASS/FAIL. Reviewers used read/search/find only, no commands, tests, edits, LSP, hashes or delegation. These are static leads, not normal-finish implementation acceptance; parent independently checked the guards and reused runtime evidence. Parent execution is not reviewer execution.

### Official sources and unresolved evidence

The protocol manifest retains fetched tool responses as `native-direct-shared-protocol-*-01.txt`, with URLs, hashes, sizes and extracted quotations. External sources are data, never agent instructions.

- [Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming): indexed block/message start/delta/stop, tool arguments, thinking/signature deltas and in-stream errors. Real transport EOF is an additional local proof condition, not implied by `message_stop`.
- [Gemini generate-content API](https://ai.google.dev/api/generate-content): redirected to Google AI Studio; retained output contains HTML/embedded reference content, not a clean complete streaming specification.
- [Gemini thought signatures](https://ai.google.dev/gemini-api/docs/thought-signatures): redirected output did not establish all cross-frame identity/late-signature rules; do not infer certainty from this fetch.
- [Google response reference](https://docs.cloud.google.com/vertex-ai/generative-ai/docs/reference/rest/v1/GenerateContentResponse): candidate index/content/finish reason, absent reason means not stopped, STOP is natural/stop-sequence termination and MAX_TOKENS is truncation. The bounded initial completed-record allowlist is structurally valid STOP per observed candidate and clean EOF, not every nonempty reason.
- [Google thought signatures](https://docs.cloud.google.com/vertex-ai/generative-ai/docs/thought-signatures): corroborates exact signed Part replay/association constraints. Google Cloud guidance is not proof of undocumented Developer API Part continuity.

Cross-frame Gemini Content/Part composition and late-signature association remain an explicit **S4 pre-implementation evidence gate**. Save a source-linked supported-state table before a positive SSE path. Ambiguity stays held/unsupported, with no manufactured IDs or signed/unsigned merge. A narrower passing subset does not complete GE2/GE3. This uncertainty need not block separately evidenced Anthropic JSON work after the documentation gates.

### Preservation and next gates

Before these appends, plan/evidence sizes were **138,471 / 448,702 bytes**, SHA-256 `20c4c14acf9fc79ef03bb4c7e85e4a06626f27d80410414ade87a10d12fbf84f` / `87e8ca1aa396ae5f8663014b603757d9a27239025da9634ae1b9c91991c3800e`. Preserve entire historical prefixes, format only English suffixes and require idempotence plus human fresh-reader/LLM execution-readiness PASS. A later exact-state document receipt records actual outcomes; this draft does not anticipate acceptance.

An initial parent append script failed before Python parsing completed because outer and embedded heredoc delimiters collided. `native-direct-shared-doc-parent-error-01.json` preserves exit 2, Python unterminated-string and shell syntax errors. It is a parent-script failure, not production RED. Reconfirm unchanged input maps/prefixes before the corrected independent-suffix append; never overwrite the failed result.

Then S1 → S2 → S3 → S4 → S5 uses main target, global lock, jobs2, nonzero listing and serial runtime for intended RED/GREEN. S6 requires current shared regressions, Full Gate and whole independent PASS before a new freeze. S7 delivers exact shared sources and revalidates all lane targets, preserving CH1–CH3/AN1/GE1. Only then resume reviewed lane handoffs and independently accepted M2/M3/M4 stages. M5 covers the architecture matrix, actual shipped documentation, current Full Gate and final review.

Native remains Disabled. No main lane integration, enabling, architecture change, dependency/schema/UI/settings/token-store addition, commit/push/rebase, ignored/live/credential use, app launch/install or release occurred at this checkpoint. Preserve every old artifact and status header; any later frozen-contract expansion beyond this approved scope still requires a separate decision.
## 63. S1–S4 direct-native shared amendment execution evidence

**Status:** S1–S4 are complete and sealed at stage level with the S4 malformed-candidates defect closed by fix and independent limited review. S5–S9 remain pending. This appendix records actual outcomes; it does not anticipate later acceptance.

### Stage evidence index

| Stage      | Aggregate artifact                                    | SHA-256                                                            |
| ---------- | ----------------------------------------------------- | ------------------------------------------------------------------ |
| S1 AN JSON | `native-direct-shared-an-json-stage-evidence-01.json` | `7000f4841a7cdb3b3693f77eea99e8e521e8e8532fb6c9cc84a7327adee9200b` |
| S2 AN SSE  | `native-direct-shared-an-sse-stage-evidence-01.json`  | `494a0b02d1a1255ecd515d9f8ebaceb3623d5d071921a221cf95a0ab17f87bb8` |
| S3 GE JSON | `native-direct-shared-ge-json-stage-evidence-01.json` | `80a75ac6a93f4c3738fb8c61675aa2c1bd08823d5810db93ffe38c345ecddb8f` |
| S4 GE SSE  | `native-direct-shared-ge-sse-stage-evidence-01.json`  | `f18513f11bf9a208a4b88efe8fddfc36e5dfa5f547ff17eed827fd55aa918fa6` |

### S4 evidence gate and implementation

`native-direct-shared-ge-sse-protocol-evidence-01.json` (`176606f67aa09016d76b04f180a43f397b470e31175d6c4900642cf4351a0498`) retains the official schema, thought-signature guidance and the fixed `googleapis/js-genai` commit `5e703b35c79c845c8e4d6564022d7da4e9277d1e` supporting only the append-only whole-Content narrow subset. Vesper `5017ff4aeb7b439289f1155f258197c6` completed the evidence-gate review with body PASS, actual completed; its artifact is `native-direct-shared-ge-sse-protocol-vesper-review-pass-01.json` (`f9cfaa4cffe0090c951a4bb02b4a8db4a40eda41d44d69d083acfef0e05d93e6`). TDD proof/snapshot artifacts (`proof-red-01` fixture compile error, `proof-red-02` valid production-support gap 0/3, `proof-green-01` 3/3, `snapshot-red-01` 3/1, `snapshot-green-01` 4/4) and lifecycle RED (`lifecycle-red-02` 4/1 with `NATIVE_SHARED_GE_SSE_NATIVE_OBJECT_LIFECYCLE_REQUIRED`; `lifecycle-red-01` was a fixture `E0308`) are preserved.

### S4 malformed-candidates defect closure

Juno `e6a10077b5be4acdb5c71af44bff1f5d` returned a static FAIL with tool status `completed; finish_reason=time_limit`, preserved in `native-direct-shared-ge-sse-juno-review-fail-01.json`: `gemini_chunk` conflated "candidates field absent" with "candidates present but non-array", so `data: {"candidates":null,"usageMetadata":{}}` appended to an otherwise valid all-STOP stream was accepted as usage-only by the completion proof and by `validate_restored`. Full snapshot staging in `restoration_core/native_response.rs` still rejected it, so no end-to-end sealed publication bypass was claimed.

Protocol evidence state G3 accepts only frames without a candidates field; present-but-empty or non-array candidates are malformed, and the prior empty-array acceptance was implementation-convenience drift, narrowed in the same fix.

TDD closure with parent-owned runs: RED `native-direct-shared-ge-sse-malformed-candidates-red-01` (12 listed, 11 passed/1 failed; mode 13 accepted the malformed frame and polled the client stream, polls=2!=0), minimal fix in `completion_evidence.rs` `gemini_chunk` (explicit `None`/`Some` match; empty array rejected), GREEN `...-green-01` 12/12, then `...-green-02` 12/12 after a rustfmt-only mode-15 line-wrap. Modes 13–17 cover candidates null/object/string/number/empty-array with valid usageMetadata against original stream, client stream and final-wire `validate_restored`.

Mira `6fc35d17c9f54a998c15acb7af597df6` completed a read/search/find-only five-file static review with body PASS and normal tool completion, preserved in `native-direct-shared-ge-sse-mira-review-pass-01.json`, confirming the fix covers final-wire validation and that the JSON path and snapshot staging were already strict. Reviewer runtime/hashes remain parent-owned.

### Current-source regression and compile controls

All runs are parent-owned, use new stems, the global serial Cargo lock, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, main target `src-tauri/target`, successful nonzero listing before serial runtime, and preserve superseded attempts:

- GE JSON 8/8, AN JSON 13/13, AN SSE 18/18, c5 native 176/176, CH2 89/89, unified 305/305, security 2057/2057, session 34/34, matrix 2 passed/1 ignored; all exit 0.
- rustfmt check, `git diff --check`, `cargo check` (0 warnings/errors) and `cargo clippy -- -D warnings` all exit 0; rustfmt changed only the mode-15 line-wrap in `native_driver.rs`.
- Superseded first-attempt runs (`*-rerun-01` for unified/security/session/matrix) used an expanded `proxy` filter due to a parent shell-parsing error; their only failure was the out-of-scope Chat-protocol port-binding test `agent_clients.rs:738` (503!=200) with no uncommitted `agent_clients.rs` changes. Failure evidence is preserved; correct-filter `rerun-02` runs supersede them.
- Aggregate: `native-direct-shared-ge-sse-stage-evidence-01.json` indexes 50 round artifacts and records both review verdicts with literal tool finishes.

### Explicit non-claims

Not a shared Full Gate, whole-amendment review, new freeze, exact three-lane delivery, or M2/M3/M4 integration or enablement. No commit/push/rebase, application launch/install, ignored/live/provider tests, real credentials, release, or `127.0.0.1:15722` binding occurred. The authoritative architecture document is unchanged. Native remains Disabled.

## 64. S5 native shared sealed-delivery evidence

### Scope and source identity

S5 adds three focused tests in `src-tauri/src/proxy/security/http_restoration/native_driver.rs`. The post-S4 native-driver identity `77b2857ce9a4bbf02c91972acd1768a06dee50b465a3b2bfc01c711f3dbb1d15` changed to `33c5263f4184399ae7ac4c01f74ff0468d0742a14cd80bf01587061331ed29d2` only for these tests and their rustfmt formatting. `response_restoration/finalization.rs` is unchanged at `949e393966820535f144cdb5c6ee427be6250319c50e32b5c1ab001a1b93c1fb`. The S4 identities of `completion_evidence.rs`, `completion_json.rs`, `completion_verifier.rs` and `native_response.rs` remain unchanged. No finalizer modification was test-proven necessary.

The tests use only synthetic fixtures, existing actual attempt/binding provenance, the existing sole finalizer and the internal strict verifier. They introduce no dependency, schema, UI, persistent setting, credential source, token store, route cell, cross-protocol conversion or additional activation path.

### Focused controls and TDD classification

- `native_shared_finalization_json_activation_drop_and_charged_owner_refunds`: Anthropic and Gemini, mapped and zero-mapping, proof/final-encoding activation before body return, drop before body poll, unavailable-store activation denial, persistent visible delivery after successful activation, and charged retained-body refund after the last owner drop.
- `native_shared_finalization_sse_pending_cancel_resume_and_charged_owner_refunds`: Anthropic and Gemini pending state through every yield, cancellation before the first yield and after the last yield without continuation, publication only on the continuation poll, fixed `Response stream failed.` activation failure without publication, and retained-frame charge/refund.
- `native_shared_finalization_sse_snapshot_source_and_final_mismatch_refunds`: exact snapshot, source and final-wire mismatch rejection for both protocols, with zero publication and baseline budget restoration.

Both `native-direct-shared-finalization-control-01` and the post-format `native-direct-shared-finalization-control-02` listed 3 tests successfully and passed 3/3 with unchanged pre/post input maps. Because Plan 18 requires separate RED/GREEN only for changed behavior and the first valid control passed, there is no valid S5 production RED and no production implementation change. The initial passing controls are not described as RED.

### Parent-run regression evidence

Every run used the approved main runner, a fresh stem, the global Cargo lock, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, the main `src-tauri/target`, successful nonzero listing and then `--test-threads=1`.

| Filter | Listed | Runtime result |
| ------ | ------ | -------------- |
| `native_shared_an_json_` | 13 | 13 passed, 0 failed |
| `native_shared_an_sse_` | 18 | 18 passed, 0 failed |
| `native_shared_ge_json_` | 8 | 8 passed, 0 failed |
| `native_shared_ge_sse_` | 12 | 12 passed, 0 failed |
| `c5_native_` | 176 | 176 passed, 0 failed |
| `ch2_` | 89 | 89 passed, 0 failed |
| `proxy::security::unified_core_tests::` | 305 | 305 passed, 0 failed |
| `proxy::security::` | 2060 | 2060 passed, 0 failed |
| `proxy::session::` | 34 | 34 passed, 0 failed |
| `proxy::protocol_matrix_tests::` | 3 | 2 passed, 1 ignored |

All source manifests report `unchanged_during_check=true`. The security count increased by exactly the three new focused tests. The ignored matrix test was not executed.

### Compile controls and preserved failures

`native-direct-shared-finalization-compile-controls-02` passed scoped rustfmt over the approved shared files, `git diff --check`, Cargo check and Clippy `-D warnings`; every command exited 0. Cargo check and Clippy logs contain no warning or error output.

The earlier `native-direct-shared-finalization-compile-controls-01` full-format control is retained with `all_passed=false`. Its `cargo fmt --check` exited 1 only for pre-existing `completion_tools.rs` formatting outside the S5 edit; `git diff --check`, Cargo check and Clippy exited 0. This is a formatting control failure, not a production RED. S6 must make the formatting-only change within that already-approved shared source and close the required full `cargo fmt --check` before whole-amendment freeze.

`native-direct-shared-finalization-stage-evidence-builder-01-error.json` preserves a parent artifact-builder filename assumption error. The builder expected `<stem>.json`, while the approved runner emits `<stem>-source.json` plus list/test logs. It wrote no aggregate and executed no test or source command. Builder-02 parses the actual artifacts and succeeded. This parent script error is not a production RED.

### Aggregate and limits

The aggregate is `.voidx/verification/core1-20261106/native-direct-shared-finalization-stage-evidence-01.json`, SHA-256 `7fd62d702906e96d5c2f01972e3208d737dc72739938b9ffab9ff8b916bb51ed`. It indexes 51 artifacts, including both focused controls, ten regression selections, both compile-control rounds and the preserved builder error. The aggregate excludes its own bytes/hash.

All runtime commands and hashes are parent-generated. No review-agent command execution is claimed. S5 is stage-complete only: no current Full Gate, independent whole-amendment PASS, versioned freeze, exact lane delivery, direct-native enablement, installed/live/provider test, installer, signing, notarization or release acceptance is claimed.

## 65. S6 whole shared gate, independent review and freeze evidence

### Current source and formatting closure

The S6 source-state artifact is `native-direct-shared-s6-source-state-01.json`, SHA-256 `9adcf3ac655bb962443fef7be6fe7ed9e97bb7816fa563c3b5e1b8f1e4946404`. It records ten shared source/test identities and the reviewed Plan/Evidence/authority document identities. The authoritative architecture remains unchanged at `13a1ea25967d570679cc94a0f61cc6257ddd0bd06243cef4ffeda8728e3a6166`.

The preserved S5 full-format failure was closed by applying rustfmt only to the already-approved shared `completion_tools.rs`. Its identity changed from `dfb0474007ef1ebbc2ab66a9ebab572b25a0337a80499909677ac1c7f75c3e00` to `a38d6258cfb608ea1032d7b350752f7e5d88b8b1b98ef53ce8edfa7af604cf47`. No semantic edit was made in this formatting closure. Nova statically reviewed the resulting implementation but, under the read/search/find restriction, did not independently prove the formatting-only delta.

### Whole shared selections

Each selection used the approved main runner, a fresh `native-direct-shared-s6-` stem, successful nonzero listing, serial runtime and unchanged pre/post input maps.

| Exact filter | Listed | Result |
| ------------ | ------ | ------ |
| `proxy::security::http_restoration::` | 233 | 233 passed, 0 failed |
| `proxy::security::c5_native_` | 63 | 63 passed, 0 failed |
| `proxy::security::unified_core_tests::` | 305 | 305 passed, 0 failed |
| `proxy::security::protocol_adapters::chat::tests::` | 1 | 1 passed, 0 failed |
| `proxy::security::protocol_adapters::anthropic::tests::` | 1 | 1 passed, 0 failed |
| `proxy::security::protocol_adapters::gemini::tests::` | 1 | 1 passed, 0 failed |
| `proxy::security::` | 2060 | 2060 passed, 0 failed |
| `proxy::session::tests::` | 34 | 34 passed, 0 failed |
| `proxy::protocol_matrix_tests::` | 3 | 2 passed, 1 ignored |

### Current Full Gate

A new artifact-only wrapper, `native-direct-shared-s6-full-gate-01.py`, was syntax/control-flow validated before execution. It acquired the global lock with `fcntl.flock(LOCK_EX | LOCK_NB)`, used jobs2, `CARGO_INCREMENTAL=0` and the absolute main target, exclusively created logs/receipt, checked input drift after every command and gated each runtime on its matching successful nonzero listing.

Every Plan 18 command exited 0: `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit`, `pnpm format:check`, Cargo check, full `cargo fmt --check`, Clippy `-D warnings`, both listing/runtime pairs and `git diff --check`.

- Library listing: 6562 tests; runtime: 6547 passed, 15 ignored, 0 failed.
- Full Cargo listing: 6725 tests; every runtime summary reported 0 failed. Default ignored-test behavior was not overridden.
- `unchanged_during_check=true`; no runtime executed without its own successful nonzero listing and unchanged inputs.

The receipt is `native-direct-shared-s6-full-gate-01.json`, SHA-256 `76361da7bf674fb06c8085db319653f1cf1cbde5fdeb7a46018ebf6030404294`. This is a current main-worktree Full Gate, not lane, installer, live, provider or release acceptance.

### Independent static review

Nova, agent `75ff307c64f441e29c127770131c2937`, returned literal body verdict PASS with `mandatory=[]` and completed normally. The review covered strict EOF/proof binding, Gemini malformed-candidates and STOP behavior, signed immutability and append-only Content composition, native decoded string leaves, charged owners, sole finalizer timing, the three S5 tests and document scope. The complete findings and residual limitations are preserved in `native-direct-shared-s6-nova-review-pass-01.json`, SHA-256 `ea5cdbf0f24293533921c919854d13c28fb2929b783e89a73a137f3443e4956c`.

Nova used only read/search/find static inspection. It did not execute commands, modify files, use LSP, calculate hashes or delegate. Parent runtime and hashes are not represented as reviewer execution. The PASS does not cover provider compatibility, real network tee/backpressure, lane delivery, integration or M2–M5.

### Versioned freeze

Immediately before freezing, the parent rechecked every reviewed source/test/document identity and found zero drift. The freeze builder also asserted the Nova normal PASS, current Full Gate success, all nine selection manifests/logs and equal main/Chat/Anthropic/Gemini HEAD `ca7f883c742df07abe9bcaa9b0eade02aae5e119`.

- Contract name: `native-direct-shared-conservative-20261109-frozen-01`.
- Canonical source contract identity: `c5bc94249a2252dc0b77b64543e98dc9f92f086dfdd7d4dac45e37e6f2e1ed27`.
- Receipt: `.voidx/verification/core1-20261106/native-direct-shared-s6-freeze-01.json`.
- Receipt SHA-256: `ae0d005936195b28126185075b374c153361e1d358dbd9cdfa611a0f84159065`.
- Frozen delivery set: exactly ten shared source/test files, including `native_driver.rs`; no documents or lane-owned files.
- Status: `frozen_for_S7_exact_three_lane_delivery_only`.

The canonical identity binds the reviewed pre-suffix Plan/Evidence checkpoint, source files, unchanged authority, runner identities, Nova PASS and Full Gate. This Section 65 and Plan Section 21 are post-freeze evidence suffixes and are deliberately excluded from the canonical identity to avoid circularity. No commit, push, rebase, application launch/install, ignored/live/provider test or release action occurred.

## 66. S7 exact three-lane delivery and runtime evidence

### Frozen delivery and preserved lane state

The S7 source contract remained `native-direct-shared-conservative-20261109-frozen-01`, identity `c5bc94249a2252dc0b77b64543e98dc9f92f086dfdd7d4dac45e37e6f2e1ed27`. The delivery set contained exactly the ten frozen shared source/test files and no documents or lane-owned paths.

- Pre-delivery manifest: `.voidx/verification/core1-20261106/native-direct-shared-s7-pre-delivery-01/manifest.json`, SHA-256 `94480a7039ac6ebe684f9428e38c5bb86c3bf6e398e193d51de871977ebf0922`.
- Delivery receipt: `.voidx/verification/core1-20261106/native-direct-shared-s7-delivery-01.json`, SHA-256 `e561c6eb4931ea2cdfe187aa3871067f4145b5a755f803614503c09a63133bb2`.
- Chat: eight shared files changed, two were already byte-identical; CH1/CH2/CH3 counts remained 18/23/10.
- Anthropic: all ten shared files matched the freeze after delivery; AN1 retained 14 tests.
- Gemini: all ten shared files matched the freeze after delivery; GE1 retained 32 tests.
- Every snapshotted lane-owned file retained its exact bytes and every ordered test-name list matched the pre-delivery snapshot.

### Twelve focused lane selections

Each row used the approved lane runner, its exclusive lane target, jobs2 and the global serial Cargo lock. Listing succeeded with a nonzero count before serial runtime execution. Every source manifest has equal pre/post maps and `unchanged_during_check=true`; every runtime summary reports zero failed tests.

| Lane | Exact filter | Listed | Runtime result |
| ---- | ------------ | ------ | -------------- |
| Chat | `proxy::security::protocol_adapters::chat::tests::` | 45 | 45 passed, 0 failed |
| Chat | `proxy::security::` | 2104 | 2104 passed, 0 failed |
| Chat | `proxy::session::tests::` | 34 | 34 passed, 0 failed |
| Chat | `proxy::protocol_matrix_tests::` | 3 | 2 passed, 1 ignored |
| Anthropic | `proxy::security::protocol_adapters::anthropic::tests::` | 15 | 15 passed, 0 failed |
| Anthropic | `proxy::security::` | 2074 | 2074 passed, 0 failed |
| Anthropic | `proxy::session::tests::` | 34 | 34 passed, 0 failed |
| Anthropic | `proxy::protocol_matrix_tests::` | 3 | 2 passed, 1 ignored |
| Gemini | `proxy::security::protocol_adapters::gemini::tests::` | 33 | 33 passed, 0 failed |
| Gemini | `proxy::security::` | 2092 | 2092 passed, 0 failed |
| Gemini | `proxy::session::tests::` | 34 | 34 passed, 0 failed |
| Gemini | `proxy::protocol_matrix_tests::` | 3 | 2 passed, 1 ignored |

The ignored matrix tests were not executed. No native HTTP integration filter existed or ran in S7.

### Final identity and aggregate closure

After all twelve runs, the parent recomputed the ten shared identities in the main worktree and every lane. All four maps equal the freeze. The parent also rechecked every lane-owned hash, ordered test names and prefix counts, and ran `git diff --check` in each lane with exit 0 and empty output. No source or test input drifted during any lane run.

`native-direct-shared-s7-stage-evidence-builder-01.py` stopped at its first lane record with `KeyError: 'filter'`. Lane manifests store the filter in `commands[*].command`, not in a top-level field. The failed builder, its preserved error receipt and successful builder-02 are all indexed. Builder-02 additionally asserted exact list/test commands, listed counts, runtime summaries, pre/post maps and current source identities before exclusively creating the aggregate. The builder error executed no test and is not a production RED.

The final aggregate is `.voidx/verification/core1-20261106/native-direct-shared-s7-stage-evidence-01.json`, SHA-256 `d36a67c4ad60e33e9e5b37e13cda02e07c52f624a7e9521d8aaa055cb0a0185e`. It records twelve passing runs and indexes 88 artifacts, excluding its own bytes. All commands and hashes are parent-generated; no review-agent execution is claimed.

S7 is exact shared delivery and lane verification only. It is not an AN2/AN3 or GE2/GE3 handoff, native HTTP integration, current main Full Gate, route enablement, direct-native cell, provider/live acceptance or release. No commit, push, rebase, application launch/install, real credential, ignored/live/provider test or `127.0.0.1:15722` binding occurred.

## 67. M2 Chat integration evidence and retained failures

The current stage boundary is Chat-only; historical CORE-1, Responses, P3/P4, lane handoffs, pending entries and failed attempts remain unchanged. All filenames below are relative to `.voidx/verification/core1-20261106/`. The final stage record is `native-direct-m2-acceptance-01.json`, issued only after this appended suffix passes prefix, formatting and accuracy checks.

### Current behavior and test provenance

`http_restoration/native_chat.rs`, `http_restoration.rs`, `forwarder.rs`, `handlers.rs` and `response_processor.rs` now connect trusted direct Chat admission to actual-attempt preparation, original transport capture and the single final client exit. Registration alone grants no authority. `protocol_adapters/chat/` retains its delivered contract. JSON proof/encoding precedes activation and body return; SSE last yield is not activation. Only real upstream `None` creates original-ready evidence. Gzip/layered decoding follows real EOF under existing entity and parser peak budgets.

Cumulative assistant replay is matched as complete, ordered delivered batches using both frozen JSON and composed SSE shapes. Strictly increasing existing publication serials reject reordering/repetition, and imports are staged on a fork and committed atomically only after all batches match. The four-round transitive test is not described as an appended-transcript test; the later appended-transcript tests provide that separate coverage. First tool results do not manufacture delivered-history authority.

Genuine native SSE errors use the existing budgeted safe sanitizer, preserve safe native type/code/message, release one atomic error envelope, and never restore error references, release privileged tools, emit `[DONE]` or publish success. A choice-level `finish_reason` is not a whole-stream terminal; errors after `[DONE]` remain rejected.

| Current selection | Listed | Runtime result |
| --- | ---: | --- |
| `proxy::security::native_chat_http_tests::` | 48 | 48 passed |
| `proxy::security::protocol_adapters::chat::` | 45 | 45 passed |
| `proxy::security::native_route_scope_tests::` | 22 | 22 passed |
| `proxy::security::` | 2,153 | 2,153 passed |
| `proxy::session::tests::` | 34 | 34 passed |
| `proxy::protocol_matrix_tests::` | 3 | 2 passed, 1 ignored |

These are the parent-run `native-direct-m2-sol-green-01-54d4b97d05-{http,chat,route,security,session,matrix}-{source.json,list.log,test.log}` records. Compilation and listing succeeded before execution; every source receipt reports unchanged inputs and was compared with the current tree. Two Cargo formatting passes were identical. HTTP fixtures use synthetic data, in-memory storage, dynamic loopback ports, deadlines and test-only proxy isolation.

### Independent review and minimal feedback closure

Sol `f4aae0d1a5194e5e9153d667b37f3443` completed normally, original `verdict=NEEDS_CHANGE`, with literal `terminal` in the raw report. Only read/search/find were permitted; parent tests are not reviewer execution. The report covers cumulative M2 and found one P2: any choice finish prematurely prevented delivery of a later genuine upstream error while another choice was active.

`native-direct-m2-sol-red-01-9d3f32f352-behavior-*` compiled successfully, listed 1 test, and failed that runtime test at `m2_multichoice_native_error_was_transport_failure`; inputs were unchanged. The minimal fix removes only the per-choice terminal assignment, retaining `[DONE]` rejection. `m2_chat_sol_multichoice_upstream_error_preserves_semantics_without_completion` is now included in the green HTTP48/security2153 runs. It checks safe upstream semantics, no tool/plaintext/reference leakage, no successful terminal/publication and complete budget refund. `native-direct-m2-sol-feedback-closure-01.json` binds the original report and all current regressions. The approved small-feedback rule closes the finding by parent verification without another independent review; the original NEEDS_CHANGE is not rewritten as reviewer PASS.

Earlier Nexus history/error feedback remains separately reproducible: `native-direct-m2-nexus-red-01-58df7b1a30` listed 5, passed 3 and failed 2. Green01 failed compilation due to a test constructor field; Green02 passed the new cases but exposed two capacity-injection regressions from swallowed sanitizer setup errors. Green03 passed 45/45 after explicit propagation. These attempts, prior Flux/Juno feedback, retry proxy diagnosis and original failure logs remain retained, not overwritten.

### Actual Full Gate and invocation diagnosis

`native-direct-m2-actual-full-gate-04-003f63fffd.jsonl` ended with `all_passed=true`, `runner_exit=0`, `unchanged_inputs=true`; `native-direct-m2-invocation-05-003f63fffd-parent.json` records real exit 0. All twelve commands in the approved Full Gate completed: typecheck, renderer build, renderer unit tests, format check, Cargo check, Cargo fmt check, Clippy with denied warnings, library listing/runtime, all-test listing/runtime and diff check.

- Renderer: 254 files / 3,064 tests passed.
- Library: 6,655 listed; 6,640 passed / 15 ignored.
- Full Cargo run: 6,818 binary-qualified names listed; 6,803 executed / 15 ignored. Ordered binary/name and numeric runtime totals match the listings.
- Shared nonblocking exclusive `fcntl.flock`, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0` and absolute main-tree target were enforced. No concurrent Cargo ran; the source was frozen through the terminal receipt.

The environment uses independent HOME, a short isolated workspace TMPDIR, copied cached pnpm 10.12.3 and a synthetic Codex catalog stub that rejects other arguments. The runner itself remains the reviewed fix2 revision. No production credential or real Codex CLI is required by this invocation. This is not proof of exhaustive network/filesystem isolation: MSW warnings are not a hard network barrier, and macOS system proxy behavior is distinct from environment variables.

Retained actual attempts are not promoted retroactively:

- Gate01 `47621c6946`: no terminal receipt after the metadata pnpm version event; original logs remain. A later system restart and recursive global/project pnpm version-management staging were diagnosed. No exit or PASS is invented.
- Gate02 `562f22c6fd`: real parent exit 1, Clippy exit 101 for inline `OriginalReady` enum size. Only the existing justified local lint configuration pattern was applied: the entire `CaptureState` is already charged, while boxing adds uncharged allocation. This is not behavioral RED. Snapshot: `native-direct-m2-clippy-inline-01-12c52e2e47-*`.
- Gate03 `444766a85a`: Clippy passed; library runtime was 6,636 passed / 3 failed / 15 ignored, parent exit 1. The failures were Unix socket fixture path length under the long isolated TMPDIR. Same newly compiled binary, only TMPDIR changed: long 11/14, short 14/14 in `native-direct-m2-sol-red-01-9d3f32f352-socket-{long,short}.log`. The executable path came from Gate03 listing but was rebuilt before this diagnostic; its hash is not claimed to be the original Gate03 binary. See `native-direct-m2-socket-tmpdir-diagnostic-01-3f664fc28c.json`. No production fix was made for this environment issue.

Vega step-limit FAIL, Prism limited PASS lacking literal terminal, and Nexus time-limit NEEDS_CHANGE never granted the cumulative gate. Retrieval now returns Unknown run for them and historical Helix; `native-direct-m2-review-retrieval-02-d61ae70846.json` records the limitation. Their unavailable original bodies are not reconstructed. Prior valid terminal outcomes are not overwritten by later retrieval anomalies.

### Limits and ordered handoff

This stage enables only direct-native Chat/openai_chat JSON/SSE. Anthropic/Gemini owned candidates and all original lanes stay untouched until their respective M3/M4 stage. No new gateway/agent/intent/global, cross-protocol or Gemini non-generation/framing permission is implied. The authoritative architecture and frozen adapter-facing contract are not revised.

The one-MiB entity limit does not promise every one-MiB JSON fits the frozen retained-plus-scratch parser peak. Capacity sweeps prove covered failure atomicity/refunds, not physical OOM, allocator workspaces or exact RSS. Truncated trailer tests do not establish exhaustive CRC/ISIZE coverage. No live/provider, installer, signing, notarization or release acceptance is claimed. M3 must PASS before M4; M4 must PASS before M5 final shipped-documentation and development acceptance.

### Append-only document verification

The historical prefix is retained byte-for-byte from the M2 start snapshots. A whole-document Prettier attempt changed historical formatting and failed the prefix assertion; its output and diagnostic are preserved in `native-direct-m2-documents-prefix-diagnostic-01.json` and the referenced failure snapshots. Only this new English suffix is formatted twice and checked for idempotence. The repository `pnpm format:check` checks `src/`, not these complete Markdown files; no whole-document Prettier PASS is claimed.

Gate04 predates this documentation-only suffix. The final acceptance compares its input inventory with the current tree, permits only these two appended documents, and reuses unchanged production/test evidence rather than claiming the Gate tested a later document snapshot. Current prefix, suffix, repository format, diff, source identity and preservation checks are recorded in `native-direct-m2-documents-closure-01.json`.

## M3 Anthropic delivery and verification

### Delivered behavior

Native Anthropic `/v1/messages` requests now pass a guarded DLP path: request admission, bounded inspection, history observation, tokenized forwarding, response restoration, and final outbound-history binding. Claude Desktop keeps the strict resolver: exact model/route matching, alias and legacy lookup order, role handling and fallback order are unchanged, unknown routes fail before transport, and the resolver copies the request model only after a shared-budget reservation. Native resolver failures use the fixed client message instead of echoing the request model.

The registry matrix expectations for the same-protocol Anthropic cell were corrected in two M2-only tests; production registration was not changed. All Responses, Gemini, cross-protocol and compact-SSE negative assertions are retained.

### Immutable vendor-normalization boundary

An independent cumulative review found a P1 gap: the generic vendor normalizer ran before native history import and could remove `thinking.signature`, insert placeholder thinking, or replace `redacted_thinking` for Claude MiMo, Claude DeepSeek and Desktop DeepSeek histories. The fix refuses such requests before the mapped body is touched, but only when the DLP request is native Anthropic and the same vendor predicate that would trigger normalization matches. Unsigned normalization, legacy/disabled paths, resolver matching order and all other mapping paths are unchanged. The boundary therefore applies only while DLP admission is active; without DLP admission the historical behavior of those vendor paths is untouched.

Real HTTP RED: `native-direct-m3-vendor-immutable-red-01-e4c04a9d74` listed six tests, runtime exit 101, all six failed with `m3_vendor_normalizer_rewrote_immutable_history`. A same-file RED on the final test bytes with only the guard removed (`native-direct-m3-vendor-same-file-red-01-597292e52d`) reproduced the same six failures at exit 101; restoring the guard byte-for-byte reproduced the Full Gate input hash. The guard GREEN (`native-direct-m3-vendor-immutable-green-02-c6e0e2192c`) ran six passed with parent exit 0 and unchanged inputs. Refusals reach no transport, write no capture record or publication row, leak no payload or signature text, return the fixed `proxy_processing_failed` client error, and fully refund the payload budget.

A direct predicate unit matrix (`native-direct-m3-vendor-predicate-green-01-84e6940ef2`, with `test_vendor_immutable_predicate_matrix` plus the six HTTP tests, seven passed) pins signed and redacted carriers as refused, unsigned and empty-text thinking as not refused, signed carriers without a tool-bearing turn as not refused, and non-native formats or non-vendor providers as outside the boundary.

Targeted regressions stayed green: `native-direct-m3-vendor-boundary-regression-01-b305ba5082` (51 passed, including unsigned Desktop MiMo normalization and resolver tests) and `native-direct-m3-vendor-provider-regression-01-678fb41374` (67 passed). Clippy on the intermediate tree passed (`native-direct-m3-clippy-check-01-2c0e5a37f8`, exit 0); the unused Anthropic re-export is now test-only instead of being handled by a lint suppression.

### Cumulative evidence

- `native-direct-m3-registry-integration-03-e5abd6b8a5`: pre-boundary cumulative Integration, all eight groups green.
- `native-direct-m3-resolver-integration-02-755a03d4ac`: preserved terminal failure; the two failing tests were stale M2-only expectations, fixed narrowly in the tests.
- `native-direct-m3-vendor-integration-04-3d49a11eda`: post-boundary Integration before the unit matrix, all eight groups green.
- `native-direct-m3-vendor-integration-05-eb7b886cc3`: final Integration, all eight groups green, format idempotent with no changed path.
- `native-direct-m3-actual-full-gate-01-b79f827191`: preserved Full Gate failure (Clippy `unused import: request::observe_request`).
- `native-direct-m3-vendor-actual-full-gate-02-30d5756594`: Full Gate PASS before the unit matrix, twelve commands exit 0.
- `native-direct-m3-vendor-actual-full-gate-04-eac8336f71`: final Full Gate PASS on the accepted map; twelve commands exit 0, `all_passed` and `unchanged_inputs` true.
- `native-direct-m3-preservation-current-04.json`: three original lanes each 1,536 files equal to their M2 start maps with unchanged HEADs and clean `git diff --check`; M2 acceptance, S6 receipt and authoritative architecture hash retained; the five main-tree Gemini adapters still equal the M2 main start; both documents still carry their M3 start prefixes before the suffixes; the current input map equals the final Full Gate baseline.

### Limits

M3 covers direct-native Anthropic JSON/SSE development integration only. It does not integrate Gemini, enable installed clients, use live providers or real credentials, install or replace the application, sign, notarize or release. M4 must wait for the M3 acceptance receipt. Historical artifacts other than those rechecked here are not revalidated as complete maps. Only these appended English suffixes are formatted; no whole-document Prettier PASS is claimed for the historical Markdown prefix.

## M4 Gemini delivery and verification

### Delivered behavior

Direct native Gemini `generateContent` JSON and `streamGenerateContent?alt=sse` requests now pass the guarded DLP path: handler-owned admission, bounded request observation, tokenized forwarding, strict response completion verification, and final restoration. Text carriers cover `systemInstruction`/`system_instruction` Parts, `contents[*].parts[*].text`, and the string leaves of Struct-valued `functionCall.args` / `functionResponse.response`. Media (`inlineData`, `fileData`), signatures, call names/IDs and unknown members stay opaque. Any Part carrying `thoughtSignature` or `thought_signature` is immutable, so a sensitive value inside it refuses the request instead of being rewritten.

Route and framing come from the URI and query only: `:generateContent` with absent/`alt=json`, and `:streamGenerateContent` with exactly one `alt=sse`. GET/model listing, `countTokens`, unknown aliases, duplicated or bare `alt` keys and every other method stay non-reversible. Query parameters are preserved to the upstream unchanged.

### Failure behavior

An admitted request that later fails verification never forwards partial or unsanitized content. JSON failures return the fixed Gemini application error. SSE failures return a single fixed `data: {"error":{...}}` frame with the same client message, including the earlier client/capture failure path, so the client never sees a silently truncated 200 stream. Strict terminal reasons are unchanged: only `STOP` completes; `MAX_TOKENS`, blocked prompts and unsupported carriers fail closed.

### Evidence

- Same-file RED/GREEN on identical bytes: `m4-gemini-same-file-red-01-9e1558b5db` (8 listed, 4 failed, exit 101 — JSON, SSE, query and function-response cases) and `m4-gemini-same-file-green-01-0f669758a5` (8 passed, exit 0, inputs unchanged). The removed admission block is preserved in `m4-gemini-same-file-red-01-removed-block.txt`.
- Earlier pre-integration RED: `m4-gemini-native-http-red-01-591dbc565a`.
- SSE fixed-frame RED/GREEN pair: `m4-gemini-sse-frame-red-01-fb401156ac` (exit 101, broken stream) and `m4-gemini-sse-frame-green-01-30fdbfe43c` (exit 0).
- `native-direct-m4-integration-01-24b187accc`: first cumulative integration; its format step reformatted five files, so RED/GREEN above ran on the pre-format bytes and every later gate ran on the formatted bytes.
- `native-direct-m4-integration-02-d766637c01`: final integration, ten groups all green, format idempotent with no changed path.
- `native-direct-m4-actual-full-gate-01-92de8c5dd8`: Full Gate PASS before the review fixes.
- `native-direct-m4-actual-full-gate-02-f2a8d63207`: final Full Gate PASS; twelve commands exit 0, `all_passed` and `unchanged_inputs` true; library runtime 6,816 passed / 15 ignored.
- `native-direct-m4-preservation-current-02.json`: three original lanes each 1,536 files equal to their M2 start maps with unchanged HEADs and clean `git diff --check`; M2 and M3 acceptance receipts, the S6 freeze and the authoritative architecture hash retained; lane-owned Gemini bytes unchanged, while `mod.rs`, `request.rs` and `history.rs` intentionally diverge in the main tree by adding the admitted-route entry points and module visibility.
- `native-direct-m4-lumen-findings-closure-01.json`: review findings, decisions and their RED/GREEN evidence.

### Limits

M4 covers direct native Gemini JSON/`alt=sse` development integration only. It does not support Gemini tool-call streaming across frames, media restoration, error-frame sanitation for arbitrary upstream shapes, installed clients, live providers or real credentials, and it performs no install, signing, notarization or release acceptance. M5 must wait for the M4 acceptance receipt. Historical artifacts other than those rechecked here are not revalidated as complete maps. Only these appended English suffixes are formatted; no whole-document Prettier PASS is claimed for the historical Markdown prefix.

### M4 final gate correction

After the M4 suffixes above were appended, the tests for the review findings gained two additions: the fixed Gemini SSE error frame is now compared byte-for-byte, and a Gemini upstream error envelope case was added. The test-only rename of the second route-scope test was also completed. Because those test bytes changed, the final M4 evidence is `native-direct-m4-actual-full-gate-04-989751878c` (twelve commands exit 0, `all_passed` and `unchanged_inputs` true) with `native-direct-m4-integration-03-72cab97748` (ten groups green, format idempotent, no changed path). Gate `-02` remains valid for the production code but predates these test additions.

One gate attempt, `native-direct-m4-actual-full-gate-03-d14572c18b`, failed `all-test` on `reversible_closure_resp4_reasoning_part_added` (a Responses-path test outside M4: the continuation send count was zero). The same test passed in isolation and in two subsequent full library runs, and the failure was not reproducible; it is retained as a real failure with the order-sensitivity noted, not as a pass.

The narrow re-review that verified the findings is preserved unchanged at `native-direct-m4-gemini-athena-recheck-6e8ac91fa0b94e97a66168363254fdce.txt` with verdict `NEEDS_CHANGE`; only its naming item was still open at review time and is fixed afterwards. The closure record for both reviews is `native-direct-m4-lumen-findings-closure-02.json`.

## M5 acceptance matrix

Every row below maps an architecture Section 9 case to named tests that ran in one frozen integration (`native-direct-m4-integration-03-72cab97748`, all ten groups green). Each listed test name was present in that run and reported `ok`; the per-test outcome map is `native-direct-m5-matrix-outcomes-01.json`.

| Section 9 case | Chat | Anthropic | Gemini |
| --- | --- | --- | --- |
| Signed/thinking unit plus a tool call, then the first result | `ch1_first_tool_results_need_no_delivered_result_snapshot_but_receive_current_dlp` | `an3_lane_exact_ordered_block_call_and_signed_two_turn_replay`, `m3_anthropic_json_exact_history_imports_unrepeated_dependency`, `m3_anthropic_sse_exact_history_imports_unrepeated_dependency` | `ge3_lane_exact_ordered_contents_and_signed_two_turn_replay` |
| Late signature association / signed Parts stay intact | `ch3_lane_eviction_removes_history_privilege_without_authorizing_unknown_refs` | `m3_anthropic_signed_sse_never_releases_partial_privileged_output` | `ge2_lane_sse_signed_text_and_argument_units_are_immutable`, `ge2_lane_json_candidate_part_leaf_isolation_and_signed_units` |
| Mandatory redaction inside an immutable readable site | `ch1_first_tool_results_need_no_delivered_result_snapshot_but_receive_current_dlp` (current-input DLP) | `m3_anthropic_immutable_sensitive_request_rejected_before_send`, `m3_anthropic_vendor_immutable_claude_mimo_signed`, `m3_anthropic_vendor_immutable_desktop_deepseek_redacted` | `m4_gemini_signed_part_is_never_rewritten`, `ge3_lane_authorized_reference_in_signed_text_or_args_fails_atomically` |
| Signed replay without stable identity or after eviction | `m2_chat_missing_identity_restores_current_request_without_history_privilege` | `m3_anthropic_missing_identity_restores_current_without_history_privilege` | `ge3_lane_eviction_removes_history_privilege_not_current_dlp` |
| First tool result / function response bodies | `m2_chat_tool_result_alone_and_duplicate_identity_cannot_import_hidden_history` | `m3_history_first_and_edited_tool_results_never_grant_hidden_dependency` | `m4_gemini_current_function_response_text_is_tokenized`, `ge3_lane_first_and_edited_function_response_remain_current_dlp` |
| Replayed, edited or ambiguous pairing; wrong session/provider | `m2_chat_edited_reordered_or_truncated_native_replay_cannot_import_dependencies`, `m2_chat_hidden_history_isolated_by_session_provider_target_and_principal` | `m3_anthropic_hidden_history_isolated_by_actual_session_provider_target_and_principal` | `ge3_lane_inexact_delivered_contents_never_import_dependencies`, `ge3_lane_absent_cross_session_provider_protocol_store_retry_isolation` |

Shared-contract obligations map to the existing suites: byte/delta splits and final snapshots to `request_literal_tests` and `reversible_completion_tests`; lease/namespace/generation/attempt isolation and sealed publication to `reversible_integration_tests` and the lane replay tests above; zero publication on interruption to `m2_chat_sse_drop_before_upstream_eof_discards_pending_history_and_budget`, `m3_anthropic_sse_drop_after_last_yield_discards_history_and_budget` and `ge2_lane_sse_drop_at_every_yield_including_last_never_publishes`; generic client errors and fixed server-side frames to `m4_gemini_sse_incomplete_terminal_returns_fixed_error_frame`, `m4_gemini_inline_media_response_is_refused`, `m4_gemini_blocked_prompt_is_refused_without_partial_output` and `m4_gemini_upstream_error_propagates_without_sensitive_plaintext`.

Closed Gemini cells and framing are covered by `m4_gemini_unsupported_framing_with_restore_rule_is_refused`, `m4_gemini_unsupported_framing_without_match_forwards_untouched`, `m4_gemini_model_listing_is_not_admitted`, `m4_gemini_query_is_preserved_and_framing_comes_from_uri`, `c4_dispatch_gemini_registered_generation_is_admitted_natively`, `m4_registry_describes_only_same_protocol_gemini_generate_content_json_and_sse` and `c5_native_evidence_fixture_does_not_expand_integrated_registration`.

Limits: this matrix proves the named local contracts on one frozen input. It does not prove provider compatibility, live-service behavior, installer, signing, notarization or release readiness, and it does not extend restoration to unlisted Gemini framings or media.

Matrix errata: the Chat SSE drop test cited above is named `m2_chat_sse_body_drop_before_upstream_eof_discards_pending_history_and_budget`; the truncated spelling in the previous paragraph is not a test name. No other row changes.

## M5 acceptance matrix (corrected)

This section supersedes the earlier matrix paragraph. It corrects the Chat column, cites the exact architecture cases in `docs/design/security-dlp-unified-core-protocol-adapters.md` Section 9 ("Verification and acceptance", per-protocol table), and names the preserved run for every row: `native-direct-m4-integration-03-72cab97748`. Chat has no signed reasoning unit, so the three signed-unit cases are explicitly not applicable there instead of borrowing Anthropic-shaped tests.

| Architecture case | Chat | Anthropic | Gemini |
| --- | --- | --- | --- |
| Signed reasoning unit plus a tool call, then the first result | Not applicable: Chat has no signed reasoning unit. Its first-result behavior is covered by the row below. | `an3_lane_exact_ordered_block_call_and_signed_two_turn_replay`, `m3_anthropic_json_exact_history_imports_unrepeated_dependency`, `m3_anthropic_sse_exact_history_imports_unrepeated_dependency` | `ge3_lane_exact_ordered_contents_and_signed_two_turn_replay` |
| Late signature association; signed Parts stay intact | Not applicable: no signed reasoning unit exists in Chat. | `m3_anthropic_signed_sse_never_releases_partial_privileged_output` | `ge2_lane_sse_signed_text_and_argument_units_are_immutable`, `ge2_lane_json_candidate_part_leaf_isolation_and_signed_units` |
| Mandatory redaction inside an immutable readable site | Not applicable: Chat has no immutable readable site. Current-input redaction is covered by the first-result row. | `m3_anthropic_immutable_sensitive_request_rejected_before_send`, `m3_anthropic_vendor_immutable_claude_mimo_signed`, `m3_anthropic_vendor_immutable_desktop_deepseek_redacted` | `m4_gemini_signed_part_is_never_rewritten`, `ge3_lane_authorized_reference_in_signed_text_or_args_fails_atomically` |
| Signed replay without stable identity, or after eviction | `ch3_lane_eviction_removes_history_privilege_without_authorizing_unknown_refs`, `m2_chat_missing_identity_restores_current_request_without_history_privilege` | `m3_anthropic_missing_identity_restores_current_without_history_privilege` | `ge3_lane_eviction_removes_history_privilege_not_current_dlp` |
| First tool result or function-response body | `ch1_first_tool_results_need_no_delivered_result_snapshot_but_receive_current_dlp`, `m2_chat_tool_result_alone_and_duplicate_identity_cannot_import_hidden_history` | `m3_history_first_and_edited_tool_results_never_grant_hidden_dependency` | `m4_gemini_current_function_response_text_is_tokenized`, `ge3_lane_first_and_edited_function_response_remain_current_dlp` |
| Replayed, edited or ambiguous pairing; wrong session or provider | `m2_chat_edited_reordered_or_truncated_native_replay_cannot_import_dependencies`, `m2_chat_hidden_history_isolated_by_session_provider_target_and_principal` | `m3_anthropic_hidden_history_isolated_by_actual_session_provider_target_and_principal` | `ge3_lane_inexact_delivered_contents_never_import_dependencies`, `ge3_lane_absent_cross_session_provider_protocol_store_retry_isolation` |

Shared-contract obligations map to the existing suites: byte and delta splits to `request_literal_tests` and `reversible_completion_tests`; lease, namespace, generation and attempt isolation to `reversible_integration_tests` plus the replay rows above; zero publication on interruption to `m2_chat_sse_body_drop_before_upstream_eof_discards_pending_history_and_budget`, `m3_anthropic_sse_drop_after_last_yield_discards_history_and_budget` and `ge2_lane_sse_drop_at_every_yield_including_last_never_publishes`; generic client errors and fixed server-side projections to `m4_gemini_sse_incomplete_terminal_returns_fixed_error_frame`, `m4_gemini_inline_media_response_is_refused`, `m4_gemini_blocked_prompt_is_refused_without_partial_output` and `m4_gemini_upstream_error_propagates_without_sensitive_plaintext`.

Enabled-cell and framing coverage: `m4_gemini_unsupported_framing_with_restore_rule_is_refused`, `m4_gemini_unsupported_framing_without_match_forwards_untouched`, `m4_gemini_model_listing_is_not_admitted`, `m4_gemini_query_is_preserved_and_framing_comes_from_uri`, `c4_dispatch_gemini_registered_generation_is_admitted_natively`, `m4_registry_describes_only_same_protocol_gemini_generate_content_json_and_sse` and `c5_native_evidence_fixture_does_not_expand_integrated_registration`. The preserved gateway Responses recovery that the earlier "only" wording contradicted is `c4_dispatch_gateway_responses_preserves_json_and_sse_restoration`.

Limits, stated explicitly:

- Gemini does not restore media Parts, tool-call streaming that spans frames, or upstream error frames beyond the single fixed local envelope. An upstream Gemini non-2xx response keeps its HTTP status and is returned as a JSON error, never as a successful SSE stream.
- Chat and Anthropic capture-failure paths still terminate the stream instead of emitting a fixed frame. Only the Gemini path gained that fixed frame in M4; the three protocols are not symmetrical here.
- The acceptance-state preservation record for the current map is `native-direct-m4-preservation-current-04.json` (the `-02` record predates the documentation suffixes).

Errata: the earlier matrix paragraph cited `m2_chat_sse_drop_before_upstream_eof_discards_pending_history_and_budget`, which does not exist; the real test is `m2_chat_sse_body_drop_before_upstream_eof_discards_pending_history_and_budget` and it is cited correctly above.

This matrix proves the named local contracts on one frozen input. It does not prove provider compatibility, live-service behavior, installer, signing, notarization or release readiness.

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
