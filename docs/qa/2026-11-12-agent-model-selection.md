# Agent model selection retention

## Scope and cause

The active Lark feedback reported that saved Agent models reverted to the first
catalog entry after waiting or returning to the Tools page, with Codex unaffected.
Shared Agent refresh checked the receipt's public model alias against internal
menu keys. The failed lookup replaced a valid selection with the catalog default.

Refresh now checks listed aliases and uses an alias for the fallback when the
menu has no default. The implementation is shared by macOS and Windows.

The regression uses a generated three-model catalog, temporary configuration
files and receipt stores for Kimi and Qwen. It checks explicit nondefault
selection, repeated refresh, status reads through a new manager, suspension and
resume, selected-model deletion, and fallback without a menu default.

## Checked environment

- Base revision: `c382853491ac3ec64e0308433ef1c5cc97fc9a26` plus existing staged
  merge changes and this working-tree change; the merge was already in progress.
- Host: macOS, arm64.
- Rust: `rustc 1.98.1 (48a229cea 2026-09-01)`, Cargo `1.98.1`.
- Node: `v20.20.2`; pnpm: `10.12.3`.
- Changed implementation: `src-tauri/src/services/agent_connections.rs`, alias
  selection in `Manager::update` only. Other working-tree changes in that file
  belong to concurrent merge work.
- Added regression: `src-tauri/src/services/agent_connections/parity_tests.rs`.
- Updated behavior guide: `docs/guides/agent-default-models.md`.

## Verification

Initial Rust compilation was blocked by existing DeepSeek Harness merge errors
(missing `sections`, `START` and `END`); the merge handoff corrected that code.
The final regression compares parsed configuration after resume because table
ordering may change without changing the saved model or other settings.

Final source-manifest SHA-256:
`67047d2c85b85dac13b4f7cfb766ff6917e5f0c366293acf242a4df74f3c6305`.
The manifest covers tracked source, tests, resources and package manifests under
`src-tauri`, `src`, `tests` and `scripts`, plus the root package, lock and Vitest
files. Logs and the manifest are under `/tmp/varyai-agent-model-*`.

| Command | Result |
| --- | --- |
| Focused model-retention regression | 1 passed |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 297 files, 3,883 tests passed |
| `pnpm format:check` | Passed |
| `pnpm audit --prod` | Passed |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,344 passed, 19 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,509 passed, 19 ignored |

The existing nonfatal macOS debug-linker compact-unwind warning remains.
Frontend files did not change during the final backend checks. The only source
change after the first frontend run was the new Rust regression, which passed
in the final complete backend runs. The unrelated routing-design document was
left outside the submission.

No application was launched, installed or replaced. No Lark messages were sent.
Windows, Linux, installer, signing and live-client acceptance are not established
by the local checks. Commit and push were authorized after the merge handoff;
remote CI must be checked for the exact pushed revision before acceptance.
