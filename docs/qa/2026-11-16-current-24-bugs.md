# November 16 current 24-row acceptance list

The existing browser tab was read without reload or filter changes. Its current filtered view contains 24 records. The Claude CLI model-list report has reappeared as row 2 with acceptance failed. The other 23 rows correspond by title to `2026-11-16-current-23-bugs.md`, shifting rows 2 onward by one. No tracker fields were edited. Numbers refer only to this snapshot.

| Row | Report | Tracker | Repair and acceptance status |
| --- | --- | --- | --- |
| 1 | Gemini-compatible Base URL cannot load models | Acceptance failed | Unresolved reported endpoint; investigation deferred by user |
| 2 | Claude CLI shows only part of the model list | Acceptance failed | Reopened report; no new repair or acceptance established |
| 3 | Account login cannot sign in or discover models except Grok | Pending | Unresolved onboarding case; Antigravity account-specific failure does not resolve other platforms |
| 4 | First routing template has no usable Add action | Acceptance failed | Follow-up: all template Add actions open editable drafts even with missing models or catalog errors; requirements are guidance, and Save validates the draft; manual retest pending |
| 5 | Request-graph supplier icon differs from configuration | Acceptance failed | Earlier icon fix exists; reported failed build remains unresolved and deferred |
| 6 | Codex reports Block for Hide and restore locally | Pending | Partial supported-path repair; native acceptance outstanding |
| 7 | Codex repeats a block after an ordinary follow-up | Pending | Earlier history/restoration repair; native acceptance outstanding |
| 8 | WorkBuddy restoration has an unsuitable message and HTTP 400 | Pending | Partial supported-path repair; native acceptance outstanding |
| 9 | WorkBuddy ordinary follow-up still receives HTTP 400 | Pending | Earlier history/restoration repair; native acceptance outstanding |
| 10 | Hide and restore locally blocks instead of forwarding | Pending | Partial; unsupported restoration paths intentionally remain guarded |
| 11 | Signed-in Qoder CN account cannot refresh | Pending | Refresh invokes native login read when offered; regression passed; native permission flow needs retest |
| 12 | WorkBuddy old chat keeps the previous provider after restart | Pending | Unresolved reported case; existing conversation selection is preserved |
| 13 | Gemini Base URL protocol detection fails | Pending | Unselected-catalog detection repaired; original endpoint needs acceptance |
| 14 | Agent restart requires two confirmations | Pending | Earlier local repair; native retest needed |
| 15 | Qoder CN supplier names do not match | Pending | Display labels repaired while retaining wire aliases; native retest needed |
| 16 | Cannot switch configuration scheme | Pending | Partial repair for routed Codex to native selection, rollback and Undo; full scenario retest needed |
| 17 | Codex has no restart prompt after supplier change | Pending | Desktop configuration freshness recorded; native retest needed |
| 18 | Restart confirmation window keeps jumping | Pending | Earlier local repair; native retest needed |
| 19 | Scheme preview omits supplier names | Pending | Shared-model preview labels repaired; retest needed |
| 20 | Native dialog stays English with Chinese system and app | Pending | macOS bundle metadata repaired; packaged-build retest required |
| 21 | Cannot change upstream format | Pending | Same-endpoint/auth editing repaired; live retest needed |
| 22 | Equivalent commands prompt inconsistently across agents | Pending | Partial tool-attribution repair; remaining paths unverified |
| 23 | Codex permission-change command has no prompt or event | Pending | Original live scenario unresolved; synthetic mode-4755 detection passes |
| 24 | Custom-rule security events omit session ID | Pending | Session propagation repaired for new events; historical/missing IDs cannot be reconstructed |

Additional chat reports have no tracker row number: Qoder confirmed-date calendar and WorkBuddy cumulative/date semantics; Doubao legacy/new landing-card compatibility; Doubao selected-account row summary. Native UI showed the Doubao current account's confirmed 400-credit claim and the repaired row label. Do not count synthetic tests as manual acceptance. The earlier duplicate WorkBuddy restart screenshot was not reproduced on the latest native screen, which showed one restart control; no separate source fix was made for that visual report.

After the main integration and push, the user requested reusing a task model for both smart-template roles. That follow-up supersedes the earlier decision to disable the first template when a cheaper task model is absent. It is a subsequent local change; it is not part of commit `d156a4c0`.

The user then requested removing model-related disabling from every template entry. All three Add actions now open the editor without requiring available models or classifier support. Missing conditions are shown as guidance and users complete their drafts before saving. This later follow-up is separate from the single-model change committed as `6ea31624`.
