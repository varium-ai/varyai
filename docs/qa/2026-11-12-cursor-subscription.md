# Cursor subscription verification — 2026-11-12

## Checked source and environment

- Base checkout: `0526276ed91df01a945fe854f7ab5f6b883afeff` with the working-tree changes described below.
- Scope: Cursor desktop/CLI unified entry and the Cursor subscription provider.
- Source manifest: 65 changed source/documentation files, excluding QA reports.
- Manifest digest: `bfcffbfe7966f760c8870b7df134e599e9c6c7653ef62099f5566f2653d28a81`. The sorted path/NUL/SHA256/newline manifest is retained at `/tmp/vary-cursor-subscription-verification/manifest.json`.
- Host: macOS 26.6.2, Darwin arm64.
- Rust/Cargo: 1.98.1; `CARGO_INCREMENTAL=0`.
- Required frontend/helper checks: Node 22.12.0, pnpm 10.12.3.
- Additional Cursor helper compatibility check: Node 20.20.2.
- Protocol source: `@magpie-community/opencode-cursor-auth` 0.2.44, pinned to `magpie-community/plugins` commit `401b549fb3aa56e3e8c5dfb7805a89e7e9b3158a`.

## Behavior and regression coverage

- Browser PKCE login, cancellation, late-result suppression and server-verified account identity.
- Local Cursor CLI credential snapshots and explicit JSON OAuth/API-key imports. Mutable CLI markers are rejected; an existing account cannot be reauthenticated as another identity.
- Exact model IDs, context sizes, tool/image capability flags and reasoning variants.
- Native dynamic-tool handoff and continuation, cross-account refusal, exact error/image results, duplicate exec identity validation and refusal of Cursor-native shell requests.
- Private worker authentication, ephemeral loopback listener, request cancellation, process lifecycle and startup through paths containing spaces/symlinks.
- Provider HTTP error status preservation with sanitized error bodies.
- Current-step and deferred tool usage across Chat, Responses, Anthropic and Gemini, streamed and collected; cache-read/write accounting survives conversion without increasing current context counts.
- Monetary/unbounded allowance displays use the provider observation instead of an invented remaining percentage.
- Existing TRAE bridge implementation and identity protections are unchanged.

The initial red tests exposed the missing provider entry and missing deferred usage. Full checks also identified an onboarding fixture that omitted Cursor, a protocol fixture that explicitly expected Gemini cache-write loss, and a helper entry-point comparison that failed through the macOS temporary-directory symlink. The fixtures now require Cursor and complete cache preservation; the helper compares real paths. HTTP status regressions were reproduced before correcting status propagation.

## Local verification

Frontend, formatting, renderer build and production dependency audit passed. The frontend suite passed **294 files / 3,882 tests**. The production dependency audit reported no known vulnerabilities.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed: 3,882 tests |
| `pnpm audit --prod` | Passed |
| `pnpm test:dev` | Passed: 8 tests |
| `pnpm test:macos-dmg` | Passed: 5 tests |
| `pnpm test:trae-bridge` | Passed: 66 tests |
| `pnpm test:model-default` | Passed: 9 tests |
| `pnpm test:workbuddy-checkin-auth` | Passed: 5 tests |
| `pnpm test:doubao-checkin` | Passed: 13 tests |
| `pnpm test:agent-rewards` | Passed: 9 tests |
| `pnpm test:qoder-codec` | Passed: 3 tests |
| `pnpm test:cursor-subscription` | Passed: 19 tests on Node 22.12.0; 19 on Node 20.20.2 |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed: 5 tests |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed: 6,331 tests; 19 ignored |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Passed: 6,495 tests; 19 ignored |

Rust test linking emitted an unwind-table-size warning; all required commands still exited successfully.

Raw logs and command exit records are retained under `/tmp/vary-cursor-subscription-verification/`. Earlier failed/stale runs are not acceptance evidence for the final tree.

## Acceptance limits

- Tests use synthetic credentials, messages and wire fixtures, temporary directories, in-memory databases and dynamic ports. No real subscription login or paid inference was performed.
- Live Cursor entitlements, model availability and exact invoice reconciliation remain unverified.
- Windows/Linux backend CI and the Windows-to-WSL2 contract have not run for this checked tree. CI now includes the Cursor Node protocol fixtures on all three backend platforms. Cross-platform acceptance remains incomplete until the exact revision passes those jobs.
- The running desktop application was not launched, installed or replaced. Installer, signing, notarization and release acceptance were not exercised.
