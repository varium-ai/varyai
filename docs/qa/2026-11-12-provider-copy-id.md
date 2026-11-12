# Provider copy ID alignment with Magpie

Reference: Magpie revision
[`138d806f7a04559015f5591e9540522c917829bf`](https://github.com/yetone/magpie/tree/138d806f7a04559015f5591e9540522c917829bf).
Reviewed `duplicateProvider`, new-provider name editing and `slug` in
`internal/gui/assets/app.js`, plus `Slug`, `add`, `AddCopy`, `hostID` and
`freeName` in `internal/provider/provider.go`. The reference's
`provider_test.go` and `copy_test.go` confirm hostname fallback and independent
numeric suffixes for IDs and display names.

New IDs use the final display name's ASCII slug. With an existing `gs`, the
Chinese copy name produces `gs-2`, then `gs-3`; the English copy name produces
`gs-copy`. Names without ASCII letters or digits fall back to the endpoint
hostname, or `custom` for IP endpoints. Repeated display names get ` 2`, ` 3`,
and so on, with case-insensitive collision checks. Existing saved IDs and
credential-copy protections remain intact. The implementation is shared Rust
code for macOS and Windows; no platform-specific branch was added.

## Verification environment

- Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`, with pre-existing
  unrelated working-tree changes preserved.
- Host: macOS 15.3.1, Apple Silicon.
- Node 24.11.0, pnpm 10.12.3, rustc 1.98.1, cargo 1.98.1.
- Implementation: `src-tauri/src/database/dao/routing_upstreams.rs`.
- Regression tests: `src-tauri/src/database/routing_upstreams_compat_tests.rs`.
- Checked-file SHA-256 hashes, working-tree fingerprint and toolchain details:
  `/tmp/vary-provider-copy-verification.json`.
- Command logs: `/tmp/vary-provider-copy-*.log`.

The two new regression tests failed before the implementation, including
`gs-副本` versus the expected `gs-2`, then passed after the change. They exercise
15 copy cases across localized names, repeated punctuation, renamed copies,
case-insensitive name collisions, domain prefixes, ports, IPv4 and IPv6.
They use in-memory databases and synthetic credentials, make no network
requests, and verify copied credentials without printing them.

| Check | Result |
| --- | --- |
| Focused `cargo test --lib ... provider_copy_ids` | 2 passed |
| `pnpm typecheck` | Passed, including final rerun |
| `pnpm build:renderer` | Passed, including final rerun |
| `pnpm format:check` | Passed, including final rerun |
| `pnpm test:unit` | Final run: all 299 files and 3,916 tests passed |
| `pnpm test:dev`, `test:macos-dmg`, `test:trae-bridge`, `test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`, `test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed, no known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed after formatting the new tests |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,337 passed, 2 failed, 19 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Same 6,337 passed, 2 failed, 19 ignored; stops after the failing library target |
| All 14 integration targets, explicitly selected with `cargo test --no-fail-fast --test ...` | 164 passed |
| `git diff --check` | Passed |

The initial renderer run had 3,914 passing tests and one failure in
`ProviderEditorModels.test.tsx`'s vendor-mode select-all checkbox assertion.
The working tree's checkbox implementation was updated independently during
verification. The final renderer test run passed completely. The independently
updated routing CSS was also included in a subsequent successful renderer build
and formatting check; it does not affect the TypeScript or test results.

The Rust failures are
`reasoning_catalog_projects_into_native_agent_files_and_restores_them`
(expects `synthetic · synthetic` where the menu now supplies `synthetic`) and
`provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs`
(looks for the old `Custom · Synthetic` label). Both concern model display names
in the pre-existing menu changes. The first test constructs its menu directly
without database allocation; the second uses the separate provider-import
transaction. Neither failure exercises the modified ID-allocation branch.
All provider registry, rename and new copy-ID regression tests passed.

Linux, Windows, WSL2 and remote CI were not run. Verification is incomplete
while the full-suite failures remain. No installed application was launched,
replaced or installed, and no commit or push was made.
