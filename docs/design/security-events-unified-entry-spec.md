# Unified security events: implementation contract

> Status: implemented in the current worktree, not shipped or accepted by a live desktop end-to-end test. This document describes the existing implementation contract and its acceptance boundaries; it is not evidence that checks have run in this worktree.
> Audience: product, frontend/backend developers, testers, and coding agents.

## 1. Decision and scope

Security Center keeps its three top-level destinations: **Security Events**, **Detection Rules**, and **Tool Call Risk**. The default Security Events detail view combines existing DLP audit records with persisted tool observations whose verdict is `risky`. A secondary **All Tool Calls** view exposes _all saved_ tool records, including `risky`, `unchecked`, `uncertain`, `low_risk`, and legacy missing verdicts. It does not claim to capture every real tool invocation. Tool Call Risk remains the home of switches, rules, sandbox, black/white lists, and policy history, not a second log table or a redundant navigation link.

The implementation unifies **queries and presentation, not detection or storage**: read both existing extension-database tables, project, filter, count, sort, and page on the backend; render source-specific details on the frontend. Do not merge tables, double-write DLP rows, interpret tool warnings as execution/authorization/blocking, or alter forwarding, detection, workers, policy versions, or notification semantics. No cloud search, telemetry, export, deduplication across sources, or new generic event platform is in scope.

Acceptance questions: Does a newly persisted `risky` tool row appear in both warning count and details? Do home, activity summary, and Security Center agree on the record total for the **same time, application, and query snapshot**? Can users still inspect all saved tool verdicts, create a rule, and draft list entries? Does the UI distinguish an observed risk from an executed or blocked operation? Different page queries are not promised an atomic cross-page snapshot.

## 2. Storage, projection, and counts

Implementation references: [Rust DAO](../../src-tauri/src/database/dao/security_events.rs), [DAO regression tests](../../src-tauri/src/database/dao/security_events_tests.rs), [tool read projection](../../src-tauri/src/database/dao/tool_risk.rs), [TypeScript contract](../../src/types/securityEvents.ts), and [storage architecture](../architecture/storage.md). Both event tables live in the extension database (`vary-ai.db`); the historic `cc-switch` names remain compatibility names. No event migration, new table, column, index, backfill, or dual write is required. Shared v20, extension v1, and historic single-file v24 are separate version chains; inspect the schema before making future migration claims.

An event is **one database row**, not an execution, raw finding, suppressed observation, or current-page group. Default-view summary:

| Field                        | Counted rows                                                                         |
| ---------------------------- | ------------------------------------------------------------------------------------ |
| `total`                      | All DLP rows, including `allow`/unknown actions, plus tool rows with verdict `risky` |
| `dlpCount` / `toolRiskCount` | The two source contributions to `total`                                              |
| `redacted`                   | DLP `action_taken = redact` only                                                     |
| `warnings`                   | DLP `action_taken = audit` plus tool `verdict = risky`                               |
| `protectiveBlocked`          | DLP `action_taken = block` only                                                      |
| `other`                      | DLP `allow`/unknown action, not recast as warning, redaction, or block               |

Require `total = dlpCount + toolRiskCount = redacted + warnings + protectiveBlocked + other`. Do not derive total by adding the three visible action cards. `unchecked`/`uncertain` describe coverage or judgment, not a risk hit; `low_risk` is not a safety guarantee. Keep DLP's existing aggregation: a row may represent `triggerCount` hits across sessions; `sessionId` identifies only the first source, `timestamp` the first hit, and `lastSeenAt` the last hit. A tool row represents one saved observation: project `triggerCount = 1`, `timestamp = lastSeenAt` without rewriting storage or asserting that a tool executed once. Dates and ordering use **row timestamp**; yesterday's DLP row updated today still belongs to yesterday.

Use `eventType: "dlp" | "tool_risk"` for product source, distinct from a tool record's collection `source`. Each mixed row has a composite `key` (`dlp:<id>` or `tool_risk:<id>`), `timestamp`, `lastSeenAt`, `triggerCount`, `appType`, `sessionId`, `riskLevel`, `result`, and a source-specific `record`. Never assume numeric IDs are unique across tables. DLP's new record projection retains unknown historical action and level strings without widening the legacy `AuditLogRecord`; the common result maps `redact/block/audit/allow` to `redacted/blocked/warning/allowed`, otherwise `unknown`. Only valid uppercase DLP levels project to the common `riskLevel`; unknown is `null`, not LOW. Tool `risky` projects to `warning`; `unchecked/uncertain/low_risk` project verbatim; missing/invalid verdict becomes `unknown`. Only risky tool `high/medium` project to HIGH/MEDIUM; other or invalid severities project to `null`. A HIGH filter matches DLP HIGH **and CRITICAL** and risky tool high; CRITICAL remains queryable separately and its original value visible in details. MEDIUM/LOW match exactly; null matches no specified level.

The application is the stored DLP `app_type` or tool `client_category`, never inferred from the active app, provider, session, or neighboring events. In particular, tool `claude` does not alias `claude-desktop`; include supported clients such as `openai` in selection and preserve unfamiliar stored names for display. Do not filter historical events by the _current_ feature switch, deleted rules, or tool `policyVersion`; notification-only policy constraints remain independent.

## 3. Views and filter contract

The types are in [securityEvents.ts](../../src/types/securityEvents.ts). `SecurityEventFilter` contains required `view: "security_events" | "tool_calls"` and optional `eventType`, `startTimestamp`, `endTimestamp`, `appType`, `riskLevel`, `result`, `category`, `sessionId`, `keyword`, and `toolVerdict`. The default `security_events` view always constrains tools to `risky`; `tool_calls` constrains source to tools and defaults verdict to **all saved verdicts**, including null/unknown. All specified conditions combine with AND **before** global sorting/paging. Summary accepts the default view and the same filter semantics; the actual screen scopes below restrict how callers use it.

| Condition                 | Contract                                                                                                                                                                                            |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Time                      | Unix milliseconds, inclusive endpoints, filtered by `timestamp`, not `lastSeenAt`; nonnegative JS-safe values, start no later than end                                                              |
| Application/source/result | Exact stored app; `dlp`/`tool_risk` product source; projected result. `tool_calls` permits absent or `tool_risk` source, never DLP                                                                  |
| Level                     | Projected level as above, with HIGH including DLP CRITICAL                                                                                                                                          |
| Category                  | Prefix `dlp:<rule_category>` or `tool_risk:<risk_category>`; match exact category only within that source                                                                                           |
| Session                   | Trimmed exact DLP first-hit session ID; excludes tools if nonempty; never manufacture a tool session                                                                                                |
| Keyword                   | Trimmed literal substring of DLP rule name or _masked_ snippet only; escape SQL LIKE `%`, `_`, and escape character, bind parameters; excludes tools. No raw command or historical plaintext search |
| Tool verdict              | Only for `tool_calls`; omitted means all persisted verdicts. Reject it for `security_events`, never silently admit non-risky calls                                                                  |

Trim session/keyword; blank means omitted, each nonblank value is limited to 512 Unicode characters. Reject unknown enums, invalid view/verdict combinations, unsafe timestamps, page < 1, pageSize outside 1..100, and offset overflow with a fixed parameter error. Valid but contradictory conditions return zero rows, not a broadened result. The UI hides the DLP-only keyword/session controls for pure tool sources and All Tool Calls and tells users that keyword search covers only DLP; it clears incompatible source-specific filters and returns to page one on view/source/date/filter changes. Switching to All Tool Calls fixes tool source and defaults verdict to all; switching back restores all sources. Preserve common time/app/level filters. `resolveUsageRange` supplies today/7-day/30-day live ranges (open upper bound) and custom dates, whose final second includes 999 ms; do not show stale or failed query totals as zero.

## 4. IPC, consistency, and privacy

[IPC commands](../../src-tauri/src/commands/security_events.rs) are exported from `src-tauri/src/commands/mod.rs` and registered in `src-tauri/src/lib.rs`; [frontend API](../../src/lib/api/securityEvents.ts) is exported through `src/lib/api/index.ts`:

| Command / client method                                        | Input                        | Output                                          |
| -------------------------------------------------------------- | ---------------------------- | ----------------------------------------------- |
| `get_security_events` / `securityEventsApi.getEvents`          | `{ filter, page, pageSize }` | `{ items, total, page, pageSize }`              |
| `get_security_events_summary` / `securityEventsApi.getSummary` | `{ filter }` (default view)  | Summary fields in section 2                     |
| `clear_security_events` / `securityEventsApi.clearEvents`      | `{ filter }`                 | `{ deletedTotal, deletedDlp, deletedToolRisk }` |

Run every new command's database work via `tokio::task::spawn_blocking`. Return fixed safe parameter/database/task errors, not SQL, stored content, command summaries, session IDs, or raw IPC input. A failed count/read must **not** become an empty successful page or zero summary. Use only `extension_connection()` and a short transaction: count and page rows, and all summary parts, each share a query snapshot. Parameterize per-table filters, combine with `UNION ALL` (or an equivalent bounded query), and sort by `timestamp DESC, eventType ASC, id DESC`. Fetch the selected page's source/IDs and batch-hydrate source rows in the same snapshot; no per-row lookup or full-table hydration to calculate total. OFFSET requests do not share a persistent snapshot across pages: newly saved rows may move a page boundary. Do not hold a database mutex across await, notifications, network/file work, or request forwarding.

Hydrate tools through the existing `tool_risk.rs::read_event` privacy projection, including imported/historical summaries. Do not bypass its read-time handling or claim summaries are completely sanitized; masking recognizable private material is best-effort. Return only the existing masked DLP snippet, never reconstruct plaintext. Preserve protocol/forwarding behavior and the old DLP/tool read, count, clear, and notifier IPC signatures: new mixed types must not silently replace `AuditLogRecord` or legacy tool responses.

## 5. Page behavior and risk language

[SecurityCenterPage](../../src/components/security/SecurityCenterPage.tsx) owns top-level navigation, filters, page, range, and a one-shot pending rule seed. [SecurityEventsPanel](../../src/components/security/SecurityEventsPanel.tsx) owns mixed details, All Tool Calls, and current-filter clearing; [SecurityEventDetailsDialog](../../src/components/security/SecurityEventDetailsDialog.tsx) renders source-specific details. [ToolRiskEventsPanel](../../src/components/security/ToolRiskEventsPanel.tsx) retains its current-page similar grouping for uncertain/low-risk observations, but grouping does not change raw `total` or imply persisted deduplication; All Tool Calls uses the unified query rather than a second unscoped request. Do not add grouping to the default mixed view.

The Security Center's three cards and total summary count **all applications, all sources in the default event set, and the selected time range**, independently of list-level source/severity/keyword filters or secondary view. Label that scope. The list `total` obeys every active list filter; All Tool Calls' count is not a security-event total. Replace claims based on first-page sensitive-type samples with explicit DLP/tool record counts. [SecurityActivitySummary](../../src/components/security/SecurityActivitySummary.tsx) uses today/all apps/default event set. Home statistics and details use today/current app/default event set, with the same app identifier and a true total rather than three cards added together. Home details show safety metadata only: no command summary or sensitive snippet. Call the home count **security event records**, not blocks or event groups. Home protection status remains based on `useSecurityProtection`, not tool observations.

Mixed rows retain time, hit count, session, severity, result, application, and explicit source. The type is a DLP rule name or tool risk category. Missing tool session/provider/summary remains missing; do not fill it from `activeApp`. Show `risky` as a **risk warning**, not proof of blocked or executed work; tool outbound/execution status is **not applicable: tool call observation only**. Tool details explain: **Warning only; the tool may already have executed, and the final executed content is unknown.** Show available source/stage, integrity, verdict, category, severity, reason, rule IDs, and bounded backend summary. A missing historic summary cannot be reconstructed. Command summaries are best-effort redacted, not guaranteed anonymized. Other tool verdicts and missing/invalid verdicts use their actual labels, not a generic warning. DLP known values keep existing semantics; unknown DLP action and level display **unknown**, not LOW, allowed, or transmitted.

Keep tool log actions: black/white-list operations use `ShellCommandListDialog` with a user-confirmed draft derived only from the backend summary (disabled when missing), never automatic saving or execution. A rule seed contains only permitted enumerated fields, not raw arguments or summary; switch to top-level `toolRisk`, call `ToolRiskRulesPanel.openCreate(seed)` once, then consume it. Never route the tool rule into DLP Detection Rules or save without confirmation. Use composite event keys for list identity and selected details; on successful clearing, close the detail. Render new/changed user text in all four runtime locale files: `src/i18n/locales/{en,zh,ja,zh-TW}.json`; unknown values get safe labels rather than bare enum keys. Keyboard-openable rows, bounded long summaries, and horizontal table scrolling remain usable.

## 6. Refresh and notifications

[Unified hooks](../../src/hooks/useSecurityEvents.ts) use separate TanStack Query keys: `["securityEvents", "page", normalizedFilter, page, pageSize]` and `["securityEvents", "summary", normalizedFilter]`, never the old `["security"]` DLP cache. Disable list focus/reconnect automatic refetch. Both existing bridges (`useSecurityEventBridge` and `useToolRiskEventBridge`) retain their own notifier callback and legacy cache invalidation; on either payload-free `security-audit-recorded` or `tool-risk-recorded`, also invalidate/refetch active unified summaries and mark unified pages stale with `invalidateQueries({ queryKey: ["securityEvents", "page"], refetchType: "none" })`. Failure of one invalidation must not prevent the others or notification catch-up.

The list-only `useSecurityEventsLiveRefresh` subscribes to both signals: refresh page one, but on a deeper page leave items, total, detail snapshot, and page number intact and show **updates may be available**. A signal has no filter payload, so it does not prove matching rows arrived; even low-risk-only traffic, a DLP-only filter, or a fixed historical range must not force a jump. Explicit Refresh returns to page one and forces a fetch even if cached; paging/filter changes query their new snapshot and late old-key results must not overwrite it. Initial subscription catch-up, fixed/safe subscription errors, and unlisten on unmount are required. Keep the existing home count's 30-second background-disabled polling policy; add no list polling. Do not change notifier read APIs, cursors, throttles, permissions, policy filters, or mount timing, and never trigger historical notifications merely by querying. Neither new notification nor diagnostics may include tool summaries, rule free text, or raw parameters; the inherited DLP notifier's custom-name behavior is not changed by this contract.

Both unified and legacy successful clear operations invalidate unified summary plus the two old query domains; unified page queries are only marked stale. The initiating list returns to page one and refetches, while another open deep page signals changed data without claiming its old snapshot is current. No clear operation emits a newly-recorded signal or resets notifier cursors.

## 7. Clearing and rollback

Label the action **Clear currently filtered results (all pages)**, not a vague clear logs. Confirmation states view, sources, dates, inclusion of non-risky tool rows, irreversibility, and that a preview total is only an estimate. Freeze the filter on submission and cap its upper timestamp at the earlier of a fixed upper bound and confirmation time; concurrent backfills/same-millisecond rows might still match. Delete using the exact request-time filter in one extension-database write transaction, rolling both tables back if either delete fails. Default view deletes DLP and matching `risky` tools only; All Tool Calls follows the selected verdict and only its **all** option includes other or null verdicts. Report the actual three deletion counts, not the preview. On failure preserve filters and show a fixed safe error; on success close detail, reset to page one, and invalidate affected caches. Never delete rules, lists, settings, or policy history; do not change `policyVersion`. Clearing does not stop future capture, and rollback of the UI cannot restore deleted data.

## 8. Verification and performance acceptance

Test with synthetic fixtures, mock Tauri events, in-memory or temporary **split-file** databases, and dynamic ports. Do not access user data under `~/.cc-switch`, start/replace the running app, occupy `127.0.0.1:15721`, or execute commands shown in observations. Source-specific cases: equal IDs/timestamps in both tables, stable cross-page order, count independent of page length; DLP repeated triggers counted once and tool `suppressed_count` excluded; unknown actions/levels/verdicts, missing fields, old imported summary read masking; every filter and combination (including HIGH/CRITICAL, literal `%`/`_`, DLP-only search, midnight and custom 999 ms boundaries), invalid inputs, safe failures and split-storage isolation. Verify both-table clear rollback and preservation of settings/history. UI cases: both sources and four verdicts, source-specific details and observation caveat, summary scope and error placeholder rather than zero, app equality between home count/details, no sensitive home details, one-shot safe rule seed and confirmed list drafts, clear confirmation/receipt/failure, all four locales, keyboard and long-row layout. Test both event bridges, active summaries/first-page refresh, deep-page stability, focus/reconnect, manual refresh, stale responses, subscribe/unsubscribe, notifier isolation, and legacy clear invalidation.

Performance budget (acceptance target, **not a measurement in this worktree**): use at least 100,000 synthetic rows per table with sparse `risky` tools in an isolated temporary database. On macOS arm64 release builds, warm up each scenario 5 times and sample 30 serial queries; report machine/compiler/build, fixture distribution, `EXPLAIN QUERY PLAN`, elapsed median/p95/max and actual mutex-held time, and an explicitly _non-equivalent_ old-API baseline. Cover default first page, today, 30 days, whole history, app, HIGH/CRITICAL, same timestamps, deep pages, and literal keyword search. Default first page and summary individually target p95 <= 250 ms; all other named scenarios target p95 <= 500 ms; each lock hold <= 1 second. Investigate budget breaches or evident degradation instead of asserting indexes are sufficient. Never hydrate a full table to implement keyword search. Add an **extension** migration for a proven necessary composite index only with before/after evidence and old-database upgrade tests; do not touch the shared/historic schema versions for that purpose. A separate storage review is needed if the single connection cannot meet lock bounds. Synthetic benchmarks do not represent arbitrary production distributions.

From the repository root, targeted checks include:

```bash
cargo test --lib database::dao::security_events --manifest-path src-tauri/Cargo.toml
cargo test --lib commands::security_events --manifest-path src-tauri/Cargo.toml
cargo test --lib database::split_storage_tests --manifest-path src-tauri/Cargo.toml
pnpm exec vitest run src/lib/api/securityEvents.test.ts src/hooks/useSecurityEvents.test.tsx
pnpm exec vitest run tests/hooks/useSecurityEventBridge.test.tsx tests/hooks/useSecurityAudit.test.tsx src/hooks/useToolRisk.test.tsx
pnpm exec vitest run src/components/security tests/components/SecurityCenterPage.test.tsx
pnpm exec vitest run tests/components/SecuritySystemNotifier.test.tsx src/components/ToolRiskNotifier.test.tsx
```

Full repository acceptance checks (run and record actual results before claiming completion):

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml
pnpm format:check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

The ignored release performance fixture can be run explicitly with isolated test-home and temporary split-file data; it must not be confused with ordinary unit coverage:

```bash
CC_SWITCH_TEST_HOME="$(mktemp -d /tmp/security-events-home.XXXXXX)" \
cargo test --release --lib \
  database::dao::security_events::tests::performance_100k_per_table \
  --manifest-path src-tauri/Cargo.toml \
  -- --ignored --nocapture --test-threads=1
```

The upstream design recorded passing isolated automation and synthetic performance on an earlier worktree, but those numbers do not validate this worktree or a shipped build; its older API baselines are not semantically equivalent. A passing mocked test, review, or synthetic benchmark does not establish live Tauri IPC, desktop visual layout, operating-system notification permissions, installer/signing/notarization, or production-data performance. Keep those acceptance claims separate.

## 9. Release and compatibility boundary

Retain legacy IPC compatibility and verify every public **security event record** counter uses the defined scope. The mixed collection consists of observations and DLP audit rows, not deduplicated incidents or evidence of tool execution. With no data migration, switching the UI back to old APIs is possible; confirmed deletions remain irreversible. If future indexing requires migration, document old-app compatibility and rollback separately rather than promising that lowering a schema version will restore data. Related boundaries: [DLP architecture](../architecture/security-audit.md), [tool observations](../architecture/tool-risk.md), and [shared/extension storage](../architecture/storage.md).
