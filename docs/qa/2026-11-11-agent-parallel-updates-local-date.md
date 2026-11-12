# Parallel Agent updates and local check-in dates

Validated on macOS (Darwin, Apple Silicon), 2026-11-11. Node 20.20.2,
pnpm 10.12.3, rustc 1.98.1, cargo 1.98.1.

## Scope

Working-tree changes in `useAgentUpdates.ts`, `AgentUpdates.tsx`, the WorkBuddy
and Doubao Work check-in panels, their tests, four runtime locales, and the
Agent update/daily check-in guides. During verification, no commit, push,
installation or application launch was performed for this change. Other work advanced HEAD independently
from `e2e37b2d` through `2b748a9c` to `a17b92de` during verification; these results
are working-tree observations, not acceptance of an immutable revision.

Individual update clicks reserve only their Agent, including during preflight.
Concurrent tasks retain progress and results; cancelling, confirming or failing
one preflight does not release another running task. Duplicate starts for the
same Agent remain blocked. Shared package-manager writer coordination is unchanged.
Check-in panels use the computer's local date formatter. Vendor daily record
expiry and campaign eligibility rules remain unchanged.

## Results

- `pnpm exec vitest run tests/components/AgentUpdates.test.tsx tests/components/WorkBuddyCheckinPanel.test.tsx tests/components/DoubaoWorkCheckinPanel.test.tsx`: passed, 77 tests across 3 files.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed after the local-date implementation.
- Prettier check of all changed TS/TSX files and `git diff --check`: passed.
- `pnpm test:unit`: 3,842 passed, 6 failed in `AgentConnectionsPage.test.tsx`, concerning WorkBuddy, WorkBuddy AI and Qoder model-control consent/default model saving. The full suite is not green; no unrelated implementation or assertions were changed to suppress these failures.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec`: passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed, 5 tests.
- `pnpm audit --prod`: passed.
- `pnpm test:trae-bridge`: failed, 3 of 58 tests, reproduced on a second run. Failures concern resident bridge replacement, native model-cache refresh and readable-label migration. These bridge files were outside this change.
- `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: incomplete; observed failures in proxy intent global-route/model tests. The extended backend run was stopped for manual testing handoff.
- Clippy and the complete Rust integration suite were not run to completion. Cross-platform CI and native manual acceptance were not performed. Verification remains incomplete.

An earlier unit run overlapped test edits and was superseded by the final unit
run above. Its failures are not reported as final results. Raw logs remain in
`/tmp/varyai-parallel-*`; they are not repository artifacts.

The user subsequently approved committing all remaining work, including other
sessions' changes. The recorded failures and incomplete checks were disclosed
before submission. Tests and CI were not rerun, as requested; submission does
not establish a passing full suite or cross-platform acceptance.
