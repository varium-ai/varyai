# Check-in button modes — 2026-11-10

## Scope

The Agent list check-in trigger displays **Manual check-in** when automatic check-in is disabled and **Automatic check-in** when any listed account for that Agent has it enabled. This applies to WorkBuddy, Doubao Work, Qoder CN, TRAE CN and ZCode in both the main and compact views. The trigger still expands the existing panel. Successful automatic-setting mutations update the label through the shared query cache.

The list reads existing status queries for installed Agents and shares their one-minute freshness interval with the panels. All four runtime languages include the manual label. No backend or platform-specific implementation was introduced for this change.

## Verification context

- Host: macOS, Darwin arm64.
- Node v20.20.2; pnpm 10.12.3; Rust 1.98.1.
- Base revision: `c092e24eb20c4f30e44b9b3f8c5d77f78fc85098`; checks ran against the working tree, including unrelated existing edits.
- Other work concurrently changed Agent restart handling, backend bridge code and the shared Agent page test file. These edits were preserved. Whole-tree verification is limited to each command's observed snapshot and does not establish acceptance of subsequent edits.

## Results

| Check                                                                                                                               | Result                                                                                                                                                                                                                          |
| ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test-first mode regressions                                                                                                         | All 10 failed before implementation and passed afterward.                                                                                                                                                                       |
| Focused Agent page check-in tests                                                                                                   | 17 passed, including the 10 new mode cases across five Agents and two layouts.                                                                                                                                                  |
| Three check-in/reward panel suites                                                                                                  | 25 passed.                                                                                                                                                                                                                      |
| `pnpm typecheck`                                                                                                                    | Passed.                                                                                                                                                                                                                         |
| `pnpm build:renderer`                                                                                                               | Passed.                                                                                                                                                                                                                         |
| `pnpm format:check`                                                                                                                 | Passed.                                                                                                                                                                                                                         |
| `pnpm test:unit`                                                                                                                    | Initial full run: 3,615 passed and 3 unrelated TRAE repair-action failures. A temporary baseline module without the check-in change reproduced those three failures.                                                            |
| Final full frontend run                                                                                                             | 3,619 passed; 3 failures in concurrently added manual-reopen/restart cases for TRAE, WorkBuddy and Codex. Check-in cases passed.                                                                                                |
| `pnpm audit --prod`                                                                                                                 | Passed; no production vulnerabilities reported.                                                                                                                                                                                 |
| `pnpm test:dev`, `test:trae-bridge`, `test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`, `test:qoder-codec` | Passed. The optional installed Qoder codec case was skipped.                                                                                                                                                                    |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                                                  | Passed.                                                                                                                                                                                                                         |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                                                                             | 6,239 passed, 19 ignored.                                                                                                                                                                                                       |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                                                            | Failed on concurrent edits in `src-tauri/src/services/agent_connections/bridge.rs`; no Rust files were edited by this task.                                                                                                     |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                                                  | Passed.                                                                                                                                                                                                                         |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                                                                   | 6,239 library cases passed, 19 ignored; the concurrently added `pending_delivery_does_not_require_restart_after_a_manual_reopen` case failed. Cargo stopped before integration tests, so full backend acceptance is incomplete. |
| `git diff --check`                                                                                                                  | Passed.                                                                                                                                                                                                                         |

The new mode cases read saved state through mocked APIs, verify both setting transitions, exercise a mixed manual/automatic account list, and assert that opening the panel or changing the setting never claims rewards. No installed application was launched or replaced. No commit, push, CI run, installer or cross-platform acceptance is claimed.

## Combined-tree acceptance

The failures above describe intermediate concurrent snapshots. After the Agent
restart changes were completed, the combined working tree passed 3,623 frontend
tests, 6,240 Rust library tests, and 163 Rust integration tests; 19 explicit Rust
acceptance/performance tests remained ignored. Type checking, renderer build,
formatting, strict Clippy, the helper suites, storage-recovery tests, and the
production dependency audit also passed. The checked source fingerprints still
matched before the user requested committing and pushing all current changes.
See [Agent connection action verification](2026-11-10-agent-connection-actions.md)
for the final source manifest, toolchain, commands, and platform limits. Remote
CI remains separate from these local results.
