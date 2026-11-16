# November 16 latest 23-row tracker snapshot

The existing browser tab was read again without reloading or changing filters. It now shows 23 reports: rows 1, 3 and 4 have an acceptance-failed tracker status; the other 20 are pending. The previous row 18, Codex quits without reopening, no longer appears in this view. Its acceptance outcome is unknown; disappearance from a filtered view is not acceptance evidence.

The scheme-preview supplier-name report moved from row 19 to row 18. The native-language dialog report is now row 19. Use report names as well as numbers when retesting or changing tracker fields. No external tracker fields were edited.

Most source statuses carry forward the [previous repair and verification record](2026-11-16-list-follow-up.md). Rows 12 and 14 now include the [Qoder and detection follow-up](2026-11-16-qoder-detection-follow-up.md); no new installed-client acceptance is implied. The user deferred the three acceptance-failed investigations and selected retaining template disabling with a clear unmet-requirement explanation.

| Current | Previous | Report                                                       | Tracker           | Source status                                                                                              |
| ------- | -------- | ------------------------------------------------------------ | ----------------- | ---------------------------------------------------------------------------------------------------------- |
| 1       | 1        | Gemini-compatible Base URL cannot load models                | Acceptance failed | Existing native/fallback handling; reported endpoint unresolved                                            |
| 2       | 2        | Account login does not discover models or sign in            | Pending           | Unfixed reported onboarding flow                                                                           |
| 3       | 3        | First routing template has no usable Add action              | Acceptance failed | Retain disabling per user; localized requirement now explains the button                                   |
| 4       | 4        | Request-graph supplier icon differs from configuration       | Acceptance failed | Existing configured-icon resolution; failed build needs comparison                                         |
| 5       | 5        | Codex reports Block for Hide and restore locally             | Pending           | Partial supported-path fixes; live retest required                                                         |
| 6       | 6        | Codex repeats a block after ordinary follow-up               | Pending           | Existing history/restoration fix; live retest required                                                     |
| 7       | 7        | WorkBuddy restoration receives HTTP 400                      | Pending           | Partial supported-path fixes; live retest required                                                         |
| 8       | 8        | WorkBuddy ordinary follow-up still receives HTTP 400         | Pending           | Existing history/restoration fix; live retest required                                                     |
| 9       | 9        | Hide and restore locally stops the request                   | Pending           | Partial; unsupported paths remain guarded                                                                  |
| 10      | 10       | Signed-in Qoder CN account cannot refresh                    | Pending           | Refresh now invokes offered native login read; local regression passed, system-permission acceptance pending                                                                                      |
| 11      | 11       | WorkBuddy old chat keeps the previous provider after restart | Pending           | Unfixed reported case                                                                                      |
| 12      | 12       | Gemini Base URL protocol detection fails                     | Pending           | Reproduced unselected-catalog detection failure repaired; original endpoint acceptance remains outstanding |
| 13      | 13       | Agent restart needs two confirmations                        | Pending           | Earlier local repair; native retest required                                                               |
| 14      | 14       | Qoder CN supplier names do not match                         | Pending           | Shared model/supplier display labels refresh while preserving wire aliases; native retest required         |
| 15      | 15       | Cannot switch scheme                                         | Pending           | Earlier partial repair for routed Codex to native selection, rollback and Undo                             |
| 16      | 16       | Codex does not request restart after supplier change         | Pending           | Native-supplier switch now records desktop configuration freshness; installed-client retest required       |
| 17      | 17       | Restart confirmation window keeps jumping                    | Pending           | Earlier local repair; native retest required                                                               |
| 18      | 19       | Scheme preview omits supplier names                          | Pending           | Shared-model preview labels repaired; installed-client retest remains outstanding                          |
| 19      | 20       | Native dialog is English while system and app are Chinese    | Pending           | Earlier macOS bundle metadata repair; packaged app retest required                                         |
| 20      | 21       | Cannot change upstream format                                | Pending           | Earlier same-endpoint/auth repair; live retest required                                                    |
| 21      | 22       | Equivalent commands prompt inconsistently across agents      | Pending           | Earlier partial tool-attribution repair; remaining invocation paths unverified                             |
| 22      | 23       | Codex permission-change command has no prompt or event       | Pending           | Original live command remains unresolved; synthetic mode-4755 detection passes                             |
| 23      | 24       | Custom-rule security events omit session ID                  | Pending           | Earlier new-event session propagation repair; historical/missing IDs cannot be reconstructed               |

The additional Qoder seven-day reward display report is tracked in [the calendar follow-up](2026-11-16-qoder-calendar.md). It is not assigned a new browser row number. No tracker field was changed.

The user subsequently reported an available Doubao Work reward being shown as inactive. The additive landing-card fix and the five-provider semantics audit are recorded in [the reward audit](2026-11-16-reward-semantics-audit.md); this report has no assigned browser row number.
