# More tools agent icons

Date: 2026-11-10. Host: macOS / Darwin 25.6.0 arm64. Base revision:
`5ba154ca1991b19d926b451d43ae73a5e896719c`, with unrelated check-in changes
preserved in the shared working tree.

## Behavior

The home page's More tools list now renders each agent's existing
`APP_ICON_NAME` asset through `ProviderIcon`, matching the installed agent list's
32 px icons. Compact tray rows retain their 22 px icons. The home installation
row aligns the icon, name, and version horizontally.

Regression coverage checks the actual image URL or inline SVG and dimensions
for all 18 agents in both layouts. The home case reproduced the missing icon
before the fix. The running development app was inspected without launching,
restarting, installing, or changing any agent connection: the WorkBuddy China
entry displayed its icon with the expected alignment in the expanded More tools
list.

## Verification

Commands ran from the repository root. Node 20.20.2 was used for TypeScript,
Vitest, and formatting; Node 22.22.0 was used for the renderer build, helper
tests, and production dependency audit. pnpm: 10.12.3. Rust/Cargo: 1.98.1.
Logs: `/tmp/vary-more-tools-*.log`.
Unrelated bridge and documentation edits continued during verification; the
source hashes below identify this update's checked files. These results do not
establish acceptance of ongoing work elsewhere in the shared tree.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 281 files and 3,634 tests passed |
| `pnpm format:check` | Passed |
| Prettier check for the changed test file | Passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `pnpm test:dev` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Failed: TRAE reconnection fixture abort |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Failed in the same library fixture; integration tests did not run |

Both Rust test commands aborted in
`services::proxy::tests::trae_reconnection_delivers_latest_offline_selection_without_enabling_audit`.
The fixture's worker panicked at `src-tauri/src/services/proxy.rs:6332` with
`configuration sync must not enable auditing`, followed by poisoned-lock and
destructor panics. Running that test alone with `--exact` reproduced the abort
(log: `/tmp/vary-more-tools-trae-reconnection.log`). No Rust implementation or
fixtures were changed for this icon update. The shared tree already contained a
TRAE helper revision change from 10 to 11, while this fixture advertises revision
10; that discrepancy is recorded for separate investigation. Backend test
acceptance remains incomplete.

Checked source SHA-256 values:

```text
c1f6577a8b551cf7f1375d5303d8ca899cbab87a6dc38b27ca8a75963793103b  src/components/agents/AgentConnectionsPage.tsx
d390d364a1ac5b8ec96391dce62aa2db002a1030a3228610a6f39f7d0ef4ff9f  src/components/layout/workspace.css
5a95dfc2d6599077df77c7146f3b8e2bc92efd560210fc3bbcd3e4af3507ff5b  tests/components/AgentConnectionsPage.test.tsx
```

No commit or push was requested. Remote CI, Windows/Linux runtime appearance,
packaging, signing, and installation were not verified.

## Combined-tree acceptance

The failures above record the intermediate icon-verification snapshot. After both
revision-10 TRAE fixtures were updated to revision 11 without weakening their
assertions, the combined tree passed 3,634 frontend tests and all 6,244 Rust
library plus 163 integration tests; 19 existing Rust tests remained ignored.
See [TRAE and Qoder runtime verification](2026-11-10-trae-qoder-runtime.md) for the
final commands, source inventory, and live/platform acceptance limits. The user
subsequently requested committing and pushing the completed working-tree changes.
