# Independent WorkBuddy editions

Date: 2026-11-06 (Asia/Shanghai). Base: `5984530cbe725447dc68628431ecef7b4b96b00a`
plus the WorkBuddy edition changes in this working tree.
Host: macOS 26.6.2 arm64. Node.js 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1.
Rust checks use `CARGO_INCREMENTAL=0`.

## Behavior and evidence

- `workbuddy` identifies WorkBuddy China and opens `https://www.workbuddy.cn/`.
  `workbuddy-ai` identifies international WorkBuddy AI and opens
  `https://www.workbuddy.ai/`. Renamed bundles retain their edition identity;
  a trashed China bundle and a remaining international installation do not count
  as an installed China client.
- Uninstalled tools show only installation and website actions, even with saved
  active or recovery-pending connections. Regression coverage checks both layouts,
  both WorkBuddy editions and a CLI, including stale cached version data.
- China defaults to `.workbuddy/models.json`, retaining its explicit override and
  legacy `.codebuddy` fallback. International uses `.workbuddy-ai/models.json`.
  Overlapping paths and recovery receipts that own the other edition's file
  reject a new connection rather than taking over that file.
- Both tools consume the shared supplier/routing-group catalog independently.
  Local credentials, receipts, routing conditions, gateway policies, request
  usage and audit conversation prefixes remain separate.
- Synthetic dynamic-port HTTP tests cover both JSON and SSE requests, protocol
  conversion, readable and legacy model IDs, cross-edition credential rejection,
  separate usage accounting and disconnecting one runtime connection.
- Model selection changes are persisted in receipts even when the projected
  catalog bytes remain identical. Shared profile tests include both editions
  through capture, apply and undo without exposing local gateway credentials.
- Legacy China profile fixtures use the corrected China directory. Integration
  test homes include the process ID so concurrent test processes cannot clear
  one another's fixtures.

## Verification

- Before implementation, website/installation regressions, installation identity
  regressions and directory/connection isolation regressions reproduced the bugs.
  The overlap guard and unchanged-catalog selection regression also failed before
  their fixes.
- Final renderer checks: `pnpm typecheck`, `pnpm build:renderer`,
  `pnpm format:check`, and `pnpm test:unit`: passed; passed; 279 files, 3,495 tests.
- Development, TRAE bridge, WorkBuddy authorization, Doubao check-in, Agent rewards,
  Qoder codec and shared-storage recovery helper suites: passed. The existing
  conditional Qoder codec skip remains.
- `pnpm audit --prod`: no known vulnerabilities.
- Screenshot regression: all 22 installation UI tests passed. The six saved-connection
  cases failed before the missing-tool presentation fix and passed afterward.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,069 passed, 18 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 6,232 passed,
  18 ignored across all targets; every integration-test binary passed.
- `cargo check`, `cargo clippy -- -D warnings` and `cargo fmt --check`, each with
  `--manifest-path src-tauri/Cargo.toml`: passed.
- The earlier failure in a concurrently edited Codex screenshot-body test was
  superseded by the successful current library rerun and complete test run.
- `git diff --check` and changed-document Prettier checks: passed.

Command results, checked source fingerprints and logs are retained under
`/tmp/varyai-workbuddy-editions-*`. Concurrent Codex native-request diagnostics, tool-risk UI changes and the existing
browser-review report are preserved and are not authored or claimed as part of
the WorkBuddy feature. No installed application was
launched or replaced. Remote cross-platform CI has not run for these changes.
