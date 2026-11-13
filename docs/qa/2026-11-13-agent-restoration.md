# Agent restoration follow-up and acceptance checklist

Base revision: `9804cdb78f509a7b47ce2db90da1ce18adc496c8`, plus the preceding uncommitted fixes. The user confirmed that the new build has not yet been manually retested. No commit, push, installation or client restart is part of this batch. Tracker statuses remain untouched.

## Latest requirement update

The user retired automatic-discovery controls because the product requirement had changed. Removed the locally restored switch, its heading and help text, toggle mutation/loading state, and the added heading styles. Restored the existing absence assertions for both legacy preference values and preserve the unrelated JSON-export fix. The backend and its legacy configuration compatibility are unchanged from the fully checked restoration checkpoint below.

Original issues 2, 3 and 7 (current pending-view rows 1, 2 and 5) are retired requirements, not pending fixes or accepted bug tests. This leaves 15 applicable records in the current view, including two previously deferred records. Tracker fields remain unchanged. The restoration checkpoint results below predate this UI retirement; affected checks and the current source snapshot are recorded below.

## Reproduction and change

The earlier added HTTP regression exercised Codex Responses and WorkBuddy AI Chat against synthetic upstreams. The ten-Agent selection test concerned model-selection retention, not restoration. Neither result established live acceptance by every installed Agent.

The new matrix initially passed Chat but failed Messages, Gemini generation and non-Codex Responses admission with HTTP 400. The fix connects authenticated shared routes to the existing request-preparation and completion owners for these protocols. It preserves the original direct/gateway admission rules and unsupported-conversion refusals.

Native shared recovery now has a separate `Shared` ingress identity, authenticated in token namespaces and delivered evidence. Shared Responses use a separate per-Agent namespace. Regression checks prevent equal session identifiers from joining different Agents or direct/shared recovery contexts. Ordinary route flags cannot acquire shared authority.

For Gemini, the actual selected upstream model replaces the catalog name in the generation URL. The internal group `model` field is removed from the outbound JSON. Generation parameters and native JSON/SSE framing are preserved.

A mixed regression run exposed a test-isolation defect: Gemini GE2/GE3 fixtures reset the process-wide policy without holding the repository's serial-test lock. A temporary writer trace identified `ge2_lane_json_strict_shared_validation_and_negative_shapes` replacing the restoration policy during the Chat matrix. All 20 tests using that fixture now hold the same lock. No restoration assertion or production security guard was relaxed; the temporary trace was removed.

## HTTP matrix

The matrix covers these 18 shared connection identities: Claude Code, Claude Desktop, Pi, WorkBuddy, WorkBuddy AI, OpenCode, Gemini, Grok Build, OpenClaw, Hermes, Qoder, Kimi, Qwen, ZCode, DeepSeek Harness, T3 Code, TRAE CN and TRAE International.

Each identity exercises Chat, Responses, Anthropic Messages and Gemini generation, with JSON/SSE and sensitive first request/safe follow-up carrying earlier sensitive history: 288 successful request scenarios. Another 72 requests check that incorrect credentials never reach the upstream. The matrix checks hidden phone/identity text at the upstream, restored client text, the exact target model and retained generation parameters. All fixtures use synthetic text, in-memory storage and dynamic loopback ports.

Codex remains covered by its separate shared Responses-to-Chat JSON/SSE regression. Existing direct native, history, completion-proof, explicit-block, immutable-content and unsupported-conversion regressions remain required.

This is proxy HTTP coverage. No actual Agent client, production account or vendor service was launched to produce it. Clients may add fields or use protocol combinations not represented by these fixtures.

### Scope still requiring care

Native Chat/Messages/Gemini restoration requires the upstream to use the same protocol. The existing owned Responses conversion facades remain available. Other protocol conversions, specialized account transports, official native targets and Responses namespace flattening still refuse required restoration. They must not silently send sensitive plaintext.

Doubao Work uses an audit-only connection, not this shared model transport; local restoration is not established for that channel. These limits prevent issue 32 from being described as universally fixed for all models and all client channels.

## Manual acceptance

Use a build containing the current working-tree changes. A matching `0.3.0` label alone does not identify its source snapshot. At this checkpoint, every row below still needed live manual acceptance; unit/HTTP tests do not change the tracker to Accepted. Later user results using the current 18-row numbering are recorded in [Manual acceptance updates](2026-11-13-manual-acceptance.md). Original issue numbering is retained below, because the pending view uses different row numbers.

The final browser review, performed without reloading, still showed 18 pending records under the existing three filters. No new visible record was added, and no tracker field or filter was changed.

| Issue | Acceptance action                                                                                                                                                                                                                                         |
| ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | Add excessive or invalid provider keys; verify the limit and safe save feedback, including a closed key editor.                                                                                                                                           |
| 2     | Retired by the user: automatic-discovery controls are removed.                                                                                                                                                                                            |
| 3     | Retired by the user: automatic-discovery controls are removed.                                                                                                                                                                                            |
| 4     | Fetch models from the Gemini-compatible endpoint used in the report.                                                                                                                                                                                      |
| 5     | Deferred by user: installed Claude model-list completeness.                                                                                                                                                                                               |
| 6     | Deferred by user: real account login and model discovery.                                                                                                                                                                                                 |
| 7     | Retired by the user: automatic-discovery controls are removed.                                                                                                                                                                                            |
| 8     | Select a nonfirst Agent model; wait for refresh and reopen the page without losing the selection.                                                                                                                                                         |
| 9     | Check template Add actions and their unmet-prerequisite explanation.                                                                                                                                                                                      |
| 10    | Export a group as JSON; check download initiation feedback and the resulting file.                                                                                                                                                                        |
| 11    | Delete a group, find it in removed groups and restore it.                                                                                                                                                                                                 |
| 12    | Deselect a supplier model; its saved route member must be marked unavailable and unselectable. The definition is retained for review.                                                                                                                     |
| 13    | Add two or more members successively without losing the add controls.                                                                                                                                                                                     |
| 14    | Inspect a long billing-model identifier without losing text.                                                                                                                                                                                              |
| 15    | Inspect newly recorded native-source attribution; do not infer missing historical source identities.                                                                                                                                                      |
| 16    | Call configured models and verify request/response/billing display and model filters.                                                                                                                                                                     |
| 17    | Compare request-graph supplier icons with their configured sources.                                                                                                                                                                                       |
| 18    | Toggle account masking in lists, model selectors and request graphs.                                                                                                                                                                                      |
| 19    | Inspect one selected Agent/model/session; unrelated graph errors must not be mixed into it.                                                                                                                                                               |
| 20    | Search supplier/model names and multiple terms; inspect the scope hint and matching results.                                                                                                                                                              |
| 21    | Save and exercise both generic-key hiding actions using a synthetic generic-only fixture.                                                                                                                                                                 |
| 22    | On a supported Codex path, verify the configured restoration action is used; unsupported paths must explain the limitation without claiming the rule was Block.                                                                                           |
| 23    | Send a safe follow-up after a finding; old historical findings must not generate repeated notifications.                                                                                                                                                  |
| 24    | Hover the reset button for the first time; the tooltip should appear promptly and not reset anything.                                                                                                                                                     |
| 25    | Capture a Codex shared selection, change it, preview/apply the scheme and undo it.                                                                                                                                                                        |
| 26    | Follow a sensitive Codex request with ordinary conversation in the same chat.                                                                                                                                                                             |
| 27    | Reselect the current WorkBuddy model; it must not introduce another configuration write or restart requirement.                                                                                                                                           |
| 28    | On a supported WorkBuddy path, exercise JSON/SSE restoration with synthetic phone/identity text.                                                                                                                                                          |
| 29    | Continue the WorkBuddy conversation normally while retaining earlier sensitive history.                                                                                                                                                                   |
| 30    | Change an Agent model and check its restart/new-session guidance; no automatic restart is expected.                                                                                                                                                       |
| 31    | Toggle an unrelated route group; each Agent's still-available selected model must remain selected.                                                                                                                                                        |
| 32    | Repeat restoration and normal follow-up for each installed Agent on supported transport/protocol pairs. Record the Agent, model and protocol/channel for any remaining refusal. Doubao's audit-only and unsupported conversions are separate open limits. |
| 33    | Create a single-Agent scheme for an Agent other than the default without selecting and clearing a multi-Agent list.                                                                                                                                       |
| 34    | Inspect enabled/disabled switches in both themes and assess the darker checked track.                                                                                                                                                                     |

## Verification

Host: macOS 26.6.2 (25G83), Apple Silicon. Node 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1. Rust checks use `CARGO_INCREMENTAL=0`. The final source snapshot contains 50 changed source/test/architecture-guide files; SHA-256 of its sorted JSON mapping is `1e8db68c5a7e4ad661be3c4e74f5a6578ed751430473469a1472fb2a496442ca`. The [per-file manifest](2026-11-13-agent-restoration-sources.json) was checked against the final working tree after verification. QA reports are excluded from this source mapping.

Frontend validation passed 4,059 tests in 304 files. Rust library validation passed 7,478 tests with 21 existing ignored tests. The complete Cargo run passed 7,643 tests with 21 ignored tests across 16 binary/doc-test groups. All four Agent matrices, per-Agent/direct/shared namespace isolation, native evidence isolation, unsupported-conversion and explicit-block guards passed in the full parallel suite. The separate storage recovery helper passed five tests. The test binary emitted a nonfatal macOS linker warning about its large unwind section; compilation, Clippy and tests completed successfully.

No current cross-platform CI or installer/live-client acceptance is established. New code has no remote CI result because the user requested no push. The pushed base revision's macOS CI failure remains the recorded remote result; its Qoder shutdown correction is covered locally in this working tree.

| Command                                                            | Result                          |
| ------------------------------------------------------------------ | ------------------------------- |
| `pnpm typecheck`                                                   | Passed                          |
| `pnpm build:renderer`                                              | Passed                          |
| `pnpm format:check`                                                | Passed                          |
| `pnpm audit --prod`                                                | Passed                          |
| `pnpm test:unit`                                                   | Passed: 4,059 tests / 304 files |
| `pnpm test:dev`                                                    | Passed                          |
| `pnpm test:macos-dmg`                                              | Passed                          |
| `pnpm test:trae-bridge`                                            | Passed                          |
| `pnpm test:model-default`                                          | Passed                          |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                          |
| `pnpm test:doubao-checkin`                                         | Passed                          |
| `pnpm test:agent-rewards`                                          | Passed                          |
| `pnpm test:qoder-codec`                                            | Passed                          |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                          |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                          |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                          |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 7,478; 21 ignored       |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 7,643; 21 ignored       |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed: 5 tests                 |

## Discovery-retirement verification

The two existing absence checks first failed with the restored controls, then passed after removal. Both routing test files passed all 113 tests. The final complete frontend suite passed 4,056 tests in 304 files; the three removed tests exclusively exercised the retired toggle. Type checking, renderer build, formatting, production dependency audit and all eight helper suites passed. No application was launched, replaced or installed.

The host and toolchain are unchanged from the restoration checkpoint. Backend file hashes still match that fully checked snapshot, so Rust checks were not repeated for this UI-only follow-up. The current [per-file source manifest](2026-11-13-discovery-retirement-sources.json) contains 49 changed files and has sorted-mapping SHA-256 `0167d6023f72068049a11d90269f1f00bc7d678818587b084c07d44a4ec838eb`. QA reports are excluded. No commit or push was made.

| Affected check                     | Result                         |
| ---------------------------------- | ------------------------------ |
| `pnpm typecheck`                   | Passed                         |
| `pnpm build:renderer`              | Passed                         |
| `pnpm format:check`                | Passed                         |
| `pnpm audit --prod`                | Passed                         |
| `pnpm test:unit`                   | Passed: 304 files, 4,056 tests |
| `pnpm test:dev`                    | Passed                         |
| `pnpm test:macos-dmg`              | Passed                         |
| `pnpm test:trae-bridge`            | Passed                         |
| `pnpm test:model-default`          | Passed                         |
| `pnpm test:workbuddy-checkin-auth` | Passed                         |
| `pnpm test:doubao-checkin`         | Passed                         |
| `pnpm test:agent-rewards`          | Passed                         |
| `pnpm test:qoder-codec`            | Passed                         |
