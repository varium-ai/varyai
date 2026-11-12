# Agent configuration and Codex authentication submission

Date: November 11, 2026 (Asia/Shanghai).

## Scope and checked tree

Base: `4892d762675155dcf3cd3365ab4230a0c0910c46` (`origin/main` at preparation time). Branch:
`codex/agent-config-and-codex-auth` in the original project checkout. This submission
includes all remaining local source, localization, tests and documentation changes.
The earlier routing work is already in main through PR #48.

- Native Agent catalogs carry each model's protocol, native model identity,
  context/output limits and reasoning metadata into the client-specific schema.
  Qwen JSONC comments and Hermes provider/default selection are preserved.
- DeepSeek Harness reads structural Cordis entries, merges supported protocol
  routes into the existing adapter and restores native choices. Client comment
  rewrites no longer determine ownership or configuration readability.
- Codex allowance queries honor the selected credential store and canonical
  credential-home keychain account. Legacy OAuth records without a mode marker
  remain readable; an unavailable explicitly selected store fails safely.
- Repairing a Codex account refreshes model discovery and allowance queries.
  Onboarding adds a localized API-key example hint. All four runtime locales
  retain their navigation and disconnect wording updates.
- The navigation regression retains exact translated-label and destination
  assertions with the updated wording. The new native model visibility fixture
  includes the protocol fields required by the shared model structure.

The first focused run reproduced four stale navigation assertions. They passed
after the expected labels were updated. Rust library compilation then exposed a
new upstream visibility fixture lacking `api` and `native_model`; those fixture
fields were added before the final Rust checks. No assertion was removed.

## Environment and verification

macOS 15.3.1, Darwin arm64; Node v24.11.0; pnpm 10.12.3; rustc 1.98.1 and cargo 1.98.1.
Fixtures use temporary files and synthetic credentials. No installed Agent was
launched, installed or replaced for this submission's verification.

The focused Codex provider/navigation run passed 54 tests. The final renderer
suite passed 292 files and all 3,836 tests. Source hashes below identify the final
working tree. Logs and receipts are under `/tmp/vary-ai-all-submit-20261111/`;
`rust-final-results.json` supersedes the intermediate Rust runs from before the
visibility fixture was corrected. The superseded full Rust process was interrupted
before retrying against the updated fixture.

| Command                                                            | Result |
| ------------------------------------------------------------------ | ------ |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed |
| `pnpm test:dev`                                                    | Passed |
| `pnpm typecheck`                                                   | Passed |
| `pnpm test:macos-dmg`                                              | Passed |
| `pnpm format:check`                                                | Passed |
| `pnpm test:trae-bridge`                                            | Passed |
| `pnpm test:model-default`                                          | Passed |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed |
| `pnpm test:doubao-checkin`                                         | Passed |
| `pnpm test:agent-rewards`                                          | Passed |
| `pnpm build:renderer`                                              | Passed |
| `pnpm test:qoder-codec`                                            | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed |
| `pnpm audit --prod`                                                | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed |
| `pnpm test:unit`                                                   | Passed |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`   | Passed |

The standalone library suite passed 6,318 tests with 19 ignored. The complete
Rust command passed 6,481 tests across library and integration targets, with
19 ignored and no failures. `git diff --check` also passed.

## Platform and acceptance boundaries

The configuration and credential-store logic includes macOS and Windows paths.
Local macOS checks do not establish Linux, Windows or Windows-to-WSL2 acceptance.
Exact pushed-revision CI must finish before acceptance. Installer, signing,
notarization and real vendor inference are outside this submission verification.
See the November 10 configuration-parity and credential-store records for the
original reproductions and broader adapter limitations.

## Source hashes

| File                                                           | SHA-256                                                            |
| -------------------------------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/claude_desktop_config.rs`                       | `b77c731dd2cac4b37b44842a71f9d751d971979a57984960fd8a0068fd7ad545` |
| `src-tauri/src/claude_desktop_config/longest_tests.rs`         | `f6ac3c0e21b2abe726b549b5d7c4d6c56fe505d5111a9e8a15bb0363ac5a1261` |
| `src-tauri/src/claude_desktop_config/reasoning_tests.rs`       | `a57eaa2c2e13250ef7cef3d69dc3d47dd7727ec332aa4c32d9c355ec8a22de1a` |
| `src-tauri/src/claude_desktop_config/shared_recovery_tests.rs` | `96165018065549119a637c4e8b6360bfa08b3de044d704a7c232e3f844224007` |
| `src-tauri/src/services/agent_clients.rs`                      | `ed0e8fc1188d3e81ec8fe03d889dd9bef0d91f5c717f979f4ee3fbb9c474356e` |
| `src-tauri/src/services/agent_connections.rs`                  | `20fff03b5891c1fba2b05429384d782e07765992f26445448388fa6f784f5a9f` |
| `src-tauri/src/services/agent_connections/jsonc.rs`            | `0764bab704f781ec1b1c4dcf670eefa67ef4bc576708b90631d008986d88f88b` |
| `src-tauri/src/services/agent_connections/native.rs`           | `63b48b876cc04a6f756a8b0a48e0e40527b9b8570ccebd3f990003cff67410be` |
| `src-tauri/src/services/agent_connections/parity_tests.rs`     | `50cebdd8ea909de643cc724841a02d47f97cd80b0906f925840ee50049b836be` |
| `src-tauri/src/services/agent_connections/projections.rs`      | `b1b2f92cde2c7d11077fd531177221ddfd477d587fc48e359f761d6d628396b6` |
| `src-tauri/src/services/agent_connections/reasoning.rs`        | `8b280e6fb9437b3da735f3f0361565f188d6df566a21684d24ff0821951936a2` |
| `src-tauri/src/services/agent_connections/selection.rs`        | `ba55092458362be5ffd09a1127c106c1eeda6fd15dbbbb9c291a270b9792463a` |
| `src-tauri/src/services/agent_connections/visibility.rs`       | `1f3b696c0274b17a6b7c97fa6254f68497e920b0fdb05fb34206bb0a4f74dace` |
| `src-tauri/src/services/deepseek_harness.rs`                   | `c325eae8b56188ed776b72882fed4c25427dd692bfa8645e494056ec7913e190` |
| `src-tauri/src/services/subscription.rs`                       | `48c350c989ce7e902b0eaa6b0199c7cdbae8d15ff209fdb4a00fc830a900f691` |
| `src/components/onboarding/OnboardingWizard.tsx`               | `04dad8ae65ab2b2ab017422edae20a97b0abc4146c5661e56433a82122a79930` |
| `src/components/providers/CodexProviderFields.tsx`             | `522f69896a33300c4ca283b5b8682cb318e4eb7c5801118650aa30817baa0169` |
| `src/i18n/locales/en.json`                                     | `d956ce2d813a2cabe5238b994831ad324485e7d043297efc6ff5378dd0d181fd` |
| `src/i18n/locales/ja.json`                                     | `67c9de2f44c18cb6492aaa9f50ef956f6979a5b8c057e8fdb89a3587aa6dc013` |
| `src/i18n/locales/zh-TW.json`                                  | `695287f47bdf2e5b577756516491164ca1152c9e1354879ba2cce103aa1d0c41` |
| `src/i18n/locales/zh.json`                                     | `b662b504d653b86bc796a2f200f112c48bfc8b715e9a28d673d58e710a0a8ef5` |
| `tests/components/CodexProviderEditor.test.tsx`                | `3f441c0d95925077d9e4a69af4fd788af3ac0676f4fda0261fe7e9245d51dfe3` |
| `tests/components/WorkspaceNavigation.test.tsx`                | `082be5fb933bbc98fc563601ec2d8342d0b8825244804a9cc6f94d0df2afc488` |
