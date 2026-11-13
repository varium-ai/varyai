# Routing group table verification

Date: November 12, 2026 (Asia/Shanghai).

## Scope

Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`, plus the renderer
working-tree changes listed below. Existing backend, provider editor, icon,
localization and documentation edits were preserved. Other work continued in
this checkout during verification. No commit, push, application launch,
installation or configuration changes were requested or performed.

The routing group section follows the supplied table reference: title and total
count, status filter, icon actions, muted column headings, aligned rows, a status
dot and an Edit action. Search still matches names, aliases, providers and models.
The total count remains independent of the combined text and status filters.
Manage now reveals discovery, provider exclusions, templates and bulk selection,
including when there are no groups. Group publication, strategy selection,
manual model selection, ordering, context menus, import and editor dialogs retain
the existing API operations.

Upstreams counts distinct provider IDs across configured and dynamically matched
members, including configured members that are currently switched off. The
catalog API does not expose cumulative request, switch or Agent error totals by
group. Those three reference columns show an em dash with an accessible
unavailable explanation; they do not manufacture zero values or infer lifetime
statistics from the bounded live trace buffer. No backend/storage contract was
added. macOS and Windows use the same React implementation. English, Simplified
Chinese, Japanese and Traditional Chinese labels are provided.

## Environment

macOS 15.3.1, Apple Silicon; Node v24.11.0; pnpm 10.12.3;
rustc 1.98.1 (48a229cea 2026-09-01); cargo 1.98.1 (797e8a9bc 2026-08-05).

## Browser verification

An isolated Vite server rendered the actual component using English synthetic
providers/groups and stubbed routing APIs. It did not start the desktop app or
read real provider credentials/configuration.

- Desktop: title/count, search, single-chevron status filter, table columns,
  provider icons, status dots and Edit actions rendered correctly.
- Enabled-only filtering removed the disabled fixture and retained the total
  count. Manage exposed discovery, templates and bulk selection.
- At a 520px viewport, document width remained 520px. The table scroller measured
  470px with a 790px table, keeping overflow within the table.
- Dark mode used the existing theme variables, including the corrected search
  input surface. The viewport override was reset afterward.
- A desktop preview screenshot was saved outside the repository at
  `/Users/guitar/.codex/visualizations/2026/11/12/01a1cdd8-9ce8-7212-9843-48b553599790/routing-groups-desktop.png`.

## Automated verification

The new table, combined-filter and upstream-count tests failed before the
implementation. The final focused run passed all 105 tests:

`pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx tests/components/RoutingPage.test.tsx`

Final `pnpm typecheck`, `pnpm format:check`, and `pnpm build:renderer` passed.
An earlier type/format run encountered another task's temporary
`src/provider-style-preview.tsx`; the file was gone before the successful reruns.

The full `pnpm test:unit` run finished with 296 files passing and three failing:
3,907 tests passed and eight failed. The failures were outside routing:

- `tests/integration/App.test.tsx`: two project-switcher assertions.
- `tests/components/ProviderEditorModels.test.tsx`: five assertions covering
  image/chat separation, model-list/fetch placement and selection.
- `tests/components/ProviderParity.test.tsx`: one model-selection assertion.

Provider editor files and other surrounding modules already had unrelated edits
when this task started. These full-suite failures are reported without treating
the routing-only pass as full acceptance or changing unrelated assertions.

`pnpm audit --prod` passed with no known vulnerabilities. The following helper
checks passed: `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
`pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`,
`pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec`,
and `python3 -B scripts/tests/recover-shared-v23.test.py`. Qoder codec reported
two passing tests and one skipped installed-client fixture test. The Rust runs
reported 19 ignored tests; those ignored tests were not treated as passing.

`cargo fmt --check --manifest-path src-tauri/Cargo.toml`,
`cargo check --manifest-path src-tauri/Cargo.toml`, and
`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` passed.
`cargo test --lib --manifest-path src-tauri/Cargo.toml` finished with 6,335
passing, two failing and 19 ignored tests. Its failures were:

- `commands::agent_connections::tests::reasoning_catalog_projects_into_native_agent_files_and_restores_them`:
  a generated model label differed from the expected provider-prefixed label.
- `services::provider_import::tests::provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs`:
  the imported model was not found.

A separate `cargo test --manifest-path src-tauri/Cargo.toml --test '*'` run
passed all 164 integration tests across 14 targets (including the empty support
target). Assertions were not changed to bypass the failing library tests.

`cargo test --manifest-path src-tauri/Cargo.toml` also failed with the same two
library failures: 6,335 passed and 19 ignored. Because Cargo stopped at that
library failure, the integration results above come from the separately targeted
run. This change edits no Rust files. Remote Linux, Windows and macOS CI, installer, signing and
notarization acceptance were not exercised. Verification is incomplete where the
full-suite failures or remote coverage above apply.

## Checked files

- `src/components/routing/IntentRoutingPanel.tsx`
- `src/components/routing/routing.css`
- `src/i18n/locales/en.json`
- `src/i18n/locales/zh.json`
- `src/i18n/locales/ja.json`
- `src/i18n/locales/zh-TW.json`
- `tests/components/IntentRoutingPanel.test.tsx`

Snapshot hashes after the initial table implementation checks (superseded for
the two renderer files by the compact-width correction below):

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `dfc3bf1c5b569ec85b140eace859a9cde25cbb2f1d3241ba2c6d9fc7a5f025d1` |
| `src/components/routing/routing.css`            | `1615941a2c9a69f6b4e1d0d973bfed26a82f7bf01aeaf7cdc940120ef97c8d06` |
| `src/i18n/locales/en.json`                      | `d5d5da2a83c2ac42f4d1a7441ade945194199b745a4498db05a7bc1e30e7d721` |
| `src/i18n/locales/zh.json`                      | `637306e4624b3d9af3d3db68e84520edd1ea2b85bfcc5264fdd8f5db982d246e` |
| `src/i18n/locales/ja.json`                      | `6b4c2cf38550bb0901a2614406934ff626c670fa73c230ab5830a82e9bc54e1c` |
| `src/i18n/locales/zh-TW.json`                   | `e7fff51bbfdeb22c8b24ad5ab4d80acfab3012b77af5c8941d8e48e206e58567` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `f17c81d4edc953e12ed5b5021f37fc72587b9054b14f10f1250f2e5d7c7c29c3` |

## Compact-width correction

The user's follow-up exposed the empty table at a normal compact desktop width.
The original 790px minimum table width clipped the right-hand columns, while a
900px container breakpoint forced the toolbar onto a second row. A fixed minimum
height also amplified the empty area. This was reproduced in the isolated browser
before changing the stylesheet.

The corrected table uses percentage columns and the available container width.
The toolbar stays beside the title; search collapses to an icon and expands on
focus or while a query is present. Below the smaller breakpoint, toolbar actions
use their icons while retaining translated accessible names and hover labels.
Row strategy controls now override the inherited minimum width so group names
remain visible. Empty-state content centers within the actual table width.

Browser measurements and visual checks:

- At a 700px viewport, the original empty panel was 267px tall and its 790px
  table exceeded the 614px content area. The corrected panel is 186px tall,
  with a 618px table matching its 618px content area. Title and tools share
  the same row, and all seven headers fit.
- English and Simplified Chinese labels were checked at 700px and 520px.
  All seven headers fit within their cells; document width equals viewport
  width. At 520px, the English headers use a smaller type size and icon actions.
- Populated groups, search expansion, full desktop width and dark mode were
  checked. Search expands to 240px without moving the status filter or actions.
  The temporary viewport override was reset.
- Updated preview images are outside the repository:
  `routing-groups-compact-empty.png` and `routing-groups-compact-populated.png`
  in the same visualization directory as the initial screenshot.

`pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` and
`git diff --check` passed after the correction. The focused routing component run
passed 105 tests. Subsequent edits only adjusted CSS sizes and were verified in
the browser and renderer build. The complete `pnpm test:unit` rerun finished
with 298 files passing and one failing: 3,913 tests passed and five failed, all
in `tests/components/ProviderAddSheet.test.tsx`. Its failures cover add-page
navigation/focus, import-card focus, setup methods and sign-in cancellation.
Provider-related work continued independently in this checkout; none of those
files or assertions was changed by this routing refinement. All routing tests
passed in the full run as well.

The Rust and helper results earlier in this report belong to the initial
implementation; this correction changes only presentation and does not claim
new backend or remote CI acceptance.

Corrected renderer snapshot:

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `f1d2b89ac861978c4afcea728345777fd5adf66ef90e975570376d8e769cf0b6` |
| `src/components/routing/routing.css`            | `2012004ddfbe4eff2d21c74e25f5a071d7e1751a6d0a88ce2fe2a334fcafc8f9` |

## Illustrated empty state

Added a small, theme-aware vector illustration showing a source, routing hub
and three destinations above the existing empty-state guidance. It uses the
workspace's muted borders and blue accent, with no text, external asset fetch,
animation or additional interaction. The SVG is decorative (`aria-hidden`) and
not keyboard-focusable. Existing translations and routing operations are unchanged.
The image appears only when the catalog contains no groups; a filtered list with
no matches keeps its compact text-only feedback.

An isolated browser preview verified light mode at 700px and dark mode at 520px,
with no document overflow. Switching from a populated list to a query with no
matches produced no illustration. Clearing the query and selecting the empty
fixture restored the illustration. The viewport override was reset. The updated
English preview is saved outside the repository as
`routing-groups-empty-illustration.png` in the visualization directory above.

`pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` and
`git diff --check` passed. The complete `pnpm test:unit` run passed all 299
files and 3,918 tests on the illustrated empty-state working tree. Concurrent
provider work had updated the previously failing provider page behavior/tests;
this illustration change does not claim those fixes. Prior backend/helper
results do not establish a new backend or remote CI acceptance result for
concurrent changes in this checkout.

Illustrated empty-state snapshot:

| File                                                  | SHA-256                                                            |
| ----------------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/RoutingEmptyIllustration.tsx` | `836ea6ebee2341a07e4a0c9cba21db4b316a6891413d3c58a32b43623fa19c26` |
| `src/components/routing/IntentRoutingPanel.tsx`       | `29a95cf9a8783022e7e3b4064d87bd2d8f2ca6e8a44b31b321b2c80e13b7ef6a` |
| `src/components/routing/routing.css`                  | `36b57b2b80d471dafc6ba1b8fc806dfeb55e9c5524874882d833eb1591f8f862` |

## Toolbar icons and import dialog

Reduced the Import, Refresh and Manage glyphs from 16px to 12px, retaining the
existing button hit areas. Import now uses Lucide's downward-to-tray Download
symbol. A scoped SVG size rule keeps the three glyphs consistent.

Moved the import form and completion feedback into the shared `AppDialog`
component. The form is no longer part of the table panel's layout. Opening focuses
the JSON field; Cancel and Escape reset the import draft and return focus to the
Import button. Pending saves disable form actions and block dismissal. Parsing
and save errors stay inside the dialog and keep the pasted draft available;
errors remain sanitized. Successful imports show the existing completion details
and a Done action in the same dialog. Existing routing import operations and all
four runtime translations are reused. The shared React implementation applies
to macOS and Windows.

Two test-first dialog assertions failed against the inline form. The final
focused run passed 113 tests across `IntentRoutingPanel`, `RoutingPage` and
`AppDialog`, including cancellation/reset, focus restoration, pending-save
protection, completion and the existing sanitized-failure coverage. Final
`pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`, and
`git diff --check` passed. The complete `pnpm test:unit` run passed all 299 files
and 3,921 tests. No Rust or dependency files were changed by this correction;
this does not establish new backend, installer or remote CI acceptance.

The isolated browser preview confirmed:

- All three toolbar glyphs measured 12px by 12px; Import used `lucide-download`.
- Invalid JSON retained the field and displayed validation feedback in the
  dialog. A synthetic valid import displayed completion there, then returned
  focus to Import when Done was clicked.
- The table top remained at 119px after the dialog was closed. Reopening showed
  a cleared field with initial focus.
- At a 520px viewport, the dialog was 488px wide with 16px side margins and
  approximately 401px high. The viewport override was reset afterward.
- English preview artifacts were saved outside the repository as
  `routing-toolbar-small-icons.png` and `routing-import-dialog.png` in the
  visualization directory above. The isolated preview server was stopped.

Toolbar/dialog renderer snapshot:

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `274c8bd7ca684d2fa9913ccdfc4444b870354f6e07232391a14b7922209a2f23` |
| `src/components/routing/routing.css`            | `fe36b0fcbd6989466069fd8627ab0618859d7cbece3933a759ee9fd5bf2550de` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `654c5bfa0e1b354958e5c61873882657d9ee3fbb3ee7a5096221e7d45c59d8a0` |

## Persistent search field and compact selector

Removed the group-count badge from the heading and its unused stylesheet rule.
Search is now a permanently visible input at every layout width, rather than an
icon that expands over the heading. The toolbar lets the input use the available
space, up to 240px, alongside the existing actions. The search field and status
selector are both 24px tall; the status selector was previously 30px tall. Its
chevron is 12px. Existing filter behavior, smaller toolbar glyphs, import dialog
and illustrated empty state are preserved.

The existing combined-filter test was updated first to require the badge's
absence and failed against the previous implementation. The focused routing run
then passed 108 tests. `pnpm typecheck`, `pnpm build:renderer`,
`pnpm format:check` and `git diff --check` passed. An initial global format run
reported the concurrently edited `SubscriptionModelChip.tsx`; the final global
rerun passed without changing that file in this task.

The complete `pnpm test:unit` run passed 298 files and failed one: 3,922 tests
passed and one failed. The failure was outside routing, in
`tests/components/SubscriptionProviderFields.test.tsx`, in the case for selecting
and clearing all discovered subscription models. Its visibility assertion expected
hidden content but received visible content. That provider module and its test
were not changed by this routing update. The full-suite failure is retained as a
verification limitation. Prior backend/helper checks do not constitute new
backend or remote CI acceptance for this presentation-only update.

The isolated browser preview verified English and Simplified Chinese at 700px
and 520px. Both controls measured 24px tall. Search remained visibly bordered
with a placeholder before focus and retained its query/width after blur. A model
query filtered the populated fixtures correctly; no count badge remained.
Document width matched the viewport. The temporary viewport override was reset,
and the English preview was saved outside the repository as
`routing-toolbar-search-field.png` in the visualization directory above.

Current toolbar snapshot:

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `3c41d74bc35f3c599dcf0cec256672f75dacbc9e99096bb0b79b49d0dbf89be3` |
| `src/components/routing/routing.css`            | `1ce38c9b1a76004a3ce57273c57c7b7093d2c0d466620cf89a606af0938e59ec` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `dcbcfe6b22dec34eb39a76e24e060fa6801fe3c1839a3488438f6f9d5ebf4442` |

## Batch-only management and hidden discovery controls

Manage now enters list selection for bulk removal only. It no longer acts as a
disclosure for configuration or template cards, and no longer advertises
`aria-expanded`/`aria-controls`. Empty catalogs keep Manage visible but disabled.
The discovery switch, its explanation and provider-exclusion entry are hidden.
The saved discovery preference and existing catalog/discovery APIs are preserved;
entering or leaving management does not write that preference.

Templates remain available through **Create group → Templates** in the new-group
dialog footer. The menu describes unavailable presets and reuses the same
validated template drafts. Choosing a preset in a pristine new dialog requires no
discard prompt; modifying a draft first retains the existing discard confirmation.
Choosing a template does not save or publish it. The empty-state guidance was
updated in English, Simplified Chinese, Japanese and Traditional Chinese to
remove its former instruction to look for templates under Manage. The routing
guide now documents the batch-only action and temporarily hidden discovery UI.

The new management/empty-list expectations failed against the previous UI.
Updated panel tests cover hidden controls without preference writes, saved
discovery during refresh, disabled empty-list management, template availability,
all three preset drafts and preservation of an edited draft after cancelling a
template change. Direct `AutoGroupProviders` tests continue to exercise the
retained exclusion component. The focused final run passed 115 tests across
`IntentRoutingPanel`, `RoutingPage` and `AutoGroupProviders`.

`pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` and
`git diff --check` passed. The complete `pnpm test:unit` run passed all 299
files and 3,959 tests. This change modifies no Rust, storage or dependency
contracts; prior backend/helper results are not new backend or remote CI acceptance.

The isolated 700px browser preview verified that Manage exposes row checkboxes,
Select all and Remove selected. Selecting all enabled the two-item removal action;
its confirmation was opened and cancelled. No discovery control or template cards
were present in management, and the button had no disclosure state. After leaving
management, an empty fixture disabled Manage. The new-group footer exposed the
three template choices, and choosing Cheap first populated the form without a
discard prompt or a save. Cancelling returned to the unchanged list. The viewport
override was reset and the isolated server was stopped.

English preview artifacts outside the repository:
`routing-groups-batch-management.png` and `routing-group-template-menu.png`
in the visualization directory above.

Batch-management snapshot:

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `70c6fd2e756d3357a009766385707939efc8c3e2aac41e3cb7f21dcb3b93200a` |
| `src/components/routing/routing.css`            | `d9dfb38c249d9242939060f694a91aacf50c5f7041c49cbc6eccd0704e261570` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `7b5f7397028a932c91a06475246d6badb016c1ad7465432e6b04c30d7fb08e16` |
| `src/i18n/locales/en.json`                      | `0beb6135af15bb71640a5d48e8350a2198d823d66c4453feb446f03cfe281ab8` |
| `src/i18n/locales/zh.json`                      | `997d65a516e1ce6ed59d89c2731b72735715265ff7bfaea2a41b66be9f8c8bb3` |
| `src/i18n/locales/ja.json`                      | `cb191d548622854b7dcba4dccaa08a1ef4e2272fbc53935701f2abcc5b3c88c8` |
| `src/i18n/locales/zh-TW.json`                   | `7572563604b4c88adda8b9419cd987ac648677f9950afaabc67bd256663770d8` |
| `docs/guides/intent-routing.md`                 | `c5b1d1d1dfec66fa4cbe9202fee6e55cd18e2b7a2a935cd52bdb9a7d93c7e28f` |

## Persistent template cards

Restored the three preset cards as a permanent section above the routing list.
They render for both empty and populated catalogs and remain present when entering
or leaving batch management. Their visibility is independent of list filtering.
Each available card retains its Add action, model chain and explanation; missing
requirements remain visible. The extra template menu in the new-group footer was
removed. Add opens the existing populated editor and requires explicit Save.
Automatic-discovery controls remain hidden and Manage remains batch-only.
The routing guide now describes the persistent cards as the template entry.

The updated template-visibility assertion failed against the previous menu-only
implementation. Final focused routing checks passed 112 tests, including all
three template drafts, explicit save/cancel behavior, account masking and batch
management. The template detail/tooltip line uses `AccountLabel` to retain the
existing masking behavior. After that final rendering adjustment, the focused
suite, renderer build and format check were rerun successfully. `pnpm typecheck`
and `git diff --check` also passed. The complete `pnpm test:unit` run reported
301 passing files and 3,990 passing tests. The final focused rerun after the
`AccountLabel` rendering adjustment passed all 112 routing tests. No backend,
storage or dependency contract changed; these local renderer results do not
establish new backend or remote CI acceptance.

The isolated 700px browser preview confirmed that cards were visible with sample
groups, remained visible during Manage, and remained visible after switching to an
empty catalog. The second card's Add action opened its prefilled Cheap first
editor. Cancelling returned focus to Add with all three cards still visible.
The viewport override was reset. The English preview was saved outside the repo as
`routing-template-cards.png` in the visualization directory above, and the preview
server was stopped.

Persistent-card snapshot:

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `96f1b26f379f439623cb51db48a24b281106d401ef4833a98aadf4858caf424e` |
| `src/components/routing/routing.css`            | `09eed3555b60531303482f0a20fe78688daa4fe06119f30bc81db714ea6b32ab` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `4306f71a5d93f3f1f639735849ea95065d0d6addf95b42982c6f74fff3cbb477` |
| `docs/guides/intent-routing.md`                 | `30b6120279825c53d4110d2dbaaea920824c179c9e5514494781e83fdf49bb83` |

## Consolidated row actions and provider artwork

Moved the routing strategy selector and gateway-publication switch into Actions
next to an accessible pencil icon for Edit. Removed the separate status column
and hid group IDs/public aliases in list rows. Alias filtering and persisted
IDs/aliases are unchanged. The toolbar status filter is still available, and
automatic-group disable confirmation retains its existing behavior. Rebalanced
the six table columns, kept the switch at 32 × 18 px and the pencil glyph at
13 px, and allowed controls and narrow headings to wrap without horizontal
overflow. Persistent template cards and batch management remain available.

The provider-artwork regression came from explicitly rejecting registered image
assets and uploaded data images before rendering the group icon stack. The stack
now reuses the provider list's UpstreamIcon component, including registered image
assets, normalized legacy keys, uploaded images and curated built-in vectors.
Missing artwork uses the provider list's neutral icon. Explicit saved icon choices
and existing account masking remain respected. No routing algorithm, backend,
identity or publication semantics were changed by this UI adjustment.

Test-first checks failed for the previous column layout/visible alias and for the
missing switch. Provider-artwork tests then reproduced five failing cases across
the row and icon component before the renderer was shared. Final focused checks
passed 113 tests across IntentRoutingPanel, RoutingGroupIcon and BuiltinIcons.
TypeScript, renderer build, source formatting and diff whitespace checks passed. The final full frontend run passed 301 files and 4,003 tests. The subsequent CSS-only hover adjustment was checked with a fresh renderer build, source formatting and browser verification.

The isolated browser preview used synthetic providers only. At 700 px, strategy,
switch and pencil fit together in the action cell; toggling the switch persisted
the fixture, strategy changes updated it and the pencil opened its editor. At
520 px the page did not overflow horizontally. A synthetic saved data-image icon
loaded successfully in the group stack at 13 × 13 px. The English preview is
`routing-actions-column.png` in the visualization directory above. Viewport
overrides were reset and the owned preview servers were stopped. No native
application was launched or installed and no real provider settings were changed.

Verification host: Darwin 24.3.0 arm64, Node 24.11.0, pnpm 10.12.3,
Rust 1.98.1. Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`,
with the existing dirty working tree retained. The snapshot below identifies
the UI files checked, rather than attributing other concurrent changes to this
request.

Backend verification was rerun against that working tree: `cargo check`,
`cargo fmt --check` and Clippy with `-D warnings` passed. Both `cargo test --lib`
and the full `cargo test` reported 6,337 passed, 2 failed and 19 ignored library
tests. The failures are outside this UI change: the native-agent reasoning
capabilities fixture expects a synthetic display-name prefix, and the provider
backup import fixture reports a missing imported model. The full command stopped
at those library failures; a separate `cargo test --test '*'` passed all 14
integration targets (164 tests). Backend acceptance therefore remains incomplete;
these checks do not establish Windows/Linux or remote CI acceptance.

The final hover adjustment explicitly gives enabled strategy, publication and
Edit controls a pointer cursor; disabled controls use the unavailable cursor.
Edit uses the theme accent for its foreground and a 10% accent background on
hover and visible keyboard focus. The browser preview confirmed the accent
feedback in both themes. The English light-theme preview is
`routing-actions-hover.png` in the same visualization directory.

Final UI snapshot:

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `c83a5b649f3a710e0b8cb2ea0d9b68e7cd2238c5db8c51f277635ac4685ae7b5` |
| `src/components/routing/RoutingGroupIcon.tsx`   | `13b12485574aa77a1ede81c797010b194d155fab70c6418e2e52862a59082751` |
| `src/components/routing/routing.css`            | `c9f6271d2e42aa89e8c494ebe8db774ef57c396a2fd17003fba60eeaced32166` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `69125e377eab66bad9a16ef03a3cbe1a126b0f598b3b3f0d3927f5f69123e874` |
| `tests/components/RoutingGroupIcon.test.tsx`    | `dac26523367e2cb309a120728dd77f1cd71b282b8f6b9e3580715d31baa498ad` |
| `docs/guides/intent-routing.md`                 | `1ede0104fd7588c650808b28b09e52b28003a68fdfe27e23effa053355839950` |

## Row deletion and narrower editor

Moved group deletion out of the editor footer and into each row's Actions cell
as an accessible 13 px trash icon next to Edit. The icon has a red foreground
and a subtle red hover/focus background. It opens the existing confirmation
using the saved group's ID and revision. Deletion remains blocked during
editing, pending changes, another confirmation or batch selection. The editor
footer now contains only Cancel and Save. Member/rule editing controls retain
their own scoped removal actions. Reduced the editor's maximum width from
1,040 px to 880 px and retained the 48 px viewport allowance on narrow screens.

Updated tests first: all three focused deletion cases failed against the previous
editor-only entry. Final IntentRoutingPanel tests passed 97 cases, including
confirmation cancellation/focus return, exact revision, pending dismissal
protection, successful list refresh and sanitized errors. `pnpm typecheck`,
`pnpm format:check`, `pnpm build:renderer` and `git diff --check` passed.
The complete frontend suite passed 301 files and 4,004 tests. After the last
CSS spacing adjustment, formatting and renderer build were rerun successfully.
The host/toolchain and base revision are unchanged from the preceding record;
the final dirty UI snapshot is recorded below. Backend files were not changed
for this request, so backend checks were not rerun; the two previously recorded
Rust library failures remain unresolved and are not treated as acceptance.

The isolated browser preview confirmed the trash icon in the Actions column,
Cancel returning to the unchanged list, and confirmed deletion removing the
synthetic fixture. The fixture was restored through the preview controls.
The editor measured 880 px wide at a 1,180 px viewport and 652 px wide at a
700 px viewport, with no horizontal body overflow. Its footer exposed only
Cancel and Save. English previews are `routing-row-delete.png` and
`routing-editor-compact.png` in the visualization directory above. The viewport
override was reset and the owned preview server was stopped. No installed
application or real provider configuration was changed.

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `d9468916e0271348377c41e23046b31d9e58fb4df60cc9b7f7d22dfd7345af30` |
| `src/components/routing/routing.css`            | `65f49ffe2efbc6c85bce2ae53787d070957dab6c7d48e510d9d133508724a7d4` |
| `src/components/routing/routing-group-form.css` | `18e223c03906c3714ca08af450a9cc853edded19b900812900cc52cadb9d208a` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `73a8ec83908b25a9eff07925ad2423fa4336f93948fa4e0a9f00db84201efb80` |
| `docs/guides/intent-routing.md`                 | `c8eb3172ce7b6616c0dbb5a80d6309246577e4b505ff50ff2ef8e244a0740d2a` |

## Separators only between group rows

Replaced each row's bottom border with a top border on adjacent group rows.
A single visible result has no divider, including a result obtained by filtering.
Multiple rows retain one separator between each pair and have no trailing line.
This update changes only CSS and the guide; no JavaScript or backend behavior
changed. Existing working-tree changes were retained.

The isolated 700 px synthetic preview measured 0 px top/bottom borders on the
first row and a 1 px top border / 0 px bottom border on the second. Filtering
to one result measured 0 px on both edges and visually removed the trailing
line. The English preview is `routing-single-row.png` in the visualization
directory above. The viewport override was reset and the owned preview server
was stopped. No installed application or real configuration was touched.

`pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` and
`git diff --check` passed on this CSS/documentation update. No additional unit
tests were introduced for this visual-only adjustment; the preceding 4,004-test
frontend run remains a historical result for that earlier tree. Backend tests
were not rerun; previously recorded failures remain unresolved. Host/toolchain
and base revision are unchanged from the preceding record.

| File                                 | SHA-256                                                            |
| ------------------------------------ | ------------------------------------------------------------------ |
| `src/components/routing/routing.css` | `9041b7e272ba7e2d700775d2cb22af7594f9d454656e2cc55d0ed37cf6b2fa07` |
| `docs/guides/intent-routing.md`      | `54cc313e45dfcc36504099b28718967e22056ada6bb85dc6d45bb12c677adbb0` |

## More menu, toast feedback and fixed actions

Moved Duplicate and Copy to share from the provider-icon/context menu into a
More dropdown in Actions. These are its only two entries, with no duplicate
icons. Removed the Move up/down entries and JSON export implementation. The
provider icon retains drag and Alt+arrow ordering without opening a menu.
Copy success now calls the standard Sonner success toast after the clipboard
operation resolves; no copied-status element is inserted into the action row.
Clipboard failures retain sanitized errors and never show a success toast.
All four runtime languages include the More label.

Group names display in full and wrap as needed. The table has a 1,120 px
minimum width and horizontal scrolling, with a fixed 228 px Actions column on
the right. The strategy selector is capped at 90 px and all action controls
remain on one line. Fixed metric widths leave at least 442 px for the name
column at the minimum table width. The drag grip now sits inside its icon
button's reserved left padding and is not clipped by the scroll viewport.
The empty-state illustration remains centered within the viewport while the
table is horizontally scrolled.

Replaced the initial per-cell outer shadows with one continuous inset shadow
on the fixed-column boundary. The shadow geometry follows Ant Design's table
fixed styles (`inset -10px 0 8px -8px` for the right boundary and the mirrored
left edge), using the existing theme line color. Scroll and resize observers
show a shadow only while content remains hidden beyond that edge. The fixed
column has an opaque theme background, and the edge overlay does not intercept
pointer events. No Ant Design dependency was added. Reference read:
https://github.com/ant-design/ant-design/blob/master/components/table/style/fixed.ts .
The downloaded reference snapshot's SHA-256 is `9fdcdeb2cea17522488cee9bc918ac60ef083ff2b0841722a5997f7ded871dc6`.

Test-first menu/toast checks failed four cases against the old implementation.
The final focused routing suite passed 98 tests, covering the two-entry menu's
placement, no menu on icon/context clicks, duplicate references, successful
clipboard completion, no inline copied status, sanitized clipboard failure,
batch/editor disabled states and keyboard ordering. Final `pnpm typecheck`,
`pnpm format:check`, `pnpm build:renderer` and `git diff --check` passed.

A full frontend run during the evolving working tree reported 302 files:
300 passed / 2 failed, with 4,019 passing and 8 failing tests. The failures were
in AppUpdateDialog and releaseNotes, outside this routing change; the initial
release-note failures reported a missing fetchReleaseHistory export. Those
uncommitted files changed afterward. A targeted recheck passed 29 tests and
failed one AppUpdateDialog test: changing interface language found two `v0.3.0`
labels where the test expected one. This check remains incomplete; neither the
initial full run nor the later recheck is described as full-suite acceptance.
No update-dialog/release-note files were edited for this routing request.
Backend checks were not rerun for this renderer work, and previous results do
not validate the other concurrent backend changes.

The synthetic browser preview verified full wrapping names, 228 px actions,
442 px names and equal vertical centers for all five action controls. At
700 px, only the table scrolled; the page did not overflow. At 1,000 px the
scroll range was 202 px, and the action/header right edge stayed at x=959.
The right shadow changed from visible to hidden at the end while the left
shadow appeared. At 1,400 px the 1,150 px viewport fit the table and both shadows
were hidden. Native dragging through the now-visible grip reordered the fixture
without opening a menu or editor; the fixture was restored. The More menu showed
two text entries, and a mocked clipboard operation displayed the actual Sonner
Copied toast. The mock never wrote to the system clipboard.

Final English artifacts in the visualization directory above:
`routing-fixed-actions.png`, `routing-more-menu.png`, `routing-share-toast.png`.
The viewport override was reset and the owned preview server was stopped.
No installed application, real provider configuration or credentials were used.
Host/toolchain and base revision are unchanged from the preceding record.

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `ed5e5a7dec840024dd91812678504c8e09ec45f4d8dab1ec70c3f347b09eba61` |
| `src/components/routing/routing.css`            | `970e8299d172492b5e1fe383b2fbde698d83766729d637f0ba70ecfa44613f0e` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `74a3f8f554346805abfd3073fc33fcda9e51faa4ed5c48d4cc757e26acfe82b7` |
| `src/i18n/locales/en.json`                      | `c482aa4b8837ce86f9a10ad53694c52e44110588379093a468708d33ae154b25` |
| `src/i18n/locales/zh.json`                      | `8620f724ea0f3a7bd4edde914011b6db6f7dc32f8434af344bd912a0ec3e0f7f` |
| `src/i18n/locales/ja.json`                      | `aa2ba0d9c4aee0a3fec512d21a1250d9573c012c57a99818bc9f8358af393a6c` |
| `src/i18n/locales/zh-TW.json`                   | `249c1cef013c81fcbd2d29ec21f561c6efc8ddd1d30dd5d7548ec8431d3135dd` |
| `docs/guides/intent-routing.md`                 | `8ee4668f1d339fd7c29cc0656deca504e673dcc02903a78d1fa40f9616d1ef67` |

## Complete table row drag previews

The reported clipping happened when the row sorter transformed native table
rows that also contained sticky action cells. Table dragging now creates one
inert, aria-hidden preview outside the table, with explicit measured column
widths, row height, current horizontal offset, typography and theme variables.
Its fixed action cell stays aligned with the viewport. The source cells retain
their layout but hide their contents until the drag ends. Adjacent table cells
animate without transforming their parent rows. Non-table consumers keep the
existing sorter path. The preview clones DOM only, without mounting controls,
queries or listeners from another React row. HTML control IDs and label links
are removed; SVG paint-server references remain intact for provider artwork.

Completion, Escape, pointer cancellation, lost capture, blur, resizing and
unmount remove the preview and restore the original cells. The vertical
auto-scroll search now skips horizontal-only containers whose contents do not
overflow vertically, so it scrolls the surrounding workspace instead. The guide
describes the preview and cancellation behavior. This is shared renderer code
for macOS and Windows; no native or storage contract changed.

Test-first table cases reproduced five failures against the old row-transform
path. A separate auto-scroll regression also failed before the ancestor search
was corrected. Final focused routing and drag checks passed 110 tests, including
12 drag-hook cases. They cover nested non-table slots, the pointer threshold,
inert full-row previews, preserved column widths/horizontal offset, no live row
transforms, saved source/target IDs, cleanup/restored visibility, cancellation,
unmount and workspace edge scrolling. A test typing cleanup removed an
unsupported Testing Library option; the final hook suite passed all 12 cases
afterward. Final `pnpm typecheck`, `pnpm format:check`,
`pnpm build:renderer` and `git diff --check` passed. The complete frontend run
passed 302 files and 4,035 tests, including other consumers of the shared hook.
Earlier update-dialog failures in this evolving working tree were not reproduced
by this complete run.

The isolated synthetic browser harness exposes a held-drag control so the
in-progress frame can be inspected. At 1,000 px, live and preview widths were
442/110/100/100/140/228 px, both row heights were 65.5 px and both action-column
right edges were x=959. Live table rows had no transforms. The complete wrapped
name and all five action controls remained visible on one floating row. The
same preview was checked in both themes. At 700 px, live and preview action
edges both measured x=659. Resizing and Escape removed the preview and left no
hidden source cells. An actual native mouse drag also reordered the synthetic
rows successfully. Fixtures were restored after verification.

The final English in-progress screenshot is `routing-drag-preview.png` in the
visualization directory above. Viewport overrides were reset, held drags were
cancelled and the owned preview server was stopped. No installed application
or real provider configuration was used. Backend checks were not rerun for
this renderer change; earlier backend results do not validate the unrelated
concurrent backend edits. Host/toolchain and base revision remain as recorded
above.

| File                                     | SHA-256                                                            |
| ---------------------------------------- | ------------------------------------------------------------------ |
| `src/lib/routingTableDragPreview.ts`     | `0c41a95cab3359b600b9da19296cb86b0e303d457e5ed2840e6b9e217542538c` |
| `src/hooks/useRoutingRowDrag.ts`         | `067448242fd7c9b1f465a701996d6c45edd143cb4de69a12d38e357befda5af3` |
| `src/components/routing/routing.css`     | `621e8654026d57ec0da458a64b0fcea7000308e6111f15c5756da316c60fdc83` |
| `tests/hooks/useRoutingRowDrag.test.tsx` | `8a2974170d3e533cf430bf0442aedaa66da530cf7333c9505f4d739a76a0f049` |
| `docs/guides/intent-routing.md`          | `e09112fcce98530da9b9acabe64eed4a62dbb3930e0c5fc92ea203ce1a617f22` |
