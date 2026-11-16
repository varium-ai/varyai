# Qoder refresh and reward-calendar follow-up

## Scope

The user requested that Qoder Refresh invoke Read client login when that action is offered, and selected keeping seven dates with only confirmed dates marked. This addresses current tracker row 10, **Signed-in Qoder CN account cannot refresh**, and an additional unnumbered calendar report. The browser tracker was not modified. No commit, push, installation or application restart was performed.

## Findings and implementation

- The running UI displayed a confirmed reward while passing a constant null streak to its seven-step component. The Qoder adapter reads campaign claim status and boundaries; it does not supply a continuous streak or a complete historical calendar.
- Refresh now invokes the existing explicit native-login read when its button is available. Background polling still reads status without requesting Keychain access. Ordinary refresh for an already readable native account is unchanged; neither path requests a reward claim or enables automation.
- The Qoder panel shows seven reward-window dates, not consecutive-day counts. Per the [official CN campaign](https://docs.qoder.cn/events/100credits), windows begin at 10:00 UTC+8. A date names the window's start, not an inferred claim timestamp.
- Only confirmed claims with matching official 24-hour daily boundaries supply a date. Longer or shifted campaigns, failed claims, and observation timestamps do not establish a date. Confirmed dates are retained locally by account for seven reward days, including confirmations obtained by status refresh, explicit native reads, manual claims and automatic claims. Older stores remain readable without fabricating history. Missing identity never borrows another account's calendar.
- All unconfirmed cells remain unknown. WorkBuddy's official date history and campaign total semantics remain unchanged. The helper, storage and renderer calendar behavior is shared across platforms; native-login availability continues to use the existing platform support.

## Verification

Host: macOS 26.6.2, arm64; Rust 1.98.1; Node 20.20.2; pnpm 10.12.3. Working tree based on `3f1daf658fc92a492a03b3c9ec0becc009773e3f`, including pre-existing uncommitted repairs.

Test-first evidence: the new UI tests failed (3 failures, 20 passes); the new helper date test failed (1 failure, 10 passes); the new Rust tests initially failed to compile against the absent calendar contract. After implementation, focused UI tests passed (29 tests) and helper tests passed (11 tests).

Two Rust compilation attempts exhausted disk space while writing generated artifacts; the concurrent frontend run was also interrupted during storage exhaustion. Failed working caches and superseded compiler caches were removed while retaining source, dependency artifacts and app bundles. A single-job retry was interrupted to remove orphaned library-test object files before storage could fill again. The final Rust retry uses `CARGO_BUILD_JOBS=1` to limit simultaneous compiler output and completed with approximately 9 GiB free. Frontend checks were restarted against the final source tree after storage recovery. Final verification results follow below.

CUA inspection confirmed the running development renderer now displays seven date labels and the explanatory text. It still used the old backend, so no live confirmed-date acceptance is claimed. A subsequent visual inspection was limited to the visible portion of the panel; further scrolling became unavailable when the app had no accessible window. No account settings were changed and no reward was claimed during inspection.

## Final local checks

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit`, and `pnpm format:check`: passed. Unit suite: 4,099 tests across 307 files.
- `node --test src-tauri/resources/agent-rewards.node.cjs`: 11 passed. Focused renderer regression tests: 29 passed.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, `cargo check --manifest-path src-tauri/Cargo.toml`, and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed against the final Rust source.
- `CARGO_BUILD_JOBS=1 cargo test --manifest-path src-tauri/Cargo.toml`: passed; 7,691 tests passed across 16 result groups, 21 existing ignored tests, zero failures. This includes the complete library suite (7,526 passed) and the three new Qoder calendar/storage regressions. The linker emitted a large `__eh_frame` compact-unwind warning; it did not fail the run.
- All 72 changed source-file hashes matched after verification. Source hashes for the checked working tree are recorded in `2026-11-16-qoder-calendar-sources.json`. No staged changes, commit or push.
- Temporary command logs use `/tmp/varyai-qoder-calendar-*`; successful frontend checks are summarized in `/tmp/varyai-qoder-calendar-final-frontend.json`. The final Rust run uses `/tmp/varyai-qoder-calendar-final-rust.log`.

## Manual acceptance with the updated backend

1. With a detected but protected Qoder native login, open Manage accounts and click Refresh. Handle any system permission prompt; the current native account should appear. Background refresh must not open the permission prompt.
2. For an already claimed daily campaign, refresh status. Only its reward-window start date should be marked. No additional claim request is required.
3. Reopen VaryAI and refresh the same account. Confirmed dates should remain visible. Switch the native account and refresh: the second account must not inherit the first account's dates.
4. Across 10:00 UTC+8, the new reward date stays unknown until confirmed. Missing prior dates remain unknown; they are not shown as missed check-ins.

Installed-client acceptance and Windows/Linux CI remain outstanding. These results do not establish packaging or release acceptance.
