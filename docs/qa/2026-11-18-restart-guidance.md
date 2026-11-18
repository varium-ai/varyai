# Desktop restart guidance verification — 2026-11-18

Superseded by [restart feedback and update verification](2026-11-18-agent-feedback-and-updates.md)
following the user's request for Magpie-style footer capsules.

The Agents page presents all desktop restart outcomes that require manual action
as neutral information. An app that remains open gets a suggestion to check its
window for a confirmation. Failed or unsupported automatic restart asks the user
to save work, fully quit and reopen the named app. Main and compact/tray views use
a muted information icon and polite status; restart notifications use information
toasts. Other operation errors retain their existing error presentation. English,
Simplified Chinese, Japanese and Traditional Chinese are updated.

No native restart, process termination, platform adapter, configuration write,
protocol conversion, request or response handling was changed. The per-Agent
matrix was recorded before implementation in the [coverage ledger](agent-feature-coverage.md#desktop-restart-guidance-presentation).
Existing unrelated working-tree changes were preserved. No commit or push was made.

## Checked tree and environment

- Base revision: `eb24a0e3b947ba89bd9aa668148e23b0e9bbe59c`, with existing uncommitted work.
- Host: macOS Darwin arm64. Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.
  Node differs from the repository's `.node-version` (22.12.0); pinned-Node CI
  remains unverified.
- SHA-256 of the ordered changed frontend/test paths below, encoded as
  path + NUL + contents + NUL: `f9fb3da8196c9480a8ed825755725e7a8e563e461a5dd62f8aa9b8792bd5d847`.
  `src/components/agents/AgentConnectionsPage.tsx`, `src/i18n/locales/en.json`,
  `src/i18n/locales/zh.json`, `src/i18n/locales/ja.json`,
  `src/i18n/locales/zh-TW.json`, `tests/components/AgentConnectionsPage.test.tsx`.
- Tests were written first: the initial presentation regression run failed
  45 cases as expected; the subsequent manual-restart regression failed three
  cases before the final informational presentation was implemented.

## Local checks

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed on final frontend changes |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | Passed: 324 files, 4,456 tests |
| Focused component, Agent inventory and locale suites | Passed: 3 files, 393 tests |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| Development, CI policy, macOS DMG, TRAE bridge, model-default, Pi session, WorkBuddy auth, Doubao check-in, Agent rewards and Qoder codec helper suites | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed with `CARGO_INCREMENTAL=0`: 7,732 passed, 21 ignored. Initial default run exhausted disk while writing its incremental query cache |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Could not complete with `CARGO_INCREMENTAL=0`: archive generation for `libcc_switch_lib.a` failed with no space left on device. Full integration acceptance remains incomplete |

## Presentation and native acceptance

Synthetic component tests cover each of the eleven desktop restart registrations,
including domestic/international editions, macOS/Windows user-agent branches,
main and compact views, enabled and restored connections, failed/unsupported
results, malformed input, private error sanitization and an explicit retry.
The existing suite retains CLI/session paths and other operation errors. These
are UI regressions; they do not establish native process acceptance.

An isolated browser preview used the actual React component and application CSS
with synthetic API results. Light main view and dark compact view were inspected:
the message is muted, the information icon aligns with the first line, the app
name resolves and long text wraps. Both app-still-open and failed restart results
were exercised. The preview never called a real restart or touched native Agent
configuration. Screenshot artifacts are outside the repository.

Installed-client restart tests, Linux/Windows native runs, cross-platform CI and
release acceptance were not performed. Native acceptance remains unverified for
every Agent; this change makes no new native-support claim.
