# Navigation label overflow verification

## Change and scope

The workspace follows the active interface language immediately. Above the
640 px compact-navigation breakpoint, the sidebar uses 240 px for English,
208 px for Japanese, and the existing 184 px for simplified and traditional
Chinese. Labels stay on one line. An ellipsis contains unexpectedly longer text
without moving the icon or letting text paint over the page. The macOS drag
strip inherits the same width variable as the sidebar. Compact navigation stays
52 px wide for every language.

Width changes use a 220 ms easing transition. The macOS drag strip paints its
sidebar background in a pseudo-element with the same width transition, so the
two boundaries move together. Reduced-motion preferences disable both
transitions.

The application root exposes the current i18next language through its `lang`
attribute. CSS language matching also handles regional language tags. The same
renderer code applies on macOS and Windows.

## Reproduction and regression checks

- Before the fix, a browser fixture using the actual workspace stylesheet and
  locale labels measured English routing content at 198 px inside a 160 px
  button. Japanese routing also exceeded the button width.
- After the fix, all navigation labels in all four languages fit completely on
  one line in that fixture. Changing its language immediately changed the width
  to 184, 240, 208, and 184 px for `zh`, `en`, `ja`, and `zh-TW` respectively.
- At a 640 px viewport, all four sidebars measured 52 px with hidden labels.
  At 641 px, their language-specific widths and labels returned.
- Browser animation samples confirmed intermediate widths when shrinking from
  240 to 184 px and expanding back to 240 px. Sidebar and drag-strip widths
  stayed within 0.01 px of each other throughout the sampled transitions.
- The new application-shell regression first failed because the root did not
  expose the changed language. It now passes across all four languages while
  retaining the selected routing page and its active navigation item.
- The focused application-shell, navigation, and spacing suites passed:
  `pnpm exec vitest run tests/integration/GlobalApp.test.tsx tests/components/WorkspaceNavigation.test.tsx tests/components/WorkspaceSpacing.test.ts`
  (18 tests).

The browser fixture checks CSS geometry; it does not claim native WebView or
installer acceptance. No installed application was launched or replaced.

## Verification environment

Checked on macOS with Node v24.11.0, pnpm 10.12.3, and rustc 1.98.1, starting
from revision `98a894d84ed2774c769db22757075777f4d5ef7a` with existing unrelated
working-tree edits. This change touches `src/App.tsx`,
`src/components/layout/workspace.css`, the application-shell test, and this note.
Unrelated provider, routing, locale, and documentation edits remain separate.

The full local verification results are recorded below. Remote CI and Windows
and Linux acceptance were not run for this uncommitted change.

| Check                                                                                                                 | Result                                                                                                                                                |
| --------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                                                      | Passed again after the final renderer changes.                                                                                                        |
| `pnpm build:renderer`                                                                                                 | Passed again after the final renderer changes.                                                                                                        |
| `pnpm format:check`                                                                                                   | Passed again after the final renderer changes.                                                                                                        |
| Changed test and QA note formatting; `git diff --check`                                                               | Passed.                                                                                                                                               |
| Focused shell, navigation, and spacing tests                                                                          | 18 passed on the final renderer changes.                                                                                                              |
| `pnpm audit --prod`                                                                                                   | Passed with registry access; no known vulnerabilities.                                                                                                |
| Development launcher helper tests                                                                                     | 8 passed with temporary localhost listeners permitted.                                                                                                |
| DMG, TRAE bridge, model default, WorkBuddy auth, Doubao, agent rewards, Qoder codec, and shared recovery helper tests | Passed.                                                                                                                                               |
| `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings` with the repository manifest                        | Passed.                                                                                                                                               |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                                                               | Failed: 6,778 passed, 693 failed, 21 ignored. Logs include sandbox-denied dynamic-port listeners.                                                     |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                                                     | Failed in the library phase: 6,777 passed, 694 failed, 21 ignored. Logs include the same listener restrictions; later integration phases did not run. |

The initial `pnpm test:unit` run finished with 4,050 passed and three failed
tests. It overlapped the subsequent requested language-based layout change and
loaded the pre-change application with the newly added regression test; that
test failed on the missing root language attribute. Its results do not validate
the final renderer tree. The final affected suites were rerun separately as
recorded above and passed. The Pi provider model-fetch test timed out in the
full run but passed in a focused rerun. A focused rerun of the
legacy profile-switcher test also reproduced its five-second timeout; it imports
the separate `tests/fixtures/LegacyApp.tsx` fixture rather than `src/App.tsx`.
Full-suite acceptance remains incomplete; these gaps are not reported as passing.
