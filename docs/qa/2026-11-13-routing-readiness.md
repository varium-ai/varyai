# Routing group readiness feedback

Date: November 13, 2026 (Asia/Shanghai).

## Scope

The group list now reports **No member ready** only when none of the group's
participating members is locally available. It includes resolved pattern members,
excludes switched-off members and checks only the selected/default member in
manual mode. A provider catalog still loading does not produce a warning.
Partially available groups no longer receive a group-level warning or a generic
configuration warning in their compact summary.

The editor retains saved unavailable members and explains whether their provider
is missing, their provider is not ready for routing, or their model is absent
from the provider's current catalog. An omitted catalog retains the existing
unknown-catalog behavior; an explicitly empty catalog has no available models.
These checks do not probe live credentials, quota or remote availability. Request
forwarding, persistence and fallback semantics are unchanged. The shared React
implementation covers macOS and Windows; all four runtime languages are updated.

Magpie reference: local `yetone/magpie` checkout at
`c5b9e448a8bc9bc2a1bf6704e5d8bb3384d084a6`, specifically
`internal/gui/assets/plugins/routing/client.js` (`groupRow`, `pickRow`),
`internal/gui/groups.go` (`groupJSON.Ready`) and `internal/provider/group.go`
(`membersFound`). This change follows its group-level readiness distinction,
not a claim of complete behavioral parity or a new backend health check.

## Regression evidence

The first regression run failed 12 assertions against the old implementation.
After implementation, 119 focused routing tests passed across
`IntentRoutingPanel`, `RoutingMemberPicker` and `routingMemberSelection`.
Coverage includes partially available groups, empty groups, all members switched
off, no resolvable provider, manual targets, pattern targets, catalog loading,
missing provider/model explanations and retaining/removing unavailable members.
A later combined rerun including the concurrently edited Agent page passed all
346 tests across four files.

## Verification context

Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b`, plus the working-tree
changes for this task. Host: macOS, Darwin arm64; Node 24.11.0, pnpm 10.12.3,
rustc/cargo 1.98.1. Command logs and a SHA-256 manifest of the changed routing
sources, tests, translations and guide are in
`/private/tmp/vary-routing-readiness-checks/`. Concurrent Agent page and Codex
source-label changes were preserved and are not part of this task's write set.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed on rerun; the first run caught an unused variable in the concurrently edited Agent test. |
| `pnpm format:check` | Passed. |
| `pnpm build:renderer` | Passed. |
| Focused routing tests | Passed: 119 tests in three files; the later combined Agent/routing rerun passed 346 tests in four files. |
| `pnpm test:unit` | Incomplete acceptance: the first run passed 4,088 tests with one Agent test failure; that file subsequently passed its focused rerun. The full rerun passed 4,073 tests in 305 files, but `SettingsDialog.test.tsx` could not load because of `ENOSPC`. |
| `pnpm audit --prod` | Passed with network approval; no known production vulnerabilities. The sandbox attempt could not resolve the registry. |
| `pnpm test:dev` | Passed with approval for temporary loopback listeners; the sandbox attempt failed with `listen EPERM`. |
| `pnpm test:macos-dmg`, `pnpm test:trae-bridge`, `pnpm test:model-default` | Passed. |
| `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec` | Passed. |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed. |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed. |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed for the then-current backend tree. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed for the then-current backend tree. |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Not passed: after approval, 7,482 passed, six failed and 21 were ignored. The sandbox run had additional listener failures. |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Not completed: the initial run failed building an archive with `ENOSPC`; the approved retry was stopped while waiting for the build lock to avoid further writes to the nearly full disk. |
| `git diff --check` | Passed for the final routing changes. |

The six approved Rust library failures were investigated without changing
unrelated code: two assertions expected model labels without the provider suffix
being introduced by the concurrent Codex source-label work; two skill archive
tests explicitly reported `No space left on device`; one certificate fixture
reported generation exit status 1; and one sync-publication fixture failed to
reach its expected live extension database. The latter two causes were not
established. The host data volume had about 209 MiB free when checked. No user
files were deleted, no assertions were weakened and no application was launched,
installed or replaced.

The routing files in the verification manifest remained unchanged through final
review. Other chats continued editing backend files during verification, so the
earlier backend compilation/lint results do not validate those later changes.
No commit or push was made for this task. Remote CI and Linux/Windows backend
acceptance were not run; overall verification remains incomplete pending disk
space recovery and resolution/retesting of the unrelated backend failures.
