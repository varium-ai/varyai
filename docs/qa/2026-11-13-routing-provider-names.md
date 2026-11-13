# Routing provider name disambiguation

Date: November 13, 2026 (Asia/Shanghai).

## Behavior

The request-flow stage previously displayed only the recorded provider name,
making different suppliers with the same name and model indistinguishable.
Provider labels now append ` · <provider ID>` when another configured or staged
supplier has the same name. The workspace and compact tray share this behavior,
including the card tooltip. Name matching ignores surrounding whitespace and
case. A supplier's different models, keys and execution aliases do not cause a
duplicate-name suffix.

The suffix uses the trace's public `sourceProviderId` when present and falls back
to `providerId`. It works from recorded candidates when catalog data is absent,
and includes simultaneous requests shown on the stage. Existing account and
multi-key labels retain their names. Combined provider labels pass through the
existing account mask, including email-shaped IDs and tooltips. Forwarding,
stored identities and candidate merging are unchanged. This shared renderer
implementation applies to macOS and Windows.

Magpie reference: `yetone/magpie` at
`e6eaf4d69c2ff74491c141538226b6469f4a3c89`,
`internal/gui/assets/plugins/routing/client.js`. Its `rowSub` / `provsOf` logic
adds provider names for matching account or key names across providers; provider
rows themselves retain the provider name without an ID suffix.

## Regression coverage

Five assertions failed before implementation: duplicate suppliers in both stage
layouts, a single-key supplier with a duplicate configured outside the request,
duplicates across concurrent requests, and masked combined labels. Additional
coverage verifies that multiple models and execution aliases of one public
supplier do not trigger suffixes and that account/key labels are preserved.
The final focused run passed 73 tests across RoutingStage, RoutingPage and
TrayPanel. The masking assertion waits for catalog-backed masking to settle.

## Verification

Base revision: `98a894d84ed2774c769db22757075777f4d5ef7a`, plus this working-tree
change. Concurrent provider loading, navigation and routing-list changes were
preserved. Host: macOS, Darwin arm64; Node 24.11.0, pnpm 10.12.3, rustc/cargo
1.98.1. Logs and command results are under
`/tmp/vary-routing-provider-label-checks/`; the initial regression log is
`/tmp/vary-routing-provider-label-red.log` and final focused log is
`/tmp/vary-routing-provider-label-focused-final.log`.

| Check                                                              | Result                                            |
| ------------------------------------------------------------------ | ------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed, including final rerun                     |
| `pnpm format:check`                                                | Passed, including final rerun                     |
| `pnpm build:renderer`                                              | Passed, including final rerun                     |
| `pnpm test:unit`                                                   | Passed; 4,052 tests across 304 files              |
| `pnpm audit --prod`                                                | Passed; no known production vulnerabilities       |
| Auxiliary helper tests listed below                                | Passed                                            |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                            |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                            |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed; 7,471 passed, 21 ignored                  |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                            |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed; 7,636 passed across 16 suites, 21 ignored |
| `git diff --check`                                                 | Passed                                            |

Auxiliary checks: `pnpm test:dev`, `pnpm test:macos-dmg`,
`pnpm test:trae-bridge`, `pnpm test:model-default`,
`pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
`pnpm test:agent-rewards`, `pnpm test:qoder-codec`, and
`python3 -B scripts/tests/recover-shared-v23.test.py`.

The sandbox initially denied temporary loopback ports in the launcher and Rust
tests. The blocked Rust library run was stopped, and the launcher and Rust tests
were rerun with loopback access. The interrupted sandbox run is not a passing
result. No installed application was launched or replaced. No commit, push,
remote cross-platform CI, installer, signing or notarization validation was
performed.

## Checked files

The hashes identify this change in the shared working tree.

| File                                        | SHA-256                                                            |
| ------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/RoutingStage.tsx`   | `3067effd2c6124d8241666be4fba8727cf4d5fb04c89cd66feeab0ca6a9e3200` |
| `tests/components/RoutingStage.test.tsx`    | `62e67600ac1cee38101f45942f9ffed853698324e497f67340716cf372612614` |
| `tests/components/RoutingPage.test.tsx`     | `f99ca74e35d9f69d73a4a77755e316d4bcd0496712b5b6028c20916f3cf13bd5` |
| `docs/architecture/routing-observations.md` | `1cb235bd88f3127e4cfabb619efc08f8facea57c11d371e14e933aa3d9b1a304` |
