# Other Agent connection latency

## Scope and findings

This follow-up reviews all 19 non-Codex Agent integrations, their public
connection commands, shared catalog refresh and restoration, native configuration
adapters, desktop lifecycle helpers, and background model-control helpers.
Only synthetic fixtures were used for mutations. Installed clients were not
launched, restarted or disconnected for verification.

Two reproducible dependencies unnecessarily serialized independent Agents:

1. `ProxyService::sync_shared_agent_catalogs` awaited Codex, TRAE China, TRAE
   International, then every native adapter in order. A slow bridge request or
   occupied switch lock prevented later Agents from receiving their catalogs.
2. `ProxyService::restore_shared_connections` restored both TRAE editions before
   native adapters and processed native adapters one at a time. One busy Agent
   prevented otherwise independent configurations from being restored.

TRAE's apply, remove, select and configuration-match requests each allow 330
seconds for the native client's model queue. Multiple sequential requests or
helper endpoints can compound that time. This is an upper waiting allowance,
not an observed normal latency or an end-to-end operation deadline. The timeout
was introduced to accommodate large native catalogs; shortening it could retry
while an earlier native mutation is still executing.

Both bulk operations now poll their independent Agent operations concurrently.
Each operation retains its own switch lock, configuration ownership checks and
recovery journal. Both operations still await all participants. Restoration
attempts every applicable Agent before returning an error; it does not acquire
switch locks for unowned Agents. The implementation is shared by macOS and
Windows.

## Remaining waits and inspected paths

| Agents | Connection/restoration path | Finding |
| --- | --- | --- |
| Claude Code, Pi, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes, Kimi, Qwen Code, Z Code, DeepSeek Harness | Local configuration projections and ownership receipts | No Codex-style conversation-history migration found. Individual connection changes already use separate Agent locks and do not refresh every other Agent. |
| T3 Code | Settings plus an isolated Codex home and model catalog | No migration of the user's primary Codex history. |
| Claude Desktop | Profile/configuration edits and desktop freshness checks | Native application exit waits belong to an explicitly confirmed restart. Ordinary disconnect restores files. |
| WorkBuddy, WorkBuddy AI, Qoder CN | Local configuration and a model-control plan | Model readback is handled by the detached helper. Saving/restoring a plan does not wait for renderer readback. Explicit restart may wait for the native process to exit. |
| TRAE China and International | Durable outbox plus native bridge acknowledgments | Their own operations can still wait for a busy native model queue. They no longer delay another Agent's bulk refresh/restoration. Offline cleanup remains pending until acknowledgment. |
| Doubao Work | Audit connection and paused-request cleanup | Disable allows up to four seconds for cleanup before aborting its worker. This bounded cleanup protects in-flight native requests. |

Sanitized local logs contained only sub-second completed refreshes for the other
Agents: the largest observed values were 500 ms for OpenCode, 237 ms for DeepSeek
Harness, 132 ms for Grok Build and 124 ms for Claude Desktop. These are existing
completed-operation observations, not a benchmark of every installed client;
unconnected adapters can be no-ops and unfinished waits have no completion log.
No native credentials, configuration values, model identifiers or conversation
contents were included in this report.

## Verification

The two initial regressions failed against the serial implementation: holding a
Codex switch lock prevented Gemini catalog synchronization, and holding a TRAE
switch lock prevented Gemini restoration. Both passed after the change.

The final tests cover occupied Codex, TRAE China, TRAE International and native
Agent locks, both suspension and explicit disconnection, independent Gemini
progress, credential revocation, retained connection intent, and the requirement
that the bulk call still wait for the busy participant. An additional regression
checks that unowned TRAE and OpenCode switches do not delay restoration.
They use temporary homes, an in-memory database and an ephemeral listener port.

Host: macOS 15.3.1 arm64, Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3.
Base revision: `015521e022f3b850fdb56a2177bbdb419c68575f` plus the cumulative
working-tree changes from the earlier Codex fixes and this follow-up. This
follow-up changes only `src-tauri/src/services/proxy.rs` and documentation.
Final checks passed:

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7,581 passed,
  21 existing ignored tests.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 7,581 library and 165
  integration tests passed, with the same 21 existing ignored library tests.
- `git diff --check`

The final SHA-256 of `src-tauri/src/services/proxy.rs` is
`315e686608153b0b96c093359dbaad4eba2b1157cdcd7bac508425d7a2e6b0f0`.
It was rechecked after all tests. Test builds retain the existing test-only
`quota_panel::expiring` and macOS linker unwind-table warnings.

Logs are retained locally under `/tmp/varyai-other-agents-20261114/`; the initial
red/green regression logs are `/tmp/varyai-other-agents-red.log` and
`/tmp/varyai-other-agents-green.log`.

The previously passed renderer checks (4,182 tests), helper tests and production
dependency audit remain applicable because their inputs did not change. See the
[Codex role report](2026-11-14-codex-chat-instruction-roles.md) and
[connection-latency report](2026-11-14-codex-connection-latency.md) for those records.
Live native-client timing and Linux, Windows, Windows-to-WSL2 and remote CI
acceptance remain unverified. No commit or push is included.
