# Routing workspace simplification and group form verification

Date: November 10, 2026 (Asia/Shanghai).

## Scope and reference

Base revision: `72ce390fa473087505934f91ed77728fac51ef01`, with the working-tree
files identified below. Existing edits in the group dialog, routing stylesheet,
locale files and tests were preserved. Concurrent backend and provider-editor
work was outside this change. No commit or push was requested.

The routing page omits the decision log/replay, request history/list and account
observation panel. The live stage and summary counts precede global groups.
The new/edit form follows Magpie's model/pattern/model-list/routing/settings/rules
structure, segmented draft strategies and anchored multi-model picker. Save
normalizes inapplicable rate-limit placement and trims names. Existing validation,
publication controls, sanitized errors and pending-save protection remain.

Reference: `yetone/magpie`, `internal/gui/assets/plugins/routing/client.js`
(`groupEditor`, model picker, `drawSink`, execution choices and save normalization),
plus its routing stylesheet and `app.js` segmented controls. The inspected source
snapshot has SHA-256 `2254cc33bdafa815188672cfdf14da99e9a215785e997a50a33da5e1b5b33d97`.
The existing attribution is in `third_party/routing-reference/NOTICE.md`.
This renderer update does not claim support for reference engine features absent
from Vary's routing schema.

## Environment

macOS 15.3.1, Apple Silicon; Node v24.11.0; pnpm 10.12.3;
rustc 1.98.1 (48a229cea 2026-09-01); cargo 1.98.1 (797e8a9bc 2026-08-05).
The renderer is shared by macOS and Windows.

## Verification

- Test-first page assertion failed against the original decision log/list.
- Test-first form assertions failed before segmented strategy selection and
  sink normalization were implemented.
- Focused final run: `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx tests/components/RoutingPage.test.tsx tests/components/RoutingExecutionSettings.test.tsx` passed, 95 tests.
- Final `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`: passed.
- Changed tests and documentation also passed explicit Prettier checks.
- Final full `pnpm test:unit`: 288 files passed, one failed; 3,757 tests
  passed and four failed. All four failures are the pre-existing navigation
  label assertions described below. All routing suites passed.
- `pnpm audit --prod`: passed, no known production vulnerabilities.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`, and
  `python3 -B scripts/tests/recover-shared-v23.test.py`: passed.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`,
  `cargo check --manifest-path src-tauri/Cargo.toml`,
  `cargo test --lib --manifest-path src-tauri/Cargo.toml`,
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, and
  `cargo test --manifest-path src-tauri/Cargo.toml`: passed locally.
  Library-only run: 6,304 passed and 19 ignored. Later complete run: 6,305
  library tests and 163 integration tests passed, with 19 library tests ignored.
  Concurrent backend edits account for the different library counts; these are
  local checks of their respective working-tree snapshots, not an immutable
  cross-platform backend acceptance result. This change edits no Rust files.
- `git diff --check`: passed.

The earlier full frontend run also exposed a missing asynchronous wait in the
new live-refresh test. It was corrected with `waitFor`, then the focused suite
and final full suite were rerun. Four pre-existing navigation assertions expect
old names in the locale files; their current names were already changed when
this task began. Those unrelated changes were preserved.

## Browser verification

An isolated Vite preview used synthetic models/providers and stubbed native API
calls. It never connected to vendors or changed real client settings.

- At 1280 by 720, the dialog measured 820 by 624 at x=230, y=48: centered on
  both axes, with 48px clearance above and below.
- At 520 by 740, it measured 488 by 644 at x=16, y=48. The body scrolled
  vertically and had no horizontal overflow; the footer stayed visible.
- The model menu remained open after selecting the first model; a second model
  could be added before closing it. The draft's Save action then became enabled.
- A final screenshot was saved as a local preview artifact. No installed app,
  installer, signing/notarization or remote CI acceptance was exercised.
- Linux, Windows and macOS remote CI were not run. Verification is incomplete
  wherever the full-suite or remote limitations above apply.

## Checked renderer files

| File                                                  | SHA-256                                                            |
| ----------------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/RoutingPage.tsx`              | `2814c53411528a783cce0eecd7a6d8f68638d416ee03d0cff5cd29b12243e725` |
| `src/components/routing/IntentRoutingPanel.tsx`       | `6e0f6f89af6701d9d9715f8b426254fdd9da12d903824f3285303e98a548153d` |
| `src/components/routing/RoutingExecutionSettings.tsx` | `2021eb991a3b257bbee2e9e2def85d44bd9598c03678d7728e3b0120f77255c2` |
| `src/components/routing/RoutingSegments.tsx`          | `508747e713732bd1754336e5070198dddf40a2b8e64ebba60fc63f0ca5799337` |
| `src/components/routing/routing.css`                  | `380d79982969c16c0d059a220dfede270d8686ee859d7b508362365be4949862` |
| `src/i18n/locales/en.json`                            | `23d129dbbeb3b9d2054e924a01c517d131ada1581d8a8d2d90d58fea3b0a9954` |
| `src/i18n/locales/zh.json`                            | `0266712320d337bb844b2eae8b08ffd84a0d0ef64b6a592fa7a1d8944395593f` |
| `src/i18n/locales/ja.json`                            | `6632d380b66e604c9b7b9872b6751117e3ed46f89581bb8d9a0f107bc8c5a14e` |
| `src/i18n/locales/zh-TW.json`                         | `964ba235f09588b6ea6e48d6292bde7c752ffac2b9984e3bd32a2fe9744b810a` |
| `tests/components/RoutingPage.test.tsx`               | `c4e59ed03431d9e623e8bc2446d37d1372d231eec40cefab65489eef6aef63d3` |
| `tests/components/IntentRoutingPanel.test.tsx`        | `070b5eba28a7e92edec210fd195a6bdf939bc9b8ef36aeeed691b50273e37176` |
| `tests/components/RoutingExecutionSettings.test.tsx`  | `c06790c156912148a634c993845e11a58c4487f8154d18e2d136e01072b4d6ea` |
