# November 16 current 14-row retest list

The existing browser tab was read again without reload or filter changes. Its rendered grid now explicitly shows 14 records: rows 1 and 3 have acceptance-failed tracker status, and the other twelve are pending. No tracker fields were edited.

The previous 18-row view no longer contains Gemini model discovery (old 1), Qoder CN refresh (old 5), Gemini protocol detection (old 7), or Qoder CN supplier labels (old 9). Disappearance alone does not establish acceptance. In particular, the old row 7 protocol-detection report is no longer current row 7; current row 7 concerns the Codex restart prompt.

Source repair status carries forward from the [18-row record](2026-11-16-current-18-bugs.md). This read performs no new native acceptance tests.

| Row | Report | Repair status | Manual retest |
| --- | --- | --- | --- |
| 1 | Claude CLI shows only part of the model list | Reopened acceptance failure unresolved | Compare the actual available model list against the configured catalog on the affected device. |
| 2 | Account login cannot discover models or sign in except Grok | Original onboarding case unfixed | The Antigravity account that also fails in the official client is a separate account limitation. |
| 3 | Request-graph supplier icon differs from configuration | Existing repair failed acceptance; investigation deferred | Compare the configured icon with a new request graph on the reported build. |
| 4 | WorkBuddy old chat retains the previous supplier after restart | Original case unfixed | Test an existing chat separately from a new chat; do not infer success from a new conversation alone. |
| 5 | Agent restart requires two confirmations | User retest failed; confirmed Codex/WorkBuddy restart now bypasses connection/freshness gates; local regressions passed; native acceptance pending | Identify the affected Agent and exact error before assigning a cause. Earlier lifecycle tests did not establish native acceptance. |
| 6 | Cannot switch configuration scheme | Partial repair; ready to retest the original scenario | Save two distinct schemes, switch between them, and verify actual Agent selections plus rollback/Undo. Full scenario acceptance remains outstanding. |
| 7 | Codex has no restart prompt after supplier change | Repaired; ready for native retest | With Codex running, switch a native supplier. Expect a restart requirement that clears after the user restarts Codex. |
| 8 | Restart confirmation window keeps jumping | Repaired; ready for native retest | Confirm restart once; observe a stable pending state and eventual completion or actionable retry feedback. |
| 9 | Scheme preview omits supplier names | Repaired; ready for native retest | Preview a saved scheme containing models with identical names from different suppliers. Current and target selections must identify suppliers; groups must identify groups. |
| 10 | Native dialog is English with Chinese system and app | Bundle metadata repaired; packaged-build retest required | Open the corresponding native dialog from a newly packaged app. A development launch alone does not validate bundle localization. |
| 11 | Cannot change upstream format | Repaired; ready for native retest | Edit the format of an existing supplier while retaining the endpoint and credentials, save, reopen, and verify the saved format plus a normal request. |
| 12 | Equivalent commands prompt inconsistently across Agents | Partial tool-attribution repair; original paths unverified | Compare one harmless synthetic command under the same security rule in the affected Agents. Record both prompts and security events. |
| 13 | Codex permission-change command has no prompt or security event | Original live case unfixed | Synthetic mode-4755 detection passes, but the original client invocation remains unresolved. Do not treat the unit result as acceptance. |
| 14 | Custom-rule security events omit session ID | New-event propagation repaired; ready for native retest | Trigger a custom rule in a client that supplies a session ID. Inspect the newly created event; historical events and requests without an ID cannot be backfilled. |

Suggested short retest: row 9, scheme preview supplier labels. Then rows 11, 5 and 8. Row 10 requires a packaged app. Rows 1, 2, 3, 4 and 13 remain unresolved reported cases; rows 6 and 12 are partial repairs.

User follow-up: current row 5 repeatedly reports an error and does not restart the Agent. It is reopened as a failed manual retest, not ready for acceptance. The current application log contains no corresponding restart failure diagnostics.

The user identified both Codex and WorkBuddy and supplied the fixed error key `agentConnections.restartChanged`. The follow-up manual path no longer requires a connection receipt, matching configuration fingerprint, or stale reload state. Real-client acceptance remains outstanding; no application is restarted during automated verification.
