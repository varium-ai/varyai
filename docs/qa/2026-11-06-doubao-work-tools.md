# Doubao Work links and uninstalled tool discovery

Date: 2026-11-06 (Asia/Shanghai).
Checked base: `00c94e67d696eb6247d744ad85451ac1418c553e` plus this commit's changes.
Host: macOS 26.6.2 arm64 (Darwin 25.6.0).
Toolchain: Node.js 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1.

## Changes and regression evidence

- Doubao Work website and installation shortcuts open `https://www.doubao.com/work`.
  Both UI regressions failed against the previous homepage URL before the fix.
- Desktop discovery resolves application paths and rejects bundles in `.Trash`
  and `.Trashes`, including symlinks pointing there. Both new regressions failed
  before the fix; all nine desktop discovery tests passed afterward.
- Tools confirmed as uninstalled move into **More tools**, preserving their saved
  connection, installation and recovery controls. Regression coverage includes
  both full and compact layouts and verifies that these rows cannot be reordered.

## Final local checks

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm test:unit`: 279 files, 3,479 tests passed.
- `pnpm test:dev`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`: passed; the existing
  conditional Qoder codec skip remains.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `git diff --check`: passed.

Backend source and test inputs are unchanged from the preceding checks in this
same checkout and chat. The SHA-256 of
`src-tauri/src/commands/misc/desktop_apps.rs` is
`af59b4b300161fb07a7965f39c03139abd3306a19cacc71d258ee6ee203ddbe8`.
Cargo commands used `--manifest-path src-tauri/Cargo.toml`:

- `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings`: passed.
- `cargo test --lib`: 6,054 passed, one failed, 18 ignored.
- `cargo test --no-fail-fast`: 6,217 passed, one failed, 18 ignored across all
  targets; every integration-test binary passed.

Both Rust test runs failed
`database::routing_upstreams_compat_tests::provider_notes_survive_save_edit_and_do_not_enter_upstream_configuration`
with `routingUpstreams.invalid`. This failure also occurred before the desktop
discovery change; no routing implementation or test assertion was changed here.
Full backend verification is therefore incomplete.

No installed application was launched or replaced. Remote CI and Linux/Windows
acceptance have not run for this revision. Local logs are retained under
`/tmp/varyai-commit-*.log` and `/tmp/varyai-trash-*.log`.
