# PR #43 platform fixture verification

Date: 2026-11-06. Checked base: `b97e76487046c042e32d720d87dfc8c457dbaa7f`
plus the two Rust test changes and development documentation in this commit.
Unrelated shared-model picker work and the local pnpm cache are excluded.

## Failure evidence and repair

[CI run 42027708055](https://github.com/varium-ai/varyai/actions/runs/42027708055)
passed frontend, macOS and Windows-to-WSL2 checks, but Linux job `127246718029`
and Windows job `127246717998` failed
`parity_native_projection_preserves_stable_ids_picker_and_jsonc`.
The fixture used macOS paths on every platform and attempted unsupported Claude
Desktop projection on Linux. Windows instead reads the consumer configuration
from Roaming AppData and writes the gateway profile under Local AppData.

The repaired test retains portable-client assertions on every platform, uses
the appropriate temporary Desktop paths on macOS and Windows, and verifies
rejection on Linux. Existing Desktop restoration, native setting and user-edit
assertions remain. Added assertions check that both MCP entries and the locale
are initially copied before exercising edits and removal.

The first local full Rust run also reproduced a separate macOS fixture race in
`trae_reconnection_delivers_latest_offline_selection_without_enabling_audit`.
Its accepted Unix socket inherited nonblocking mode and `read_line` returned
`WouldBlock`, followed by an assertion and cleanup panic. The synthetic server
now explicitly puts accepted sockets into blocking mode before reading. The
focused regression passed twice, followed by successful full runs. No forwarding
implementation, supported platform, workflow filter or ignored-test list changed.

## Local verification

Host: macOS 15.3.1 arm64. Node.js 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.
Rust 1.99.1 was additionally installed to match the failed CI run's compiler.
All commands ran from the repository root. Cargo commands below use
`--manifest-path src-tauri/Cargo.toml`.

| Check                                                                     | Result                                |
| ------------------------------------------------------------------------- | ------------------------------------- |
| Focused shared projection regression                                      | Passed                                |
| Focused TRAE reconnection regression, twice                               | Passed                                |
| `pnpm typecheck`                                                          | Passed                                |
| `pnpm build:renderer`                                                     | Passed                                |
| `pnpm test:unit`                                                          | 279 files, 3,471 passed               |
| `pnpm format:check`                                                       | Passed                                |
| `pnpm test:dev`, `pnpm test:trae-bridge`                                  | Passed                                |
| `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`            | Passed                                |
| `pnpm test:agent-rewards`, `pnpm test:qoder-codec`                        | Passed                                |
| Combined helper suites                                                    | 94 passed, 1 skipped                  |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                     | Passed                                |
| `pnpm audit --prod`                                                       | No known vulnerabilities              |
| Rust 1.98.1 `cargo fmt --check`, `cargo check`, Clippy with `-D warnings` | Passed                                |
| Rust 1.98.1 `cargo test --lib`                                            | 6,025 passed, 18 ignored              |
| Rust 1.98.1 `cargo test`                                                  | 16 binaries, 6,188 passed, 18 ignored |
| Rust 1.99.1 `cargo fmt --check`, Clippy with `-D warnings`                | Passed                                |
| Rust 1.99.1 `cargo test --no-fail-fast`                                   | 16 binaries, 6,188 passed, 18 ignored |

Rust 1.99.1 checks used `CARGO_INCREMENTAL=0`. The macOS linker emitted the
existing nonfatal compact-unwind size warning. The ignored Rust tests and skipped
Qoder codec test retain their existing external/native acceptance requirements.
No installed application was launched or replaced.

The final checked SHA-256 fingerprints are:

- `src-tauri/src/services/agent_connections/parity_tests.rs`:
  `df75a7bb8fad295ff82dfd698e5caa90ebe2913727067733a590433d6326eb67`
- `src-tauri/src/services/proxy.rs`:
  `32782428914fb85aa178dabd43babe7f0193d91420af4ea33c2ea5866a535961`

These Rust inputs remained unchanged during the final successful runs. Command
logs and machine-readable results were retained locally. Cross-platform
acceptance requires fresh CI for the exact pushed revision; the failed parent
run does not validate this repair.
