# November 16 current 18-row retest list

The existing Chrome tab was read on November 16 at approximately 19:38 Asia/Shanghai, without reloading or changing filters. Its rendered grid explicitly shows 18 records. This supersedes the numbering in the earlier 24-row snapshot. Tracker fields were not edited. The current tracker has three acceptance-failed reports (1, 2, 4) and fifteen pending reports.

Source repair status and user acceptance are separate. Existing regression results support the retest candidates below; this read did not perform native acceptance tests. A report disappearing from this filtered view is not proof that it passed.

| Row | Report | Repair status | Manual retest |
| --- | --- | --- | --- |
| 1 | Gemini-compatible Base URL cannot load models | Reported failure unresolved; previously deferred | Reproduce with the original endpoint; existing fallback tests do not establish acceptance. |
| 2 | Claude CLI shows only part of the model list | Reopened acceptance failure unresolved | Compare the actual available model list against the configured catalog on the affected device. |
| 3 | Account login cannot discover models or sign in except Grok | Original onboarding case unfixed | The Antigravity account that also fails in the official client is a separate account limitation. |
| 4 | Request-graph supplier icon differs from configuration | Existing repair failed acceptance; investigation deferred | Compare the configured icon with a new request graph on the reported build. |
| 5 | Signed-in Qoder CN account cannot refresh | Repaired; ready for native retest | Keep Qoder CN signed in. Click Refresh in Vary AI; when Read client login is offered, Refresh must invoke that operation and respect the system permission prompt. |
| 6 | WorkBuddy old chat retains the previous supplier after restart | Original case unfixed | Test an existing chat separately from a new chat; do not infer success from a new conversation alone. |
| 7 | Gemini Base URL protocol detection fails | Reproduced catalog-selection case repaired; endpoint acceptance pending | With a populated catalog and no selected models, click Detect protocol. Expect a result without altering selected models, or a visible sanitized error. |
| 8 | Agent restart requires two confirmations | Repaired; ready for native retest | Perform one restart action and its confirmation. Expect one restart operation with no repeated confirmation cycle. |
| 9 | Qoder CN supplier names do not match | Repaired; ready for native retest | Refresh the connection and reload Qoder. Two suppliers with the same model name must have distinct supplier-qualified labels; renames must refresh without changing route aliases. |
| 10 | Cannot switch configuration scheme | Partial repair; ready to retest the original scenario | Save two distinct schemes, switch between them, and verify actual Agent selections plus rollback/Undo. Full scenario acceptance remains outstanding. |
| 11 | Codex has no restart prompt after supplier change | Repaired; ready for native retest | With Codex running, switch a native supplier. Expect a restart requirement that clears after the user restarts Codex. |
| 12 | Restart confirmation window keeps jumping | Repaired; ready for native retest | Confirm restart once; observe a stable pending state and eventual completion or actionable retry feedback. |
| 13 | Scheme preview omits supplier names | Repaired; ready for native retest | Preview a saved scheme containing models with identical names from different suppliers. Current and target selections must identify suppliers; groups must identify groups. |
| 14 | Native dialog is English with Chinese system and app | Bundle metadata repaired; packaged-build retest required | Open the corresponding native dialog from a newly packaged app. A development launch alone does not validate bundle localization. |
| 15 | Cannot change upstream format | Repaired; ready for native retest | Edit the format of an existing supplier while retaining the endpoint and credentials, save, reopen, and verify the saved format plus a normal request. |
| 16 | Equivalent commands prompt inconsistently across Agents | Partial tool-attribution repair; original paths unverified | Compare one harmless synthetic command under the same security rule in the affected Agents. Record both prompts and security events. |
| 17 | Codex permission-change command has no prompt or security event | Original live case unfixed | Synthetic mode-4755 detection passes, but the original client invocation remains unresolved. Do not treat the unit result as acceptance. |
| 18 | Custom-rule security events omit session ID | New-event propagation repaired; ready for native retest | Trigger a custom rule in a client that supplies a session ID. Inspect the newly created event; historical events and requests without an ID cannot be backfilled. |

Suggested sequence: 5, 9, 13, 15, 8, 12, 10, 11, 18, then 7. Row 14 requires the packaged app. Row 16 is diagnostic rather than completed acceptance. Rows 1, 2, 3, 4, 6 and 17 remain unresolved reported cases.

The user separately confirmed that the latest Codex routing/restoration test now works. That confirmation is retained for the tested scenario; it does not establish acceptance for every ordinary-follow-up case or other Agent. Codex restoration and routing-template reports are absent from this current filtered view and are not assigned new row numbers here.

Earlier repair evidence is recorded in [the 24-row snapshot](2026-11-16-current-24-bugs.md), [the remaining-bug repairs](2026-11-16-remaining-bugs.md), and [the Qoder and detection follow-up](2026-11-16-qoder-detection-follow-up.md). This update changes documentation only; no new source verification run is required.
