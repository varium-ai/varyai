# Unified security-event entry

Status: implemented with isolated functional tests and synthetic performance measurements. Real desktop acceptance remains separate. Updated 2026-10-22.

This specification concerns querying and presentation. It does not change detection, forwarding, persistence workers, tool execution, or system-notification semantics.

## Scope and architecture

Security Center presents DLP audit rows and risky tool observations in one timeline with shared dates, source filters, and statistics. A secondary All tool calls view retains access to saved non-risky observations. Tool-risk configuration retains rules, sandbox, command lists, and policy history; its embedded log table and extra navigation link are removed.

A unified query layer reads the existing extension-database tables. Detection and storage remain independent: no shared rule model, merged table, duplicate DLP writes, cross-source deduplication, backfill, or new telemetry. Tool observations do not prove execution, prevention, or final executed arguments.

Primary implementation paths:

| Layer | Files |
| --- | --- |
| DAO and tests | src-tauri/src/database/dao/security_events.rs, security_events_tests.rs |
| IPC | src-tauri/src/commands/security_events.rs |
| Types and API | src/types/securityEvents.ts, src/lib/api/securityEvents.ts |
| Queries and subscriptions | src/hooks/useSecurityEvents.ts |
| List and details | src/components/security/SecurityEventsPanel.tsx, SecurityEventDetailsDialog.tsx |
| Parent/rules/tool display | SecurityCenterPage.tsx, ToolRiskRulesPanel.tsx, ToolRiskEventsPanel.tsx |
| Statistics | SecurityActivitySummary.tsx |

The original private implementation also updated HomeStatsBar/HomeStatsDetails. Those components were removed by the public baseline and are not restored during migration.

## Views, counts, and presentation

The default security_events view contains every saved DLP row plus tool rows with verdict=risky. The tool_calls view contains all saved tool rows, including unchecked, uncertain, low_risk, and historical null verdicts; it does not promise that every actual tool invocation was collected.

All counts represent database rows, not execution counts, raw findings, or frontend groups:

| Summary field | Meaning |
| --- | --- |
| total | DLP rows plus risky tool rows |
| dlpCount / toolRiskCount | Source split |
| redacted | DLP action_taken=redact |
| warnings | DLP action_taken=audit plus tool verdict=risky |
| protectiveBlocked | DLP action_taken=block |
| other | DLP allow or unknown actions |

The summary satisfies total=dlpCount+toolRiskCount and total=redacted+warnings+protectiveBlocked+other. It need not equal the three visible action cards alone. DLP aggregation remains one row with triggerCount, first/last timestamps, and the original session. Tool projection uses triggerCount=1 and timestamp for both times; suppressed_count is not a row count. Neither projection changes storage.

Date filtering uses timestamp: the DLP first-hit time or tool observation time. A row first created yesterday and hit again today remains in yesterday's range. Security Center summaries use the selected time range across all apps and both default sources, regardless of list filters or the secondary view. List total follows all current filters. Statistics use today's default security-event set across apps. Independent requests do not promise a shared cross-page atomic snapshot.

Rows and selection use composite eventType+id keys. Tool details show source/stage, integrity, verdict, category, severity, reason, rule IDs, and the bounded backend summary. Missing historical fields display Not recorded; unknown values display Unknown. Never infer a session/provider from the active app or nearby events. Tools display an observation-only outbound status and a warning that the tool may already have run and final execution is unknown. Unchecked/uncertain/low_risk rows retain their actual verdicts. Unknown DLP actions/levels must not become allowed, sent, or LOW.

Tool summaries use best-effort privacy masking, not a claim of complete redaction. Lists may create a blacklist/whitelist draft from the backend summary, with confirmation; disable this action if no summary exists. Creating a rule passes only permitted enum fields in ToolRiskRuleSeed, never command text, summary, or raw parameters. The parent switches to tool-risk configuration, consumes the seed once, and opens an unsaved editor. Page-local similarity groups do not change row totals or pagination.

## Types and filter contract

SecurityEvent is a discriminated union of DLP and tool records, with common key, timestamp, lastSeenAt, triggerCount, nullable appType/sessionId/riskLevel, and result. Legacy unknown DLP strings remain in a widened new record type without changing AuditLogRecord. Rust uses matching camelCase serialization. See src/types/securityEvents.ts for exact field definitions.

DLP redact/block/audit/allow map to redacted/blocked/warning/allowed; unknown actions map to unknown while preserving the raw value. Valid uppercase risk levels remain public; unknown levels map to null. Tool risky maps to warning, other supported verdicts retain their names, and null/unknown maps to unknown. Only risky medium/high tools map to MEDIUM/HIGH; none, invalid, or non-risky levels map to null. HIGH includes DLP HIGH/CRITICAL and tool high; explicit CRITICAL is exact. Other levels match exactly. Client identities are not aliased, including claude versus claude-desktop or openai.

Apply all specified filters with AND before sorting and pagination:

| Filter | Contract |
| --- | --- |
| view | security_events or tool_calls |
| eventType | dlp/tool_risk; tool_calls only accepts omitted/tool_risk |
| timestamps | Inclusive Unix milliseconds; timestamp, not lastSeenAt |
| appType | Exact stored DLP app_type or tool client_category |
| riskLevel/result | Filter the projected values described above |
| category | Namespaced dlp:<category> or tool_risk:<riskCategory>; exact source category |
| sessionId | Trimmed exact DLP session only; excludes tools |
| keyword | Literal substring of DLP rule name/masked snippet only; excludes tools |
| toolVerdict | Only legal in tool_calls; omitted includes historical null verdicts |

DLP session/keyword controls appear only when the DLP source is explicitly selected. Selecting all sources or a pure tool source clears and hides those controls. Pure DLP hides tool categories. Switching views retains applicable common filters, clears unsupported filters, and returns to page one. The default risky restriction cannot be widened by another filter. Valid but incompatible filters yield zero rows; illegal combinations return explicit errors.

Today/7d/30d ranges have an open upper bound. Fixed custom ranges include the last second through +999ms and retain liveEndTime semantics. Dates remain stable while reading deeper pages, including across midnight. Filter/date changes return to page one. Errors display an unknown count instead of zero or stale totals.

## IPC, transactions, and privacy

| Command | Arguments | Result |
| --- | --- | --- |
| get_security_events | filter, page, pageSize | items, total, page, pageSize |
| get_security_events_summary | filter with view=security_events | Summary fields above |
| clear_security_events | filter | deletedTotal, deletedDlp, deletedToolRisk |

Page>=1, pageSize=1..100, checked offset multiplication. Timestamps must be nonnegative, JS-safe integers with start<=end. Trimmed session/keyword values permit at most 512 Unicode characters. Empty strings are omitted. Reject unknown enums and invalid combinations without silently broadening queries. Bind SQL parameters and escape LIKE wildcard/escape characters for literal keyword matching.

IPC executes DAO work through spawn_blocking. Database/join failures return fixed safe errors, never raw SQL, summaries, sessions, or input objects. Read failures must not become successful empty pages.

The DAO accesses only extension_connection(), outside forwarding. Count/page and each summary use a short transaction snapshot with no await, notification, network, or file operations under the mutex. Filter each table, UNION ALL, and sort timestamp DESC, eventType ASC, id DESC. Do not fetch one page per source and sort in the browser, hydrate all rows to count, or issue per-row rule lookups. Batch hydration uses the same snapshot and the existing tool read_event privacy projection; DLP returns only existing masked fields.

OFFSET does not guarantee a persistent snapshot across requests. New writes may move page boundaries. Deeper pages preserve their current data and show a possible-update flag until manual refresh; subsequently requested pages may overlap or move. No cursor service or new connection architecture is introduced.

Existing timestamp/verdict indexes are reused. No table, field, index migration, backfill, or version changes are required. Any future index migration needs measured evidence and extension-only migration/compatibility tests; shared and historical schema versions are independent.

## Clearing records

The action means Clear matching records across all pages. A confirmation describes view, source, dates, inclusion of non-risky tools, irreversibility, and continued future collection. Preview total is an estimate.

At confirmation, freeze the filter and cap endTimestamp at the earlier fixed endpoint/current time. The backend deletes the actual matches; late/backdated or same-millisecond writes may still qualify. In security_events, delete only matching DLP and risky tool rows. In tool_calls, honor verdict filters; only All includes other/null verdicts.

Delete both sources in one extension write transaction; either failure rolls back both. Return actual per-source counts. Success closes details, returns to page one, refreshes summaries, and invalidates unified and legacy domains. Failure retains filters and reports no success. Do not delete rules/settings/lists/policy history, change policyVersion, emit recorded signals, or reset notifier cursors. UI rollback cannot undo confirmed deletion.

## Refresh and compatibility

Query keys use securityEvents/page/normalizedFilter/page/pageSize and securityEvents/summary/normalizedFilter. Keep legacy DLP caches separate. Disable list focus/reconnect refetch and avoid polling. Existing event bridges invalidate active unified summaries while marking page queries stale with refetchType=none. Failures in one invalidation must not block other domains or notifier catch-up.

The lightweight live hook listens to security-audit-recorded and tool-risk-recorded. Page one refetches; deeper pages retain items/total and show Possible updates, without claiming the current filter necessarily has new records. Non-risky traffic, pure DLP filters, and fixed historical ranges must not force page jumps. Manual refresh returns to page one and forces a request even if cached; ignore cancelled/late results. First-page details update by composite key, while deeper-page details retain the snapshot.

Subscriptions perform an initial read after registration, log fixed errors, tolerate one failed source, and clean up on unmount. Legacy and unified clear actions invalidate all relevant domains; other deep-page readers receive an update warning even if their query was already stale. Lists do not become a second notifier.

Retain legacy IPC parameters/results, notification-specific reads, cursors, mounting, throttles, permissions, policy filtering, and backup/sync/telemetry exclusions. New tool notifications/errors must not include command summaries, free-text rule content, or raw arguments. Existing DLP custom-rule-name behavior is outside this change. Do not change detection, aggregation, redaction, collection, payloads, headers, retries, or fail-safe forwarding for this query feature. All user text covers en/zh/ja/zh-TW.

## Acceptance and historical evidence

Backend regressions cover composite IDs, deterministic ties/pages, four verdicts and nulls, historical unknown values, aggregated DLP counts, summary conservation, timestamp boundaries, literal wildcards, exact sessions, HIGH/CRITICAL semantics, invalid input, safe SQL/join failures, privacy hydration, atomic rollback, and actual split-file extension storage. History remains queryable after rule deletion, disabling collection, or changing policy versions.

Frontend regressions cover source-correct details, missing/unknown metadata, observation-only language, source/view filter cleanup, summary scope, deep-page snapshots across midnight, first-page refresh, old results, subscription cleanup/failures, notifier independence, rule seed consumption, command-list confirmation, clear scope/receipts/errors, locales, keyboard access, and bounded table layout. Notification tests remain separate from mixed-list tests. All fixtures use temporary databases/directories, mock events, and dynamic ports; no installed app or production database is required.

The original 2026-10-21 checks passed typecheck, renderer build, 2204 frontend tests, 3778 backend library tests (13 ignored), 17 split-storage tests, and 78 notifier tests. The 2026-10-22 UI follow-up passed 2206 frontend tests and 136 targeted cases. These are historical evidence, not new-repository test results. Ignored tests are not claimed as covered.

Synthetic performance used Apple M4 Pro, macOS arm64, Rust 1.98.1 release builds, temporary split SQLite files with 100,000 rows per table (1,000 risky tools), 60-day timestamps and ties. Each case warmed up five times and sampled 30 serial queries. Budgets were list/summary p95<=250ms for default/today, <=500ms otherwise, and lock hold<=1s. Representative historical results in milliseconds:

| Case | List median/p95/max | Summary median/p95/max | Lock max list/summary |
| --- | --- | --- | --- |
| Default history | 1.634/1.749/2.112 | 18.041/18.604/18.681 | 2.108/18.679 |
| Today | 1.488/1.550/1.575 | 2.035/2.509/2.568 | 1.571/2.567 |
| 30 days | 1.831/1.920/1.985 | 39.008/40.459/44.382 | 1.981/44.380 |
| App filter | 3.341/3.568/3.669 | 78.635/82.227/84.473 | 3.664/84.470 |
| HIGH including CRITICAL | 8.302/8.933/9.176 | 21.017/21.559/21.605 | 9.170/21.601 |
| Timestamp ties | 2.137/12.172/21.553 | 3.189/6.526/7.320 | 21.535/7.312 |
| Page 1800 | 15.677/16.665/17.379 | 18.177/19.026/19.235 | 17.373/19.232 |
| Literal %_needle | 33.939/58.584/78.737 | 8.092/8.590/8.591 | 78.728/8.588 |

Maximum measured p95 was 82.227ms and maximum lock hold 84.470ms. No index migration was introduced. Older sequential DLP/tool APIs were not semantically equivalent, so these results do not establish a relative speedup. Query plans used timestamp/verdict indexes, MERGE (UNION ALL), and temporary sorting where necessary; literal keyword search may scan DLP without hydrating the full table.

Reproduce the ignored performance fixture explicitly with an isolated test home:

```bash
cargo test --release --lib \
  database::dao::security_events::tests::performance_100k_per_table \
  --manifest-path src-tauri/Cargo.toml \
  -- --ignored --nocapture --test-threads=1
```

Run the repository's required frontend, Rust, formatting, Clippy, and integration checks before submission. Functional and synthetic performance evidence does not establish real Tauri IPC, OS-event/notification permission, installer, or desktop visual acceptance. Tool observations, best-effort masking, OFFSET limits, and irreversible clearing remain explicit product boundaries.
