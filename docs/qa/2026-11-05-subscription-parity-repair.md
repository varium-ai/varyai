# Subscription repairs and connection ownership — 2026-11-05

This follow-up implements repairs for the findings in
[the subscription review](2026-11-05-subscription-parity-review.md) and for the
connection switch remaining on after another manager takes over a client.
The original review records the pre-repair observations and remains historical.

## Implemented behavior

| Finding | Repair and local regression coverage |
| --- | --- |
| SUB01, SUB15 | Managed Grok model discovery, Responses inference and billing use the CLI proxy contract. CLI headers, model/context-window shaping and one rejected-token renewal are shared by the production paths and tested with synthetic loopback HTTP responses. The separate legacy native-CLI billing integration is preserved. |
| SUB02 | Claude receives typed replay history and the current user turn instead of one flattened prompt. A fake CLI verifies the prior tool call, matching result, resumed reply and transcript cleanup. |
| SUB03 | Claude usage first asks its isolated CLI. Direct usage rejection does not disable a renewable CLI credential. |
| SUB04 | Refresh coordination is per account; network waits occur outside the account-store mutex. Credential generations guard publication against removal, reimport and reauthentication. |
| SUB05, SUB06 | Copilot accepts partial snapshots, both reset-date fields and bounded numeric strings; missing/invalid values stay unknown. Exhausted unlimited Enterprise premium pools remain visible. Organization policy facts survive the DTO. |
| SUB07 | Explicit Copilot model discovery bypasses the internal model cache. |
| SUB08 | Discovery retains model capabilities and Copilot endpoint/policy facts. Picked models keep refreshed vendor facts and explicit preferences. Shared targets and retargeting select a model's supported Copilot protocol. Known Gemini 2.5 limits are published; unsupported rollout limits are not invented. |
| SUB09 | Codex account holds, model availability, credit balances, reset-credit counts and spending controls survive quota parsing. The account editor displays restrictions and balances without inventing windows or changing spending policy. |
| SUB10 | Both Codex quota entry points share terminal/transient authentication classification. |
| SUB11, SUB12 | Gemini inference reads cached experiment observations without a network lookup. Quota labels follow the same rollout aliases; failed observations retain known flags. |
| SUB13 | Google quota/model inspection observes a missing plan label even with an existing project. |
| SUB14 | Account selection triggers discovery; cached results are scoped by service/account and stale binding responses are rejected. |
| External takeover | Switches stop treating saved intent/receipts as proof of ownership. Status refresh preserves external edits. An explicit reconnect uses reapplication and a new restoration baseline. |

## Validation boundary

All added cases use synthetic data, temporary directories, fake processes or
loopback listeners on dynamic ports. No installed app was launched/replaced and
no real account, credential, paid inference or vendor endpoint was used for
functional acceptance. These changes do not establish complete Magpie parity,
live subscription acceptance, installer/signing acceptance or cross-platform CI
acceptance. Claude CLI-version compatibility and the managed Grok service still
need real-client release acceptance. Account pools and automatic paid-credit
spending are separate feature contracts.

## Verification record

Checked base: `a30e86d1d11641cf40dbbdaf02c11985d3e34df0`, with the recorded
uncommitted working files. Host: macOS 15.3.1 / arm64. Rust and Cargo 1.98.1;
pnpm 10.12.3; Node 22.22.2 (the repository/CI pin is 22.12.0). No commit or push
was performed. The checkout contained concurrent, unrelated work, which was
preserved.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed again after the final account-editor change. |
| `pnpm build:renderer` | Passed again after the final locale change. |
| `pnpm format:check` | Passed on the final renderer sources. |
| `pnpm test:unit` | 279 files / 3469 tests passed. Subsequent account-editor changes passed the four affected suites (179 tests); final locale changes passed 47 locale cases. |
| Development launcher, TRAE bridge, WorkBuddy auth, Doubao check-in, agent rewards and Qoder codec helper tests | All passed through the documented pnpm commands. |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed. |
| `pnpm audit --prod` | Passed; no known vulnerabilities reported. |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed on the final backend sources. |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6025 passed, 18 ignored, zero failures on the final backend sources. |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed on the final backend sources. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed on the final backend sources. |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Passed: 6025 library tests and 163 integration tests; 18 library tests ignored. |

The 18 ignored library tests require installed clients, explicit live-service
opt-in, private replay corpora, or performance/download acceptance. They were not
counted as executed acceptance. Linux, Windows, Windows-to-WSL2 and remote macOS
CI were not run for this uncommitted tree, nor was the exact CI Node patch release.
Cross-platform acceptance remains incomplete.

Earlier attempts encountered insufficient build-cache disk space, an unformatted
in-progress fixture, and two assertions in Codex paginated-history migration and
WorkBuddy duplicate naming. Cargo package-cache cleanup recovered build space;
completed working-tree changes and reruns resolved the earlier check failures.
No test assertion was weakened to accept the previous mismatches. The macOS
linker emitted its compact-unwind size warning for the large test binary; test
execution completed.

The final source/configuration snapshot includes 1425 files under `src/`,
`tests/`, `src-tauri/src/`, `src-tauri/tests/`, the Cargo manifest/lockfile and the
package manifest/lockfile. Its SHA-256 is:

`ddcd14d68af19a1ffebef8c5cb8dbf989ed87bd6eb4b9d6f90bed41822219f72`

This is SHA-256 of Python's default JSON serialization, with sorted keys, of
`{repository_relative_path: SHA256(file_bytes)}`. Generated builds, local stores,
credentials and unrelated files outside that source snapshot are excluded.
The implementation fingerprints below provide narrower review anchors:

| File | SHA-256 |
| --- | --- |
| `src-tauri/src/proxy/native_subscription.rs` | `663b09ecd1f9f749579fde54bbbba41cf2195c88e08495c7ca5e591c0280addd` |
| `src-tauri/src/services/native_subscription.rs` | `88429a2b1b08eb735f4f70029fbd4496f87f874ddd19480922a61a5179bef74b` |
| `src-tauri/src/services/grok_subscription.rs` | `ef05ddb6b0fc3c8e94519957421afbc35271ebfbd1806bdf66667d5f947d9199` |
| `src-tauri/src/proxy/providers/copilot_auth.rs` | `789b75b6fd5f81cbe9a44434cae81d08e8cc0887dcee05dc882f5183802faace` |
| `src-tauri/src/services/subscription.rs` | `b3a4b0643aa01e4b1620db093fbbb7501c0f231c630f06f7fe268436e9da7225` |
| `src-tauri/src/commands/agent_connections.rs` | `411df0fb1a61ff3714c2188ae7652b2f44d4d16169a1a76dbae40d0ee4e50486` |
| `src-tauri/src/services/agent_connections.rs` | `bc989909c2766e0a23f0cb09eced97b31e99af25711fee462c550f33c1e5b340` |
| `src/components/agents/AgentConnectionsPage.tsx` | `763eb8b2ad122bb078a9aa87b83e20951a27c9a5226dd5e992df869b386ba655` |
| `src/components/providers/SubscriptionProviderFields.tsx` | `0d4be831051c231ecaae25173f5d75891abfce790c0d112187b61401dcf2c96b` |
