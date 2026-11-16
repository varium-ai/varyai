# Combined provider, usage, quota and desktop fixes

This report covers the complete source/documentation working tree submitted for
review on November 16, 2026. The earlier task reports remain historical evidence;
the final checks below supersede their earlier incomplete provider-name fixture
results for the combined tree.

## Scope

- Provider names remain editable while internal IDs stay private. New duplicate
  names are rejected after trimming and case folding, including concurrent-save
  conflicts. Unchanged historical duplicates remain usable for toggles and account
  refreshes. Existing references and credentials retain their identities.
- The Add provider page keeps its four entry methods and fixed popular shortcuts,
  without the redundant page search. Directory dialogs retain their own search.
- API provider rows no longer initiate allowance queries. Relay interface discovery
  is shared by origin/credential digest and persisted privately without credentials,
  endpoint URLs or balance responses. Manual refresh bypasses discovery expiry.
- Tray quota cards keep all independent windows visible, use compact pool labels,
  and refine current-cycle sparklines. Relay balance cards omit quota history.
- Statistics show clear source names without exposing internal session IDs or extra
  attribution badges. Request-log paging retains the current table while loading,
  restores valid pages after count changes, scrolls short pages into view and offers
  a generic retry after errors. Different filter scopes do not share displayed rows.
- Confirmed desktop relaunches wait for a new verified instance. Codex's confirmed
  catalog refresh restarts the desktop application on macOS and Windows, while
  automatic routing updates retain their idle-aware internal reload behavior.

## Checked tree

- Base revision: `2efc772ff8de57c2e0dd242bcab22d600e3b41b2` (`origin/main` at preparation).
- Branch: `codex/provider-usage-and-desktop-fixes`.
- All 61 reviewed source/documentation inputs matched both complete
  check snapshots immediately before this verification record was added.
- SHA-256 of the sorted compact JSON path/hash mapping: `386acb1e6ab7300a53c1f9c95620671ff4cfadfe43863ae54598acafe104ef2b`.
- Command results, logs, tree snapshots and the input hash mapping are retained
  in `/tmp/vary-final-pr/`. This record adds documentation only after checks.

No generated binaries, local cache files, logs, private configuration or live
credentials are included. A credential-pattern scan of all changed files found
no private-key or credential literals. The change uses shared React/Rust behavior
and the existing native platform implementations for macOS and Windows.

## Local checks

| Check                                                              | Result                                                      |
| ------------------------------------------------------------------ | ----------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                      |
| `pnpm format:check`                                                | Passed                                                      |
| `pnpm build:renderer`                                              | Passed                                                      |
| `pnpm test:unit`                                                   | Passed: 317 files, 4,225 tests                              |
| `pnpm test:dev`                                                    | Passed                                                      |
| `pnpm test:macos-dmg`                                              | Passed                                                      |
| `pnpm test:trae-bridge`                                            | Passed                                                      |
| `pnpm test:model-default`                                          | Passed                                                      |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                                      |
| `pnpm test:doubao-checkin`                                         | Passed                                                      |
| `pnpm test:agent-rewards`                                          | Passed                                                      |
| `pnpm test:qoder-codec`                                            | Passed                                                      |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                                      |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                            |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                      |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 7,607 tests; 21 ignored                             |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                      |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 7,607 library and 165 integration tests; 21 ignored |
| `git diff --check`                                                 | Passed                                                      |

The host is macOS 15.3.1 (24D70), arm64. Toolchain: Node 22.22.2, pnpm 10.12.3,
Rust/Cargo 1.98.1 stable. Local Node differs from the exact 22.12.0 CI pin.
Tests use temporary storage, in-memory databases, synthetic credentials and dynamic
loopback ports. No installed application or client was launched, installed,
replaced, restarted or quit as a verification step.

## Limits

Local success is not cross-platform CI acceptance. The PR tracks CI for the exact
pushed revision, including Linux, Windows, macOS and the Windows-to-WSL2 contract
triggered by the storage-related paths. No merge is performed as part of this task.
Live installed-client relaunch, installer, signing, notarization and live provider
inference acceptance remain outside this validation.
