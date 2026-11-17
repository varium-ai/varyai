# Confirmed desktop restart when the application has exited

## Reproduction and change

Computer Use observed the WorkBuddy AI row displaying `agentConnections.restartChanged` while the international application had no running process. The domestic WorkBuddy process was running independently. The installed international bundle identifier matched the lifecycle selector.

Confirmed restart already ignored configuration freshness, but an empty process list still returned the same generic state-change error. This also prevented recovery when an earlier restart closed the application without successfully reopening it.

The shared macOS and Windows restart orchestration now opens the selected installed application when no process is running. A running application still follows normal exit and reopen. Both paths wait for a verified application instance before reporting success. Installation and process identity checks remain in place; connection state does not authorize or block a confirmed manual restart. Automatic refresh and internal recovery retain their separate behavior.

Windows stopped-app launch covers the same supported application identities as running-process discovery. WorkBuddy editions remain separate. Existing model-control launch consent is respected, and ordinary application launch remains available without that consent.

The four runtime languages now provide concrete recovery instructions for restart failures and process identification failures instead of asking users to refresh configuration status.

## Verification

- Host: macOS 26.6.2, Rust 1.98.1, Node 20.20.2, pnpm 10.12.3.
- Revision: working-tree changes on `3c2435ebbea1185d8ba0226785746b3fa56882b4`, including the existing WorkBuddy conversation-model and Codex restart-notice changes. Source fingerprints recorded locally at `/tmp/varyai-nov17-final-source-hashes.json`.
- Before the implementation, both new stopped-application regression tests failed with `agentConnections.restartChanged`.
- Tests also cover unsuccessful stopped-app launches and Windows installation candidates preserving the selected product identity.
- Final local checks passed: `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit` (4,257 tests in 318 files), `pnpm format:check`, `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, and `cargo test --manifest-path src-tauri/Cargo.toml` (7,821 passed, 21 ignored; includes 7,656 passing library tests and 165 integration tests).
- The unchanged model-default helpers passed `pnpm test:model-default` (16 tests) in the combined check. All five new restart regressions passed in the final full Rust run. Source fingerprints matched after all checks.
- Logs: `/tmp/varyai-stopped-restart-before.log`, `/tmp/varyai-stopped-restart-frontend.log`, `/tmp/varyai-stopped-restart-backend.log`, `/tmp/varyai-two-model-bugs-helpers.log`. An existing test-only dead-code warning in `quota_panel` and nonfatal macOS linker warnings remain; the commands exited successfully.
- Pre-commit helper checks passed again: `pnpm test:dev` (8), `pnpm test:macos-dmg` (5), `pnpm test:trae-bridge` (66), `pnpm test:model-default` (16), `pnpm test:workbuddy-checkin-auth` (8), `pnpm test:doubao-checkin` (16), `pnpm test:agent-rewards` (11), `pnpm test:qoder-codec` (3), and `python3 -B scripts/tests/recover-shared-v23.test.py` (5). `pnpm audit --prod` reported no known vulnerabilities. Logs use `/tmp/varyai-precommit-*.log`.
- The Windows native cross-compiler is unavailable on this host. Windows orchestration tests ran through synthetic runtimes; native Windows execution and remote CI are not claimed.

## Native self-test

Build the updated VaryAI normally. No installed application was replaced or restarted during this investigation.

1. With WorkBuddy AI running, change its default model and click **Restart**. Confirm the application exits and returns without a configuration-state error.
2. Fully quit WorkBuddy AI, then use the pending **Restart** action in VaryAI. Confirm the international application opens directly; the domestic application is unaffected.
3. Repeat the stopped-app case for another supported desktop Agent. For Codex, enable routing only for the test and turn it off afterward.
4. Verify a refused exit or unavailable installation produces an actionable message and is not reported as success.

Native Windows behavior and Linux backend checks remain unverified locally. No push or remote CI validation is included in this check.
