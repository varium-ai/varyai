# UI and feedback regression record — 2026-10-26

This batch covers six reports from the currently open Vary AI bug view. Row numbers below refer to that 18-record view at inspection time and are not permanent tracker identifiers. External acceptance statuses were not changed.

| View row | Report                                                                | Change and verification                                                                                                                                                                                                                                                                                                        |
| -------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 3        | All event sources still show a sensitive-information-only search hint | Show the hint only when the DLP source and its search input are selected. Regression checks source changes and clears the hint on return to all sources.                                                                                                                                                                       |
| 7        | Snapshot refresh hover hint appears after a two-second delay          | Use an immediate accessible hover tooltip instead of the browser-native delayed title. Immediately disable refresh actions and animate the selected action. Announce completion as soon as capture finishes, before waiting for the list refresh. Regression uses an unresolved capture and checks duplicate-click prevention. |
| 8        | Saving a connection preset has no completion feedback                 | Show a success toast and an accessible green status in the fixed dialog footer after a successful save. Clear it on a new operation; failures keep safe error feedback. Add translations in all four supported languages.                                                                                                      |
| 11       | A long profile name prevents scrolling the preview                    | Limit the title to two lines, retain its full name in a tooltip, and let the content shrink and scroll independently of the footer. Browser verification used a synthetic 2,160-character unbroken name and six client previews; the final client and apply/close actions remained reachable.                                  |
| 12       | A long profile name prevents opening the delete confirmation          | Keep management actions from shrinking out of view. Wrap and scroll long confirmation text while keeping actions visible. Regression opens and cancels deletion; browser verification used the same synthetic long name.                                                                                                       |
| 18       | Applying a profile does not identify clients needing a restart        | Repeat each selected client's existing restart/session instructions in the completion notification, preserving apply warnings. Cover bundle application and legacy Claude Code, Claude Desktop, and Codex profiles.                                                                                                            |

The targeted regressions failed before their fixes. Browser layout checks used isolated synthetic data and did not read or modify installed-app configuration. Temporary preview files and the development server were removed after verification. This is renderer and regression acceptance, not installer or live-client acceptance.

Other reports remain for separate reproduction. In particular, password-field and text-assignment rules already have distinct descriptions in this branch; that report was not counted as fixed. New-preset behavior, legacy profile details, duplicate-name handling, model/snapshot changes, event counts, Shell command creation, and official-login refresh remain outside this batch.

## Final verification

- TypeScript typecheck, renderer build, and Prettier passed.
- Frontend: 2,392 tests passed across 206 files.
- Rust check, rustfmt, and Clippy with warnings denied passed.
- Rust: 3,867 library tests and 152 integration tests passed; 14 existing ignored library tests remain outside acceptance.
- The installed application was not launched, installed, or replaced. No external tracker status was changed.

Follow-up: the original row 7 explicitly reports hover delay. The initial batch addressed click feedback only. A subsequent failing hover regression led to an immediate custom tooltip; hovering does not trigger snapshot capture.

## Manual self-test

Use a build from `codex/ui-copy-bugfixes`; the previously installed application does not include these source changes automatically.

1. Row 3: In Security Center, open Security Events and select all sources. The DLP-only search hint must be absent. Select the sensitive-information source to see the search input and its hint, then return to all sources and verify both disappear.
2. Row 7: Open Manage Profiles with a multi-agent test profile. Hover over its refresh icon; the snapshot hint should appear immediately. Click once; the icon should spin and refresh actions should be disabled until completion. Completion should show the profile-updated notification.
3. Row 8: Open Manage Presets inside a custom provider form. Save a preset named `UI feedback test`, with URL `https://example.com/v1` and synthetic key `ui-test-only-key`. The dialog must show a saved toast plus a green saved status in its fixed footer and include the preset in its list. This checks local persistence and does not require running configuration detection.
4. Row 11: Rename a multi-agent test profile to a long unbroken name, such as `LongProfileName` repeated 50 times. Open its preview. The title should be bounded, all client cards should remain scrollable, and the footer actions should remain visible.
5. Row 12: Find the same long-name profile in Manage Profiles. Its rename, refresh, and delete actions must remain visible. Open deletion and cancel; the confirmation must wrap/scroll the name, retain its buttons, and leave the profile intact.
6. Row 18: Prepare and apply a multi-agent test profile containing Claude Code and Codex. The completion notification should identify the CLI/new-session requirement and the Codex client restart requirement. Existing legacy single-client profiles should show the applicable instruction after switching, too.

Preset-feedback follow-up: the first inline-only status was easy to miss because the entire dialog, including the footer, scrolled. Keep only the form body scrollable, pin the footer, and emit the standard success toast after persistence succeeds. The live development window confirmed the footer is visible without scrolling. Tests assert the success toast and ensure failed persistence emits no success feedback. Existing presets were not edited during inspection.

## Connection preset deletion follow-up

Deleting a connection preset now requires a second confirmation naming the preset. Cancellation preserves both the preset and any unsaved form edits. Deletion disables repeated confirmation and dismissal while persistence is pending. Success removes the preset and shows a success toast plus a green fixed-footer status; failure keeps the preset available for retry and shows a safe error toast without underlying error details. All four supported languages include the new copy.

Regression tests first failed for the missing confirmation and cover cancellation, confirmed deletion, duplicate-click prevention, pending Escape handling, success feedback, and safe failure. Browser verification used only a temporary page with in-memory synthetic presets: the confirmation appeared above the management dialog, cancellation retained the row, and confirmed deletion displayed both the toast and footer status. The temporary page was removed afterward; no real presets were deleted.

Self-test: open Manage Presets and click Delete on a disposable test preset. Check the displayed name, cancel once and verify the row remains, then repeat and confirm. Expect “Preset deleted” at the top and in the footer, and the row to disappear.
