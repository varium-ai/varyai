# November 16 updated tracker follow-up

This is the historical 24-row snapshot. The [latest browser read shows 23 reports](2026-11-16-current-23-bugs.md); supplier-name preview is now row 18. The previous Codex quit-without-reopen report left the visible view without an inferred acceptance result.

The user requested another read of the existing browser tab and continued repairs, without committing or pushing. The tab was read without reloading or changing filters. The view initially showed 25 rows and then 24 as collaborators updated it. The table below uses the final observed 24-row snapshot; earlier 29-row and 32-row numbers are historical. No tracker status was changed by this agent. Reports leaving this view are not assumed to have passed acceptance.

Three visible reports have a tracker status of **acceptance failed**: Gemini model discovery, the first routing template's Add action, and configured supplier icons in the request graph. The remaining 21 visible reports are pending. The user requested deferring these three investigations and explicitly chose to retain template disabling with a clear explanation of the unmet requirements. No further build information is required for this deferred work.

## Attachment findings

- The first template's comment says that Add is now present but cannot be clicked. Current code keeps the action visible and disables it when the available catalog lacks a cheap model or an eligible classifier. Per the user's decision, this restriction is retained. The disabled action now exposes its existing localized requirement as its hover title and accessible description, in addition to the visible card explanation. A regression first failed because the button had no accessible explanation, then passed after the change. No unsupported model is fabricated and no incomplete draft is opened.
- The supplier-icon screenshots compare the configured generic box with an incorrectly displayed Gemini icon for the same supplier. The current request stage resolves the configured upstream by public supplier identity, including renamed suppliers. Existing renderer regressions pass, but that does not establish acceptance of the failed build.
- The Gemini model-discovery report still says that it cannot load the catalog. Current code already handles native catalog pagination and same-origin OpenAI catalog fallback after missing native endpoints. No production endpoint probe or credential retrieval was performed. The reported endpoint remains unresolved without a reproduced failure.
- The missing supplier names in scheme previews are model-source labels, not merely empty strings: the screenshot shows Vary AI plus model names without the actual supplier that distinguishes otherwise identical models. This batch adds a focused regression for that information across Codex, Claude, Gemini and Pi shared-connection previews.
- The account-login comments narrow the issue to onboarding: normal provider creation can sign in, while the first-run flow needs another pass. This evidence does not prove a general account failure.

## Native Codex restart notice

The ordinary native supplier-switch path wrote the Codex configuration without recording desktop freshness. Shared routing updates already recorded it, so a normal supplier switch could leave no applied timestamp for the restart decision. A new regression first failed on the absent freshness record. After a successful native switch and MCP projection, the service now records the final configuration fingerprint through the existing platform-independent manager. Failure of this auxiliary record remains non-fatal to an already completed switch and logs a fixed message. Switching to a missing supplier does not create or replace the successful record. The regression checks two distinct synthetic endpoints, the exact final configuration digest and unchanged metadata after refusal. It does not launch, inspect or restart the installed Codex process.

The first broad Rust run also found an existing shared-profile roundtrip assertion that expected a model-only label. Its expected strings now include the synthetic supplier name; selector, credential-exclusion, ownership, external-edit and Undo assertions remain in place.

## Snapshot mapping

Source repair status is separate from the tracker field. No new installed-client acceptance has been performed.

| Row | Report                                                       | Tracker           | Source status                                                                                        |
| --- | ------------------------------------------------------------ | ----------------- | ---------------------------------------------------------------------------------------------------- |
| 1   | Gemini-compatible Base URL cannot load models                | Acceptance failed | Existing native/fallback handling; reported endpoint unresolved                                      |
| 2   | Account login does not discover models or sign in            | Pending           | Unfixed reported onboarding flow                                                                     |
| 3   | First routing template has no usable Add action              | Acceptance failed | Retain disabling per user; localized requirement now explains the button                             |
| 4   | Request-graph supplier icon differs from configuration       | Acceptance failed | Existing configured-icon resolution; failed build needs comparison                                   |
| 5   | Codex reports Block for Hide and restore locally             | Pending           | Partial supported-path fixes; live retest required                                                   |
| 6   | Codex repeats a block after ordinary follow-up               | Pending           | Existing history/restoration fix; live retest required                                               |
| 7   | WorkBuddy restoration receives HTTP 400                      | Pending           | Partial supported-path fixes; live retest required                                                   |
| 8   | WorkBuddy ordinary follow-up still receives HTTP 400         | Pending           | Existing history/restoration fix; live retest required                                               |
| 9   | Hide and restore locally stops the request                   | Pending           | Partial; unsupported paths remain guarded                                                            |
| 10  | Signed-in Qoder CN account cannot refresh                    | Pending           | Unfixed reported case                                                                                |
| 11  | WorkBuddy old chat keeps the previous provider after restart | Pending           | Unfixed reported case                                                                                |
| 12  | Gemini Base URL protocol detection fails                     | Pending           | Unfixed reported endpoint                                                                            |
| 13  | Agent restart needs two confirmations                        | Pending           | Earlier local repair; native retest required                                                         |
| 14  | Qoder CN supplier names do not match                         | Pending           | Unfixed reported case                                                                                |
| 15  | Cannot switch scheme                                         | Pending           | Earlier partial repair for routed Codex to native selection, rollback and Undo                       |
| 16  | Codex does not request restart after supplier change         | Pending           | Native-supplier switch now records desktop configuration freshness; installed-client retest required |
| 17  | Restart confirmation window keeps jumping                    | Pending           | Earlier local repair; native retest required                                                         |
| 18  | Codex quits without reopening                                | Pending           | Earlier local repair; native retest required                                                         |
| 19  | Scheme preview omits supplier names                          | Pending           | Shared-model preview labels repaired; installed-client retest remains outstanding                    |
| 20  | Native dialog is English while system and app are Chinese    | Pending           | Earlier macOS bundle metadata repair; packaged app retest required                                   |
| 21  | Cannot change upstream format                                | Pending           | Earlier same-endpoint/auth repair; live retest required                                              |
| 22  | Equivalent commands prompt inconsistently across agents      | Pending           | Earlier partial tool-attribution repair; remaining invocation paths unverified                       |
| 23  | Codex permission-change command has no prompt or event       | Pending           | Original live command remains unresolved; synthetic mode-4755 detection passes                       |
| 24  | Custom-rule security events omit session ID                  | Pending           | Earlier new-event session propagation repair; historical/missing IDs cannot be reconstructed         |

## Client acceptance steps

1. For the supplier-label report, configure two suppliers with the same model display name. Save a shared-connection scheme using the first, then select the second. Preview the saved scheme and check that the current and target model labels identify their respective suppliers. A routing-group selection should display the group name instead of claiming one fixed supplier.
2. For the Codex restart-notice report, keep an already running Codex desktop instance and use the ordinary native supplier path with shared routing disabled. Switch to another configured supplier, then check the Agent page for the restart requirement. After a user-performed restart, verify that the state becomes current. This is still an outstanding native acceptance step; automated tests do not restart the installed client.
3. For the template clarification, leave a required provider, cheaper model or eligible classifier unavailable. Add remains disabled; the card and button explanation identify the missing requirement. No routing group is created.

## Verification

Base revision: `3f1daf658fc92a492a03b3c9ec0becc009773e3f` plus this uncommitted tree. All earlier repairs and unrelated work are preserved. Verification uses synthetic fixtures, temporary client homes, in-memory application databases and a private build target. Installed clients are not launched, restarted or replaced. Host: macOS 26.6.2 arm64; Rust 1.98.1; Node 20.20.2; pnpm 10.12.3. The final source/config/test manifest is [recorded separately](2026-11-16-list-follow-up-sources.json). Logs use `/tmp/varyai-list-followup-*`. All 47 source/config/test fingerprints were verified unchanged after the final checks. The closing browser read still showed the same 24 rows and tracker states.

| Final check                                                        | Result                                                                              |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| Routing renderer regressions                                       | 66 passed across four files                                                         |
| Scheme regressions                                                 | 17 passed                                                                           |
| Ordinary native Codex switch regression                            | 1 passed                                                                            |
| Shared Codex profile/legacy roundtrip                              | 1 passed                                                                            |
| `pnpm typecheck`                                                   | Passed                                                                              |
| `pnpm build:renderer`                                              | Passed                                                                              |
| `pnpm format:check`                                                | Passed                                                                              |
| `pnpm test:unit`                                                   | 4,091 passed across 307 files                                                       |
| `pnpm audit --prod`                                                | Passed; no known vulnerabilities reported                                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                                              |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                                              |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                                              |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 7,682 passed, 21 ignored across 16 groups; library subset: 7,517 passed, 21 ignored |

The final frontend run followed the template explanation change. The final backend run followed the native-switch repair and all updated preview-label assertions. Final machine-readable results are `/tmp/varyai-list-followup-final-frontend-results.json` and `/tmp/varyai-list-followup-final-backend-results.json`. Earlier red runs are retained separately. A test-fixture timestamp assertion was corrected to the existing camelCase `appliedAtMs` wire field; the original missing-file regression preceded the production fix. The macOS linker emitted a non-fatal compact-unwind size warning. Existing ignored tests remain ignored; no checks or assertions were disabled.

These are local macOS results. Linux, Windows, Windows-to-WSL2 CI, packaging and installed-client acceptance remain unverified. The private build cache is disposable; the repository's original target and the running applications are preserved. No commit, push, staging or tracker-status write was performed.
