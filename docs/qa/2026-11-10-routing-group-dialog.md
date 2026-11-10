# Routing group creation dialog verification

Date: November 10, 2026 (Asia/Shanghai).

## Checked scope

Base revision: `72ce390fa473087505934f91ed77728fac51ef01`, plus the working-tree files below. Other edits were already present and continued changing during verification; they were preserved. No commit, push, installer replacement or application launch was performed.

Both Create group controls open the shared dialog. Its body scrolls independently of the Save/Cancel footer. Existing groups retain their inline editors. Saving blocks dismissal and duplicate clicks; a failed save retains the draft with a sanitized error. Closing restores focus.

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `3d5818131abfa7f043d15a6e39828a6815296c4d6edfaa69c8e1d4d55c5f6e88` |
| `src/components/routing/RoutingPage.tsx`        | `c3f4abf8f4324723396e97e4d73954892bc0b481a0d6d42e804c06e799169c51` |
| `src/components/routing/routing.css`            | `8e1a848d5d77da0ce7ab6230f09f0c3533ecf739df3a97d8d4e4f59458bd74d6` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `ec16c5627629edf8d35c4ff4f1b84d604b785264e33bbc1bffca25e11db989f6` |
| `tests/components/RoutingPage.test.tsx`         | `b733b1a2888c98e1c20c5a548ea96abedbdcee852ced0634d7e4dbc7bb6743a5` |
| `docs/guides/intent-routing.md`                 | `b605234b31a851b6484fee8c70a096561eff12caad736750c656aea4dd8dee12` |

## Environment

macOS 15.3.1 (24D70), Apple Silicon; Node v24.11.0; pnpm 10.12.3; rustc 1.98.1 (48a229cea 2026-09-01); cargo 1.98.1 (797e8a9bc 2026-08-05). The React implementation is shared by macOS and Windows.

## Results

- Test-first verification: four new modal assertions failed against the original inline implementation.
- `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx tests/components/RoutingPage.test.tsx`: passed, 100 tests across two files on the final changed test files.
- `pnpm typecheck`: passed.
- `pnpm build:renderer`: passed.
- `pnpm format:check`: passed. Changed test files were also formatted with Prettier.
- `pnpm test:unit`: 287 files passed and two failed (3,765 tests passed, five failed). Four failures are navigation label assertions in `WorkspaceNavigation.test.tsx` against pre-existing locale edits. The fifth was a missing jsdom scroll spy in the newly added toolbar test; it was corrected, and both affected routing suites were rerun successfully (100/100). The complete suite was not repeated after that test-only correction.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec`: passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed.
- `pnpm audit --prod`: passed, no known production vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`: failed during concurrent backend edits in `agent_connections/projections.rs` (unresolved `doc` and `config`).
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: could not compile the test target during concurrent backend edits (`config` required a mutable binding in `agent_connections/projections.rs`).
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: failed on unrelated backend formatting, including Claude Desktop test fixtures.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed after the concurrent backend edits advanced.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed after waiting for the shared build lock and recompiling the concurrently edited backend. 6,304 library tests and 163 integration tests passed; 19 library tests were ignored. No test failed. This later run compiled and exercised the library successfully; the earlier standalone `cargo check`, `cargo test --lib` and formatting checks were not repeated.

Overall verification is not fully green because of the frontend navigation assertions and the earlier standalone backend check failures described above. No live desktop, packaged-build or remote CI acceptance is claimed. The UI assertions use synthetic providers and model catalogs; no application was installed or started. Linux/Windows/macOS backend CI results were not obtained.
