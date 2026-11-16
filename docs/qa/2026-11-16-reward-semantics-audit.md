# Reward data semantics and shared calendar layout

Date: 2026-11-16 (Asia/Shanghai). The user requested the WorkBuddy layout for Qoder and a review of all reward integrations. No commit or push was requested.

## UI change

Qoder now renders the same calendar component as WorkBuddy: summary column, seven-date column, information tooltip and the existing card styles. The separate full-width Qoder layout and always-visible explanatory paragraph were removed. Qoder does not report a campaign total, so that summary remains unavailable; unconfirmed dates stay unknown, and the latest reward-window date is labeled Current window. WorkBuddy still uses its official total and complete date history, including known unmarked dates. This layout change does not alter claim behavior. The subsequent Doubao landing-card correction below changes status recognition in the shared SDK helper.

## Adapter audit

| Integration | Evidence and available semantics | Result |
| --- | --- | --- |
| WorkBuddy China | Current adapter reads `streak_days` and `checkin_dates`. Previous verified responses include nonconsecutive dates; the count is therefore treated as a campaign total. The current opt-in status-only native test passed again. | Keep the existing total plus actual-date calendar. |
| Doubao Work | The official SDK returns separate `daily_checkin.continuous_days` and `daily_checkin.total_days`, plus `last_checkin_date` rather than a full history. A read-only native query confirmed distinct counters before today's claim. | Preserve the continuous-day UI. The initial inactive adapter result contradicted the user's official-client screenshot; direct SDK inspection found the new landing-card response, now supported and validated without claiming. |
| Qoder CN | Active claim-benefit campaign status and start/end timestamps; the adapter has no vendor streak or complete historical date list. | Keep account-scoped confirmed reward-window dates, with unknown gaps. Reuse WorkBuddy's layout. |
| TRAE CN | The installed client's status IPC calls `/trae/api/v2/ug/checkin_credits/status`. Its check-in handling and our adapter consume `enable`, `checked_in`, credits and extra credits. | Today's status is not a continuous count or historical calendar. Native status verification awaits an open, connected client. Do not infer a streak. |
| ZCode | Installed client 3.17.2 exposes marketing deliveries and `claim_zcode_plan` actions with a plan ID. The current read-only marketing response returned HTTP 200 and zero deliveries; no recognized check-in count/date fields were present. | This integration claims eligible activity benefits. An inactive response is not evidence of a missed daily check-in or a zero-day streak. Do not manufacture a calendar from plan claims. |

A field not consumed by an adapter is not proof that no other official endpoint could provide it. Claims above distinguish source inspection, protocol fixtures and current live observations.

## Live-operation boundaries

Only status reads were performed. The ZCode audit allowed only GET `/api/v1/marketing/touch`, retained only status, delivery count and presence of recognized check-in fields, and did not log account IDs, account labels, headers or credentials. The WorkBuddy native test uses temporary state and explicitly verifies no reward-store write. Doubao initially failed because no native client was connected, then the old broad status test passed after the user opened it; CUA displayed an inactive reward and unknown progress. That result did not establish correctness: the user showed an available official claim. A targeted read-only SDK query revealed `chosen_type=3`, a `daily_signin` landing card with interaction type 2, and campaign details indicating an unclaimed reward. The corrected helper returned the available state and separate counters against the same live session. No raw account, credential or private string data was exported. No claim, account switch, automatic-mode change, installation or application restart was performed by the assistant. The user opened Doubao through the existing connection action.

## Verification

Host: macOS 26.6.2 arm64; Rust 1.98.1; Node 20.20.2; pnpm 10.12.3. Base revision: `3f1daf658fc92a492a03b3c9ec0becc009773e3f` plus the existing working tree.

- Test-first layout regression: failed before the UI change, then passed.
- Focused calendar/Qoder/WorkBuddy renderer suite: 45 passed.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit`, `pnpm format:check`: passed; 4,100 unit tests across 307 files.
- Doubao/TRAE/Qoder/ZCode helper protocol fixtures: 29 passed.
- WorkBuddy and connected Doubao opt-in native status tests: each 1 passed. An initial short-name exact filter selected zero tests and was corrected to fully qualified test names; zero selected tests were not counted as acceptance.
- The shared Doubao helper was subsequently changed for the reproduced card-response bug. Two new protocol tests failed against the old parser; all 16 Doubao helper tests passed after the fix. Tests cover read-only status, postclaim detail refresh, already-checked state, account changes, missing details, unrelated actions and ambiguous cards. Final backend verification passed: `cargo fmt --check`, `cargo check`, `cargo clippy -- -D warnings`, and `CARGO_BUILD_JOBS=1 cargo test --manifest-path src-tauri/Cargo.toml`. The full run passed 7,691 tests across 16 result groups, with 21 existing ignored tests and zero failures. The existing compact-unwind linker warning remained nonfatal. Native claiming, Windows execution and remote CI were not performed.
- CUA verified the shared Qoder summary/date/tooltip structure in the running renderer. Native accounts and activity eligibility remain separate from UI acceptance.

Logs: `/tmp/varyai-reward-layout-*`, `/tmp/varyai-reward-audit-*`, and `/tmp/varyai-qoder-layout-*`. No external tracker fields were edited.

## Doubao correction

The prior audit interpreted the adapter's inactive response too strongly. Official-client UI and a targeted SDK read contradicted it. The legacy parser accepted only landing types 1 and 2, so type 3 was misclassified even though a daily claim card was present. The corrected parser recognizes the matching daily task/card, preserves account checks before any action, and fetches fresh details after synthetic successful claims. Unknown or ambiguous card data cannot trigger claiming. The parser change is platform-independent and leaves the existing native-connection availability unchanged; Windows native execution has not been verified. The user confirmed retaining Doubao's continuous-day UI; neither its total count nor its last check-in date is used to invent historical dates.


## Legacy compatibility

Legacy type-1 `main_page_info.buttons[].button_type` and type-2 completion responses remain separate, preserved branches. The new type-3 `landing_cards` handling is additive. Existing reward field names, saved-record schemas, account-switch checks and postclaim confirmation behavior remain intact. Legacy fixtures continue to cover current and older SDK export layouts, unclaimed/claimed/inactive responses, missing or delayed reward details, invalid counters and account changes. No local record reset or migration is required.


Final helper rerun: 32 protocol tests passed across the Doubao, TRAE and agent-rewards helpers. All 74 changed source-file hashes matched after verification; the checked working-tree manifest is `2026-11-16-reward-semantics-sources.json`. Final backend logs and command exits are in `/tmp/varyai-doubao-card-checks.json` and matching logs. Frontend sources did not change after their 4,100-test run. No staged changes, commit or push.

Final live verification had one non-accepted outcome after 10.31 seconds; its exact transient cause was not captured and is not assigned to the network or the parser. A subsequent direct read of the unchanged corrected helper returned the expected unclaimed state and separate reward counters. The compiled native status test then passed on an unchanged-source retry (1 test, 0.80 seconds). Both attempts are retained in `/tmp/varyai-doubao-card-native-final.log` and `/tmp/varyai-doubao-card-native-retry.log`. No live claim was issued; new-build manual acceptance remains outstanding.


## User-authorized check-in reproduction in the running app

The user subsequently authorized one click on the Doubao check-in button. CUA observed the button change to the pending state and then return to Check in; the inactive message and unknown progress remained unchanged. No second claim was attempted. A separate read-only check with the current helper still returned `not_checked`, credit 400, continuous days 0 and total days 3.

The running process used `target/debug/bundle/dev/Vary AI.app/Contents/MacOS/varyai`. Its embedded helper contains the legacy landing parser but not the new card branch. The separately built `target/debug/varyai` contains the new card branch. The helper is embedded through Rust `include_str!`, so renderer hot reload cannot update the running backend. This UI attempt exercised the older bundled backend and does not validate the repaired build. No visible exception was observed; the old parser misclassified the available activity as inactive. Installing or restarting the user's application was not performed. New-build UI claim verification remains pending.
