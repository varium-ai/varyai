# Qoder confirmed check-in dot

## Finding and change

The persisted Qoder claim inspected read-only ends at 09:59 UTC+8. The helper
previously required an exact 24-hour 10:00-to-10:00 campaign before returning a
confirmed reward date. That difference allowed a successful claim without a
calendar mark. No credentials or account identifiers were copied into this report.

Keep the exact official window start and accept campaign ends from 09:59 through
10:00 at the next boundary. Keep the vendor expiry, amount, claim/status behavior
and account checks unchanged. Shifted starts, earlier/later ends, longer or mixed
windows, failed claims and unconfirmed offers still cannot establish a date.
The helper is embedded in the Rust backend and requires a rebuilt backend to
take effect; renderer hot reload alone does not update it.

The shared helper applies to native/browser Qoder accounts and both manual and
automatic claims on existing supported platforms. No native credential adapter
or other Agent's reward semantics changed. The full inventory assessment is in
[the coverage ledger](agent-feature-coverage.md#qoder-reward-date-refresh-2026-11-17).

## Verification

Host: macOS 26.6.2 / arm64; Rust 1.98.1, Node 20.20.2, pnpm 10.12.3.
Base revision: `1c53f58fdaf6878c52e8e618d2b3540cffe69bd9`, with pre-existing
uncommitted tool-risk work retained. This repair changes the helper, its synthetic
regressions, renderer regressions and documentation only.

Test-first: the new last-minute daily-window regression failed before the helper
change (12 passed, 1 failed); afterward all 13 helper tests passed. Two renderer
regressions check that successful native/browser claim responses immediately
update the current dot and preserve previous confirmed dates without a refetch.

Final frontend checks passed: `pnpm typecheck`, `pnpm build:renderer`,
`pnpm format:check`, and `pnpm test:unit` (322 files, 4,280 tests). The first full
unit run observed an intermediate coverage-table formatting failure; the final
full rerun passed after restoring the machine-readable table format. Subsequent
concurrent document formatting exposed the inventory check's exact-space parser
again. Its two matching expressions now accept padded Markdown table cells while
retaining the same full-inventory equality assertions. After this final test-only
adjustment, the coverage, reward panel and calendar suites passed together
(33 tests); the 4,280-test full run above predates that parser adjustment.
Prettier checks for all four changed source/test files and `git diff --check`
passed after the adjustment.

Development, CI-policy, DMG, TRAE-bridge, model-default, Pi-session, WorkBuddy-auth,
Doubao check-in, Agent-rewards, Qoder-codec and shared-storage recovery helper
checks all passed. `pnpm audit --prod` passed. Changed helper/renderer test files
passed Prettier; `git diff --check` passed.

`cargo check` and `cargo clippy -- -D warnings` passed. Backend checks use
`CARGO_BUILD_JOBS=1`, `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0`, matching CI's debug-information policy. An initial
default-debug compilation was stopped and restarted with those CI profiles.
Final `cargo fmt --check` passed. The first formatting run observed unrelated
concurrent lifecycle edits before they were formatted; no Rust source was edited
for this repair.

`cargo test --lib` completed with 7,687 passed, one failed and 21 ignored. All
five Qoder reward/native-helper regressions passed. The failure is
`services::agent_connections::projections::tests::deepseek_desktop_projection_is_readable_reversible_and_keeps_foreign_edits`,
which returned `agentConnections.conflict` in the existing, separately modified
Harness projection/session-helper path. It does not exercise the Qoder campaign
adapter. That unrelated implementation and test were left intact.
The complete `cargo test` command reproduced the same failure (7,687 passed,
one failed, 21 ignored). Cargo stopped after the library failure, so integration
and documentation test execution remains incomplete. No unrelated test was
skipped or weakened, and no overall backend acceptance is claimed.

Logs and command summaries are under `/tmp/varyai-qoder-dot-*`. The repair source/test hashes are recorded in
`/tmp/varyai-qoder-dot-source-hashes.json`. The helper and reward-panel test hashes
matched their verified versions at final review; the final inventory-parser test
hash was added after its successful focused rerun.

## Native acceptance limits

Read-only UI inspection found the development app running with protected Qoder
credentials and unknown dates. No credential prompt, reward claim, application
restart, installation or route change was performed. Live confirmation with the
rebuilt backend and a readable account remains pending. Windows/Linux native
acceptance and remote CI were not performed. Synthetic checks do not establish
cross-platform native acceptance.
