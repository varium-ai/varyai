# Original and new routing UI comparison

Historical record: the experimental four-policy implementation and UI comparison were subsequently withdrawn at the user's request on 2026-11-12. This record does not describe the current feature set.

Date: 2026-11-12.
Checkout: `codex/intent-recognition`, base `ab5cf647b304d080370af4838f96778f063ca5f0`
plus the existing working-tree routing implementation and this renderer follow-up.
No commit, push, installer replacement or installed application launch was performed.

## Behavior

- An unchecked **Use new routing UI** checkbox defaults to the original interface.
  It restores all three original templates, nine strategies and complete advanced
  settings. Template generation and its original regression tests are restored.
- Checking the box selects the four-policy creation flow. Switching views never
  writes or converts a group. An open editor, pending save or nonempty import
  draft disables the checkbox to protect work in progress.
- Existing original groups keep their exact configuration. Existing new-policy
  groups are read-only in the original editor until explicit confirmation makes
  an original custom draft, retaining members, rules and execution settings.
  Fallback semantics may then differ; saving is a separate explicit action.
- New-policy explanations and order lists span the form instead of occupying its
  narrow label column. Both views share the same existing dialog and footer.

Frontend/test working-tree manifest SHA-256: `cfa69f4725ce5b47d6652278fecdb5f8c500eb6e8c009e3f9640a0cbb4f65917`.
This covers sorted changed paths under `src/` and `tests/`, each followed by its
content SHA-256 or a deletion marker.

## Checks

Host: macOS, Darwin arm64; Node 20.20.2, pnpm 10.12.3.

| Check                                                    | Result                                                                                  |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Test-first UI switching regressions                      | Failed before the checkbox was implemented                                              |
| Routing panel/page, templates, policy and transfer tests | Passed initially: 144 tests across 5 files; final focused run: 152 tests across 7 files |
| `pnpm typecheck`                                         | Passed                                                                                  |
| `pnpm build:renderer`                                    | Passed                                                                                  |
| `pnpm format:check`                                      | Passed                                                                                  |
| `pnpm test:unit`                                         | 3,916 passed; 1 failed across 300 files                                                 |
| `git diff --check`                                       | Passed                                                                                  |

Browser verification used an isolated loopback Vite preview, synthetic providers,
English UI text and stubbed native APIs. At 1280 x 720, both form views had a
1004px content width and 1004px scroll width, without horizontal overflow. The
original template opened with two members, four rules, classifier selection and
all execution controls. Cancelling, switching to the new view, creating a draft
and switching back restored the original templates without saving a group.
The preview server and browser tab were closed afterward.

Local screenshots:

- `/tmp/varyai-routing-ui-toggle.png`: original view and unchecked checkbox.
- `/tmp/varyai-routing-original-ui.png`: original template with advanced settings.
- `/tmp/varyai-routing-original-settings.png`: original rules and classifier.
- `/tmp/varyai-routing-new-ui.png`: new policy editor with corrected content span.

Concurrent provider, member-availability, export and backend changes appeared in
the shared checkout during verification. They were preserved. Three synthetic
model fixtures in the concurrent tests were given their required display names
to restore type checking. A first full unit run observed a template-action test
while its implementation/assertion was changing; that focused test passed on
the updated tree before the final full run.

The final full suite failed only
`ProviderAddSheet.test.tsx > returns to the list after importing and restores the header action’s focus`: the document body held focus instead of the Import
trigger. The isolated test passed (1 passed, 10 skipped), while the complete
ProviderAddSheet file reproduced the failure (10 passed, 1 failed). Its provider
flow was not changed by this UI follow-up; the failure remains unresolved.
Verification is not fully green.

Rust sources, helper scripts, dependencies and lockfiles were not changed by this
UI follow-up. Concurrent backend edits are not validated by the earlier backend
results. This renderer check does not claim acceptance of those separate edits. Earlier
routing backend checks are recorded in [strategy verification](2026-11-12-routing-strategies.md).
Remote Linux/Windows/macOS CI and live installed-client acceptance remain unverified.
