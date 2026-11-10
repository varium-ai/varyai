# Routing provider model selection verification

Date: November 10, 2026 (Asia/Shanghai).

## Scope and environment

Base revision: `72ce390fa473087505934f91ed77728fac51ef01`, plus the working-tree
files hashed below. Existing routing form work and concurrent provider, locale,
backend and documentation changes were preserved during implementation. The
submission verification below covers the subsequently requested commit and push.

Host: macOS 15.3.1, Darwin arm64. Node v24.11.0, pnpm 10.12.3,
rustc 1.98.1, cargo 1.98.1. The renderer implementation is shared by macOS and
Windows; no platform-specific behavior was introduced.

## Resulting behavior

- Model selection appears inline as wrapping checkbox chips grouped by provider.
  Each group has a separate disclosure button after the provider name, tri-state
  checkbox and selected/total count. All providers start expanded. Folding preserves selection and works
  independently of bulk selection.
- New blank groups select all task-eligible catalog models, up to the existing
  256-member limit. The limit has an inline explanation in all four runtime
  languages. Templates and saved groups preserve their initial member choices.
- Identical model IDs from different providers remain separate choices. Existing
  custom or unavailable seats remain visible. New selections cannot use an
  unavailable provider.
- The dialog is up to 1040px wide, with 24px side gutters. The name input has a
  localized example placeholder. The duplicate Advanced member list was removed.
  Saved effort variants, weights, ordering and disabled IDs continue to round-trip.
- Chips display model names only. Context-capacity badges are absent.
- Selection updates only the draft until Save. Deselecting members repairs default,
  manual, disabled and rule references. Surviving rule fallbacks are promoted;
  targetless rules are removed. Dynamic pattern targets remain intact.

## Verification

New integration assertions first failed against the old add-model popover for
expanded provider chips, default selection, independent provider selection and
bulk actions. A separate regression reproduced lost dynamic pattern targets
before the membership helper was corrected. The provider-folding assertion also
failed before disclosure controls were implemented.

- `pnpm typecheck`: passed.
- `pnpm format:check`: passed.
- `pnpm build:renderer`: passed.
- Final focused routing, membership and height suites: 5 files, 112 tests passed.
- Final `pnpm test:unit`: 290 files passed, 1 failed; 3,774 tests passed and
  4 failed.
  All four failures are existing `WorkspaceNavigation.test.tsx` navigation-label
  expectations for en, zh, ja and zh-TW. The final routing and membership suites
  pass in this full run, including the animation height regression.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: passed; 6,305 passed,
  19 ignored.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed.
- `git diff --check`: passed.

Focused suites:

```sh
pnpm exec vitest run tests/hooks/useRoutingDialogHeight.test.tsx tests/utils/routingMemberSelection.test.ts tests/components/IntentRoutingPanel.test.tsx tests/components/RoutingPage.test.tsx tests/components/RoutingExecutionSettings.test.tsx
```

Logs are under `/tmp/vary-routing-members-accepted-*`,
`/tmp/vary-routing-members-final-focused.log` and
`/tmp/vary-routing-members-rust-*.log`. Final animation checks, including the
full renderer suite, are under `/tmp/vary-routing-motion-accepted-*`.

## Disclosure and dialog-height follow-up

The provider arrow now follows its name. CSS grid-height and opacity transitions
animate provider content, while chevrons rotate over 220ms. Collapsed content is
inert and hidden from assistive technology throughout the transition. A callback
ref installs a form ResizeObserver after the dialog portal mounts, so the dialog
also transitions its measured height rather than jumping between intrinsic sizes.
The observer disconnects on unmount and remains active after StrictMode effect
replay. Fractional heights round up to avoid clipping. Stable scrollbar gutters
avoid width/height feedback. Reduced motion disables all added transitions.

The desktop height cap is `min(760px, 100dvh - 144px)`, with an 80px total gutter
for windows no taller than 600px. The field layout and scrolling body are retained.
A regression test first failed at 324.4px before fractional heights rounded to
325px. It also verifies the cap, shrinking after a content change and observer
cleanup. Final follow-up checks are recorded under `/tmp/vary-routing-motion-accepted-*`.

## Browser verification

An isolated Vite preview used synthetic providers and models with stubbed native
APIs. No installed application or real provider configuration was changed.

- At 1280 by 720, the centered dialog measured 1040px wide. All 14 synthetic
  catalog models were selected in a new blank draft.
- The name placeholder displayed `e.g. Daily development` in English.
- Collapsing the backup provider hid its two chips while retaining its `2 / 2`
  count. The primary provider stayed expanded. The disclosure arrow changed
  orientation. The Advanced member list was absent.
- At 520 by 740, the dialog measured 472px wide. The model field stacked into
  one column and had zero horizontal overflow. The temporary viewport was reset.
- Follow-up: during provider collapse, the dialog measured 443.6px on its way
  to 408.6px; it settled at the target. The provider region settled at 0px and
  retained its selection. The dialog's height changed when Advanced opened.
- With Advanced open at 1280 by 720, the dialog measured 1040 by 576 at y=72.
  At 520 by 740, width remained 472px with zero horizontal overflow. The viewport
  was reset after the check. Provider height/rotation and dialog height computed
  transitions were 220ms. Final screenshot: `/tmp/vary-routing-dialog-motion-final.png`.

No remote CI, installed-app launch, installer, signing or notarization acceptance
was exercised. Local macOS checks do not establish Linux or Windows acceptance.

## Submission verification

The routing patch was applied to `311778bc1489352139a12e9f67aa18ed64eb4822`, the
latest `origin/codex/intent-recognition` after its concurrent TRAE/Doubao update, in the isolated
`codex/routing-form-submit` checkout. The original shared checkout remains intact.
Only routing-related source, tests, localization and documentation changes are
included. `ConfirmDialog` gains the sanitized error display required by routing
bulk-removal failures. Unrelated native-client, onboarding, provider-editor and
navigation-label edits were excluded. In particular, the original workspace's
four navigation-label failures are outside this patch.

Final source and test hashes below identify the isolated tree. The host and
toolchain remain those recorded above. Full checks, helper tests and the production
audit are recorded in `/tmp/vary-routing-submit-validation/results.json`.

The final isolated full renderer suite has 3,831 passing tests and two failures:
`AgentConnectionsPage.test.tsx` (disconnect conflict reporting) and
`AgentUpdates.test.tsx` (Desktop model visibility during a CLI update). Both
failures are also present in the existing remote baseline's
[CI frontend job](https://github.com/varium-ai/varyai/actions/runs/42807197973/job/129799601818).
That log was inspected before submission. Both failures remain after integrating
`311778bc`; this patch does not modify either
Agent implementation or its tests. All routing tests and the navigation-label
suite pass in the isolated tree.

The final submission checks are listed below. Helpers unchanged by the later
TRAE/Doubao update retain their earlier passing results; the affected helpers,
production audit, renderer and Rust checks were rerun on `311778bc` plus this
patch. These results supersede the earlier development-tree results for submission.
Logs: `/tmp/vary-routing-submit-final/results.json` and
`/tmp/vary-routing-submit-validation/results.json`.

| Command                                                            | Result                                           |
| ------------------------------------------------------------------ | ------------------------------------------------ |
| `pnpm test:dev`                                                    | Passed                                           |
| `pnpm typecheck`                                                   | Passed                                           |
| `pnpm test:macos-dmg`                                              | Passed                                           |
| `pnpm test:trae-bridge`                                            | Passed                                           |
| `pnpm format:check`                                                | Passed                                           |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                           |
| `pnpm test:doubao-checkin`                                         | Passed                                           |
| `pnpm build:renderer`                                              | Passed                                           |
| `pnpm test:agent-rewards`                                          | Passed                                           |
| `pnpm test:qoder-codec`                                            | Passed                                           |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                           |
| `pnpm audit --prod`                                                | Passed                                           |
| `pnpm test:model-default`                                          | Passed                                           |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                           |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                           |
| `pnpm test:unit`                                                   | Failed (existing baseline assertions; see below) |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Failed (existing baseline assertions; see below) |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Failed (existing baseline assertions; see below) |

Both Rust test commands fail the unchanged gateway catalog assertion
`proxy::gateway::tests::catalog_covers_every_agent_without_returning_credentials`:
its expected non-local-routing count is 1, while the new baseline returns 2.
Each final library run has 6,297 passes, 1 failure and 19 ignored tests. The
ordinary full command stops at that library failure, so its integration targets
were not reached locally. No Rust source is included in this routing patch.
The earlier pre-rebase full run also recorded a separate tool-risk event-summary
mismatch; the standalone library run before it passed. That intermediate result
is retained in the earlier validation log rather than counted as a final pass.

## Concurrent upstream rebase

The first push was rejected because two additional check-in commits reached the
remote while validation was running. The routing commit was rebased without
conflicts onto `9483b842a` before retrying. The routing source and tests were
unchanged by this rebase; the locale files retain the upstream's additional
check-in strings. The full local results above identify the previously checked
`311778bc` tree and do not claim to validate the later upstream backend changes.
Affected renderer checks, routing/check-in suites and the rewards helper were
rerun after rebase; their logs are `/tmp/vary-routing-rebased-*.log`. Typecheck,
formatting, build and the rewards helper passed. The combined routing/check-in
run passed 149 tests and failed one upstream `AgentRewardsPanel` assertion
(`does not offer a claim for own_app_required`); all routing tests passed. Exact pushed
revision CI must establish the final remote result. The table below reflects the
rebased source files.

## Checked files

| File                                                  | SHA-256                                                            |
| ----------------------------------------------------- | ------------------------------------------------------------------ |
| `docs/architecture/routing-observations.md`           | `41d15590b889274765cfcce5044b36b4555c0433255bb5e4f247b6ab58383028` |
| `docs/guides/intent-routing.md`                       | `3b4b4d7933d2b1419d4ae4d0307c733f9f9d87a87dc3aba8befd4a4ee29a26d8` |
| `src/components/ConfirmDialog.tsx`                    | `951bd86a0aa2d2db93002685dd06531a48fe77fa29f6ee5a34f0725d3499bf9d` |
| `src/components/routing/IntentRoutingPanel.tsx`       | `5260ff48d1bedd007ca7580cfd41efbc4837e989a6c863776b8cf8fc96ced7bc` |
| `src/components/routing/RoutingExecutionSettings.tsx` | `2021eb991a3b257bbee2e9e2def85d44bd9598c03678d7728e3b0120f77255c2` |
| `src/components/routing/RoutingMemberPicker.tsx`      | `beabcffbb95373db1e42e64cd983ff7af16b71b64e5d29d9e8dbd820a38242a2` |
| `src/components/routing/RoutingPage.tsx`              | `1d082bcbe2982d89df2df02a2b61d15ebd984df25d1e5b2c0e473c51776971f0` |
| `src/components/routing/RoutingSegments.tsx`          | `508747e713732bd1754336e5070198dddf40a2b8e64ebba60fc63f0ca5799337` |
| `src/components/routing/routing-group-form.css`       | `0ae1d7ea7f16f6945e6910048d823d9932a3f934e34889d87785ef3d91c36512` |
| `src/components/routing/routing.css`                  | `e329cc8f56b168721c33fa2ae366774ce7d9304271f535324ce258a3c2d7d005` |
| `src/components/routing/useRoutingDialogHeight.ts`    | `5711594e63d4761b822f6258388d446d5dcb68b539103e8af0934811832e0f42` |
| `src/i18n/locales/en.json`                            | `a19f6192a2e69ac60eba856e245b003c8d1eb8bc52b7f24e249d282b14b1edb6` |
| `src/i18n/locales/ja.json`                            | `b8216a8b920b4098edd2ec2aef12e258a0295aced93be699a901a379fc63cca4` |
| `src/i18n/locales/zh-TW.json`                         | `146c457872413e6486b04e537e0e21759041ce67773a8a185409eece64334e22` |
| `src/i18n/locales/zh.json`                            | `d0929fc5f7ecf0d74a2f74584e7da67db855a765eb2879755720835c0ca6094c` |
| `src/lib/routingMemberSelection.ts`                   | `d9cf581a0ad2d0684579f74a342ed193b03d5ff29e1df1419757d90d68a7a66f` |
| `tests/components/IntentRoutingPanel.test.tsx`        | `f19dbc50fc526f854a4b61b6c00b08a784fd68f358d0fdae5f15a536a61b4365` |
| `tests/components/RoutingExecutionSettings.test.tsx`  | `c06790c156912148a634c993845e11a58c4487f8154d18e2d136e01072b4d6ea` |
| `tests/components/RoutingPage.test.tsx`               | `c4e59ed03431d9e623e8bc2446d37d1372d231eec40cefab65489eef6aef63d3` |
| `tests/hooks/useRoutingDialogHeight.test.tsx`         | `7f597a0d7692111fe75e2c65bfb47dee0cc3a7f3cd06a90658d7aa3d089b44fa` |
| `tests/utils/routingMemberSelection.test.ts`          | `35895e662122b62b326e69103eb991ce7c5ea2bbc8bc3080a766139ffb5c23e9` |
