# Onboarding copy feedback

Date: 2026-11-10 (Asia/Shanghai).
Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing unrelated working-tree changes preserved.
Host: macOS, Darwin arm64; Node v24.11.0, pnpm 10.12.3, Rust 1.98.1 and Cargo 1.98.1.

## Behavior

The onboarding setup-help button remains enabled after copying. Its success label
now returns to the normal copy label two seconds after the latest successful
copy. Another successful click restarts that interval. The timer is cleared when
the wizard unmounts. A successful retry clears only the previous copy-failure
message, preserving unrelated errors.

The copied prompt still uses `onboarding.helpPrompt` in the current application
language. The four runtime languages and the shared macOS/Windows clipboard
implementation are unchanged. No platform-specific logic was introduced.

## Regression coverage

Two new regression cases failed before the implementation change: persistent
success feedback and a copy-failure message remaining after successful retry.
After the fix, onboarding tests also cover repeated clicks, restarting the timer,
copying after the label resets, and timer cleanup on unmount.

Focused command:

```bash
pnpm exec vitest run tests/components/OnboardingWizard.test.tsx tests/components/FirstRunOnboarding.test.tsx
```

Result: passed, 45 tests across two files.

## Verification

| Check                                                              | Result                                                    |
| ------------------------------------------------------------------ | --------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                    |
| `pnpm build:renderer`                                              | Passed                                                    |
| `pnpm test:unit`                                                   | Passed: 3,697 tests across 287 files                      |
| `pnpm format:check`                                                | Passed                                                    |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                          |
| `pnpm test:dev`                                                    | Passed: 8 tests                                           |
| `pnpm test:macos-dmg`                                              | Passed: 5 tests                                           |
| `pnpm test:trae-bridge`                                            | Passed: 57 tests                                          |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed: 5 tests                                           |
| `pnpm test:doubao-checkin`                                         | Passed: 13 tests                                          |
| `pnpm test:agent-rewards`                                          | Passed: 9 tests                                           |
| `pnpm test:qoder-codec`                                            | Passed: 2 tests; installed-client round-trip test skipped |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                                    |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                    |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 6,281 tests; 19 ignored                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                    |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                    |

Additional checks:

- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, 6,444 tests across
  16 test binaries; 19 ignored.
- `git diff --check`: passed.
- Prettier check of the changed component, tests and this report: passed.

The ignored Rust tests and skipped installed-Qoder round-trip test were not
executed and are not reported as passing.

Logs: `/tmp/vary-onboarding-copy-checks/`, plus
`/tmp/vary-onboarding-copy-red.log` and `/tmp/vary-onboarding-copy-focused.log`.

Tested implementation SHA-256:
`a13d3360e07c83d0f61a834fd209a5dd8f7aa2ef93d0ca34576077e21f7e70ce`.
Tested test-file SHA-256:
`d44c1e2575f99d5deaa16935fe7ef288c0d582355161dde9381a5fe3659ef41f`.

No application launch, installation, commit or push was performed. Remote CI,
Windows/Linux runtime verification and release acceptance were not performed.
