# Pi session correlation, Qoder tools and WorkBuddy readiness

## Reports and observed boundaries

The user observed Pi risk events with no session ID, no Qoder CN warning for an enabled `kubectl delete` prefix blacklist, and a WorkBuddy AI restart button that remained after a restart. Read-only Computer Use confirmed Codex routing remained off. WorkBuddy AI was version 5.11.4; its model-control receipt matched the current plan but reported `ready: false`, while domestic WorkBuddy and Qoder reported ready.

Static inspection of the installed Pi 1.5.2 client established that Gemini requests omit a default native session ID, while its official extension API exposes `before_provider_headers` and `sessionManager.getSessionId()`. Qoder CN's bundled worker declares native `Bash` and its `command` input. WorkBuddy's generic conversation list aggregates available local and cloud providers; native `listScope("local", options)` queries only the local provider. Depending on cloud availability is inappropriate for updating existing local conversation models.

## Repairs

- Active shared Pi connection installation/refresh and native Pi provider routing now install an isolated official extension. It supplies the current native session ID only to local VaryAI Pi routes, removes stale private metadata on provider switches, and never changes prompts, tools, account settings or model definitions. The extension remains inert outside those local routes. Installation errors are auxiliary warnings and do not stop forwarding. Existing foreign files and symlinks are preserved.
- Pi session extraction recognizes the private header and its stable native affinity-header variants, with bounded ID validation and duplicate-header refusal. Per-request IDs remain ineligible. Existing missing event IDs are not relabeled. Private correlation metadata is not forwarded to upstream providers.
- Qoder CN joins the explicit native `Bash` schema mapping, client-selector validation and event client projection. This does not recognize arbitrary MCP tools or PowerShell semantics.
- WorkBuddy migration now uses the native local conversation scope instead of aggregating cloud providers. WorkBuddy adapter revision advances to 3; Qoder retains revision 1. Exact model write/readback remains mandatory. A missing model confirmation is shown separately from process-age restart requirements in all four runtime languages; the retry restart action remains available rather than silently marking an unconfirmed model ready.

## Scope audit

| Agent | Registered native shell tool | Session correlation boundary |
| --- | --- | --- |
| Claude Code / Claude Desktop | `Bash` | Recognized client session headers or metadata |
| Codex | `exec_command`, `shell_command` | Recognized thread/session headers or metadata |
| Gemini CLI | `run_shell_command` | Recognized metadata when supplied; no unconditional ID guarantee |
| Pi | `bash` | Native session extension plus stable headers/metadata |
| WorkBuddy / WorkBuddy AI | `Bash` | Native conversation header or recognized metadata |
| Qoder CN | `Bash` | Existing Qoder session-header/body extraction |
| Grok Build, OpenCode, OpenClaw, Hermes, Kimi, Qwen, ZCode, DeepSeek Harness, T3 Code, TRAE editions, Doubao Work | No complete registered shell adapter for these original client identities | Do not infer a risk-record or session guarantee from provider protocol alone |

This table describes code coverage, not native acceptance on every listed application. A risk event requires a supported observed tool call through VaryAI and an applicable enabled rule. The feature warns and records; it does not authorize or block execution.

## Verification

Pi header extraction, the official Pi helper, local-only WorkBuddy migration and pending-model UI labeling were reproduced with failing tests before their repairs. Qoder coverage was established by native source inspection and expanded schema, worker and shared-routing regressions.

An isolated real Pi 1.5.2 CLI invocation used a temporary agent directory, the final extension source and a dynamic-port local Gemini fixture. The emitted session header matched an independently observed native session-manager ID. The client exited successfully; no installed Pi configuration or user conversation was changed, and no request went to an external model provider. Only boolean verification results were retained at `/tmp/varyai-pi-session-sdk-final.json`.

Final checks passed on macOS 26.6.2 with Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3, for the working-tree changes on `488966bf590d15592a8f4c673aa4f507485d19c9`. Source/configuration fingerprints at `/tmp/varyai-pi-qoder-restart-hashes.json` matched the final checked tree.

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit` (4,257 tests, 318 files), and `pnpm format:check` passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, and `cargo test --manifest-path src-tauri/Cargo.toml` passed. The complete backend suite passed 7,827 tests (7,662 library and 165 integration), with 21 ignored and zero failures. Existing test-only dead-code and nonfatal macOS linker warnings remain.
- Helper suites passed: `test:dev` (8), `test:macos-dmg` (5), `test:trae-bridge` (66), `test:model-default` (17), `test:pi-session` (2), `test:workbuddy-checkin-auth` (8), `test:doubao-checkin` (16), `test:agent-rewards` (11), and `test:qoder-codec` (3). The Python shared-storage recovery suite passed 5 tests. `pnpm audit --prod` reported no known vulnerabilities.
- Logs: `/tmp/varyai-pi-qoder-restart-frontend.log`, `/tmp/varyai-pi-qoder-restart-final.log`, `/tmp/varyai-agent-followup-*.log`. Reproduction failures are recorded in `/tmp/varyai-pi-session-before.log`, `/tmp/varyai-pi-header-before.log`, `/tmp/varyai-workbuddy-local-scope-before.log`, and `/tmp/varyai-model-pending-before.log`. Native Windows execution, live Qoder acceptance and rebuilt WorkBuddy AI readiness acceptance remain pending. No commit or push is included in this follow-up.

## User self-test

1. Build and restart VaryAI. Reload or restart Pi so it loads the new extension. Keep the existing enabled `kubectl delete` blacklist.
2. Send the existing dry-run test twice in the same Pi session. Future risk records should show the same native session ID. Start a new Pi session and verify its ID differs. Old records remain unchanged.
3. Send the same dry-run test through Qoder CN. Confirm a warning and a Qoder risk record.
4. Restart WorkBuddy AI through VaryAI. After native model readback succeeds, its pending restart entry should clear. If model confirmation is still pending, the row must identify that condition rather than claiming that a newly started process is running old configuration.

## Pi installation follow-up

A new live Pi risk event still had no session ID. Read-only inspection found an active shared connection receipt but no session extension in its actual Pi directory. The missing helper was installed without changing settings, models or accounts; native reload and a fresh event remain required for live acceptance.

Code inspection also found that catalog synchronization can return before reaching the installer, so it cannot be the only recovery path for an existing connection. Gateway startup now independently ensures the helper for active shared or native Pi routing, before the already-running early return. An isolated dynamic-port regression reproduced missing installation before the fix and now covers active, suspended and absent receipts, repair while running, and preservation of existing settings. Tests can exercise the installer only with an explicitly isolated Pi directory; ordinary tests cannot install into the user's directory. The reproduction log is `/tmp/varyai-pi-startup-before.log`; focused validation is `/tmp/varyai-pi-startup-after.log`. The broader verification above predates this additional Rust change; its final backend checks are recorded separately at `/tmp/varyai-pi-startup-final.log`.

The final follow-up passed Rust formatting, `cargo check`, Clippy with warnings denied, and the complete backend suite: 7,828 passed, zero failed and 21 ignored. The Pi helper's two tests and renderer formatting also passed. The previous frontend/helper verification remains applicable because its source and configuration fingerprints are unchanged; the only subsequent runtime changes are the installer and gateway startup. Updated source/configuration fingerprints are recorded at `/tmp/varyai-pi-startup-hashes.json`. Native Windows execution and live Pi reload acceptance remain unverified. No application was packaged or replaced, and no commit or push was performed.

After the user reloaded Pi and repeated the dry-run test, Computer Use confirmed that the new 14:43:17 risk event contained a session ID. Historical missing IDs remained absent. This validates session delivery after helper installation and native reload; repeated same-session correlation, switching to a new session, and automatic startup delivery still have no live acceptance result.

The user then requested a `pi_` prefix in the session display. The unified list, its title and the event details now share a Pi-only display formatter, preserving an existing prefix and leaving absent IDs absent. Stored identities and request correlation remain unchanged. A component regression reproduced the missing prefix before the change and covers both raw and already-prefixed IDs across list and detail views.
