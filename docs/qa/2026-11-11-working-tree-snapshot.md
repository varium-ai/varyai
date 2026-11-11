# Working-tree snapshot verification — 2026-11-11

Base revision: `7a00b1869bce94b0c341f0f199f9a8c6993923aa`. This snapshot includes
all pending source, test, localization and documentation changes: Agent route
recovery, verified ChatGPT/Codex desktop restart, tool discovery and page-context
conversion, and the shared check-in rewards interface. Generated applications,
local configuration and credentials are excluded.

Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1; pnpm 10.12.3. Renderer verification
includes pinned Node.js 22.12.0 runs. The check-in-specific QA also records earlier
Node.js 20.20.2 observations. Current backend files match the latest applicable
recorded verification hashes. All 15 check-in UI/translation/test files match
that task's final source manifest. The combined source manifest is recorded in
`/tmp/vary-commit-snapshot-2026-11-11/source-sha256.json`.

## Results and limits

- Passed: `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, production
  dependency audit, Cargo check, Cargo fmt and Clippy with warnings denied.
- Targeted route/conversion tests pass. The final page-context run reports
  82 bridge tests passed and 215 intent tests passed, with one existing ignore.
  Earlier applicable restart/reconnection regressions and check-in tests pass.
- Full renderer tests retain three authorized-model-save failures for WorkBuddy,
  WorkBuddy AI and Qoder. One run also fails ProviderAddSheet's focus assertion;
  its isolated 11-test run passes. Both full-run outcomes remain disclosed.
- Helper checks pass except the three recorded TRAE bridge cases. The development
  launcher, macOS DMG helper, model defaults, WorkBuddy credential, Doubao check-in,
  rewards and Qoder codec checks pass. The shared-v23 recovery helper passes.
- `cargo test --lib` aborts in the recorded TRAE proxy fixture. The no-fail-fast
  full run also aborts in the recorded TRAE `profile_bundle` fixture; other test
  programs pass. Assertions were not disabled or weakened for this snapshot.
- The updated development backend is running. A synthetic request with page
  context to the exact historical upstream/model completes with HTTP 200 and
  `response.completed`. This does not replay the private conversation.
- Remote CI, native Windows/Linux/WSL2, signing and installer acceptance remain
  unverified. A local snapshot commit does not establish CI or release acceptance.

Detailed evidence is in the adjacent route-restart, restart-confirmation,
Codex tool-search, Codex page-context and check-in-layout QA records. Final backend
logs are in `/tmp/vary-codex-page-final/`; renderer/helper records are in
`/tmp/vary-codex-400-checks/` and the check-in task's recorded log paths.
