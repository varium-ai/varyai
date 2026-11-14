# Installation rescan feedback

The **Rescan installations** action now spins its refresh icon and disables both
rescan buttons while the refresh is running. The busy state covers installation
detection, CLI versions, native models and desktop versions, so finishing presence
detection alone does not stop the feedback. The state resets when the action
settles, including its error path. Both buttons expose `aria-busy`; decorative
icons are hidden from assistive technology.

This is shared renderer behavior for macOS and Windows. Main-window and compact
views use the same state, including the empty installation view.

## Verification

- Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b`, with existing unrelated
  working-tree changes preserved. This change adds only rescan state and button
  feedback in `AgentConnectionsPage.tsx`, its regression tests and this report.
- Host: macOS arm64; Node.js 24.11.0, pnpm 10.12.3, Rust 1.98.1.
- Test-first: the four main/compact and populated/empty cases failed before the
  fix, then passed. Deferred synthetic responses verify that feedback starts on
  click, blocks repeated activation, stays active after detection finishes while
  versions remain pending, and stops after completion. An additional regression
  verifies recovery and retry after refresh rejection.
- Focused component and installation-hook tests: 237 passed.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, focused Prettier,
  `git diff --check`, `cargo check --manifest-path src-tauri/Cargo.toml` and
  `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`,
  `pnpm test:doubao-checkin`, `pnpm test:agent-rewards` and
  `pnpm test:qoder-codec`: passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five tests passed.
- `pnpm audit --prod`: passed with no known vulnerabilities. The sandbox's
  network restriction required rerunning with network access. Development
  launcher tests similarly passed after allowing their temporary loopback ports.
- `pnpm test:unit`: 308 files and 4,115 tests passed. The five new feedback
  regressions also passed again after the final test-fixture type annotation.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: the sandbox run
  completed with 6,825 passed, 693 failed and 21 ignored. Its failure log contains
  647 occurrences of `Operation not permitted`, including failures to create
  temporary local listeners. This is not a passing backend result.
- `cargo test --manifest-path src-tauri/Cargo.toml`: compilation failed with
  `rustc-LLVM ERROR: IO failure on output stream: No space left on device`.
  Only 254 MiB remained available. The unrestricted library rerun was stopped
  during tests, and its subsequent full test command did not run. No successful
  full backend result is claimed. The three ignored library artifacts produced
  by the failed build were removed to reclaim space; source files and other
  build caches were preserved.
- No application was launched, installed or replaced. No commit, push or remote
  CI verification was requested; cross-platform CI and live desktop visual
  acceptance remain unverified.
