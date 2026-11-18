# Agent feature coverage

This is a required implementation and acceptance ledger, not a declaration that every listed feature is supported. Follow the mandatory Agent coverage rules in [AGENTS.md](../../AGENTS.md). Derive the inventory from `src/config/appConfig.tsx`, `AppType` and the connection registries, including native integrations outside the shared `APPS` list.

For every Agent-facing change, assess all entries below against its entry point, protocol/native adapter, runtime behavior, UI state and supported platforms. Record implementation, synthetic regression and native acceptance separately. New Agents must be added here and assessed against existing cross-Agent features. Missing or unverified entries prevent an all-Agent completion claim unless the user explicitly accepts a narrower scope.

## Qoder reward-date refresh (2026-11-17)

Pre-implementation assessment: Qoder's persisted claim expires at 09:59 UTC+8,
but the date adapter requires an exact 10:00 end. Keep the exact 10:00 start
and accept an end in the following boundary's last minute. Do not derive a
date from a claim timestamp or an arbitrary campaign. The shared helper covers
native and browser Qoder accounts on every existing supported platform; this
change does not add native credential support. Verification is recorded in
[the follow-up report](2026-11-17-qoder-checkin-dot.md).

| Agent ID           | Entry point / adapter                               | Runtime and UI impact                                                                                     | Verification / native acceptance                                                                                          |
| ------------------ | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `claude-desktop`   | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `codex`            | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `gemini`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `grokbuild`        | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `opencode`         | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `openclaw`         | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `hermes`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `pi`               | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `workbuddy`        | Separate check-in API and official date history     | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `workbuddy-ai`     | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `qoder`            | AgentRewardsPanel; native/browser qoderCampaigns    | Manual/status/automatic claims retain the confirmed reward date; current dot updates from returned status | Helper and renderer regressions passed; macOS native account currently protected; Windows/Linux native acceptance pending |
| `doubaowork`       | Separate native check-in adapter                    | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `trae`             | Shared reward panel; separate native bridge adapter | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `trae-intl`        | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `kimi`             | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `qwen`             | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `zcode`            | Shared reward panel; separate plan adapter          | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `deepseek-harness` | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `t3code`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |

## Statistics maintenance panel removal

The statistics dashboard removes the cost-pricing accordion and Codex usage
maintenance accordion, including the rebuild confirmation dialog. This is a
shared renderer change on macOS, Windows and Linux. Automatic session scanning,
manual synchronization when scanning is disabled, recorded costs and backend
pricing/rebuild APIs retain their existing behavior. Inventory was checked against
`src/config/appConfig.tsx` and `services/agent_connections.rs` before implementation.

Each row uses the statistics dashboard entry point. Existing protocol/native
adapters are unchanged; this UI removal does not invoke an adapter or require
native client execution. Synthetic verification passed on macOS for each Agent with
automatic scanning enabled and disabled, absent maintenance controls, visible
request logs, manual-sync visibility and the scan toggle callback. Native client
self-tests and Windows/Linux execution are not exercised by this change and are
not claimed as accepted.

| Agent ID | Entry point | Protocol/native adapters and runtime | UI state | Verification |
| --- | --- | --- | --- | --- |
| `claude` | Statistics; CLI | Unchanged Messages adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `claude-desktop` | Statistics; desktop | Unchanged desktop Messages adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `codex` | Statistics; CLI/desktop | Unchanged Responses, rollout accounting and rebuild API | Remove both panels and rebuild dialog; retain scan controls | Synthetic passed (macOS); native untested |
| `gemini` | Statistics; CLI | Unchanged Gemini adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `grokbuild` | Statistics; CLI | Unchanged provider-selected adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `opencode` | Statistics; CLI | Unchanged provider-selected adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `openclaw` | Statistics | Unchanged provider-selected adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `hermes` | Statistics; CLI | Unchanged Chat adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `pi` | Statistics; CLI | Unchanged provider-selected adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `workbuddy` | Statistics; China desktop | Unchanged Chat/native adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `workbuddy-ai` | Statistics; international desktop | Unchanged Chat/native adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `qoder` | Statistics; China desktop | Unchanged Chat/native adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `doubaowork` | Statistics; desktop | Unchanged separate native audit/launch integration | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `trae` | Statistics; China edition | Unchanged native Chat bridge and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `trae-intl` | Statistics; international edition | Unchanged native Chat bridge and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `kimi` | Statistics; CLI/desktop | Unchanged native provider projection and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `qwen` | Statistics; CLI | Unchanged Chat adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `zcode` | Statistics; desktop | Unchanged native protocol projection and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `deepseek-harness` | Statistics | Unchanged Chat/Responses/Messages adapters and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |
| `t3code` | Statistics; desktop | Unchanged Responses/managed Codex adapter and accounting | Remove both panels; retain scan controls | Synthetic passed (macOS); native untested |

Verification on 2026-11-17 used the working-tree changes to `UsageDashboard.tsx`,
`UsageDashboard.test.tsx`, this ledger and `docs/usage.md` over revision
`8207f3efd7dc5eaf28ec191f7d9b2eafde6f704c`. Host: macOS 15.3.1 arm64;
Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

- Passed: `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`,
  `pnpm test:unit` (322 files, 4,339 tests), `pnpm audit --prod`,
  `cargo check --manifest-path src-tauri/Cargo.toml` and
  `cargo fmt --check --manifest-path src-tauri/Cargo.toml`.
- Passed helper checks: `pnpm test:dev`, `pnpm test:macos-dmg`,
  `pnpm test:trae-bridge`, `pnpm test:model-default`, `pnpm test:pi-session`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`,
  `node --test scripts/tests/ci-policy.node.mjs` and
  `python3 -B scripts/tests/recover-shared-v23.test.py`.
- Incomplete: `cargo test --lib --manifest-path src-tauri/Cargo.toml`,
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and
  `cargo test --manifest-path src-tauri/Cargo.toml` failed during compilation or
  linking with disk-full errors. Failed compilation objects were cleaned up.
  No native client self-tests or remote CI runs were performed; Windows/Linux
  verification remains untested. No application was launched or installed.

## Catalog refresh and continuation ownership

Assessment started on 2026-11-17 before implementation. The implemented fix
excludes only `modelCatalog` from a forwarding destination's continuation identity. Authentication,
actual destination changes, ambiguous owners and opaque-state checks remain in
force. Existing model pickers and connection controls remain the entry points;
no new UI action or native configuration format is required. Shared Rust behavior
applies to macOS, Windows and Linux wherever the existing Agent adapter is
supported. Native application acceptance is separate from synthetic HTTP tests.

| Agent ID | Entry point and protocol/native adapter | Pre-fix runtime risk and current UI | Verification |
| --- | --- | --- | --- |
| `claude` | Shared connection; Messages; CLI | Catalog refresh can invalidate strict ownership; existing picker | HTTP refresh/restart passed; native unverified |
| `claude-desktop` | Shared connection; Messages; desktop aliases | Observed local 409 after catalog sync; existing picker | HTTP refresh/restart passed; native macOS/Windows unverified |
| `codex` | Managed and native Responses; CLI/desktop | Replay avoids some 409s, but catalog changes can lose affinity | HTTP refresh/restart passed; native unverified |
| `gemini` | Shared connection; Gemini CLI | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `pi` | Shared connection; provider-selected protocols; CLI | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `workbuddy` | Shared connection; Chat; China desktop | Visible text recovery exists; tool/opaque history remains exposed | HTTP refresh/restart passed; native macOS/Windows unverified |
| `workbuddy-ai` | Shared connection; Chat; international desktop | Same strict tool/opaque history as China edition | HTTP refresh/restart passed; native macOS/Windows unverified |
| `qoder` | Shared connection; Chat; China desktop | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native macOS/Windows unverified |
| `kimi` | Shared connection; native provider projection; CLI/desktop | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `qwen` | Shared connection; Chat; CLI | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `opencode` | Shared connection; provider-selected protocols; CLI | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `grokbuild` | Shared connection; provider-selected protocols; CLI | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `openclaw` | Shared connection; provider-selected protocols | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `hermes` | Shared connection; Chat; CLI | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `zcode` | Shared connection; native protocol projection; desktop | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `deepseek-harness` | Shared connection; Chat/Responses/Messages; native credentials | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `t3code` | Shared connection; Responses; managed Codex instance | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native unverified |
| `trae` | Native bridge to shared Chat; China edition | Observed call/payload ownership still depends on target identity | HTTP refresh/restart passed; native macOS/Windows unverified |
| `trae-intl` | Native bridge to shared Chat; international edition | Strict continuation ownership uses shared target identity | HTTP refresh/restart passed; native macOS/Windows unverified |
| `doubaowork` | Separate native audit/launch adapter | No shared model-catalog forwarding destination; not affected by this identity change | Source assessment; native unverified |

Implementation and verification details are recorded in
[the continuation refresh report](2026-11-17-catalog-continuation.md).

## Desktop update exit feedback

Assessment started on 2026-11-17 before implementation. An update that waits for
its target application to quit is deferred, with a neutral hint on that Agent's
version row. It does not produce a global failure banner or count as a successful
update. Existing updater eligibility, native replacement/rollback and shared
update guards remain in effect. macOS exit checks must observe kernel process
exit rather than cached AppKit termination metadata. Windows automatic desktop
replacement remains unsupported; shared feedback applies wherever an updater
returns the recognized deferred outcome.

| Agent ID | Entry point and protocol/native adapter | Runtime and UI assessment | Verification |
| --- | --- | --- | --- |
| `claude` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `claude-desktop` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `codex` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `gemini` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `pi` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `workbuddy` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `workbuddy-ai` | Existing version row and native/regional adapter | No existing automatic desktop update path; retain unsupported/absent behavior on every platform | Source assessment and shared update regressions passed; native not rechecked |
| `qoder` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `kimi` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `qwen` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `opencode` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `grokbuild` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `openclaw` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `hermes` | Tools version row; existing CLI/native adapter | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths | Source assessment and shared update regressions passed; native not rechecked |
| `zcode` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `deepseek-harness` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `t3code` | Existing version row and native/regional adapter | No existing automatic desktop update path; retain unsupported/absent behavior on every platform | Source assessment and shared update regressions passed; native not rechecked |
| `trae` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `trae-intl` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `doubaowork` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |

See [the update feedback report](2026-11-17-desktop-update-feedback.md).

## Desktop startup probe compatibility

Assessment started on 2026-11-17 before implementation. The exact authenticated
Claude Desktop startup probe must be isolated from saved conversation ownership
and may retry once after an explicit upstream requirement to enable thinking. Actual upstream success remains required. Ordinary requests,
credential failures and security decisions retain their existing behavior. The
adapter runs in shared Rust on macOS and Windows; native acceptance is separate.

| Agent ID | Entry point and protocol/native adapter | Runtime and UI assessment | Verification |
| --- | --- | --- | --- |
| `claude` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `claude-desktop` | Authenticated native startup probe; Messages to Messages/Chat/Responses/Gemini | Implemented isolated request-local probe state and bounded compatibility retry; native gateway banner clears only on upstream success | 22 shared-ingress/recovery regressions passed on macOS, including stale ownership, four upstream protocols and both policy states; installed old backend returned 409; updated macOS startup check accepted with banner removed; native Windows pending |
| `codex` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `gemini` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `pi` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `workbuddy` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `workbuddy-ai` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `qoder` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `kimi` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `qwen` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `opencode` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `grokbuild` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `openclaw` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `hermes` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `zcode` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `deepseek-harness` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `t3code` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `trae` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `trae-intl` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |
| `doubaowork` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked |

See [the startup probe report](2026-11-17-desktop-startup-probe.md).

## Tool-call risk warnings: current gaps

The full feature remains incomplete. Gateway connectivity and DLP text inspection do not establish tool-call observation. Registered native shell semantics require structured tool calls and a matching enabled risk policy. Warnings and records observe proposed calls; they do not block local execution.

| Agent ID           | Agent                   | Implementation status                                                                                                                      | Native acceptance evidence                                                                                                                                                                                                                                        |
| ------------------ | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | Claude Code             | Registered `Bash` schema and shell semantics                                                                                               | Not rechecked in this follow-up                                                                                                                                                                                                                                   |
| `claude-desktop`   | Claude Desktop          | Registered `Bash` schema and shell semantics                                                                                               | Not rechecked in this follow-up                                                                                                                                                                                                                                   |
| `codex`            | Codex                   | Registered `exec_command` and `shell_command`                                                                                              | Not rechecked in this follow-up; keep the user's route off                                                                                                                                                                                                        |
| `gemini`           | Gemini CLI              | Registered `run_shell_command`                                                                                                             | Not rechecked in this follow-up                                                                                                                                                                                                                                   |
| `pi`               | Pi                      | Registered `bash`; native session helper delivered                                                                                         | User confirmed warning/session ID and `pi_` display acceptance                                                                                                                                                                                                    |
| `workbuddy`        | WorkBuddy China         | Registered `Bash`                                                                                                                          | A native risk event with a conversation ID was observed                                                                                                                                                                                                           |
| `workbuddy-ai`     | WorkBuddy International | Registered `Bash`                                                                                                                          | A native risk event with a conversation ID was observed                                                                                                                                                                                                           |
| `qoder`            | Qoder CN                | Registered `Bash`                                                                                                                          | Native retest remains pending                                                                                                                                                                                                                                     |
| `kimi`             | Kimi Code               | Registered native `Shell`; schema, policy and shared-routing regressions passed                                                            | User reported no warning; native retest pending                                                                                                                                                                                                                   |
| `qwen`             | Qwen Code               | Registered native `run_shell_command`; schema, policy and shared-routing regressions passed                                                | User reported no warning; native retest pending                                                                                                                                                                                                                   |
| `opencode`         | OpenCode                | Registered native `shell` and legacy `bash` string-command schemas on shared routes                                                        | User confirmed native tool-warning and session-ID acceptance passed on 2026-11-17; nested code-mode calls and other platforms remain unverified                                                                                                                   |
| `grokbuild`        | Grok Build              | Registered `run_terminal_command`, internal compatibility alias `run_terminal_cmd` and Cursor preset `Shell`                               | User confirmed native tool-risk acceptance passed on 2026-11-17; other platforms and presets remain unverified                                                                                                                                                    |
| `openclaw`         | OpenClaw                | Registered native `exec` string-command schema on shared routes                                                                            | User requested skipping OpenClaw on 2026-11-17; native acceptance remains unverified; nested code-mode calls remain unverified                                                                                                                                    |
| `hermes`           | Hermes                  | Registered native `terminal` string-command schema on shared routes                                                                        | User confirmed native tool-warning and session-ID acceptance passed on 2026-11-17; scheduled/delegated scope and other platforms remain unverified                                                                                                                |
| `zcode`            | ZCode                   | Registered native `Bash` string-command schema on shared routes                                                                            | User confirmed the instructed native tool-risk acceptance passed on 2026-11-17; automated regressions remain unexecuted                                                                                                                                           |
| `deepseek-harness` | DeepSeek Harness        | Registered native `bash`; added a native request-header extension and validated session-header extraction                                  | User confirmed native warning and session-ID acceptance passed on 2026-11-17; PowerShell/interactive variants remain unverified                                                                                                                                   |
| `t3code`           | T3 Code                 | Registered `exec_command`/`cmd` and `shell_command`/`command` for the managed Codex driver, including Responses routing                    | User confirmed completion of native tool-warning testing on 2026-11-17; the session-header and prefix repairs are implemented, but final session-ID appearance was not separately confirmed; regressions remain unexecuted and other T3 drivers remain unverified |
| `trae`             | TRAE CN                 | Native `PreToolUse` / `RunCommand` observer with edition-bound authentication, supplied session identity and asynchronous risk persistence | User confirmed native warning delivery after enabling Configured Hooks on 2026-11-17; `trae_cn_` session-prefix correction is implemented, awaiting new-record retest                                                                                             |
| `trae-intl`        | TRAE International      | Separate native `PreToolUse` / `RunCommand` observer; existing user hooks and permissions preserved                                        | User reported routing_unknown_continuation / 409 after a tool call on 2026-11-17; edition-parity continuation repair is implemented, awaiting new-build retest; native warning delivery remains unverified                                                        |
| `doubaowork` | Doubao Work | Legacy `Bash` lifecycle retained; current-runtime shell report collector added with `doubaowork_` session identity and truthful reported stage | **Implemented; new-build native notification acceptance pending.** Previous native failure reproduced; actual report schema confirmed on macOS. Windows native acceptance pending. |

Shared Rust implementation applies across platforms, but native Windows/macOS/Linux acceptance must be recorded for each applicable path. A passed macOS fixture is not Windows acceptance. Do not infer session availability where a client supplies no recognized stable identity, and never substitute request IDs or generated per-request IDs.

Required verification for each missing adapter includes a structured native schema, JSON/SSE transport as applicable, enabled and disabled policy, malformed/duplicate/namespaced definitions, bounded collection, preserved forwarding, client identity, session correlation when supplied, persisted event and notification delivery. Native collectors must distinguish a proposed call from executed output and preserve existing security decisions and user configuration.

Kimi's native `Shell` command input is defined by its [official shell implementation](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/tools/shell/__init__.py). Qwen's `run_shell_command` identity and string command schema are defined in its [official tool names](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-names.ts) and [shell implementation](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/shell.ts). These contracts establish the bounded adapters; they do not prove native acceptance on the user's installed versions.

## Current follow-up evidence and limits

The seven new shared-route adapters use native string-command definitions rather than matching tool-name text in chat messages. Their schema, catalog-boundary, client-policy and shared-routing regressions were expanded, including the managed T3 Responses path. The installed ZCode `Bash` definition, DeepSeek Harness `dsh-tool-bash` packages, T3 Codex projector, OpenClaw `execSchema`, Hermes terminal schema, OpenCode shell declaration and Grok Build embedded tool/preset documentation establish these contracts. [OpenCode tools](https://opencode.ai/docs/tools/) and the [Hermes terminal implementation](https://github.com/NousResearch/hermes-agent/blob/main/tools/terminal_tool.py) provide primary references. This does not establish coverage for arbitrary JavaScript code-mode tools, PowerShell-specific syntax, interactive terminal input or other unregistered tools.

TRAE uses its [official pre-execution hook contract](https://docs.trae.cn/ide_hook-configuration-reference). With existing connection consent and live risk collection enabled, VaryAI adds an owned global `RunCommand` hook for each edition, using separately authenticated loopback endpoints. Disabled collection/consent removes the owned entry; user-edited and unrelated hooks remain untouched. The helper emits no permission decision, never rewrites tool arguments and bounds observation to 750 ms. Native Hook settings and sandbox restrictions are not overridden. Installation does not prove that the client has enabled or loaded the Hook.

Doubao Work's background bundle retains a legacy ToolCallManager lifecycle; its presence does not establish that the current native execution runtime uses it. Read-only inspection on 2026-11-17 confirmed its nine lifecycle observers and the `onBeforeExecution`/`onExecutionComplete` contract without installing an observer or executing a command. The new collector discovers that contract, observes only native-instance `Bash` calls, extracts the supported command field and supplied conversation ID, and removes its observer on shutdown/reconnect. It does not read chat histories, modify signed calls, change permissions, pause the debugger or buffer network responses. Windows uses the same observer when its existing loopback debugging connection is available; configuring or launching that connection on Windows remains outside the existing macOS-only launch helper.

The user requested no test execution in this follow-up. Added regression cases are unexecuted, and native warning delivery and applicable Windows/Linux acceptance remain pending. Do not reuse the earlier committed tree's passing results for this working tree.

Initial verification on 2026-11-17, before the subsequent Harness session-ID repair, covers the uncommitted tool-risk follow-up over `1c53f58f`, on macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3:

- `cargo check --lib --manifest-path src-tauri/Cargo.toml`: passed during implementation.
- `cargo check --tests --manifest-path src-tauri/Cargo.toml`: passed against that initial source snapshot, including compilation of its added regressions; no tests were executed. The existing unused `quota_panel::Manager::expiring` warning remains outside this change.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, Prettier checks for the changed helper scripts/documents, `node --check` for all four new JavaScript files, and `git diff --check`: passed.
- Full unit/integration test execution, renderer checks, Clippy, production dependency audit, platform-native warning delivery, Windows/Linux jobs and CI acceptance were not performed in this follow-up. No application was built, installed or restarted for acceptance; Codex routing was left untouched.

## Harness session-ID acceptance follow-up

The user confirmed that the native risk warning appears, but the tested build recorded no session ID. The installed agent loop passes its native session ID into pi-ai. Its configurable compatibility gate withholds automatic session-affinity switches for Chat/Anthropic, so the repair uses the supported synchronous `llm-pi-ai/request-headers` extension instead of unsupported provider configuration. The extension forwards only the actual session ID to local Harness connection URLs through `x-vary-harness-session-id`; it never reads histories or credentials, derives identity from cache/request IDs, or exports that private header upstream. Existing native session headers are also recognized, and stored audit/tool IDs retain the `deepseek-harness_` prefix.

The owned Cordis extension entry is journaled separately from foreign plugins, preserves pre-extension receipts, and is removed when the connection is restored. Its isolated module is installed without replacing unexpected content and repaired if missing for an active connection. After a new build, reconnect Harness and restart it once to load the extension, then generate a new event; historical missing IDs are not backfilled. Regression cases for the header waterfall, native schema round trips, restoration, supplied-session correlation, invalid/duplicate headers and upstream privacy were added but not executed. JavaScript syntax and diff-whitespace checks passed. `cargo check --tests --manifest-path src-tauri/Cargo.toml` passed for this follow-up on the same macOS/Rust host; tests were compiled but not executed, and the existing unrelated `quota_panel::Manager::expiring` warning remains. The user subsequently confirmed native warning and new-record session-ID acceptance passed on 2026-11-17. Windows/Linux verification remains pending.

## T3 Code session-ID follow-up

Managed T3 Code uses the native Codex Responses driver. Session correlation now recognizes its `thread-id`, `session-id`, `session_id` and `x-session-id` headers, with supported metadata as a fallback, and retains the `t3code_` audit prefix. Extraction validates bounded identities and rejects repeated header values; request IDs, response IDs and cache keys are never substituted. Shared-routing regressions now supply the native thread header instead of synthetic metadata. Existing missing IDs are not backfilled. Implementation is shared across platforms; native acceptance requires a new event after installing the updated VaryAI build.

## WorkBuddy native catalog additions (2026-11-18)

Both WorkBuddy editions previously compared the complete model array with the
connection receipt. Adding unrelated native models could therefore report a lost
connection even though every managed model remained unchanged. Receipt loading
now accepts additive native rows and row reordering while preserving these rows
in the disconnect/suspend restoration baseline. Existing row changes, removals,
duplicate IDs, newly added managed rows and unrelated object-field changes still
require recovery. Status reads do not write client configuration or advance its
application timestamp.

The inventory below was checked against `src/config/appConfig.tsx` and the
backend connection registries. Entry points are existing connection status,
reload, suspension and disconnect. The new adapter is limited to both WorkBuddy
editions and handles array/object catalogs using shared macOS/Windows code;
existing platform support is preserved. There are no changes to tool security,
forwarding protocols, model parameters, credentials, session correlation or
notifications. Other Agent adapters and their UI states are unchanged.

| Agent              | Variant                         | Adapter, runtime and UI impact                                                 | Verification / native acceptance                                                       |
| ------------------ | ------------------------------- | ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- |
| `claude`           | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `claude-desktop`   | Desktop                         | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `codex`            | CLI/desktop; existing WSL paths | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `gemini`           | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `grokbuild`        | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `opencode`         | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `openclaw`         | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `hermes`           | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `pi`               | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `workbuddy`        | China desktop                   | Adopt native catalog additions; preserve connection, timestamp and restoration | Array/object status, reload, conflict and disconnect; array suspend; native unverified |
| `workbuddy-ai`     | International desktop           | Adopt native catalog additions; preserve connection, timestamp and restoration | Array/object status, reload, conflict and disconnect; array suspend; native unverified |
| `qoder`            | China desktop                   | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `doubaowork`       | Desktop; separate native audit  | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `trae`             | China native bridge             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `trae-intl`        | International native bridge     | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `kimi`             | CLI/desktop                     | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `qwen`             | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `zcode`            | Desktop                         | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `deepseek-harness` | Native harness                  | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `t3code`           | Desktop; managed Codex          | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |

The new regression first failed against the old implementation. Earlier focused
validation over `2adbcd63` passed 149 connection tests with one ignored test.
Final verification uses the working tree over `81763452`, after fast-forwarding
`origin/main` and aligning `package.json`, the Rust package/lock entry and Tauri
configuration to version `0.4.0`. Host: macOS 26.6.2 arm64, Node 22.22.0,
pnpm 10.12.3, Rust/Cargo 1.98.1. Passed: `pnpm typecheck`,
`pnpm format:check`, `pnpm build:renderer`, `pnpm test:unit` (322 files,
4,376 tests), `pnpm audit --prod`, all documented helper suites (`test:dev`,
`test:macos-dmg`, `test:trae-bridge`, `test:model-default`, `test:pi-session`,
`test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`,
`test:qoder-codec`, CI policy and shared-v23 recovery), Rust formatting,
`cargo check` and `cargo clippy -- -D warnings` with the application manifest.
`cargo test --lib --manifest-path src-tauri/Cargo.toml` passed 7,740 tests
with 21 ignored. `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`
passed 7,905 tests across 16 groups with 21 ignored and no failures.
The test build reported the existing unused-method and macOS unwind-table linker
warnings. Version consistency checks, formatting of the changed JSON manifests
and `git diff --check` also passed. This section was formatted
separately; inherited document-wide table formatting was preserved.

Native rebuilt-app acceptance remains unverified. Initial cross-platform CI
results for PR #68 are recorded in the shutdown-fixture section below. Local
checks do not establish native integration, installer or release acceptance.

## Qoder TLS shutdown regression fixture (2026-11-18)

[PR #68 CI run 44226715687](https://github.com/varium-ai/varyai/actions/runs/44226715687)
checked `51e83af94462a3cd403473556ebf16c6e5e264bd`. Frontend, macOS, Linux and
Windows-to-WSL2 jobs passed. The ordinary Windows job failed only
`proxy::qoder_tls::tests::stop_waits_for_socket_cleanup_beyond_the_old_deadline`
with Winsock 10048 / `AddrInUse`; its WorkBuddy catalog regression passed.

The shutdown fixture released an OS-assigned ephemeral port and then tried to
bind that address again, leaving a race with other parallel socket fixtures.
It now holds the original listener in an `Arc` and observes its lifetime through
a `Weak` reference. Shutdown must still wait for the 3,100 ms delayed task,
complete the original cleanup marker and release the original listener.
The resource assertions no longer depend on the address remaining unclaimed.
This only changes the test fixture; production shutdown, native adapters,
forwarding and UI behavior for all Agents in the inventory above are unchanged.

The updated fixture passed locally. A negative control temporarily restored the
historical three-second `timeout(task)` implementation and failed the retained
`cleaned` assertion as expected; the correct production implementation was then
restored byte-for-byte. The first negative-control build was interrupted by
missing shared-target artifacts, so the completed negative control and subsequent
backend checks use an independent `CARGO_TARGET_DIR`, dev/test debug information
set to zero (as in CI), and incremental compilation disabled. Host: macOS 26.6.2
arm64, Rust/Cargo 1.98.1, Node 22.22.0, pnpm 10.12.3.

Frontend typecheck, renderer build, formatting, production dependency audit and
all documented helper checks passed. The first full frontend run encountered two
10-second subprocess timeouts in the unchanged mocked notarization fixture;
its 21 tests passed independently, and the final full rerun passed all 4,376 tests
across 322 files. No notarization test or timeout was changed.

Backend formatting, compile and Clippy checks passed. `cargo test --lib` passed
7,740 tests with 21 ignored; `cargo test --no-fail-fast` passed 7,905 tests across
16 groups with 21 ignored. All Cargo commands used `--manifest-path
src-tauri/Cargo.toml`. The final production prefix of `qoder_tls.rs` matches
`51e83af9` exactly, and diff whitespace checks passed. Native Agent self-tests
remain unverified. Follow-up cross-platform acceptance must be checked on the
new head of [PR #68](https://github.com/varium-ai/varyai/pull/68), rather than
inferred from these local results.

## Restart freshness after a gateway restart

The suspended shared-connection resume path previously assigned a new application timestamp even when restoring the identical projection. This made a gateway restart look like a new configuration change and invalidated WorkBuddy and WorkBuddy AI persisted default-model confirmations. The resume path now retains the previous timestamp when startup-relevant configuration is unchanged and no client loaded the temporarily restored native configuration. Native macOS launch times and Windows process creation times detect clients opened during suspension; actual configuration changes and reopened clients still require loading the current projection. This changes automatic freshness reporting, not the unconditional explicit restart action. Previously invalidated confirmation markers require one client restart to establish current readback. Added regressions cover both WorkBuddy editions, all shared Agent configuration comparisons, mixed process ages and unknown launch times; test execution and native acceptance remain pending.

Verification for the subsequent T3 session and restart-freshness working-tree changes on the same macOS host: `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, Prettier for the two updated documents and `git diff --check` passed. `cargo check --tests --manifest-path src-tauri/Cargo.toml` was delayed by another Cargo process holding the build-directory lock and then stopped during dependency compilation; this snapshot has no completed compile acceptance. No tests were executed, and Windows/Linux verification and native retests remain pending. No commit, push, application installation or Codex route change was performed.

## Explicit configuration and reconnection

Source assessment covers every entry point. Native acceptance below is pending for this follow-up. Shared reconnection archives the old receipt, retains external edits as its new baseline and writes the current managed projection. Existing native consent, credential access and platform support remain applicable. TRAE's explicit replacement intent is distinct from ordinary model selection and background refresh.

| Agent ID           | Implementation path                                                                                   | Native acceptance in this follow-up                                       |
| ------------------ | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `claude`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `claude-desktop`   | Desktop-aware shared release, visibility validation and install                                       | Not rechecked                                                             |
| `codex`            | Explicit native Codex reconnect and durable selection                                                 | Not rechecked; keep the user's route off                                  |
| `gemini`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `pi`               | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `workbuddy`        | Desktop-aware shared release, rebuild and install                                                     | Not rechecked                                                             |
| `workbuddy-ai`     | Desktop-aware shared release, rebuild and install                                                     | Not rechecked                                                             |
| `qoder`            | Desktop-aware shared release, rebuild and install                                                     | Not rechecked                                                             |
| `opencode`         | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `grokbuild`        | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `openclaw`         | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `hermes`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `kimi`             | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `qwen`             | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `zcode`            | Shared release with native rule-envelope repair, rebuild and install                                  | Not rechecked                                                             |
| `deepseek-harness` | Native credential-aware release, rebuild and install                                                  | Not rechecked                                                             |
| `t3code`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `trae`             | Explicit configuration/retry requests native managed-model replacement; failures retain the intention | Read-only saved/native catalog match passed; new replacement path pending |
| `trae-intl`        | Explicit configuration/retry requests native managed-model replacement; failures retain the intention | Read-only saved/native catalog match passed; new replacement path pending |
| `doubaowork`       | Native audit connection/launch path; the product exposes no editable provider/model projection        | Not rechecked                                                             |

## Default-model restart controls (2026-11-17)

Pre-implementation review found T3 Code absent from the desktop restart registry despite having a desktop configuration adapter. The shared UI also discarded the backend `restartRequired` flag and hid pending restart actions during configuration recovery. Harness has a macOS bundle adapter; process discovery, native reload and stopped clients are separate states. No running Harness desktop instance was observed during the read-only inspection, so that observation cannot establish the state at the user's model switch. This change must not claim that every model switch requires restarting or that existing conversations automatically change their model.

| Agent              | macOS entry point                          | Windows entry point                          | Model-change UI assessment                                                                  |
| ------------------ | ------------------------------------------ | -------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Claude Code        | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| Claude Desktop     | Verified bundle                            | Native executable/package                    | Shared restart status; native retest pending                                                |
| Codex              | Verified bundle and reload state           | Native package and reload state              | Shared restart status; route untouched                                                      |
| Gemini CLI         | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| Grok Build         | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| OpenCode           | CLI integration                            | CLI integration                              | Desktop lifecycle absent; not claimed supported                                             |
| OpenClaw           | CLI integration                            | CLI integration                              | Reload/reopen CLI; no desktop process restart                                               |
| Hermes             | CLI integration                            | CLI integration                              | Reload/reopen CLI; no desktop process restart                                               |
| Pi                 | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| WorkBuddy          | Verified bundle and default-model readback | Native executable and default-model readback | Shared restart status; previous user acceptance predates this follow-up                     |
| WorkBuddy AI       | Separate verified bundle and readback      | Separate native executable and readback      | Shared restart status; previous user acceptance predates this follow-up                     |
| Qoder CN           | Verified bundle and readback               | Native executable and readback               | Shared restart status; native retest pending                                                |
| Doubao Work        | Audit launch/restart                       | Native lifecycle adapter absent              | Audit-only; no managed default-model dropdown                                               |
| TRAE CN            | Edition-bound bridge and bundle            | Edition-bound bridge and executable          | Shared restart status and native selection readback                                         |
| TRAE International | Separate bridge and bundle                 | Separate bridge and executable               | Shared restart status and native selection readback                                         |
| Kimi Code          | Verified desktop bundle; CLI also exists   | Native desktop lifecycle absent              | Desktop/CLI runtime distinction remains necessary                                           |
| Qwen Code          | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| ZCode              | Verified bundle                            | Native lifecycle absent                      | Shared restart status on macOS; Windows gap explicit                                        |
| DeepSeek Harness   | Verified bundle                            | Native lifecycle absent                      | Shared restart status; stopped/native-reloaded states have no stale-running-instance prompt |
| T3 Code            | Missing before this change                 | Missing before this change                   | Add verified bundle/native executable lifecycle and honor backend restart status            |

Session-label assessment: existing Agent identities remain unchanged. Pi's existing display normalization is extended to T3 Code so both historical raw IDs and new prefixed IDs display exactly one `t3code_` prefix in the event list and details. Missing IDs stay missing; filtering and persisted identity are not rewritten.

T3 Code now registers `com.t3tools.t3code` on macOS and standard Windows main/Alpha/Beta executable identities for freshness detection and explicit quit/reopen. Shared restart controls honor the backend flag independently of recovery state; configuration repair remains a separate action. No client was launched, restarted or reconfigured during this inspection. CLI-only agents still have no desktop restart adapter. Harness's stopped/unknown/native-reloaded cases are not established as fixed by these changes; its reported switch requires native retest while the desktop instance is running.

Verification for the final uncommitted restart-control and T3 session-label follow-up over `1c53f58f`, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, targeted Prettier checks, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. This supersedes the interrupted compilation recorded above for the earlier snapshot, including compilation of the prior WorkBuddy freshness and T3 session repairs. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Added regressions cover all UI Agent identifiers' backend restart requirement, T3 Windows installation identity boundaries and historical/already-prefixed T3 event labels in both list and details. Tests were compiled but not executed, as requested; Windows/Linux native builds, native restart/model-switch acceptance and full CI acceptance remain pending. No commit or push was made; Codex routing remains untouched.

## OpenCode session-ID follow-up (2026-11-17)

Pre-implementation assessment: the installed Homebrew OpenCode 1.19.5 binary sends `x-opencode-session-id` on inference requests, plus `x-session-affinity` and `X-Session-Id` for custom providers. Its built-in provider path also sends `x-opencode-session`. The audit extractor previously accepted only generic body metadata for OpenCode. Read-only inspection of the bundled request builder confirms these fields contain the native session ID; request, parent-session and prompt-cache identities have different scopes and must not replace it. The fix is shared across macOS, Windows and Linux, requires no client extension or account/configuration modification, and must preserve the original request.

The existing all-Agent inventory remains applicable: OpenCode alone gains recognition of its two native private headers; other Agent extractors and native collectors keep their existing contracts. Regressions must check cross-Agent isolation against `AppType::all()`, invalid/duplicate IDs, metadata compatibility, native headers on the managed shared Chat route and the `opencode_` event prefix. Event-list and detail formatting must handle existing raw OpenCode IDs without double-prefixing new records. Missing historical identities are not backfilled. Native retest remains pending. Verification results for this snapshot follow below.

Verification for the final OpenCode follow-up over the uncommitted `1c53f58f` working tree on the same macOS/Rust/Node/pnpm host: `pnpm typecheck` and `cargo check --tests --manifest-path src-tauri/Cargo.toml` passed. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Rust formatting and diff-whitespace checks passed; the two documentation formatting findings were corrected and targeted Prettier checks rerun. Added regressions were compiled but not executed, following the user's request. Native new-record acceptance, Windows/Linux jobs and full CI acceptance remain pending. No build artifact was installed, no client or VaryAI process was restarted, no account/configuration was changed, Codex routing was left untouched, and no commit or push was performed.

The user subsequently confirmed OpenCode native new-record acceptance passed on 2026-11-17, covering the tool warning and session-ID follow-up. This does not establish other-platform or nested code-mode acceptance. OpenClaw was subsequently skipped at the user’s request.

## Hermes session-correlation follow-up (2026-11-17)

Pre-implementation assessment: Hermes custom providers intentionally omit session identity unless the native `session_affinity_header` option is enabled. Installed source `agent/opencode_affinity.py`, `agent/chat_completion_helpers.py` and `hermes_cli/config_providers.py` establish that this option adds a client-declared conversation affinity header on Chat, Responses and Anthropic requests without a plugin. VaryAI should enable a dedicated header on its two managed provider entries and recognize that header only for Hermes. The existing all-Agent matrix remains authoritative; other Agent projections and extractors retain their contracts, with explicit cross-Agent isolation regressions.

This is native conversation affinity, which can survive compression and follow the conversation root. It is not guaranteed to equal the current physical transcript ID; native cron-job scope can group scheduled runs. VaryAI must not compute it from prompt cache keys, request IDs, histories or generated per-request identifiers. Empty native identity stays absent. Managed configuration changes must preserve foreign providers and restore the preceding header option on disconnect. New events should carry `hermes_`; existing raw IDs should display consistently, while missing historical IDs remain missing. This shared configuration/extraction covers macOS, Windows and Linux, with native acceptance recorded separately.

Hermes implementation uses the supported native option on both `varyai` and `varyai-anthropic` entries; it does not add a plugin or patch the installed Hermes source. Recognized native physical-session headers take precedence when supplied; the local affinity header is next, followed by existing supported metadata. Invalid/repeated identities stay absent and private Hermes headers do not identify other Agents. Added regressions cover both managed provider declarations, foreign-provider/header preservation and restoration, all-Agent header isolation, request/cache/token exclusion, shared-route correlation/privacy, and raw/already-prefixed labels in list and details. Rebuild VaryAI, refresh/reconnect Hermes and reopen Hermes once if its running runtime has cached provider configuration; generate a new tool event. Native new-record acceptance remains pending; compile/type/format verification results follow below.

Verification for the final Hermes follow-up over the uncommitted `1c53f58f` working tree, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, targeted Prettier checks, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Installed Hermes checkout source at `3b42157858` and both managed workspace sources expose the native custom-provider affinity option and merge implementation. Tests were compiled but not executed as requested. Native identity/warning acceptance, scheduled/delegated identity scope, Windows/Linux native jobs and full CI acceptance remain unverified. No native application/configuration was changed during inspection, no app was installed or restarted, Codex routing was untouched, and no commit or push was made. Grok Build was user-confirmed passed; TRAE CN is the current native test, while OpenClaw remains skipped.

## TRAE edition parity and native Hook acceptance (2026-11-17)

Pre-implementation review: tool-call continuation normalization, durable ownership discovery, response observation and slow-header handling matched only domestic `trae`, excluding `trae-intl`. Both native editions rewrite Chat tool-call IDs; the international adapter must use the same observed-payload fingerprints while preserving edition isolation, known owners, ambiguous/unknown-call rejection and the original forwarded request. Other Agents retain their existing continuation rules; this is an edition-parity fix on shared Rust code for all supported operating systems, not a relaxation of ownership checks.

Both owned global Hook files exist with the expected RunCommand matcher. The installed native UI separately controls `global_hooks_enabled` and requires native confirmation to enable Hook commands. Merely writing hooks.json does not prove collection; enabled/disabled native Hooks and sandbox access must be distinguished from schema/rule matching. Native global and project Hook decisions remain unchanged. The user was asked to check the TRAE CN global Hook switch while the international continuation fix proceeds. Current evidence does not establish that the switch is off or that warning delivery works.

The user subsequently confirmed Hermes acceptance passed on 2026-11-17. The provided TRAE settings screenshot confirms Configured Hooks is disabled; enabling that native global switch and retesting in the existing sandbox mode is the next native step. This does not establish successful Hook delivery yet.

The continuation repair now includes both edition identities in rewritten-call normalization, persisted ownership discovery, response-payload observation and slow-header waiting. Regressions cover complete observed payloads, changed/unknown calls, authoritative known owners, original-request preservation, persisted ownership across restart and exact target selection for both JSON/SSE transports and both edition credentials. The original forwarded tool calls remain unchanged, and unknown/ambiguous ownership still fails closed. Native global Hook activation was confirmed disabled by the screenshot and the user has now enabled it; warning delivery remains pending native retest.

The user confirmed TRAE CN native warning delivery after enabling Configured Hooks and requested the domestic session label `trae_cn_` instead of `trae_`. Pre-implementation scope: retain the internal Agent ID `trae`, native schema, policy bindings and edition authentication; change only domestic event correlation prefix and historical display formatting. The subsequent international naming request is included below; other Agent prefix contracts remain unchanged. Add native collector and list/detail regressions for raw, old-prefixed and already-correct domestic IDs, preserving stored/filter identity and not filling absent IDs.

The user clarified the prefix contract: TRAE CN uses `trae_cn_`, and TRAE International uses `trae_`. Both native collector persistence and old-record list/detail formatting must follow that mapping without changing internal Agent IDs (`trae` and `trae-intl`), routing scope or policy identity. Domestic legacy audit inspection must use the same new prefix; no database migration or historical identity rewrite is required.

The final prefix implementation normalizes new native tool and audit IDs by edition (`trae_cn_` domestic, `trae_` international), including the domestic legacy audit path. List/detail display accepts raw, previous-prefix and already-correct IDs while preserving the underlying stored ID and internal client category. Added synthetic collector/audit/UI cases cover both editions and idempotent prefixing. TRAE CN warning delivery is user-confirmed passed; both new prefixes and international continuation require retest with the final build.

Final verification for the uncommitted TRAE continuation/prefix changes over `1c53f58f`, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, targeted Prettier, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. An initial compilation error in the new Observer fixture's struct-update initialization was corrected before the final successful check. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Added regressions compiled but were not executed, following the user's request. TRAE CN native warning delivery is user-confirmed passed; domestic/international prefix changes and the international 409 repair await final-build native retest. Windows/Linux native acceptance and full CI checks remain pending. Native global Hook activation was performed by the user; no application was installed/launched/restarted, no native permission or account configuration was changed by the agent, Codex routing remained untouched, and no commit or push was performed.

## TRAE reconnect latency follow-up (2026-11-17)

Pre-implementation assessment: read-only inspection found the international helper healthy and its outbox delivered, while explicit replacement deletes and recreates every owned model and forces a native list refresh after each addition. For a large catalog this causes prolonged UI waiting despite a live connection. Both editions and supported desktop platforms use the same renderer transaction. Compatible owned models can be overwritten in place without changing wire identity or skipping the user's explicit write intent; incompatible protocols/identities and prior ownership still require the existing replacement path. Failures must restore prior editable fields and ownership without persisting credentials. Other Agent mutation paths remain unchanged. Native latency and all-platform acceptance remain unverified.

Doubao Work restart is also pending: the audit launch adds loopback-only remote-debugging arguments, while generic desktop restart currently uses a plain launch. Both explicit UI actions must launch in audit mode, with restart quitting an existing instance first. The existing macOS-only audit-launch boundary and other-Agent restart arguments must remain explicit in the inventory; this is not permission to install or restart a native app during verification.

TRAE reconnect now overwrites compatible owned rows in place while still writing every requested model's current fields. New rows are created in settled batches of at most four, followed by one shared publication refresh and strict native identity readback. Successful peers of a failed batch are discovered by exact new-row identity and rolled back only after all outstanding creates have settled; existing foreign models are excluded. Incompatible prior ownership/protocols retain the existing replacement path. Prior editable fields and ownership restore after a failed compatible overwrite. Helper revision 19 loads the new renderer instead of continuing with a resident revision-18 helper. Regression cases cover a 58-model overwrite, the four-create concurrency ceiling, batched refresh count, wait-before-rollback, original-row preservation and failed-overwrite restoration. These cases are added but not executed under the user's no-test-execution preference; native performance remains pending.

Doubao's two explicit buttons now delegate to the same consent/takeover/audit-launch command. The audit launcher uses verified desktop lifecycle handling instead of rejecting an already-running client. A running verified instance quits and reopens; a stopped client opens directly. Both use loopback-only, dynamic-port Chromium audit arguments. macOS LaunchServices and the Windows native launch branches share the argument selector, and all other Agent arguments remain unchanged. The existing automatic audit launcher is still macOS-only; Windows lifecycle/launcher acceptance is not claimed. No native application was restarted or installed during implementation.

Final verification for the uncommitted TRAE reconnect/batching and Doubao audit-restart changes over `1c53f58f`, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, `node --check` for the four changed TRAE helper/test scripts, targeted Prettier, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Rust tests were compiled, JavaScript syntax was checked, and no tests were executed under the user's preference. Native concurrency/rollback performance, Doubao restart-to-audit acceptance, Windows/Linux native jobs and full CI acceptance remain pending. Read-only local helper health/list requests confirmed the international bridge was reachable and its outbox delivered; no native configuration or running client was changed during diagnosis. No commit, push, app installation or app restart was performed; Codex routing stayed untouched.

## Provider TLS compatibility and confirmed desktop restart races

Assessment for the provider TLS and pending desktop restart changes on
2026-11-17 covers the full inventory below. Provider discovery/protocol detection
uses the common Reqwest client; routed HTTP requests use its bound client or the
original-header/Hyper transports. Five synthetic TLS regressions cover these
transports, repeated connections, certificate checks and session resumption.
The live provider check establishes one authorized provider's catalog and
inference behavior, not native acceptance for every Agent.

Confirmed restart enters from the Agents row and confirmation dialog. The dialog
closes on confirmation while the row retains progress and localized failure
feedback. Supported macOS bundle and Windows process adapters resample a bounded
pre-quit discovery race, wait for a verified return, and reject unidentified
replacements. Automatic configuration guards are retained. CLI-only sessions and
unsupported native platform adapters gain no process restart action. Existing
macOS-only desktop identities remain explicit; this change does not add Windows
support for them. Synthetic lifecycle tests never inspect or restart installed
applications. Native per-Agent acceptance remains pending on every platform.

| Agent ID | Provider TLS path and entry | Confirmed restart adapter and UI | Verification and remaining native acceptance |
| --- | --- | --- | --- |
| `claude` | Shared API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `claude-desktop` | Shared API routing; provider discovery | Agents confirmation; macOS and Windows identity adapters; Linux unsupported | Synthetic lifecycle/TLS; native macOS/Windows pending |
| `codex` | Native Responses/managed API routing; provider discovery | Agents confirmation; macOS and Windows package/bundle identity; Linux unsupported | Synthetic lifecycle/TLS; native pending; keep the user's route off |
| `gemini` | Shared Gemini/API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `pi` | Shared API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `workbuddy` | Shared API routing; provider discovery | Agents confirmation; domestic macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native macOS/Windows pending |
| `workbuddy-ai` | Shared API routing; provider discovery | Agents confirmation; international macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native macOS/Windows pending |
| `qoder` | Shared API routing; provider discovery | Agents confirmation; Qoder CN macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native macOS/Windows pending |
| `kimi` | Shared API routing; provider discovery | macOS desktop bundle; CLI session separate; Windows/Linux restart unsupported | Synthetic macOS orchestration/TLS; native pending |
| `qwen` | Shared API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `opencode` | Shared API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `grokbuild` | Shared API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `openclaw` | Shared API routing; provider discovery | No native desktop lifecycle adapter | Shared TLS fixtures; native client not rechecked |
| `hermes` | Shared API routing; provider discovery | CLI session; no desktop restart action | Shared TLS fixtures; native client not rechecked |
| `zcode` | Shared API routing; provider discovery | macOS desktop bundle; Windows/Linux restart unsupported | Synthetic macOS orchestration/TLS; native pending |
| `deepseek-harness` | Shared API routing; provider discovery | macOS desktop bundle; Windows/Linux restart unsupported | Synthetic macOS orchestration/TLS; native pending |
| `t3code` | Shared API routing; provider discovery | No native desktop lifecycle adapter | Shared TLS fixtures; native client not rechecked |
| `trae` | Existing native integration and shared provider discovery | Agents confirmation; domestic macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native integration not rechecked |
| `trae-intl` | Existing native integration and shared provider discovery | Agents confirmation; international macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native integration not rechecked |
| `doubaowork` | Native audit integration has no editable provider projection; native collector unchanged | Agents confirmation on macOS; Windows/Linux restart unsupported | Synthetic macOS orchestration; native integration not rechecked |


## Routing list refresh and bounded catalog synchronization

Implementation assessment before the 2026-11-17 change: the Routing page has one
shared entry for all Agents. A local list refresh must use cached catalogs;
explicit model synchronization may fetch catalogs, with bounded concurrency and
a shared network deadline. Publication continues through the existing global
and per-client queues. No native protocol, credential, configuration-consent,
or request forwarding contract changes. The common Rust and React paths apply
to macOS, Windows and Linux; native integration support remains as recorded above.

| Agent ID | Entry and runtime path | Protocol/native adapter impact | UI and planned verification | Native acceptance |
| --- | --- | --- | --- | --- |
| `claude` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `claude-desktop` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `codex` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `gemini` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `pi` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `workbuddy` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `workbuddy-ai` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `qoder` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `opencode` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `grokbuild` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `openclaw` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `hermes` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `kimi` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `qwen` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `zcode` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `deepseek-harness` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `t3code` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `trae` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `trae-intl` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `doubaowork` | Shared cached list and explicit catalog sync; existing publication queue | No editable model projection; existing native audit integration retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |

Shared synthetic regressions passed on macOS. Full host checks are recorded with the verification results; native per-Agent acceptance and remote Windows/Linux/macOS CI remain pending. This assessment does not claim per-Agent native acceptance.

## TRAE native concurrent-write correction (2026-11-17)

Assessment: both `trae` and `trae-intl` use the same renderer model writer on macOS and Windows. The other 18 Agent writers are unaffected. The revision-19 four-create batching described above is superseded: the running international client reproduced `model_apply_failed` during a user-authorized reconnect operation, with an incomplete native catalog. Helper health was ready at revision 19; this was a write/readback failure rather than an unavailable connection.

The revision-20 writer creates missing models sequentially, retaining compatible in-place updates and one post-creation publication refresh. A regression models native read/modify/write snapshots, where overlapping successful calls can lose rows; failure coverage verifies that subsequent creates stop and only new rows are rolled back. Native storage internals were not independently proven; the snapshot regression covers the observed lost-row behavior without claiming their exact implementation.

Native verification used the corrected renderer against the already-running TRAE International instance through its existing local native connection, without installing a build, restarting an application, changing Codex routing or exposing credentials. The original writer returned `model_apply_failed` in 40.84 seconds. The corrected writer completed in 15.789 seconds with all 58 requested models and `matches=true`, including native credential proofs. This confirms this live write/readback operation, not VaryAI's visible button state or full reconnection acceptance. Computer Use was unavailable; no UI click was claimed. The temporary diagnostic connection closed after completion and no background diagnostic process was retained. Domestic-edition and Windows native acceptance remain pending.

Comparison: Magpie main at [`f9492fd8ed2c6fd3c65f1dab00062771b551d7b5`](https://github.com/yetone/magpie/tree/f9492fd8ed2c6fd3c65f1dab00062771b551d7b5) has no TRAE desktop Agent writer. Its [`trae_checkin.go`](https://github.com/yetone/magpie/blob/f9492fd8ed2c6fd3c65f1dab00062771b551d7b5/internal/provider/trae_checkin.go) documents TRAE CN as a community-plugin provider with no built-in mover. Provider access/check-in does not establish desktop model-list configuration support.

Verification scope: working-tree changes over `7919f897`, macOS arm64; Node syntax checks for the four changed helper/test scripts, targeted JavaScript/architecture-document Prettier, Rust formatting and diff whitespace checks passed. The pre-existing provider-TLS table formatting in this ledger was preserved and still fails whole-file Prettier. Regression tests were updated but not executed under the user's earlier preference. Full build, CI and cross-platform acceptance are not claimed. No commit or push.

## TRAE Hook setup guidance (2026-11-17)

Scope before implementation: the `trae` and `trae-intl` connection details and setup dialog share the same Hook instructions on macOS and Windows; the existing TRAE CN audit notice also displays them. The other 18 Agent entries keep their existing instructions because this native switch is specific to TRAE. Explain the pre-execution observation purpose, the Settings > Hooks > Configured Hooks switch, independent per-edition activation, no requirement to import Claude Hooks, and retesting after activation with an enabled matching risk policy. This is guidance only: it does not toggle native permissions, change execution mode or alter tool decisions. Native UI verification remains pending.

Guidance verification: `pnpm typecheck`, targeted Prettier for both changed React components and all four locale files, JSON parsing and `git diff --check` passed. No tests, application build, installation or native UI interaction was performed for this text-only follow-up. No commit or push.

## Doubao single audit-launch action (2026-11-17)

Pre-implementation coverage: only `doubaowork` uses the dedicated audit launcher; the other 19 Agent restart paths retain their existing controls. Both compact and full layouts must suppress the generic restart control for Doubao, expose one audit-launch action while disconnected, label a known running instance as an audit restart, and show audit-waiting status instead of stale model/configuration language. The shared frontend applies on every platform; the existing macOS-only native audit-launch boundary remains unchanged. An active audit connection must hide the launcher even if a stale restart flag arrives.

Implemented the single-action condition and localized running/stopped labels in all four runtime languages. Added compact/full-layout regressions for pending audit with stale restart flags, audit-launch dispatch, and active audit suppressing both launcher and generic restart even with stale flags. `pnpm typecheck`, targeted component/test/locale Prettier and `git diff --check` passed. Regression execution and native UI acceptance remain pending; no commit, push, build or native restart was performed.


## Doubao native tool-risk acceptance failure (2026-11-17)

The user ran the synthetic `kubectl delete pod vary-test-nonexistent --dry-run=client` command in the actual client and confirmed a native tool result. The current background collector installed successfully after audit-mode launch, but a temporary metadata-only lifecycle observer received zero callbacks during this execution; the risk-event database contained no Doubao records. Missing Kubernetes configuration affects the command result, not whether risk observation should occur.

Read-only runtime inspection confirmed dispatcher mode and an uninitialized legacy tool-context source in the background webview. The installed client separately hosts `entry-moa` in its native JSRuntime utility process. This exposes a missing adapter path, not successful tool-risk acceptance. The native `jsRuntime.enableDebuggingPort` API rejected requests from the existing background origin with `INVALID_ORIGIN`; no origin change, security-policy override or signed tool mutation was attempted, and no new inspector listener was established. A supported native observation interface is still required. Do not relabel model prose or post-execution text as a pre-execution tool call.

The temporary lifecycle diagnostic recorded only tool names and field-presence metadata, had an automatic three-minute cleanup, and was explicitly removed after the user's test. An attempted invocation of the Doubao executable as a JavaScript host unexpectedly launched extra native processes; that diagnostic process tree was stopped, and the user subsequently opened the client in audit mode. Later scripts ran under standalone Node and closed their debugging connections. No tool-risk implementation fix or acceptance is claimed from this investigation. All other Agent coverage remains as recorded above; no commit or push.

## Doubao current-runtime observation follow-up

Pre-implementation assessment: retain the legacy native lifecycle for older Doubao versions and investigate a passive structured-event feed for the current JSRuntime execution path. Do not bypass the native `INVALID_ORIGIN` decision or infer tool calls from chat prose. Both macOS and Windows collectors must use the same validated adapter where their existing audit connection is available; automatic audit launch remains macOS-only. The other 19 Agent collectors and their recorded acceptance are unaffected. New transport/source/stage claims require actual native schema evidence, bounded parsing, supplied-session correlation and preservation of normal execution.

### Current-runtime implementation

The user repeated the native terminal test while a bounded, metadata-only Network diagnostic was attached. Live completion SSE contained native file-operation cards; read-only inspection of the current rendered card confirmed `block_type=10019`, `file_type=file_content_type=shell`, `operation_type=1`, `scene=1`, and a separate `display_content.operation` exactly matching the synthetic test command. Output remained a different field and was not copied into observations. The actual endpoints were `/chat/completion` and `/chat/completion/action`. Temporary diagnostics detached and exited.

Added a separate bounded CDP Network collector and shell-report parser, retaining the old background hook. Reports use `client_report` / `reported`, a distinct schema, validated field conversion and the actual supplied conversation ID with `doubaowork_`. The collector does not claim a pre-execution warning or block. Ordinary prose, history echoes, unsupported cards, oversized input and invalid session changes are excluded. Split streams, duplicates, replacement/deletion, native-source validation, enabled/disabled queue ingress, sanitized persistence and frontend source/stage labels have regression coverage. Other Agent collectors keep their original behavior and source/stage contracts; no new acceptance is inferred for them. The parser/transport is shared by the macOS and Windows code paths; Windows compilation and native connection/launch acceptance remain pending. No native application was rebuilt, replaced or relaunched for this implementation; Codex routing stayed untouched.

### Commit verification for this follow-up

Checked working-tree changes over `f010e2b3` on macOS arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3. `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit` (322 files, 4,376 tests), `pnpm format:check`, production dependency audit, targeted helper/document formatting and diff whitespace checks passed. TRAE's 72 helper tests passed. The development runner, CI policy, macOS DMG, default-model, Pi session, WorkBuddy authentication, Doubao check-in, Doubao legacy tool hook, Agent rewards, Qoder codec and shared-v23 recovery helper suites also passed. Renderer generation did not build, install or launch the desktop application.

The initial full Rust run found a fixture pinned to TRAE helper revision 19 after the production helper moved to 20. The fixture now reads the bundled revision and retains its assertion that configuration synchronization must not enable auditing. The initial frontend run found an outdated generic Doubao restart-button expectation; the all-Agent recovery test now expects Doubao's dedicated audit launcher, while the other Agents retain generic restart expectations. Full frontend regression passed after that correction. Final `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings` and full `cargo test` (all with `--manifest-path src-tauri/Cargo.toml`) passed: 7,904 tests passed and 21 intentionally ignored, including 7,739 library tests passed. The existing test-only unused-method and macOS unwind-table linker warnings remain. Native new-build notifications, Windows/Linux CI and installer acceptance are not inferred from local checks. Earlier no-test notes above describe earlier revisions, not this final verification run.
