# Codex semantic refresh verification

## Scope and host

- Date: 2026-11-13 (Asia/Shanghai).
- Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b` plus uncommitted working-tree changes.
- Host: macOS 15.3.1, arm64.
- Toolchain: rustc 1.98.1, Cargo 1.98.1, Node.js 24.11.0, pnpm 10.12.3.
- Magpie reference: `6d15f63d52146fcadccef30c9777e45051bcd11c`.
- Changed behavior: semantic route/menu freshness receipts, metadata/order-insensitive
  menu comparison, app-server identity precedence over window age, and manual
  idle-safe app-server refresh instead of a full application restart.
- The shared implementation targets macOS and Windows. Full terminal/IDE/embedded
  client and independent daemon classification is not included.
- Existing unrelated provider/routing, branding and documentation edits were
  preserved. No commit, push, installation or running-application restart was made.

## Regression evidence

Before implementation, the new catalog metadata/order test failed against the
original byte-sensitive digest. After implementation, the semantic persistence,
visibility-change, route-change, current-server status, manual idle refresh and
unknown-startup tests passed in the compiled Rust library test run. An unknown
startup baseline only offers a manual refresh and never ends a process on its own.

The frontend connection tests verify that a current server has no restart action,
while a stale server retains the configuration/menu refresh explanation rather
than being promoted to a generic connection-not-effective warning.

## Local checks

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed. |
| `pnpm build:renderer` | Passed; repeated after the concurrent stylesheet edit. |
| `pnpm test:unit` | Passed: 307 files, 4,098 tests. |
| Focused connection and brand tests | Passed: 236 tests after the concurrent stylesheet edit. |
| Concurrent text-selection tests added after the full frontend run | Passed separately: 12 tests. |
| `pnpm format:check` | Passed. |
| `pnpm test:dev` | Passed after granting temporary local listener access. |
| `pnpm test:macos-dmg` | Passed; helper tests only, no packaging/install acceptance. |
| `pnpm test:trae-bridge` | Passed. |
| `pnpm test:model-default` | Passed. |
| `pnpm test:workbuddy-checkin-auth` | Passed. |
| `pnpm test:doubao-checkin` | Passed. |
| `pnpm test:agent-rewards` | Passed. |
| `pnpm test:qoder-codec` | Passed. |
| `pnpm audit --prod` | Passed: no known vulnerabilities. |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed. |
| `CARGO_INCREMENTAL=0 cargo check --manifest-path src-tauri/Cargo.toml` | Passed. |
| `CARGO_INCREMENTAL=0 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed. |
| Full Rust library/integration suite outside the sandbox | Passed: 7,658 tests across 16 test harnesses, 21 ignored, zero failures. Library harness: 7,493 passed, 21 ignored. |

## Environment retries and limitations

Initial Rust compilation exhausted disk space. With approval, only regenerable
incremental caches and old Rust library archives were removed, never application
executables or user configuration. Subsequent Rust verification disables
incremental caching. Test builds additionally use
`--config 'profile.test.package.varyai.debug=0'` to reduce the root package's debug
symbols without changing assertions or dependency versions.

The sandboxed Rust library run reported 6,800 passed, 693 failed and 21 ignored;
local listener operations are blocked by the sandbox. The full suite is rerun
outside it rather than treating these failures as an accepted verification result.
The linker reports an oversized compact-unwind table for the large test image;
this is recorded as a warning, not suppressed.

The source manifest at `/private/tmp/vary-codex-refresh-check-inputs.json` records
1,658 source/configuration hashes. The only later code-tree difference observed
before the final rerun was an unrelated `src/index.css` edit; renderer build,
formatting and focused UI checks were repeated afterward.

Linux/Windows CI, real application restart behavior, installer/signing/notarization
and release acceptance are not established by these local checks. No remote CI
revision was submitted in this change.

## Final outcome

The full Rust command exited successfully after the sandbox retry:

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path src-tauri/Cargo.toml \
  --config 'profile.test.package.varyai.debug=0'
```

This includes the required library and integration tests, with all assertions
unchanged. The 21 ignored tests remain explicitly ignored; none was disabled by
this change. Linux, Windows and Windows-to-WSL2 remote acceptance remains unrun.

The implementation-specific source hashes are recorded separately at
`/private/tmp/vary-codex-refresh-final-inputs.json`; all 11 implementation/test/
locale inputs remained unchanged throughout final verification. The final
implementation, documentation and locale diffs were inspected, and
`git diff --check` passed. Nothing was staged or committed.
