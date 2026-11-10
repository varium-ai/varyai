# Claude Desktop connection switch regression — 2026-11-10

## Cause and focused correction

The pre-existing uncommitted UI changes excluded Claude Desktop from the shared
connection switch and added an effect that automatically connected it when the
Agent list opened. This hid the user's connection control. The context-window
presentation remains unchanged: it is independent from connection enablement.

Restored the shared connection switch and removed the automatic connection
effect. Disconnect still requires confirmation and restores the previous
configuration through the existing API. Reopening the list does not reconnect a
disabled client. Model visibility, recovery actions and largest-window model
presentation are preserved. The shared React implementation applies on macOS
and Windows; no platform-specific backend changes were needed.

## Regression coverage

Tests were changed before implementation: the initial focused run reproduced the
regression with 12 failures (134 passing). After restoring the switch and updating
old automatic-connection expectations, all 146 Agent connection tests passed.
Coverage includes normal and compact layouts, connected and recovery states,
explicit connection, safe connection errors, confirmed disconnection, no
connection writes from model visibility/discovery, and staying off after remount.

## Verification scope

Host: Darwin 24.3.0 arm64 (macOS); Node.js 24.11.0, pnpm 10.12.3,
Rust/Cargo 1.98.1 stable. The Node version differs from `.node-version` (22.12.0).
Base HEAD: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing dirty-tree
work preserved. This report validates the local working tree, not a release.
No installed app was launched, restarted, replaced or reconfigured. No commit or
push was requested or performed; remote Linux/Windows/macOS and WSL2 acceptance
has not been established.

Switch-only checked snapshot SHA-256 values (before the subsequent Codex status fix):

- `src/components/agents/AgentConnectionsPage.tsx`: `b86bf7c282c2f274f041d01bc0bd25897f62c4cb90a40d74f78fd35f80e88d13`
- `tests/components/AgentConnectionsPage.test.tsx`: `d377b598c62d72c078ea584cf5738121c7219931ec648aa0d65cffd6ba74e7bd`
- `docs/architecture/shared-catalog.md`: `3dd1a9e0a3b164e364784be3dd70a80a35dd1b12eb57bf20f20334abd160dcee`

Checks completed:

- `pnpm exec vitest run tests/components/AgentConnectionsPage.test.tsx`: 146 passed.
- `pnpm typecheck`: passed.
- `pnpm build:renderer`: passed.
- `pnpm test:unit` on final test changes: 287 files, 3,694 tests passed.
  An earlier full run still contained the superseded automatic-connection test
  expectations; the final rerun passed. Existing jsdom diagnostics were emitted.
- `pnpm format:check`: passed.
- `pnpm exec prettier --check src/components/agents/AgentConnectionsPage.tsx tests/components/AgentConnectionsPage.test.tsx`: passed.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`: passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,281 passed, 19 ignored.

- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.

- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, including library,
  integration and doc-test targets: 6,444 passed, 19 ignored.
Logs for these checks use `/tmp/vary-desktop-toggle-*.log` (local temporary files).
