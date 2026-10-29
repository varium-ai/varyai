# Pending bug retest batch — 2026-10-28

Source: the user's open Chrome Bug Management view, filtered to Baomi and pending status. The current view contains 20 records. Details and comments for rows 2–20 were read through the browser; all have zero comments. Row numbers below refer to this refreshed view. No tracker status or comment was changed.

The user requested fixing and self-testing every item except row 1, initially retaining all changes locally for individual user retesting. On 2026-10-29 the user authorized a local commit. No push was requested.

| Row | Report | Current work |
| --- | --- | --- |
| 1 | Codex official token refresh | Excluded by the user. |
| 2 | TRAE routing switch reports success while disconnected | Existing pending-state fix; focused renderer checks pass. |
| 3 | Revealing gateway keys moves the page | Existing secret-dialog fix; focused renderer checks pass. |
| 4–7 | Budget exhaustion causes prolonged retries in ZCode, Kimi, Gemini and Qwen | Existing non-retryable quota response and prior isolated installed-client acceptance; retain for user retest. |
| 8 | Codex/Gemini cache-creation field shows N/A | Implemented and focused tests pass: replace N/A with an unreported-data explanation; retain positive creation reported by converted upstream protocols. |
| 9 | Routing edits apply independently of the outer Save action | Preserve immediate operations. At the user's request, move the save action and its scope explanation inside the expandable configuration-directory section. No global save footer remains. Settings component/integration tests pass. |
| 10 | Codex GLM use includes unexpected Qwen model records | Unresolved attribution report. The attachment shows both the client-request model and actual model as Qwen for the unexpected entry. The ingress reads the client request model separately from the forwarded model. No evidence yet establishes a GLM-to-Qwen rewrite; do not hide or relabel legitimate requests. |
| 11 | TRAE supplier switching fails | Implemented a reproduced stale-cache withdrawal fix. Added sanitized add/update/remove diagnostics with numeric native error codes. Bridge tests pass; tester-machine switching still needs acceptance and this does not claim to repair lost ownership metadata. |
| 12 | TRAE budget binding displays a network error | Unresolved. Read the screenshot and 43-second recording. They show an available all-models key, an official TRAE model request and no metered usage. The recording does not expose the underlying failure response. Native official TRAE audit and routed custom-model requests follow different paths; a generic network error alone does not establish a budget denial. |
| 13 | Long gateway key names overflow | Implemented bounded, truncated names with the full name available as a tooltip. |
| 14 | Duplicate gateway key names | Implemented trimmed, case-insensitive uniqueness in the UI and transaction-protected backend. Drafts survive rejection. Existing duplicate names may retain their names when updating other fields. |
| 15 | Duplicate connection preset names | Implemented trimmed, case-insensitive uniqueness in the UI and transaction-protected backend. Errors appear beside the fixed footer buttons without scrolling the form. Drafts survive rejection. Existing duplicate names may retain their names when updating other fields. |
| 16 | Preset list height and scroll affordance | Implemented a visible preset count, scroll hint and keyboard-focusable list with a sticky header. Focused tests pass. |
| 17 | Editing later gateway keys does not bring the editor into view | At the user's request, editing now expands inside the selected key card below its controls. Focus uses preventScroll; editing does not scroll to the global create form. Only one card editor is open at a time. |
| 18 | Raising an exhausted budget still leaves slow replies | Regression passes: an exhausted key is admitted immediately after its limit increases, preserving prior usage. Existing non-retryable quota responses prevent the previous server retry loop; installed-client timing still needs user acceptance. |
| 19 | Usage navigation for later gateway keys lands incorrectly | Implemented scroll and focus to the selected editor or usage filter. Focused renderer tests pass. |
| 20 | WorkBuddy suppliers cannot share model IDs | Implemented saving and selecting suppliers with overlapping owned model IDs. Switching replaces the managed overlapping group while preserving unrelated/manual models. All 8 WorkBuddy service tests pass. |

## User retest steps

- Row 8: Open Codex and Gemini usage. Missing cache-creation data should say that it was not reported; positive reported creation remains numeric.
- Row 9: Open Advanced settings. No global save button should appear. Expand Configuration directories and confirm its save button and scope explanation appear inside that section. Change a directory, save it, and verify the existing restart prompt when applicable.
- Row 11: After loading the rebuilt backend and reconnecting TRAE, switch between two overlapping model suppliers repeatedly. If a native operation fails, record its operation and numeric code. Manual custom models must remain intact.
- Rows 13–14: Create a long gateway-key name, then try a second key with the same name (including whitespace or case differences). The layout must stay bounded and the rejected draft must remain editable. Existing legacy duplicate names must still permit budget changes.
- Rows 15–16: Try duplicate preset names, then create at least six distinct presets. Check the visible count, scroll hint and access to every row.
- Row 17: From a later key, choose Edit. Its editor should expand below that key without jumping to the top. Switch to another key and cancel; only the selected card should contain an editor, and Cancel should close it.
- Row 19: From a later key, choose View usage. The correct filtered usage section should move into view and receive focus.
- Row 18: Exhaust a small test budget, raise its limit, and send a fresh request. Usage must remain counted and the next request must be admitted immediately.
- Row 20: Add two WorkBuddy suppliers sharing a model ID. Test both save-only and immediate activation, then switch back and forth. Unrelated native models must remain unchanged.

## Acceptance limits

The user authorized committing this batch on 2026-10-29; user acceptance remains pending. The running desktop backend has not been restarted for this batch. Renderer changes may hot-reload, but backend fixes require loading a newly built development application before user acceptance. Rows 10 and 12 are explicitly unresolved; do not change their tracker state to fixed.

## Verification completed on 2026-10-29

- `pnpm typecheck`: passed.
- `pnpm build:renderer`: passed.
- `pnpm test:unit`: 2,568 tests across 209 files passed on the final complete run. The previous run had two unrelated notarization fixture timeouts; their isolated 21-test file and the subsequent complete run both passed. No notarization scripts were changed.
- `pnpm format:check` and formatting checks for changed tests/bridge files: passed.
- `cargo check --offline`: passed.
- `cargo test --offline`: 4,089 passed and 15 ignored across library and integration suites. The library suite accounts for 3,926 passing tests; ignored desktop acceptance tests were not run.
- `cargo fmt --check` and `cargo clippy --offline -- -D warnings`: passed.
- `pnpm test:trae-bridge`: all 29 tests passed.
- `cargo build --offline`: passed; the new development executable is built but has not replaced or restarted the running application.
- `git diff --check`: passed. No merge or push is part of the requested local commit.

Validation logs are local temporary files under `/tmp/varyai-remaining-*.log`; they contain synthetic test fixtures. Passing automated checks do not establish tester-machine TRAE acceptance, installed-client response timing or release packaging acceptance.

Follow-up on 2026-10-29: the user approved placing the save action inside Configuration directories. A regression first reproduced the old global button, then passed with all 21 settings component/integration tests after moving it. Other bug fixes in this batch remain intact.

Follow-up on 2026-10-29: gateway-key editing now stays in the selected card, as requested. All 17 gateway-panel tests pass, including editing later keys, switching editors, canceling, budget updates, creation and usage navigation. No backend behavior changed.
