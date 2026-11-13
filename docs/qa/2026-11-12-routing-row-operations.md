# Routing row operation isolation

Date: 2026-11-12. Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc` with existing uncommitted work preserved. Host: macOS 15.3.1, arm64. Toolchain: Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

The routing table previously reused the editor's global pending flag for single-row actions. This disabled unrelated rows, templates and toolbar actions and propagated a page-wide editing state. Single-row operations now track the group ID, synchronously guard duplicate submissions and release only their own state. Background list fetching no longer disables the manual refresh button. Editor persistence closes the modal and releases its global pending state while the saved row finishes synchronization.

Catalog membership/order writes use a queue and the latest query cache because the backend ordering endpoint requires the complete group list. Successful saves merge their returned revisions into the cache; removals drop only the deleted revision. Concurrent copies and failed writes retain subsequent work. Modal dismissal protection remains in place while saving or confirming deletion.

Regression coverage uses synthetic providers and deferred requests. The initial four isolation cases failed before implementation; the editor-background-refresh case separately reproduced its global lock before correction. All 105 routing panel tests pass after the fix, including independent simultaneous saves, success/failure completion order, same-row duplicate prevention, strategy changes, copies, queued copy failure recovery, deletion refresh, editor focus return and retained bulk-deletion confirmation behavior.

The first full frontend run exposed an independently added provider tooltip that bypassed account masking (301 files / 4,041 tests passed; one failed). The tooltip now uses the existing account-text helper, and its existing privacy assertion passes without weakening it. A full rerun is recorded below.

No commit, push, installed application launch, installation or real provider configuration change was performed. Checks use the working tree, not a separately committed revision. Generated inactive Rust incremental caches were cleared to make space for verification; no source or user configuration was removed.

## Verification

Results for the final renderer source:

| Command                                                                                | Result                                                                                 |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                       | Passed.                                                                                |
| `pnpm build:renderer`                                                                  | Passed.                                                                                |
| `pnpm test:unit`                                                                       | 302 files, 4,042 tests passed.                                                         |
| `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx`                    | 105 tests passed; repeated after the test file received concurrent formatting changes. |
| `pnpm format:check`                                                                    | Passed.                                                                                |
| `CARGO_INCREMENTAL=0 cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed.                                                                                |
| `CARGO_INCREMENTAL=0 cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,340 passed, 19 ignored, no failures.                                                 |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                               | Passed.                                                                                |
| `CARGO_INCREMENTAL=0 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed.                                                                                |

Logs: `/tmp/vary-routing-row-{typecheck-final,build-final,unit-final,panel-final,format-final,cargo-check,cargo-lib,cargo-fmt,clippy}.log`. The complete Rust integration suite, helper suites and production dependency audit were not repeated for this renderer fix; they remain part of pre-commit acceptance. Remote CI and cross-platform execution have not been requested or run for this uncommitted renderer fix; local macOS results do not establish Windows/Linux or installed-app acceptance.

## Checked source hashes

| File                                             | SHA-256                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx`  | `0fb0f4f248a4adc2a995a87a299a6aa24faec91042cb77a24e66637fb5934a83` |
| `tests/components/IntentRoutingPanel.test.tsx`   | `3757b1d61a6b9024e8c2902efe938afce3617fd0d0832c4daa1ba3de2899a1e4` |
| `src/components/routing/RoutingMemberPicker.tsx` | `2612dbacd1c2bfca68860f391e377ae5e894794a1659a5961e6d738d204c139e` |
| `docs/guides/intent-routing.md`                  | `7eb19e029af7da2a5cb7a430ed6f8336b7d78d3dc7b8f0e2682cbb06f471457f` |
