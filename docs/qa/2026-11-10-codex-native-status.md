# Codex connection status and official-login compatibility — 2026-11-10

## Report and cause

The user reported an Agent row showing “Model menu configured · native connection
unavailable” while their selected shared Codex model was in use. The screenshot
identifies the displayed warning, but is not evidence of the precise underlying
native status or account configuration. No live credentials, client files or
running app configuration were inspected or changed for this fix.

The UI derived that warning from `nativeCompatibility` values `unsupported`,
`revoked`, `reconnect_required` and `recovery_pending`. It also offered Reconnect
for those values even when `enabled` and `configured` were true and the managed
connection had no recovery requirement. This conflated the separate official-login
compatibility channel with the selected shared model route. For example, native
activation can be unsupported when official sign-in is absent while the managed
provider menu remains configured; the screenshot alone does not prove this was
that user's exact native activation result.

## Correction

The Agent row now derives its main status from managed connection ownership and
configuration, gateway state, reload state and route-specific proxy diagnostics.
Native compatibility alone neither declares the selected route unavailable nor
offers an unnecessary configuration rewrite. Actual configuration recovery still
shows its warning and Reconnect action. The backend's authentication, compatibility
checks and forwarding behavior are untouched. A configured status is not proof
of upstream inference success. The same React code serves macOS and Windows.

The Claude Desktop connection-switch fix from the preceding request is retained.

## Regression verification

Tests changed first: the old implementation failed all four native-warning cases
(145 tests passed). The focused suite now passes all 149 tests. Coverage includes
all seven native compatibility values plus actual missing-provider, profile-
override and missing-catalog recovery with unsupported native compatibility.
The latter cases still retain the configuration warning and Reconnect control.
Existing gateway/proxy/reload tests and Desktop switch coverage remain intact.

Host: Darwin 24.3.0 arm64; Node.js 24.11.0 (repository pin: 22.12.0), pnpm 10.12.3,
Rust/Cargo 1.98.1 stable. Base HEAD:
`23a4b2be4b582f892691f5c7feb5fe88e79e6b42` plus existing working-tree changes.
No commit, push or installation was requested or performed. Remote cross-platform
CI and installed-app acceptance have not been established.

Checked source SHA-256 values:

- `src/components/agents/AgentConnectionsPage.tsx`: `e48d9c8c60b2b3ba8cd1e936265d9724a190143bf216fb8a3f1c4438adeba52f`
- `tests/components/AgentConnectionsPage.test.tsx`: `bcfc730f4da9ffe8554d7cd0a34a729b59494fa6484ed81b993f8af6db79a2b1`
- `docs/architecture/shared-catalog.md`: `866727721979ed84c6a0b06a963e4d4311962a86946d5c3772bcfeef079bdb57`

- `pnpm exec vitest run tests/components/AgentConnectionsPage.test.tsx`: 149 passed.
- `pnpm typecheck`: passed.
- `pnpm build:renderer`: passed.
- `pnpm test:unit`: 287 test files and 3,700 tests passed. Existing jsdom
  diagnostics were emitted; there were no failed tests.
- `pnpm format:check`: passed.
- `pnpm exec prettier --check tests/components/AgentConnectionsPage.test.tsx`: passed.
- `git diff --check` for the touched source, tests and documentation: passed.
- Backend, helper and dependency-audit results from the preceding switch fix
  remain applicable: this follow-up changes only React, its tests and documentation,
  not Rust, helpers or dependency manifests. See
  [preceding verification](2026-11-10-desktop-connection-switch.md).

Local temporary logs: `/tmp/vary-codex-native-status-*.log`.
