# Manual acceptance updates

These results use the current 18-row pending-list numbering. They record the user's live feedback separately from automated regression results. No tracker fields were edited.

| Current row | Issue                                                                         | User acceptance                                                                                                                                   |
| ----------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| 3           | Claude Code model-list completeness                                           | Accepted by the user on their device. The separate test-device discrepancy was not independently resolved.                                        |
| 8           | Slow tooltip on the first hover over a built-in tool-risk rule's reset button | Accepted on 2026-11-13. The user reported that the tooltip appeared quickly and explicitly passed the issue. No instrumented timing was recorded. |
| 18          | Excessively bright switches in the dark theme                                 | Accepted on 2026-11-13. The user explicitly passed the visual brightness check.                                                                   |

Rows 1, 2 and 5 were retired at the user's request. Other rows are not accepted by this record. The subsequent planned check is current row 17, direct single-Agent profile creation.

## Profile-name IME follow-up during row 17

The user reported that confirming a Chinese IME candidate with Enter created the profile immediately. The requested behavior is to save through the confirmation button; Enter in the name field must not save, including ordinary Enter outside composition.

Removed the name input's Enter-to-create handler from the shared `ProfileSwitcher`. The same component serves single-Agent and multi-Agent creation on macOS and Windows. Four regressions initially failed, then passed after the removal: ordinary Enter and IME confirmation in each mode. They verify that the dialog retains its name and makes no creation call until the confirmation button is clicked, then creates exactly once with the selected agents.

Verification baseline: `4f6de9be576d55f310c46c0ca4233b105e904f20`, plus the unstaged frontend fix and documentation updates. Host: macOS 26.6.2, Node 20.20.2 and pnpm 10.12.3. The complete profile component file passed all 44 tests, including a repeat after formatting. `pnpm typecheck`, `pnpm build:renderer` and `pnpm format:check` passed. `pnpm test:unit` passed all 4,074 tests in 304 files. Targeted formatting checks also passed for the test file and both updated documents.

Rust checks were not rerun for this frontend-only change. Concurrent staged Rust merge work was preserved. No commit, push, installation or application restart was performed during the implementation check. The IME follow-up and row 17 remain awaiting the user's live retest.

Before the subsequently requested commit and push, the production dependency audit, development launcher, macOS packaging, TRAE bridge, model-default, WorkBuddy credential, Doubao check-in, Agent rewards and Qoder codec helper suites all passed. The storage recovery helper passed five tests. The intervening merge at `37886771d026246f99cc3e318fe1a4926f8c9659` changed only Rust and documentation files; the frontend implementation and test files remained identical to the checked patch above, so the complete frontend results still apply. Only this fix's four frontend, test and documentation files are included in the new commit. Remote CI acceptance is separate from these local results.
