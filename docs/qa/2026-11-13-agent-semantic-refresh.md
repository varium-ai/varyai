# Agent semantic refresh verification

## Scope and host

- Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b` plus the final working tree.
- Date: 2026-11-13, Asia/Shanghai; host: macOS 15.3.1 arm64.
- Toolchain: rustc 1.98.1, Cargo 1.98.1, Node.js 24.11.0, pnpm 10.12.3.
- Magpie reference: `6d15f63d52146fcadccef30c9777e45051bcd11c`.
- Scope: generic managed startup comparison, semantic restoration comparison,
  persistent TRAE launch freshness, Doubao inspection-state precedence, one-second
  desktop startup tolerance for macOS and Windows, and four-language restart text.
- Existing Codex, routing, branding and Harness changes in this shared working tree
  are preserved. The only repair made to the concurrent Harness projection is a
  local variable rename to avoid shadowing its `key()` function. Readable-config
  tests also restrict model-selection assertions to Cordis model documents now
  that Harness receipts contain a separate native credential document.
- No commit, push, app installation or real client restart was requested or performed.

## Behavior covered

The initial whole-document comparison regression run failed the presentation/order
and unrelated-preference cases (three passing, two failing). The final tests cover:

- Changed upstream routing behind unchanged client-facing configuration.
- Catalog identity, membership, behavioral settings and endpoint/token changes.
- Labels, icons, descriptions and catalog order that do not affect startup semantics.
- Ownership union, unrelated user preferences and receipt bookkeeping.
- Semantic restoration and persisted timestamps after reopening the manager.
- Native model-control confirmation remaining valid after harmless refreshes.
- TRAE JSON preferences and numeric/string zero launch-argument equivalence.
- TRAE restoration/reconnection, malformed input/receipt handling and digest-only persistence.
- Delayed TRAE observations use the actual file change time when available, so a
  client launched after that change is not incorrectly aged by a later observation.
- Native catalog shapes for Claude Desktop, OpenCode, Kimi, Qwen, T3 Code and ZCode,
  retaining model identities and behavior parameters while ignoring presentation.
- Active Doubao inspection overriding historical desktop freshness.
- Equivalent one-second startup boundaries for macOS and Windows, retaining unknown
  startup handling and process/configuration rechecks during explicit restarts.

CLI and IDE session inventory is not added. CLI-only adapters retain new-session
guidance and cannot acquire a desktop restart action from a file change. Live
inspection availability on unsupported platforms is unchanged.

## Verification results

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed. |
| `pnpm build:renderer` | Passed. |
| `pnpm test:unit` | Passed: 308 files, 4,110 tests. |
| `pnpm format:check` | Passed. |
| All eight CI helper suites | Passed; development helper rerun outside the sandbox for temporary listeners. |
| `pnpm audit --prod` | Passed: no known vulnerabilities. |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed, incremental compilation disabled. |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed, incremental compilation disabled. |
| Full Cargo test suite (command below; includes library and integration tests) | Passed: 7,683 tests across 16 harnesses, 21 ignored, zero failures. Library subset: 7,518 passed, 21 ignored. Command exited 0. |

## Environment and acceptance limits

An intermediate Rust build exhausted disk space. Only obsolete generated Rust
products and incremental caches under this checkout's `target/debug` were removed
with approval (20.05 GiB). Source files, user configuration and installed/running
applications were not removed. Final test builds disable incremental compilation
and root-package test debug symbols to reduce disk usage:

```sh
CARGO_INCREMENTAL=0 cargo test --manifest-path src-tauri/Cargo.toml \
  --config 'profile.test.package.varyai.debug=0'
```

The test suite uses temporary local listeners and runs outside the sandbox.
Intermediate compilation also caught a private-method call in the new test and
errors in concurrent Harness edits; those intermediate results are not acceptance.
Logs are saved under `/private/tmp/vary-agents-*`. The final input manifest is
`/private/tmp/vary-agents-checked-inputs.json`.

Final accepted Rust logs are `vary-agents-cargo-current.log`,
`vary-agents-cargo-check-current.log`, `vary-agents-clippy-current.log` and
`vary-agents-cargo-fmt-current.log` in `/private/tmp`. All commands exited 0.
Rust source inputs remained unchanged during these final checks.

Four unrelated branding inputs changed after the initial input manifest was
captured: `scripts/generate-brand-icons.mjs`, `src/assets/brand/app-icon.svg`,
`src/assets/icons/app-icon.png` and `src/assets/icons/nav-icon.png`. The renderer
build and frontend formatting were rerun successfully on that tree. A proposed
`tests/config/brandAssets.test.ts` target did not exist and exited 1 with no tests;
it is not counted as a passing check. Existing Agent icon and shared picker
consumer tests passed instead: 3 files, 35 tests, using
`pnpm exec vitest run tests/config/agentIcons.test.tsx tests/components/SharedModelPickerBranding.test.tsx tests/components/SharedModelPicker.test.tsx`;
see `vary-agents-brand-consumers-final.log`.
The scoped final input hashes are recorded in
`/private/tmp/vary-agents-accepted-inputs.json` alongside the original manifest.

Windows, Linux and Windows-to-WSL2 remote CI, installed application behavior,
signing/notarization and release acceptance are not established by local macOS
checks. Existing conservative timestamps cannot reconstruct a pre-upgrade
client's loaded state; a first migration can still need manual confirmation.
