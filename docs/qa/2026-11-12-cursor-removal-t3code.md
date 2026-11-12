# Cursor removal and T3 Code verification — 2026-11-12

## Scope and checked tree

- Base revision: `919a3b9d970eaee6e14c258cff0bdec08036c257`.
- Remove the Cursor work introduced by `0526276e` and `919a3b9d` while retaining T3 Code and the TRAE fixture repairs. Git history is preserved; removal is a new local change.
- Remove desktop/CLI registration, Private Inference environment handling, subscription login/inference/model/quota adapters, vendored protocol resources, icons, localization, dedicated tests and Cursor-only documentation.
- Restore the shared protocol/usage/UI code introduced only for the removed subscription adapter.
- Preserve the T3 Code projection implementation unchanged, including its independent Codex home and ownership-based restoration.
- Remove the persistent yellow setup note beside the T3 Code name. Retain model selection and connection status.
- Replace the mixed integration guide with [T3 Code](../guides/t3code.md), including manual acceptance steps.
- Source manifest: 112 changed/deleted files, excluding QA reports. Sorted UTF-8 path, NUL, SHA256 or `DELETED`, LF digest: `2e855e9ca1ef0576ceb7be0a2e4397d8d0b026a493ffe6eb398fdd11d42d8f12`.
- Exact source manifest: `/tmp/vary-remove-cursor-verification/isolated-manifest.json`.
- Final raw command records: `/tmp/vary-remove-cursor-isolated-verification/`.

## Environment and test isolation

Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1; Node 22.12.0; pnpm 10.12.3.
Rust checks use `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2` and the independent target directory
`/tmp/vary-remove-cursor-verify-target`.

The initial Rust test attempt encountered missing fingerprint/dependency files in
the shared application build directory while a development build was also active.
The final verification uses its own build directory; the user's development
process was not stopped. An initial frontend run hit the unchanged Windows CLI
parser fixture's five-second timeout. Its focused rerun passed without modifying
that test or its assertions. These initial attempts are retained separately and
are not reported as successful final checks.

Final commands run in `/private/tmp/vary-remove-cursor-isolated`, a detached
worktree based on the revision above with only this removal and setup-note change.
Unrelated DeepSeek recovery and other work appeared concurrently in the original
checkout. The isolated source files match the manifest; those concurrent edits
are excluded from this verification and commit and are preserved in the original
checkout.

## Regression evidence

- Before removal, the new frontend regression failed because Cursor was still registered.
- The final contract retains T3 Code registration, keeps unsupported legacy MCP/Skills features unavailable and excludes Cursor/CLI/subscription entries.
- Rust parsing rejects retired Cursor application identifiers.
- T3 Code's public lifecycle test still covers connect, catalog refresh and disconnect; its projection tests cover isolated configuration, preserved native instances and ownership conflicts.
- The setup-note removal regression failed while the note was present, then passed with model selection and connection assertions retained.
- The 227 focused Agent/Windows CLI parser checks passed after the note removal.
- The T3 Code adapter and TRAE bridge source/helper fixture files have no changes relative to the base revision.
- The profile-bundle test excludes five remaining unsupported app variants after the two Cursor variants are removed. Its original exclusion count of seven reproduced a 15-versus-13 assertion failure; the corrected count retains coverage of all supported agents.

## Final verification

| Command                                                            | Result | Seconds |
| ------------------------------------------------------------------ | ------ | ------: |
| `pnpm typecheck`                                                   | PASS   |   18.41 |
| `pnpm format:check`                                                | PASS   |    8.93 |
| `pnpm build:renderer`                                              | PASS   |    7.94 |
| `pnpm test:unit`                                                   | PASS   |  168.52 |
| `pnpm audit --prod`                                                | PASS   |    1.26 |
| `pnpm test:dev`                                                    | PASS   |   11.89 |
| `pnpm test:macos-dmg`                                              | PASS   |    1.21 |
| `pnpm test:trae-bridge`                                            | PASS   |    1.20 |
| `pnpm test:model-default`                                          | PASS   |    0.66 |
| `pnpm test:workbuddy-checkin-auth`                                 | PASS   |    0.65 |
| `pnpm test:doubao-checkin`                                         | PASS   |    2.65 |
| `pnpm test:agent-rewards`                                          | PASS   |    0.67 |
| `pnpm test:qoder-codec`                                            | PASS   |    1.16 |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | PASS   |    0.19 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | PASS   |    6.56 |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | PASS   |   35.05 |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | PASS   |   33.19 |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | PASS   |  376.63 |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`   | PASS   |  249.99 |

Frontend: 294 test files / 3,858 tests passed. Rust library: 6,321 passed, 19 ignored. Full Cargo run: 6,486 passed, 19 ignored across 16 test targets, with no failures. Production dependency audit: no known vulnerabilities.

The full Cargo command emitted a non-fatal macOS linker warning that the test binary's `__eh_frame` section exceeds the compact-unwind limit. Clippy with warnings denied passed. The configured ignored tests remain ignored; they are not counted as successful tests.

## Acceptance limits

- No application was launched, installed, replaced or restarted for automated checks. Tests use synthetic credentials, temporary paths, in-memory databases and dynamic ports.
- A read-only schema check found `providerInstances` in the local T3 settings. The user's screenshot shows a configured connection, but this does not establish successful model inference.
- Live T3 Code conversations, tool execution and upstream usage still require manual acceptance through a new thread using its VaryAI instance.
- Remote Linux/Windows/macOS and Windows-to-WSL2 CI have not run for this local change. Cross-platform and release acceptance remain incomplete.
- No push is authorized for this work; the change is intended for a local commit only.
