# Removed routing group restoration

## Scope

Checked base revision: `f64d5015be1622bc52740b67ae68c39b0691cac7`, plus the
working-tree changes in `src-tauri/src/database/dao/intent_routing.rs`, its
`intent_routing_recovery_tests.rs`, the restoration cases in
`tests/components/IntentRoutingPanel.test.tsx`, and the routing guide.
Concurrent routing drag changes in this checkout were preserved; broad checks
also include that work. This task did not launch, install or replace the application.

Host: Darwin 24.3.0 arm64. Toolchain: Node v24.11.0, pnpm 10.12.3,
rustc 1.98.1 (48a229cea 2026-09-01).
The Node runtime used here differs from the repository's `.node-version`
(`22.12.0`); these local results do not validate that CI runtime.

## Reproduction and behavior

With automatic discovery disabled, restoring an untouched automatic group used
to remove its tombstone and return success without adding a visible group.
Restoration now saves the discovered definition through normal validation when
discovery is off. It leaves the preference unchanged. With discovery on, it
clears canonical and legacy deletion markers. Missing matching providers and
capacity failures retain the deletion record and return a recognized error.
Saved-definition archives retain their existing transactional restore path.

Before the implementation change, three new Rust regressions failed: restoration
with discovery off, legacy restoration with discovery off, and unavailable-group
restoration retaining its record. After the change, all six recovery tests pass,
including existing saved-archive tests and the added capacity case. Fixtures use
in-memory databases and synthetic providers.

## Verification

- `cargo test --lib --manifest-path src-tauri/Cargo.toml database::dao::intent_routing::recovery_tests`: passed, 6 tests.
- `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx -t 'after restoring|after restoration fails'`: passed, 5 tests.
- After the last concurrent UI edits, `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx`: passed, 111 tests. The affected type check, renderer build and formatting checks were also rerun successfully. `pnpm exec vitest run tests/hooks/useRoutingRowDrag.test.tsx`: passed, 14 tests.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm exec prettier --check tests/components/IntentRoutingPanel.test.tsx`: passed.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: passed, 7471 passed and 21 ignored.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `pnpm audit --prod`: passed, no known vulnerabilities.
- Most recent complete `pnpm test:unit` after cache cleanup: passed, 304 files and 4058 tests; the per-file checks above additionally cover the last concurrent routing UI edits.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`, `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards` and `pnpm test:qoder-codec`: passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed, 5 tests.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, 16 test targets, 7636 passed and 21 ignored.

The sandbox initially blocked helper-test loopback listeners and npm DNS. Those
checks passed after rerunning with the necessary access. Concurrent Cargo builds
and low disk space delayed broad verification. The first full Rust invocation
failed to write metadata with `ENOSPC`. Obsolete completed incremental generations
were removed while holding Cargo's build lock. On retry, the shared incremental
directory disappeared during compilation, producing an object-copy error; the
full suite was restarted after that cleanup and completed successfully. The initial full renderer suite
reported one failure in the concurrently edited routing drag test; the final
suite passed after that work settled. An intermediate renderer rerun passed
303 files and 4042 tests but could not load `SettingsDialog.test.tsx` because of
`ENOSPC`; its separate rerun passed 16 tests, followed by the successful complete
rerun above. The Rust linker emitted a compact-unwind size warning.

Submission includes the routing restoration, drag handoff and localized order-save
feedback changes together. The latest `main` revision at submission preparation,
`bc067cc73`, has the same Git tree as the checked base above. Source files were
compared with the tested inputs before committing; only these verification reports
were subsequently updated. The complete local Rust suite also supersedes the
earlier incomplete attempts described in the drag report.

Linux, Windows, Windows-to-WSL2 and the repository-pinned Node runtime require remote
CI verification for the pushed revision. Local results do not establish
cross-platform or release acceptance.

## CI toolchain follow-up

The initial PR revision, `238cbd0e75883aa71c2c2a4120b4ed1c52bbd69f`, passed
the frontend and Windows-to-WSL2 CI checks. Linux, Windows and macOS backend logs all reported
`clippy::needless_bool` in the existing thinking-budget error detector at
`src-tauri/src/proxy/thinking_budget_rectifier.rs:68`. CI installed Rust 1.100.0,
while the local checks above used Rust 1.98.1.

The detector now returns its existing three-condition boolean expression directly.
Its switches, error matching, request changes and retry behavior are preserved.
All 14 existing thinking-budget tests passed after this mechanical change. Before
the follow-up commit, the same macOS arm64 host with Rust 1.98.1 passed:

- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7471 passed, 21 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 7636 passed, 21 ignored across 16 targets.

These checks cover revision `238cbd0e` plus the boolean-expression simplification
and this report update. CI on Rust 1.100.0 must validate the follow-up revision on
all required platforms. No warning policy or test assertions were relaxed.
