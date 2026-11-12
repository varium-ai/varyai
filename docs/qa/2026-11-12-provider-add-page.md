# Provider add page and import entry

The add page follows the supplied reference with a title and back button, a two-column grid of four entry cards, and a bordered popular-provider table. The cards open subscription selection, official vendor selection, provider import and the custom editor. The API relay entry and table group are omitted. Import moved from the provider-list header into the add page and retains the existing import dialog, persistence and error handling. Closing or completing import returns focus to its card. Choosing import or an API editor cancels inline subscription authorization.

Supported subscription and official API presets remain available with search, existing-account markers and inline authorization. The default table contains four supported subscription shortcuts and four official vendors. Group cards open the full corresponding directory in a centered modal with its own search. Closing restores focus to the card and preserves the page search and popular shortcuts. The layout stacks below 680 px, uses the existing light/dark theme tokens and includes English, Simplified Chinese, Japanese and Traditional Chinese copy. All logic is shared by macOS and Windows.

## Subscription and vendor picker dialogs

The subscription and vendor entry cards now open centered selection dialogs instead of replacing the add page. Both dialogs keep independent search, display the full supported directory and restore focus to their entry card on close. Escape clears a nonempty search before dismissing. The underlying page retains its search and four shortcuts per category. The shared React implementation applies to macOS and Windows.

Subscription authorization runs inside its picker; closing immediately unmounts and cancels authorization and ignores late completion. Completed sign-in opens the existing provider editor. Choosing a vendor closes the picker and opens its configuration editor. Deferred picker exit animation must not steal focus from that editor.

The updated tests failed against the previous directory-page implementation: 13 failed, 21 passed. The final regression selection passed all 50 tests across provider selection, subscription authorization, provider import and provider parity. Coverage includes page-search preservation, close-button and Escape behavior, restored trigger focus, endpoint/alias search, native sign-in completion, cancellation, ignored late completion and focus after the picker exit animation.

| Check                                       | Picker result                                  |
| ------------------------------------------- | ---------------------------------------------- |
| `pnpm typecheck`                            | Passed                                         |
| `pnpm build:renderer`                       | Passed, rerun after the final style correction |
| `pnpm format:check`                         | Passed, rerun after the final style correction |
| Prettier check for both modified test files | Passed                                         |
| Final four focused suites                   | 4 files and 50 tests passed                    |
| `pnpm test:unit`                            | All 299 files and 3,934 tests passed           |
| `git diff --check`                          | Passed                                         |

The isolated browser harness verified both light dialogs at the default desktop viewport, vendor search and two-step Escape dismissal, restored card focus, and both dialogs in all four runtime languages at 420 px in dark mode. All eight dark-language/dialog combinations fit within the viewport without horizontal overflow. The underlying page retained eight popular shortcuts. The picker is capped at 560 px with a scrolling body and fixed header/search. Explicit auto margins preserve centering even when workspace styles load later.

English previews: `/Users/guitar/.codex/visualizations/2026/11/12/01a1cdea-3782-7243-b4c7-9abdffdb0954/vendor-picker-preview.jpg` and `/Users/guitar/.codex/visualizations/2026/11/12/01a1cdea-3782-7243-b4c7-9abdffdb0954/subscription-picker-preview.jpg`.

Implementation: `ProviderPickerDialog.tsx`, `ProviderAddSheet.tsx` and scoped provider CSS. Regression files: `ProviderAddSheet.test.tsx` and `SubscriptionAddFlow.test.tsx`. Checked-file hashes and toolchain: `/tmp/vary-provider-picker-verification.json`; command logs: `/tmp/vary-provider-picker-*.log`. Existing unrelated working-tree changes were preserved. Native APIs were mocked in tests; the browser harness did not start sign-in or access client configuration. No installed application was launched or installed, and no commit or push was made. Backend, helper-script, dependency and remote CI checks were not repeated for this renderer-only follow-up; the earlier two Rust failures remain recorded below.

## Compact follow-up

The initial layout exposed every preset and used oversized entry cards. The follow-up reduces cards from 116 px to 84 px minimum height, uses smaller icons, text and spacing, and fixes each default provider row to four columns. Subscription shortcuts are Claude, ChatGPT, Gemini CLI and Antigravity; official vendor shortcuts are OpenAI, Anthropic, Google and DeepSeek. Other supported choices remain available through the corresponding entry or search. On very narrow windows the four shortcuts wrap into two columns.

Three revised regression expectations failed against the previous implementation before this follow-up was implemented. The selected suites passed 35 tests afterward, including four-shortcut limits, full-directory access, Back/Escape navigation, preset editors and subscription authorization. The final related suites were rerun after the presentation adjustment.

| Check                                                  | Follow-up result                                                  |
| ------------------------------------------------------ | ----------------------------------------------------------------- |
| `pnpm typecheck`                                       | Passed                                                            |
| `pnpm build:renderer`                                  | Passed                                                            |
| `pnpm format:check`                                    | Passed                                                            |
| Prettier check for both modified test files            | Passed                                                            |
| Final focused provider, subscription and parity suites | 3 files and 35 tests passed                                       |
| `pnpm test:unit`                                       | All 299 files and 3,925 tests passed; final affected suites rerun |
| `git diff --check`                                     | Passed                                                            |

Browser review covered the desktop layout, a 760 px content window in all four runtime languages, and a 420 px dark window. All locale checks showed exactly four shortcuts per group without horizontal overflow. At 760 px, the bottom of the default table remained within 600 px of the viewport top for every locale. The desktop page body is about 485 px high. Full vendor-directory navigation and return to the four-shortcut table were also verified in the browser.

Current English preview: `/Users/guitar/.codex/visualizations/2026/11/12/01a1cdea-3782-7243-b4c7-9abdffdb0954/provider-add-compact-preview.jpg`.

Follow-up hashes: `/tmp/vary-provider-add-compact-verification.json`. Logs: `/tmp/vary-provider-add-compact-*.log`. Backend, helper-script and dependency checks were not repeated for this renderer-only follow-up; their earlier results and two known Rust failures are recorded below. No native application was launched or installed. No commit or push was made.

## Initial layout verification

- Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`, plus the current working tree. Existing and concurrently updated unrelated changes were preserved.
- Host: macOS 15.3.1, Apple Silicon; Node 24.11.0, pnpm 10.12.3, rustc/cargo 1.98.1.
- Implementation: `ProviderAddSheet.tsx`, the import callback and header in `RoutingUpstreamsPanel.tsx`, scoped provider styles and four locale files.
- Tests: `ProviderAddSheet.test.tsx` and the header-action expectation in `RoutingUpstreamsPanel.test.tsx`.
- Changed-file SHA-256 hashes and environment: `/tmp/vary-provider-add-verification.json`.
- Command logs: `/tmp/vary-provider-add-*.log`.

Tests were updated before implementation: six new or revised expectations failed against the old page while 68 existing checks passed. The four-file provider selection, subscription, import and list regression run then passed all 99 tests. After simplifying group focus handling, the two affected suites passed all 28 tests again.

| Check                                                                                               | Result                                                                       |
| --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                                    | Passed, including final rerun                                                |
| `pnpm build:renderer`                                                                               | Passed, including final rerun                                                |
| `pnpm format:check`                                                                                 | Passed, including final rerun                                                |
| Prettier check for the two modified test files                                                      | Passed                                                                       |
| `pnpm test:unit`                                                                                    | All 299 files and 3,918 tests passed; final affected suites rerun separately |
| `pnpm test:dev`, `test:macos-dmg`, `test:trae-bridge`, `test:model-default`                         | Passed                                                                       |
| `pnpm test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`, `test:qoder-codec` | Passed                                                                       |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                               | Passed                                                                       |
| `pnpm audit --prod`                                                                                 | Passed, no known vulnerabilities                                             |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                  | Passed                                                                       |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                            | Passed                                                                       |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                  | Passed                                                                       |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`                                             | 6,337 passed, 2 failed, 19 ignored                                           |
| `cargo test --manifest-path src-tauri/Cargo.toml`                                                   | 6,337 passed, 2 failed, 19 ignored; stops at library failures                |
| `cargo test --manifest-path src-tauri/Cargo.toml --test '*' --no-fail-fast`                         | All 14 integration targets and 164 tests passed                              |
| `git diff --check`                                                                                  | Passed                                                                       |

The two Rust library failures concern model labels in the pre-existing routing-menu changes:

- `commands::agent_connections::tests::reasoning_catalog_projects_into_native_agent_files_and_restores_them` expects `synthetic · synthetic=effort,xhigh_effort,max_effort`, but receives `synthetic=effort,xhigh_effort,max_effort`.
- `services::provider_import::tests::provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs` searches for `Custom · Synthetic` and reports a missing imported model.

Independent provider-dialog CSS edits landed during verification. The add-page rules remained unchanged; renderer build and formatting were rerun successfully against the final stylesheet.

Neither failure executes the React page or its import-entry relocation. These failures were also documented in the earlier provider-copy verification report; they remain a verification gap and were not changed as part of this UI task.

## Visual review

An isolated Vite harness rendered the actual component and application styles with synthetic callbacks. The installed application and native configurations were not opened or changed. Browser inspection covered the default desktop viewport, a 420 px narrow window, dark mode, and all four runtime languages. Each locale showed four methods and two provider groups without horizontal page overflow. The native import and authorization behaviors were verified by the component tests, not by the visual harness.

English preview: `/Users/guitar/.codex/visualizations/2026/11/12/01a1cdea-3782-7243-b4c7-9abdffdb0954/provider-add-preview.jpg`.

Linux, Windows, WSL2 and remote CI were not run. No commit or push was made. Full verification remains incomplete while the Rust suite failures remain.
