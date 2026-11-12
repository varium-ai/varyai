# Unified Cursor entry verification

Date: 2026-11-11 (Asia/Shanghai).
Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1, Node 22.12.0,
pnpm 10.12.3. Node matches `.node-version`.

## Checked scope

Base revision: `0526276ed91df01a945fe854f7ab5f6b883afeff`.
The 19 changed source, test and guide files, excluding this report, have aggregate
SHA-256 `884f63b399a2e7079b7f35ccaf27781ce03f31a1ee41e4adbeb1d110f0b2050f`.
The local manifest `/tmp/vary-cursor-unified-manifest.json` records each content
hash. Its aggregate encoding is sorted UTF-8 path, NUL, content SHA-256, LF.

- One public Cursor entry replaces separate desktop and CLI rows. The CLI API
  identifier and stored type remain compatible; saved row order maps to Cursor.
- Standard Cursor is detected on macOS and Windows. CLI-only installations also
  count as Cursor. App and CLI versions can appear in the same row.
- A separate capability check distinguishes the Private Inference build. Standard
  desktop detection grants no local gateway capability. Connect and repair
  commands enforce that boundary, independent of the UI.
- CLI Auto remains a native CLI setting. It does not modify desktop routing,
  native authentication or existing permission settings.
- The UI explains the boundary in all four runtime languages. Failed capability
  detection withholds the local connection controls.

## Reference and unresolved integration boundary

The inspected Magpie revision is
`b4fc29a6e4e89b128df9254f4e5f5824553eb133`.
Its `internal/agent/agents.go` defines Cursor as **Cursor CLI**; its
`internal/agent/cursorlocal.go` handles **Cursor Private Inference**. The author
describes the standard desktop boundary in
[issue #1254](https://github.com/yetone/magpie/issues/1254).

The standard editor is recognized and grouped by this change. Forwarding its
built-in Agent through the local VaryAI gateway is not implemented: its cloud
backend cannot reach a loopback address. A publicly reachable authenticated
HTTPS gateway would be required for the native BYOK path. No such address has
been provided for this work. No public tunnel, client patch, account bypass or
replacement installation was created.

## Test-first evidence

Before implementation, the new public-registry and ordinary-desktop UI tests
failed (three focused frontend failures). The desktop presence test also failed
on the assertion that a standard Cursor bundle should be recognized. A further
regression reproduced stale local capability surviving a failed edition rescan;
that case now withholds local controls despite retained query data. After the
changes, the Agent page/registry suite passed 230 tests and the update/version
suite passed 56. The Cursor-filtered Rust suite passed 35 tests, including both
platform detection fixtures and the existing Cursor/T3 service lifecycle.

## Complete local rerun

Rust commands use `CARGO_INCREMENTAL=0`. Test fixtures use temporary paths,
synthetic data and dynamic ports. No running application was restarted or
replaced; no native client configuration was changed for verification.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed: 294 files, 3,876 tests |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| `pnpm test:dev` | Passed: 8 tests |
| `pnpm test:macos-dmg` | Passed: 5 tests |
| `pnpm test:trae-bridge` | Passed: 66 tests, none skipped |
| `pnpm test:model-default` | Passed: 9 tests |
| `pnpm test:workbuddy-checkin-auth` | Passed: 5 tests |
| `pnpm test:doubao-checkin` | Passed: 13 tests |
| `pnpm test:agent-rewards` | Passed: 9 tests |
| `pnpm test:qoder-codec` | Passed: 3 tests, none skipped |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed: 5 tests |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed: 6,328 tests; 19 ignored |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Passed: 6,492 tests, including 164 integration tests; 19 ignored |
| `git diff --check` | Passed |

The 19 ignored library cases retain their original annotations for explicit
live-client, publisher-download, external-service, corpus and performance runs.
They are not counted as passed. Rust test linking emitted the macOS warning
about `__eh_frame` exceeding the compact unwind encoding limit.

The final frontend checks were repeated after the capability-rescan fix. Backend
files did not change after the complete Rust verification.

## Verification limits

Remote Linux, Windows, macOS and Windows-to-WSL2 CI has not been run for these
working-tree changes. The Windows discovery helper runs against synthetic
Windows layouts in portable tests; it does not establish full Windows runtime
acceptance. Live standard-Cursor inference through a public VaryAI endpoint
remains unverified and unimplemented pending the gateway decision.

Logs are retained locally under `/tmp/vary-cursor-*.log`; command status records
are `/tmp/vary-cursor-helper-results.json` and
`/tmp/vary-cursor-rust-results.json`.
