# Provider setup-help copy shortcut and localized feedback

Date: 2026-11-10 (Asia/Shanghai).
Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing unrelated working-tree changes preserved.
Host: macOS, Darwin arm64; Node v24.11.0, pnpm 10.12.3, Rust and Cargo 1.98.1.

## Behavior

The Models / Providers header includes **Copy for Agent** immediately before
**Import providers**. The header and onboarding now share
`src/components/CopyAgentSetupButton.tsx`, which copies the existing localized
`onboarding.helpPrompt` through the shared native clipboard implementation with
its browser fallback. This change is platform-neutral and applies to macOS and
Windows without changing credential handling or provider configuration.

Successful copying displays a check icon and localized success label for two
seconds after the latest successful copy. Repeated copying remains available.
Failures display only the translated copy error, never the underlying error
text; a successful retry clears that copy error without clearing unrelated
errors. Unmounting clears the feedback timer.

The reported `common.copied` label was an unresolved translation key: all four
runtime language resources lacked that common entry. Added the missing English,
Simplified Chinese, Traditional Chinese and Japanese translations. The existing
key-only component tests could not detect missing production translations; new
tests use the real locale JSON resources and verify both the rendered success
label and the copied prompt.

## Test-first evidence

- Three provider-header regression tests failed before adding the shortcut,
  then passed after implementing it.
- Four real-locale regression tests failed with the unresolved success label,
  then passed after adding the missing translations.
- The first complete unit run caught a circular production chunk dependency
  when the shared button was placed in the onboarding directory. Moving it to
  the shared components directory removed that dependency without changing
  chunk rules or weakening assertions. The real production-build regression
  subsequently passed.
- Focused final command:
  `pnpm exec vitest run tests/components/CopyAgentSetupButton.test.tsx tests/components/RoutingUpstreamsPanel.test.tsx tests/components/OnboardingWizard.test.tsx tests/components/FirstRunOnboarding.test.tsx tests/config/rendererBuild.test.ts`.
  Result: 108 tests passed across five files, including the existing repeated-copy
  and unmount timer cleanup regressions.

## Verification

| Check                                                              | Result                                                    |
| ------------------------------------------------------------------ | --------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                    |
| `pnpm build:renderer`                                              | Passed                                                    |
| `pnpm test:unit`                                                   | Passed: 3,715 tests across 289 files                      |
| `pnpm format:check`                                                | Passed                                                    |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                          |
| `pnpm test:dev`                                                    | Passed: 8 tests                                           |
| `pnpm test:macos-dmg`                                              | Passed: 5 tests                                           |
| `pnpm test:trae-bridge`                                            | Passed: 57 tests                                          |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed: 5 tests                                           |
| `pnpm test:doubao-checkin`                                         | Passed: 13 tests                                          |
| `pnpm test:agent-rewards`                                          | Passed: 9 tests                                           |
| `pnpm test:qoder-codec`                                            | Passed: 2 tests; installed-client round-trip test skipped |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed: 5 tests                                           |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                    |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 6,281 tests; 19 ignored                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                    |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                    |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed on full retry: 6,444 tests; 19 ignored             |
| `git diff --check`                                                 | Passed                                                    |

The first full Rust test invocation failed in
`proxy::security::reversible_integration_tests::reversible_closure_resp2_metadata_replay`:
the client SSE boundary returned HTTP 504 rather than 200. The same test passed
on an isolated retry without source changes. The failure's root cause was not
established, and no backend code or assertions were changed. The subsequent full retry
passed without source changes. Both outcomes remain recorded in the logs.
The test linker also reported an oversized `__eh_frame` section warning.

Ignored Rust tests and the skipped installed-Qoder round-trip test were not
executed and are not reported as passing.
Logs and tested-file SHA-256 hashes are in `/tmp/vary-provider-copy-checks/`.
The focused log is `/tmp/vary-provider-copy-focused.log`; test-first failure logs
are `/tmp/vary-provider-copy-red.log` and
`/tmp/vary-provider-copy-locale-red.log`.

No application launch, installation, commit or push was performed. Remote CI,
Windows/Linux runtime verification and release acceptance were not performed.
