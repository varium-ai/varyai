# Routing template dialog verification

Date: November 10, 2026 (Asia/Shanghai).

## Checked change

Base revision: `72ce390fa473087505934f91ed77728fac51ef01`, plus the working-tree
files identified below. Other renderer/backend work was already present and
was preserved. No commit or push was requested.

Each available template card now has one Add action. It opens the group dialog
with the template name, members, strategy, rules and classifier prefilled.
The user can edit and save; canceling creates nothing. The separate Edit first
action and immediate-persistence helper were removed. Template menu entries
share the dialog flow, fresh group identity and unique suggested name.
The four runtime languages explain the review-and-save flow.

## Environment and verification

macOS 15.3.1, Apple Silicon; Node v24.11.0; pnpm 10.12.3.
This shared React change applies to macOS and Windows.

- Test-first assertions failed for all three templates against the original two-action cards.
- `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx`: passed, 75 tests. The three template cases verify prefilled model/rule/classifier configuration, editable names, cancellation without persistence and save after review.
- `pnpm typecheck`: passed.
- `pnpm format:check`: passed.
- `pnpm build:renderer`: passed.
- `pnpm test:unit`: 288 files passed and one failed; 3,759 tests passed
  and four failed. All four failures are the pre-existing navigation label
  assertions in `WorkspaceNavigation.test.tsx`; routing suites passed.
- `git diff --check`: passed.

This follow-up changes only the renderer, tests and documentation. Rust sources,
helper scripts and dependencies are unchanged from the local checks recorded in
[routing page and form verification](2026-11-10-routing-page-and-form.md).
Their backend/helper/audit checks were not repeated for this renderer-only change.
No native application, real provider configuration or installer was launched or
replaced. Remote CI and packaged-build acceptance were not exercised.

## Checked files

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `b36d5b9a2f2975483a0ab66651661b2ca6fdad646f24e45df2db4d3513799fc3` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `8a1213a30ec0e7958e8482d4cf862596d80a05061f865d7fa82128bf49ef7f6c` |
| `src/i18n/locales/en.json`                      | `a4469d0192efddf4beacaed38c10d5cf93c6962aabf59e2635fc1636660b89c0` |
| `src/i18n/locales/zh.json`                      | `f6dfa4acff6d3e2d95f56416b4041eead391741d620c27181ca89bee78cb70cf` |
| `src/i18n/locales/ja.json`                      | `32c0196d94316acf36e6f42828b13fdd2dd7300bf1efcfe1a8927403239f012b` |
| `src/i18n/locales/zh-TW.json`                   | `fc0d7252dae021610f9f0af3273969d6fd7f2bf3f43b196c3e64fece1617e2ec` |
| `docs/architecture/routing-observations.md`     | `f6ae1c5b4cd9911342e44aea540a147a577db47fd3ad0d43d0924b04093d5d29` |
| `docs/guides/intent-routing.md`                 | `cdb79cf16b578ad9ea06fa7f0608802c43e12b1ae5a464065c6b4bb620ef622a` |
