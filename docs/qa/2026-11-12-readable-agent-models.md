# Readable managed Agent model IDs

## Scope

Managed OpenCode, OpenClaw, Qoder, DeepSeek Harness, Gemini CLI and T3 Code
connections now publish readable model IDs and save readable default selections.
Existing Claude Code, Pi, WorkBuddy, Hermes, Kimi, Qwen and ZCode name publication
continues to work. Duplicate allocation is deterministic and reserves existing
names, natural numeric suffixes and internal identities.

Gemini CLI and T3 Code reuse the Codex ASCII alias allocator. Their catalogs keep
full display labels, while receipts retain old names and Gemini URL hashes for
forwarding. Current picker IDs take precedence over retained aliases in model
lists, selection refresh and native desktop default plans. Claude Desktop retains
its native protocol IDs and separate readable labels.

Refresh preserves a surviving selected model even when the client stores only a
default, as Gemini does. Removing that model falls back to an available default.
Shared profile capture and preview resolve provider-prefixed readable selections
back to canonical identities. T3 Code still checks foreign configuration
ownership before projecting its catalog.

## Checked tree and environment

- Working tree based on `2e6ea81403bbbfdaf7b9b48dbde97f4a9f1dd232`, with
  these model-name changes and pre-existing DeepSeek recovery work retained.
- macOS 26.6.2, Darwin 25.6.0, arm64; Rust/Cargo 1.98.1;
  Node.js 20.20.2; pnpm 10.12.3.
- Final Rust verification fingerprinted 1,544 source, test, resource and manifest
  files. No fingerprinted file changed during those checks. Frontend source and
  test files also remained unchanged after the final typecheck.
- Source-manifest SHA-256:
  `57001ac6498dc0ee31873a829ba259dd57119c065ee881ed5b4dad66d2d608ee`.
- Logs and source fingerprints:
  `/tmp/varyai-readable-models-20261112/`.

Rust test processes append only numeric loopback addresses and localhost to
`NO_PROXY` and `no_proxy`. Host proxy settings remain unchanged. New tests use
temporary client homes, in-memory databases, synthetic credentials and dynamic
ports. No installed application was launched or replaced.

## Verification

The initial regression reproduced Gemini's missing readable name. Final tests
cover all six newly named adapters, repeated refresh, Unicode labels, duplicate
IDs, removal fallback, restoration, current and retained model-list identities,
old-ID forwarding to the exact upstream model, desktop default plans and shared
profile capture/apply/undo. The shared profile test additionally covers Gemini
and Qoder. Existing reasoning metadata and T3 ownership assertions remain intact.

| Command | Final result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 3,858 passed, 1 failed in each of two full runs; see below |
| `pnpm exec vitest run tests/components/AgentConnectionsPage.test.tsx` | 222 passed |
| `pnpm exec vitest run tests/components/ProviderAddSheet.test.tsx` | 11 passed |
| `pnpm format:check` | Passed |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | 6,494 passed, 0 failed, 19 ignored |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,329 passed, 0 failed, 19 ignored |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |

The unmodified `ProviderAddSheet` test
`returns to the list after importing and restores the header action's focus`
fails its focus assertion in the full renderer suite. Running that test file on
its own passes all 11 tests. Its cause has not been established; no assertion was
weakened or excluded. Overall renderer-suite acceptance therefore remains
incomplete.

The 19 ignored Rust tests retain their existing annotations; this change adds no
ignored tests. The existing nonfatal macOS debug-linker compact-unwind warning
remains. Windows, Linux, Windows-to-WSL2, installed-client compatibility,
packaging, signing and remote CI acceptance were not established by these local
checks.
