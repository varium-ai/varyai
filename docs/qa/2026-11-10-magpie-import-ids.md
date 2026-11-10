# Magpie provider ID import

## Scope and behavior

The reported provider ID was an application-generated UUID instead of Magpie's
saved ID. Import readers used a placeholder ID, both confirmation paths cleared
it, and the database always allocated a UUID. The final request explicitly asks
for Magpie IDs and direct replacement without compatibility aliases or suffixes.

Magpie API providers now retain `providers.json` IDs exactly. Saved sign-ins use
their login IDs, or an explicitly saved provider-policy ID if a login ID is absent.
Invalid or missing IDs are unavailable, rather than receiving an invented ID.
Magpie rows stay selectable when reimporting. Full import upserts the complete
source configuration under its source ID and advances the existing revision.
It does not generate UUIDs, collision suffixes or alternate display names.

Legacy UUID records identifiable by the existing credential/endpoint or account
identity matching rules are removed in the same transaction. Stored routing-group
and automatic-group references move to the source ID; old forwarding aliases are
not attached to the replacement. Source files stay read-only. Explicit key-only
operations retain their destination identity. Other applications' import policies
are unchanged. These common Rust paths apply to macOS and Windows.

## Regression coverage

Four initial regression cases were written first and reproduced the original
placeholder/UUID behavior. The final request replaced the proposed suffix policy
with direct-overwrite tests. Five ID-specific cases cover production account-aware
confirmation, changing only the display name, selectable reimport, exact-ID
replacement, legacy UUID replacement without preserving its execution identity,
invalid IDs and UTF-8/byte limits. Existing tests also cover key-only import,
source-file immutability, stale fingerprints, account-store rollback and limits.
The focused provider-import selection passed **57 tests**.

## Checked tree and environment

Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with prior dirty
changes preserved. Source hashes are recorded in
`/tmp/vary-magpie-id-final-sources.json`. Host: macOS arm64, Node 24.11.0,
pnpm 10.12.3, Rust/Cargo 1.98.1. CI uses Node 22.12.0.

Disk exhaustion interrupted an earlier Rust build. Idle generated compiler
caches were cleared, and the final Rust checks used `CARGO_INCREMENTAL=0`.
No assertion, feature or security validation was disabled. An initial library
run also caught a concurrent Codex disconnect-test change; the final library
and complete Rust commands passed after checking the current source. No import
source, real credential store or running installed application was changed by
the verification.

## Final verification

All 18 applicable local commands passed. The frontend suite passed **3,691 tests
in 287 files**; the explicit Rust library run passed **6,281 tests**, with **19
ignored**. The complete Rust run passed **6,444 tests across 16 suites**, with
**19 ignored**. The focused ID and import tests passed within those suites.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Passed |

Frontend command receipts are in `/tmp/vary-import-ids-final-front-results.json`;
backend receipts are in `/tmp/vary-magpie-id-final-back-results.json`.
The final import regression log is
`/tmp/vary-magpie-id-overwrite-focused-clean.log`.

Windows, Linux and Windows-to-WSL2 CI were not run. Installed-client reload,
installer, signing and release acceptance are unverified. No commit, push,
application installation or native-client launch was performed.
