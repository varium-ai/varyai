# Automatic check-in mode after reopening

Date: 2026-11-10. Host: macOS / Darwin 25.6.0 arm64. Source baseline:
`5ba154ca1991b19d926b451d43ae73a5e896719c`, plus this task's working-tree changes.
Toolchain: Node 22.22.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

## Observed failure

After reopening Vary, Qoder CN, TRAE CN, and Doubao Work displayed Manual check-in,
while ZCode displayed Automatic check-in. The Qoder panel had no available account,
reported protected native credentials, and showed an unchecked, disabled switch.
Read-only inspection of the local preference stores confirmed enabled automatic
preferences for all four Agents. No account identifiers, credentials, or native
configuration were copied into this report or changed during the investigation.

The status APIs returned an account-specific `autoEnabled` flag, which becomes
false when the current account cannot be observed. The homepage interpreted this
as a disabled saved preference; Doubao and WorkBuddy similarly depended on the
currently observable account list. Loading or unavailable queries also fell back
to the manual label. These observations did not establish a persistence failure.

## Change

Reward, Doubao, and WorkBuddy snapshots now expose `autoConfigured`, computed from
their persisted account opt-ins independently of native account availability.
The homepage uses that summary for the mode label. No available status means the
neutral Check-in label. Panels without an available account show the saved mode
with the switch disabled, while connection and credential outcomes remain
separate. WorkBuddy mutations propagate the updated summary through its shared
account-list query cache. Optional TypeScript fields retain compatibility with
older status snapshots.

Per-account `autoEnabled`, account identity checks, automatic worker eligibility,
and preference file formats are unchanged. An unavailable old account's opt-in
does not enable claims for a newly observed account. Preferences are not rewritten
to repair presentation. The code is shared across macOS and Windows.

## Regression evidence

- Five homepage tests failed before the fix when a new QueryClient observed an
  unavailable account with a saved automatic preference. They now check both the
  trigger and checked, disabled panel switch across two fresh mounts. Separate
  pending-query and failed-query tests reproduced the false manual label.
- Three Rust tests failed at runtime before implementation, after correcting a
  test-only private-field access. They write opt-ins to temporary files, recreate
  the managers, and inspect public serialized status without a native account.
  They now pass. Reward coverage also verifies Agent isolation and that another
  account does not inherit the saved account's opt-in.
- Existing mode-transition tests now include the saved summary, covering cache
  changes for all five Agents and both page layouts. The four focused component
  suites pass all 190 tests. No real claim or authentication interaction was used
  to verify the fix.

## Verification

Checked source hashes are in
`/private/tmp/vary-checkin-persistence-source-sha256.json`; logs are under
`/private/tmp/vary-checkin-persistence-*.log`.

- Type checking, renderer production build, formatting, Rust formatting/check,
  and Clippy with warnings denied passed.
- Production dependency audit reported no known vulnerabilities.
- Development, TRAE bridge, WorkBuddy auth, Doubao check-in, Agent rewards,
  Qoder codec, and Python storage-recovery helpers passed.
- Complete frontend suite: 281 files, 3,632 tests passed.
- Final `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,243 passed,
  19 existing ignored tests, zero failures.
- Final `cargo test --manifest-path src-tauri/Cargo.toml`: 6,406 passed across
  library and integration suites, 19 existing ignored tests, zero failures.
- An initial library run failed the unchanged Qoder TLS test's immediate
  post-stop port rebind with `AddrInUse`. Its isolated rerun passed, followed by
  the complete passing runs above. Neither the listener code nor its assertions
  were changed. This evidence does not establish the source of the port conflict.
- Extra Prettier coverage for the component test and `git diff --check` passed.
  The existing unused test-fixture variable and macOS compact-unwind linker
  warning remained nonfatal; production Clippy passed with warnings denied.

The existing running native backend was not replaced or relaunched. Loading the
new status fields requires rebuilding and normally restarting Vary. Remote
cross-platform CI, packaging, and live post-restart acceptance are not established
by these local synthetic checks.
