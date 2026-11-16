# November 16 current 12-row tracker and tool-catalog follow-up

The existing Chrome tab was read again without reload or filter changes after pushing integration commit `87a1f95e305c79cc0bb320e3b38d39407e7d3594`. The rendered grid shows twelve records. Rows 1 and 3 have acceptance-failed tracker status; the other ten are pending. No tracker fields were edited. The final closing read shows the same twelve rows. The attachment preview opened during inspection was closed.

The previous 14-row view no longer contains configuration-scheme switching or the native-dialog language report. Their departure from this filtered view is not acceptance evidence. The user separately accepted the tested Codex routing/restoration scenario earlier in the chat.

| Row | Report | Source repair and acceptance status |
| --- | --- | --- |
| 1 | Claude CLI model list is incomplete | Reported acceptance failure unresolved; no new repair. The affected-device case remains unreproduced. |
| 2 | Account login cannot discover models or sign in except Grok | Original onboarding case unresolved. The previously discussed Antigravity account also failed in the official client. The user has been asked which other platform still fails and for its safe error text. |
| 3 | Request-graph supplier icon differs from configuration | Reported acceptance failure unresolved; previously deferred. Existing icon tests do not establish acceptance. |
| 4 | WorkBuddy old chat retains the previous supplier after restart | Main includes current-label catalog migration and historical-alias/continuation compatibility. These do not establish that the original old-chat supplier-selection case is fixed; partial repair, native retest required. |
| 5 | Agent restart requires two confirmations | Confirmed restart no longer depends on connection configuration or freshness for supported desktop Agents. Local lifecycle/renderer tests pass; original repeated error/no-restart scenario requires native retest. |
| 6 | Codex has no restart prompt after supplier change | Freshness recording and main's catalog/restart presentation repairs are included; native retest required. |
| 7 | Confirmation of restart makes the window keep jumping | Local lifecycle, duplicate-click and pending-until-result repairs included; native retest required. |
| 8 | Scheme preview omits supplier names | Earlier supplier-qualified preview repair included; native retest required. |
| 9 | Cannot change upstream format | Earlier same-endpoint/auth format editing repair included; live retest required. |
| 10 | Equivalent commands prompt inconsistently across Agents | Partial attribution repair plus the new large-tool-catalog fix below. Synthetic Codex/Pi event parity passes; original installed-client warning parity is not accepted. |
| 11 | Codex permission-changing command has no prompt or event | New reproducible large-tool-catalog omission repaired. The original recording's exact request envelope is still unknown; original native acceptance remains pending. |
| 12 | Custom-rule security events omit session ID | Earlier propagation repair applies to newly generated valid client sessions. New large-catalog tests also retain the session ID. Historical or missing IDs cannot be reconstructed. |

## Reproduced omission and repair

The observer previously returned an empty built-in identity map whenever a request contained more than 64 tools, even if its supported command tool had a unique valid schema. A synthetic 331-tool request containing a completed permission-changing call produced no event. The new regression failed before the production change at the expected missing-event assertion.

Catalog scanning now has a separate 1,024-entry work bound. Gemini declarations share that flattened limit. Unknown extensions do not gain built-in authority; duplicate names and trimmed aliases, namespaces, invalid argument schemas and existing per-schema byte/depth bounds remain enforced. Descriptions and whole schemas are not copied. Overflow still refuses attribution without interrupting forwarding.

The end-to-end regression uses an in-memory observation channel, synthetic request definitions, immutable JSON/SSE response bytes and the real parser, rule engine and sanitized event projection. It verifies risky permission-change events and session IDs for Codex and Pi. The command is fixture text and is never executed. A separate boundary regression covers exact scan capacity, overflow, late duplicate aliases and oversized Gemini declarations. Existing ambiguity/namespace/schema-size tests remain intact.

## Verification

Checked base: `87a1f95e305c79cc0bb320e3b38d39407e7d3594` plus the tool observer, architecture document and this report. Host: macOS arm64, Rust/Cargo 1.98.1, Node 20.20.2, pnpm 10.12.3. `cargo check`, `cargo clippy -- -D warnings`, `cargo fmt --check` and complete `cargo test`, all using `--manifest-path src-tauri/Cargo.toml`, passed. Full Rust suite: 7,812 passed (7,647 library and 165 integration), 21 ignored, zero failed. `git diff --check` passed.

The renderer, helper sources, dependency manifests and lockfiles are unchanged from the fully checked integration tree. Its 4,257 passing renderer tests, typecheck, renderer build, formatting, helper/recovery checks and clean production dependency audit remain applicable. Logs: `/tmp/varyai-tool-catalog-before.log`, `/tmp/varyai-tool-catalog-final-tests.log`, `/tmp/varyai-tool-catalog-static.log`; integration and helper logs are recorded in the preceding precommit report. Native application behavior and Windows/Linux remote acceptance remain unverified. No installed client was restarted or replaced.

The GitHub connector reported no workflow runs and no status records for the integration SHA. The previous PR was already merged, and the workflow does not automatically run on ordinary feature-branch pushes without an open PR. No remote CI pass is claimed. The next pushed revision must be queried separately.
