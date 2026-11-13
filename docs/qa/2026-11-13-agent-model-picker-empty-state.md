# Agent model picker empty-state layout

Checked the working tree based on `c1fc6ab85781986e24d77fb1d298c25fac86d36b`
on macOS (Darwin 24.3.0, arm64), Node v24.11.0, pnpm 10.12.3,
rustc 1.98.1 and cargo 1.98.1. This checkout contains unrelated, concurrent
changes; this record covers the picker changes described below.

## Changes

- Reuse the existing `mp-am-menu-note` footer for the missing shared-model hint
  in `AgentConnectionsPage.tsx`. The hint now has 8px vertical and 12px
  horizontal padding, a separator, and the picker's theme colors and typography.
- Hide provider navigation in `SharedModelPicker.tsx` when only native models
  remain. Keep the checked, disabled native model visible and searchable.
- Ignore an obsolete provider filter when the Agent navigation disappears so
  removing the last shared provider cannot hide the native model.
- Preserve provider navigation when shared models are available. These shared
  renderer changes apply equally to macOS and Windows.

## Verification

- Test-first reproduction failed for the native-only navigation and missing
  note semantics. A separate test reproduced the stale provider filter before
  its fix.
- Final focused tests passed: 266 tests across `SharedModelPicker`,
  `SharedModelPickerBranding`, `ModelPickerPopoverContent` and
  `AgentConnectionsPage`.
- `pnpm build:renderer` passed after the final picker change.
- The final `pnpm typecheck` rerun passed.
- `pnpm test:unit` passed: 308 files and 4,118 tests. This preceded the final
  stale-filter guard; all four affected test suites were rerun afterward.
- Prettier passed for both changed source files and both changed test files.
  `git diff --check` passed.
- Helper checks passed: `pnpm test:dev`, `pnpm test:macos-dmg`,
  `pnpm test:trae-bridge`, `pnpm test:model-default`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`, and
  `python3 -B scripts/tests/recover-shared-v23.test.py`.
- `pnpm audit --prod` passed with no known vulnerabilities. The audit and
  development-launcher tests required reruns outside the network/socket sandbox.
- An isolated browser fixture using the actual picker components verified
  English and Chinese layouts, light and dark themes, and a 390px viewport.
  The footer measured 8px/12px padding, a 1px top border and 11px text; no
  horizontal overflow occurred. The narrow popover stayed within the viewport
  at x=8..382px. Shared models restored provider navigation. Temporary preview
  sources were removed and the preview server was stopped.

## Verification limits

Early `pnpm typecheck`, `pnpm format:check`, `cargo check`, `cargo fmt --check`
and `cargo clippy -- -D warnings` passed. Other work continued changing the
checkout afterward, so those results do not establish acceptance of the final
combined tree.

Later checks encountered unrelated changes: TypeScript briefly could not find
`CodexQuotaControls` before its final successful rerun. Global formatting still
flagged `src/components/providers/CodexQuotaControls.tsx`,
`src/components/providers/QuotaSparkline.tsx` and `src/lib/api/quotaPanel.ts`.
Both `cargo test --lib` and `cargo test` failed to compile because of an
untyped `VecDeque` in `src/proxy/intent/mod.rs` and a missing `quota_panel`
field in an `AppState` test initializer in `src/commands/auth.rs`.

The first backend test attempt exhausted disk space. Removing regenerable Rust
incremental compilation cache recovered space; subsequent backend attempts used
`CARGO_INCREMENTAL=0` and reached the compilation errors above. No unrelated
source changes were made to address these failures.

No native application was launched, installed or restarted. No commit, push,
remote CI or cross-platform backend acceptance is claimed.
