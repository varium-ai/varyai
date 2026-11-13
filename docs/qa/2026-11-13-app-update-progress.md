# Application update progress and cancellation

## Checked changes

Checked the working tree based on `98a894d84ed2774c769db22757075777f4d5ef7a` on macOS (Darwin 24.3.0, arm64), with Node v24.11.0, pnpm 10.12.3 and Rust 1.98.1. This checkout also contains pre-existing routing, quota and navigation changes; those changes were preserved.

The update-specific tree includes `AppUpdateDialog.tsx`, its stylesheet, `UpdateContext.tsx`, `lib/updater.ts`, `lib/api/settings.ts`, the four runtime locales, the settings commands and new `settings/update_install.rs` module, command registration, update dialog/context/API tests, and the troubleshooting guide.

The native updater now sends phase and byte progress through a channel scoped to one attempt. Preparation and download can be cancelled; the native cancellation signal drops the pending check/download future. Cancellation and entry into installation share one lock, so an accepted cancellation prevents installation. The existing macOS and Windows cleanup, installation and restart order remains in place. Duplicate attempts are refused, and completed/cancelled/failed attempts release the slot for retry.

The dialog keeps progress in its footer, shows byte totals and percentages when available, supports unknown lengths, and explains a 15-second gap in downloaded data. Cancellation preserves the available update for retry and does not show an installation-failure toast. Installation locks dismissal and cancellation. All four runtime languages have matching messages.

## Verification

- Test-first frontend reproduction: six new assertions failed against the original implementation; existing release-note checks continued to pass.
- Final focused tests: `pnpm exec vitest run tests/contexts/UpdateContext.test.tsx tests/components/AppUpdateDialog.test.tsx tests/lib/updateInstall.test.ts` passed, 22 tests.
- `pnpm typecheck`, `pnpm build:renderer` and `pnpm format:check` passed on the final source changes.
- `pnpm test:unit` passed on the final source changes: 305 test files and 4,061 tests (153.16 seconds).
- `pnpm test:dev` passed, eight synthetic launcher tests. The first sandboxed attempt could not bind local ports (`EPERM`); rerunning with local-port permission passed.
- `pnpm test:macos-dmg`, `pnpm test:trae-bridge`, `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, and `pnpm test:agent-rewards` passed.
- `pnpm test:qoder-codec` passed its two portable tests; the installed-client test was skipped by the existing suite.
- `python3 -B scripts/tests/recover-shared-v23.test.py` passed, five tests.
- `pnpm audit --prod` passed with no known vulnerabilities after allowing registry access; the first sandboxed attempt failed DNS resolution.
- `cargo check --manifest-path src-tauri/Cargo.toml`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` passed on the final source changes.
- The four new native state-machine tests passed when compiled independently with `rustc --edition=2021 --test src-tauri/src/commands/settings/update_install.rs --extern tokio=src-tauri/target/debug/deps/libtokio-0907ad274b6dfe1e.rlib -L dependency=src-tauri/target/debug/deps -D warnings -o /tmp/vary-update-state-tests`, then executed. Coverage includes cancellation before and during a pending request, dropping network work, retry, stale attempt IDs, duplicate attempts, successful completion and installation locking.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml` could not complete: the first build failed with `No space left on device`. A later attempt was stopped as free space fell below 1 GiB again. No full Rust unit-test pass is claimed. `cargo test --manifest-path src-tauri/Cargo.toml` was not run because of the same disk limit.
- `git diff --check` passed.

## Visual checks and limits

An isolated browser preview rendered the actual dialog component with synthetic update state and no native updater calls. English download progress, Japanese unknown-length/stalled progress in dark mode at 390 × 640, and Chinese installation locking remained readable with visible footer controls and independently scrolling release notes. The preview server was stopped afterward.

No running native application was launched, replaced or restarted. Package downloading, signature verification and installation were not exercised against a live release. No commit or push was requested; remote Linux, Windows and macOS CI and live installer acceptance remain unverified.
