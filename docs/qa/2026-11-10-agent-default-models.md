# Agent default-model verification — 2026-11-10

Base: `bc9d14ca105fc9e6546a7c527873d63f988acb36` on `codex/intent-recognition`, with uncommitted default-model changes.

Earlier default-model implementation snapshot, before the single-dropdown UI follow-up: 24 changed runtime/test/configuration files. SHA-256 over sorted path names, NUL separators and each file's binary SHA-256: `1a508ea22542a13f6b078c10c8470545763d00208a2f2ba94e30c88514090b36`. Documentation is excluded from this runtime fingerprint.

Host: macOS arm64. Rust/Cargo 1.98.1, pnpm 10.12.3. The complete frontend run used Node 20.20.2; the final affected frontend, helper, type and renderer-build checks used Node 22.22.0.

## Local checks

- Complete frontend unit run before final focused fixes: 281 files, 3,639 tests passed. Final affected component run: 172 tests passed.
- Complete Rust library run before the final adapter revision/privacy test: 6,254 passed, 19 ignored. Final `cargo test --lib --manifest-path src-tauri/Cargo.toml agent_connections`: 86 passed, 1 ignored.
- Final `pnpm test:model-default`: 9 passed, using synthetic storage and private pipe streams.
- Final `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, and `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`, a local `cargo build --manifest-path src-tauri/Cargo.toml`, and the final `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- Final development launch compiled and ran the current source through `pnpm dev --no-watch`.
- `git diff --check`: passed.

## Authorized live verification

The user explicitly authorized enabling the local model-control bridges and restarting Vary AI, WorkBuddy AI and Qoder CN. The installed clients were initially stopped; the development Vary build was started through the repository development command. No installed Vary application was replaced.

WorkBuddy AI 5.9.4 and Qoder CN 0.5.7 were exercised through the Vary model selector, its device-local consent dialog and separate restart confirmation. The existing saved model was retained. Native interfaces visibly showed that model on the new-task composer after application. No inference prompts or messages were sent, and existing conversations were not edited.

Both clients were then normally quit and reopened without the control helper. Their model choices persisted and the saved readback remained valid. The final development build was subsequently restarted, and both clients were launched through its confirmed restart flow. Installed helper source bytes matched the final repository helper bytes; both exact adapter-revision readbacks succeeded. Vary showed both clients configured with no remaining restart button.

Live verification caught and fixed three issues, with failing synthetic tests followed by passing regressions:

1. An unauthorized client incorrectly marked its saved Vary model as already selected, preventing a click on that same model from opening consent.
2. WorkBuddy requires the native `custom-local:` model reference; writing only the configuration ID made its composer fall back to Auto.
3. Qoder's primary renderer uses `qoder-cn-app://renderer/index.html?workbenchScope=primary`, rather than only the legacy file URL. Other origins and workbench scopes remain excluded.

The user subsequently requested live checks for the remaining installed Agents, excluding Codex. No inference prompts were sent:

| Agent | Observed verification |
| --- | --- |
| Claude Code, Gemini, Grok Build, Qwen Code | Native interactive startup displayed the saved model in a fresh temporary directory. |
| Pi | Native RPC `get_state` returned the saved model. |
| OpenCode | Native `debug config` returned the saved model. The mini TUI was started, but its current-model display was not confirmed. |
| OpenClaw | `models status --json` returned matching default and resolved models with the installed Node 22 runtime. No gateway was started. |
| Hermes | Native `config get model` matched. Interactive startup was not verified; only the probe's own stalled child was stopped. |
| Kimi Code | Native model menu matched the saved choice. Existing draft retained. |
| Claude Desktop | Profile ordering/default flag matched the saved choice, but fresh-composer UI retained an earlier native choice. Runtime default selection remains unresolved. |
| ZCode | Provider configuration matched the saved default, but the fresh composer still requested a model choice. Runtime default selection remains unresolved. |
| TRAE CN | Client launched; Vary still reported bridge pending. Default selection is not verified. |
| DeepSeek Harness | Changing the model while running displayed Restart. The previous model was restored, the confirmed restart completed, and Restart disappeared. |
| Doubao Work | Audit-only integration; shared default selection does not apply. Official composer was not tested in this follow-up. |

Claude Desktop's native default flag, first-model ordering, active-profile observation, visibility fallback and baseline restoration are covered by Rust tests; those checks do not establish native composer compatibility. WorkBuddy domestic was not installed. Windows/Linux execution was not tested on this macOS host.

## Single-dropdown follow-up

Removed the separate read-only catalog trigger for WorkBuddy, WorkBuddy AI and Qoder CN. Moved Claude Desktop visibility management into its default-model dropdown with an explicit return action. Native Vary UI verification confirmed one outer model dropdown per affected row and working Desktop management/return navigation without modifying its visibility settings.

The final affected run passed 190 tests across `AgentConnectionsPage`, `AgentModelVisibility` and restart locale checks. The component tests cover the absence of duplicate triggers, Desktop visibility editing, returning to default selection, catalog browsing/search without writes, and existing connection/restart behavior. Type checking, renderer build, formatting and `git diff --check` passed after this UI change. No Rust implementation changed during this follow-up.

## Boundaries

The helper uses inherited private DevTools pipes and no TCP inspector listener. It reads/writes only general local new-chat model keys and a model-choice recovery journal. Permission and plans are device-local and excluded from profile exports. The native control check did not change account credentials, keychain access rules, vendor installation files or existing conversation settings. Actual account/configuration contents are intentionally omitted from this report.

Remote CI was neither triggered nor followed, as explicitly requested by the user. The remaining complete integration/helper/audit checks required for a future commit were not rerun for this uncommitted feature. Local results and live macOS UI verification do not establish cross-platform CI, installer, signing or release acceptance.

## WorkBuddy restart regression follow-up

A model change followed by the confirmed WorkBuddy AI restart reproduced `agentConnections.restartChanged` in the running development build. The helper-launched client had a verified bundle and live process, but AppKit reported no launch date. The lifecycle adapter treated that missing metadata as an unknown creation identity and rejected the restart.

The macOS adapter now obtains creation time from the verified PID through `proc_pidinfo(PROC_PIDTBSDINFO)`. It requires a complete structure, matching positive PID, a positive valid timestamp and checked millisecond conversion. LaunchServices records whose PIDs are confirmed absent with `kill(pid, 0)` / `ESRCH` are ignored during exit/reopen checks; live or permission-denied processes remain guarded. Bundle checks and replacement-process guards remain in force. Windows already uses native process creation time; its synthetic lifecycle tests were included in the affected run.

Validation after the change: 18 desktop lifecycle tests passed, including the new kernel-time regression and existing unknown/replacement/configuration-conflict cases. Clippy with `-D warnings`, Rust formatting and `git diff --check` passed. The final development application was rebuilt and launched from this checkout. Two consecutive confirmed WorkBuddy AI restarts succeeded, including another model change between launches. The restart dialog closed successfully each time; persisted readback matched the desired revision, and the restart button disappeared. The native New Task composer displayed the final model, restored to the choice observed before the probe. No prompt was submitted and no existing task or account setting was edited.

Checked Rust source for the final lifecycle tests, Clippy and live build: `src-tauri/src/services/agent_connections/desktop.rs`, SHA-256 `fd3c788bb08f3ed469fb441b73a787afd611d921db81a00a6cadcc97c4a02fff`. No Rust source changed after those checks.

A separate startup behavior was observed while reloading Vary: several per-Agent model choices reverted to the global default. The previously observed choices for Claude Desktop, Gemini, Grok Build, Hermes, ZCode and DeepSeek Harness were restored through their normal model dropdowns. This lifecycle-only fix does not resolve that separate startup-selection issue.

## Commit preparation on the current remote base

The working branch was fast-forwarded to `72ce390fa473087505934f91ed77728fac51ef01` before submission, preserving the two newer remote commits. Conflicts in the Agent page were resolved by retaining the upstream recovery presentation and this change's single model dropdown, local-control consent and completed-check-in behavior.

Host: macOS arm64, Node 22.22.0, pnpm 10.12.3, Rust/Cargo 1.98.1. The 32 changed runtime/test/configuration files after integration have aggregate SHA-256 `6a9bf761acc1ec2f581c9ae9e0548958b282c2af7c892f57878f35d9029e976f`, using the path/hash method above. Documentation is excluded.

On that integrated tree, `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`, all eight documented JavaScript helper suites, `python3 -B scripts/tests/recover-shared-v23.test.py`, `pnpm audit --prod`, and `cargo fmt --check --manifest-path src-tauri/Cargo.toml` passed. The production audit reported no vulnerabilities.

The user explicitly requested stopping tests and pushing immediately. The running complete frontend test suite and Cargo check were therefore stopped; final integrated Clippy and complete Rust library/integration runs did not complete. The earlier focused and live results above remain historical evidence, not full acceptance of this integrated revision. No further tests were run after that instruction, and remote CI was neither manually triggered nor followed. Verification remains incomplete.

The check-in follow-up's earlier focused run passed 237 tests across the Agent page and three check-in panel suites. It covered confirmed and already-completed claims, disabled buttons and labels, status refresh into the next reward period, and multi-account access until all accounts complete. Native UI observation confirmed the completed TRAE button displayed Checked in and was disabled.
