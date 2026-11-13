# Simplified routing strategy verification

Historical record: the experimental four-policy implementation and UI comparison were subsequently withdrawn at the user's request on 2026-11-12. This record does not describe the current feature set.

Date: 2026-11-12.
Base revision: `ab5cf647b304d080370af4838f96778f063ca5f0`, with the routing
strategy working-tree changes. No commit or push was performed.
Host: macOS, Darwin arm64. Node 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1.

Source/test manifest SHA-256: `d6caad1adc0ed9ee651890eedd31df91e17f0025c7f17f8ab9f59373c17a760e` over the sorted changed
source/test paths, each followed by its content SHA-256 (or a deletion marker).

Later UI comparison work restores the original interface and templates by default.
See [UI comparison verification](2026-11-12-routing-ui-comparison.md) for the
subsequent renderer changes. This record describes the earlier checked snapshot.

## Scope

New groups use adaptive, capability-first, speed-first or expiring-allowance-first
policies. Legacy groups retain their algorithms and rules until explicitly
converted and saved. Tests cover native sharing/import role remapping, unknown
prices, hidden-rule rejection, strong-default classification fallback, allowance
deadlines, cold/stale/exhausted observations and cooldown. HTTP coverage exercises
classification, fallback, image eligibility and exact continuation ownership.

The design document was already modified to Chinese before implementation; its
translation is preserved, with implementation status updated. Other unrelated
working-tree state was not changed.

## Checks

| Command                                                            | Result                                             |
| ------------------------------------------------------------------ | -------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                             |
| `pnpm build:renderer`                                              | Passed                                             |
| `pnpm format:check`                                                | Passed                                             |
| `pnpm audit --prod`                                                | Passed; no known vulnerabilities reported          |
| `pnpm test:dev`                                                    | Passed                                             |
| `pnpm test:macos-dmg`                                              | Passed                                             |
| `pnpm test:trae-bridge`                                            | Passed                                             |
| `pnpm test:model-default`                                          | Passed                                             |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                             |
| `pnpm test:doubao-checkin`                                         | Passed                                             |
| `pnpm test:qoder-codec`                                            | Passed                                             |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                             |
| `pnpm test:unit`                                                   | Passed: 297 files, 3,892 tests                     |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                             |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                             |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 6,351 tests; 19 ignored                    |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 6,516 tests; 19 ignored across 16 binaries |

## Boundaries

Only official Codex allowance observations currently provide scheduler deadlines;
prepaid balance expiry and other subscription adapters are not implemented.
Existing hard conversation bindings take precedence over every policy.
The 19 ignored library tests cover explicit live-client/service acceptance,
installed SDKs and performance/corpus checks; none was enabled as a routine check.

An exploratory filtered `cargo test --lib ... policy` run hit two existing
`database::dao::tool_risk` summary assertions with an uninitialized audit gate.
Those files were not changed. The final complete library suite passed, including
those assertions. Earlier red tests for the new behavior were resolved before
acceptance checks.

No installed application was launched or replaced, no live client configuration
was edited, and tests use synthetic data and dynamic ports. No real-provider,
installer, signing or notarization acceptance is claimed. Linux, Windows, macOS
remote CI and the conditional Windows-to-WSL2 contract remain unverified; no remote
CI result exists for these uncommitted changes.
