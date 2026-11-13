# Routing list simplification

## Scope

The routing group list now has four columns: Group name, Models, Requests and
Actions. The model total includes explicit and pattern-matched members; it no
longer counts distinct upstream providers. Removed Switches and Agent errors
columns, the import button/dialog, row strategy selectors, the More menu and
the Removed groups menu. Removed handlers and styles used only by those entries.
The strategy editor remains available through Edit. Publication, editing,
confirmed deletion, manual member selection, filtering, batch management and
row ordering retain their existing behavior. Stored groups, deletion records
and backend APIs are preserved.

All four runtime languages have the new column label. The shared renderer is
used on macOS and Windows. The action column is 108 px and the table minimum
width is 700 px, with four-column empty-state spanning.

## Verification

Host: Darwin 24.3.0 arm64. Node 24.11.0, pnpm 10.12.3, rustc 1.98.1
(48a229cea), cargo 1.98.1. Base revision:
`98a894d84ed2774c769db22757075777f4d5ef7a`, plus the working-tree files listed
below. The working tree was clean before this request.

The four updated regression checks failed against the previous implementation
and passed after the change. Retired UI tests were removed; existing strategy
save and synchronization tests now use the editor or publication controls.
The transfer utility tests remain in the full suite.

| Command                                                            | Result                                                          |
| ------------------------------------------------------------------ | --------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                          |
| `pnpm format:check`                                                | Passed                                                          |
| `pnpm build:renderer`                                              | Passed                                                          |
| `pnpm test:unit`                                                   | Passed: 304 files, 4,039 tests                                  |
| `pnpm test:dev`                                                    | Passed: 8 tests                                                 |
| `pnpm test:macos-dmg`                                              | Passed: 5 tests                                                 |
| `pnpm test:trae-bridge`                                            | Passed: 66 tests                                                |
| `pnpm test:model-default`                                          | Passed: 9 tests                                                 |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed: 5 tests                                                 |
| `pnpm test:doubao-checkin`                                         | Passed: 13 tests                                                |
| `pnpm test:agent-rewards`                                          | Passed: 9 tests                                                 |
| `pnpm test:qoder-codec`                                            | Passed: 2 tests; 1 installed-codec test skipped (worker absent) |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed: 5 tests                                                 |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                                |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                          |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                          |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                          |

`cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` passed:
7,471 library tests plus 165 integration tests across 14 integration targets;
21 library tests were ignored. The main binary and integration support target
contained no tests. Rust test linking emitted a large `__eh_frame`
compact-unwind warning; it did not prevent compilation or execution.

The initial launcher test run could not bind loopback ports in the sandbox; its
rerun with temporary loopback access passed. The sandboxed dependency audit
could not resolve the npm registry; the network-enabled rerun passed. The initial
sandboxed Rust library run was interrupted after proxy fixture failures. The
rerun with temporary loopback access passed: 7,471 passed, 21 ignored, no failures
(`cargo test --lib --manifest-path src-tauri/Cargo.toml`).

An isolated browser harness used synthetic providers and groups only. The table
showed the four expected headers and member totals of 1 and 2. Only publication,
Edit and Delete remained in each action cell. At a 700 px viewport, the page
width remained 700 px, table columns measured 382/110/100/108 px, and action
contents fit within 96 px. Light and dark themes were inspected; Edit still
showed all nine strategy choices. The viewport override was reset, the tab was
closed and the owned preview server was stopped. No installed application or
real provider configuration was changed.

English preview:
`/Users/guitar/.codex/visualizations/2026/11/13/01a1d2d6-49b5-7600-a502-896a52691bec/routing-simplified.png`.

No commit, push, remote CI, installer, signing or live application acceptance is
claimed. Linux and Windows checks were not executed on this macOS host.

## Checked working-tree snapshot

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `3f6c0ee23767cbbf31974bee9b746ebe99012d1298b3346dd324489c28c0031c` |
| `src/components/routing/routing.css`            | `85a257395f7d214c181c4cb23e35cebb02c4b58519b9f4d08d5e740474c84956` |
| `src/i18n/locales/en.json`                      | `2f8329f7779e67e8c9e93e015257c958ef0c161082dfc58e08f77124a4b1a46e` |
| `src/i18n/locales/zh.json`                      | `b4fb41e7511ffb635a61a58949b1065df70edbaa78efd3c545a0540c56a2b2af` |
| `src/i18n/locales/ja.json`                      | `319db25c712e1604e94eb456c1935338cb6a6a6c41199b0c7bd28a146f059224` |
| `src/i18n/locales/zh-TW.json`                   | `4cd65d80ab843b24cd34108e9e92b55fb904b59a985d20749a8e308c1b908be1` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `8b89e05f4b88dc51d72e3e5bae8bf6dd4a3f4919da020d1f929592ab69084604` |
| `tests/components/IntentRoutingBugs.test.tsx`   | `d81193a6c17fc89634a1bb83359c48f1cf08d332ce1dcd167be57c06eaa5bc47` |
| `docs/guides/intent-routing.md`                 | `bc5570282015f2f09c58f0801d7935c9ff64bcbe63d7abaffb4fdd3768510c40` |

## Minimum-window follow-up

Removed the 700 px minimum table width. The Models and Requests columns now
use 80 px each, Actions stays at 108 px, and Group name uses the remaining
space. Names wrap and member summaries truncate within the smaller name cell.
This CSS update supersedes the earlier minimum-width geometry and snapshot
entries for `routing.css` and the guide. Existing unrelated working-tree
changes were preserved. No JavaScript, Rust or native window limits changed.

The application minimum width is 900 px and its sidebar reserves 184 px.
The isolated synthetic browser harness reproduced the remaining 716 px routing
workspace: its table viewport was 634 px wide, but the old table required 700 px.
Requests extended behind the fixed Actions column and the right-edge shadow
was visible. After the change, the viewport and table both measured 634 px,
with columns 366/80/80/108 px. There was no horizontal scrolling, column overlap
or right-edge shadow. The narrower 640 px harness also fit: table and viewport
were both 590 px, with columns 322/80/80/108 px. The page did not overflow.
Chinese and English headings, batch-selection checkboxes, both themes and the
empty state were checked. The viewport override was reset, the tab closed and
the preview server stopped. No installed app or real provider was changed.

English preview:
`/Users/guitar/.codex/visualizations/2026/11/13/01a1d2d6-49b5-7600-a502-896a52691bec/routing-minimum-width.png`.

Verification uses the same host, toolchain and base revision recorded above,
plus this follow-up's CSS and guide changes. Type checking, source formatting
and renderer build passed. Frontend test results are recorded below. Rust and
helper checks were not repeated for this CSS-only follow-up; the preceding
results describe the earlier tree. No commit, push or remote CI was performed.

The full `pnpm test:unit` run finished with 302 passing files, 2 failing
files, 4,047 passing tests and 5 failures. These failures expected duplicate
supplier labels in `RoutingStage.test.tsx` and `RoutingPage.test.tsx`; the
corresponding source and tests changed concurrently during this run. A later
focused run against the updated files passed all 156 tests across
IntentRoutingPanel, IntentRoutingBugs, RoutingStage and RoutingPage, including
all five earlier failures. Type checking and source formatting were also
rechecked successfully. No unrelated implementation or test files were edited
for this layout follow-up. The full run is not recorded as green; the later
focused result does not claim a second complete-suite run.

| Follow-up file                       | SHA-256                                                            |
| ------------------------------------ | ------------------------------------------------------------------ |
| `src/components/routing/routing.css` | `a6021dc27127cc54bd16cfef895563fc0a5ae4f3e5e67e8a155d957b3946aa69` |
| `docs/guides/intent-routing.md`      | `6ce1eba7cc857e5aba537e0cb72968da22f58a72045bddfcc550de0422658e14` |
