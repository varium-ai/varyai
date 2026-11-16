# WorkBuddy campaign totals, calendar history and 5.11.3 login compatibility

Date: 2026-11-16. Host: macOS 26.6.2 / arm64. Toolchain: Node 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1. Base revision: `3f1daf658fc92a492a03b3c9ec0becc009773e3f`, plus the 17 task files recorded in `/tmp/varyai-workbuddy-final-source.json`. This checkout also contains unrelated pre-existing work; it was preserved.

## Behavior

WorkBuddy labels the vendor's `streak_days` as a campaign total. Only validated `checkin_dates` determine the last seven UTC+8 calendar dates, with today at the right. Gaps stay unmarked. Missing history, including legacy local records, remains unknown. Doubao Work and other reward adapters retain their current progress behavior. All four runtime languages include the new labels.

Account refresh performs status-only requests. Unclaimed observations do not change automatic-claim retry records; confirmed results can save the vendor calendar. A delayed status cannot erase a confirmed same-day receipt. A successful claim performs an additional status query for dates, without turning missing calendar details into a failed claim. Account keys and legacy storage fields remain compatible.

The native helper supports older `main/dist.js` exports and the 5.11.3 `main/src.js` exports. macOS and Windows resource paths and runtime discovery are covered; the runtime's Node fuse is checked. Credentials remain in memory and are not written to reports. The actual 5.11.3 helper successfully read the current native session without changing its file hash. The opt-in Rust live test queried the official status endpoint without claiming and passed.

## Verification

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm test:unit`: 4,094 tests passed across 307 files.
- Development launcher, macOS packaging and Qoder codec helpers: 16 passed.
- TRAE bridge, model-default and check-in/reward helpers: 106 passed; the final WorkBuddy auth helper rerun passed all 8 tests.
- Shared recovery Python helper: 5 passed. `pnpm audit --prod`: no known vulnerabilities.
- Final WorkBuddy Rust regression run: 25 passed, 1 opt-in live test skipped by default. The live test was then explicitly run and passed (1 test).
- Final `cargo check`, `cargo fmt --check` and `cargo clippy -- -D warnings`: passed. An earlier formatting check ran during edits and failed; final formatting passed after formatting only task files.
- `cargo test --lib`: 7,522 passed, 21 ignored, no failures. This preceded the final extra postclaim test/fixture update; those final files passed the focused rerun and are covered by the full Cargo result below.
- Wide, narrow and dark component previews were visually inspected using synthetic, nonconsecutive dates. No desktop application was installed, replaced or restarted. No live claim was performed.

`cargo test --manifest-path src-tauri/Cargo.toml`: passed on the final implementation, 7,687 tests passed and 21 ignored across 16 suites (including 7,522 library tests and 165 integration tests). No failures. The live status test was run explicitly afterward and passed again.

Logs and command exit codes are stored at `/tmp/varyai-workbuddy-checks.json`, `/tmp/varyai-workbuddy-rust-checks.json` and `/tmp/varyai-workbuddy-final-checks.json`, with matching command logs. Source hashes were checked again after verification. The macOS test linker emitted an unwind-table-size warning; compilation and tests continued. Remote Linux/Windows/macOS CI, Windows native execution, packaging, signing and installer acceptance were not run. No commit or push was requested.
