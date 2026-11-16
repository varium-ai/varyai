# Installation statistics defaults and controls

Historical verification of the former server-backed integration. The direct
gtag.js integration supersedes its transport and schema details; see
[direct GA4 verification](2026-11-13-direct-ga4.md).

The desktop application now enables installation statistics for fresh local state
and defaults to `https://www.varyai.net/api/v1/telemetry/launch`. The server route,
four-field request and `observed_date` response were checked against the local
`varyai-server` project's router and installation-analytics documentation.
`VARYAI_ANALYTICS_BASE_URL` remains a compile-time HTTPS-origin override; an
explicit empty value disables the destination.

Settings → General → Privacy provides an installation-statistics switch. Fresh
installations start enabled; users can turn reporting off and back on. The saved
choice survives reopening Settings and restarting the application. All four
locales describe the enabled default and the effect of disabling. The renderer
uses the existing local telemetry IPC, shared by macOS and Windows.

Valid legacy installation IDs and explicit disabled preferences survive upgrades.
Turning statistics off removes the local identifier and cancels pending reporting.
Corrupt state and storage failures still disable reporting, and the identifier
must be persisted before any report.

## Verification

Checked the uncommitted changes on macOS/Darwin 24.3.0 arm64, based on
`c1fc6ab85781986e24d77fb1d298c25fac86d36b`, with Node 24.11.0, pnpm 10.12.3
and Rust 1.98.1. The working tree also contained unrelated concurrent changes.

| Check                                                                                    | Result                                                                                                                                                                                                             |
| ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Test-first settings regression                                                           | Failed when the card was absent, then passed after restoring the control.                                                                                                                                          |
| Test-first telemetry regressions                                                         | Four assertions failed before implementation: automatic reporting, fresh ID persistence, default destination and legacy migration.                                                                                 |
| Focused telemetry module tests                                                           | 11 passed. The actual production module and its tests were compiled with cached dependencies in an isolated Rust harness; only the Tauri spawn entry point was mapped to Tokio. This is not a full Tauri test run. |
| Settings component, integration and locale tests                                         | 72 passed across four files, covering the enabled default, disabling, reopening a saved disabled choice, re-enabling, persistence errors and an unavailable destination.                                           |
| `pnpm typecheck`                                                                         | Passed.                                                                                                                                                                                                            |
| `pnpm build:renderer`                                                                    | Passed.                                                                                                                                                                                                            |
| `pnpm format:check`                                                                      | Passed.                                                                                                                                                                                                            |
| `pnpm test:unit --reporter=verbose`                                                      | Passed: 4,133 tests across 310 files.                                                                                                                                                                              |
| Earlier `CARGO_INCREMENTAL=0 cargo check --offline --manifest-path src-tauri/Cargo.toml` | Passed before the renderer control was restored. The telemetry backend files are unchanged; this is not a new whole-tree acceptance run.                                                                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                 | Failed on concurrent quota changes in intent scheduling, services registration and `services/quota_panel/`; changed telemetry files pass their direct rustfmt check.                                               |
| Full Rust library and integration tests                                                  | Not run: free disk space fell to about 160 MiB during verification and later recovered only to about 562 MiB. The full test artifacts were not built under that constraint.                                        |
| Changed-file Prettier, rustfmt and `git diff --check`                                    | Passed.                                                                                                                                                                                                            |

`CARGO_INCREMENTAL=0 cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings`
failed on `too_many_arguments` in concurrent `services/quota_panel/core.rs:68`;
no telemetry warning was reported.

The Rust formatting and Clippy results above are from the preceding backend
change; they were not rerun for this renderer-only restoration. Logs for the
restored switch are under `/private/tmp/vary-analytics-setting-*`; earlier logs
and the isolated Rust harness are under `/private/tmp/vary-install-analytics-*`.
No backend files were modified or production telemetry events sent. Application
installation, release acceptance and cross-platform CI remain unverified.
