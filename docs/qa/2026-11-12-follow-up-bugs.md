# Follow-up bug verification

Base revision: `9804cdb78f509a7b47ce2db90da1ce18adc496c8`. The user requested local fixes only; this batch has not been committed or pushed. Browser records were read without refreshing the user's bug-list tab. The table's status fields were not edited.

The automatic-discovery controls described in this historical checkpoint were subsequently retired at the user's request. See the [latest requirement update](2026-11-13-agent-restoration.md#latest-requirement-update). Other fixes remain in place.

## Fix scope

| Original issue numbers | Local behavior and coverage                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2, 3, 7                | Restore the saved automatic-discovery switch beside a non-clickable heading. Announce the setting write and catalog refresh immediately, prevent duplicate operations, and recover after sanitized failures.                                                                                                                                                                                                                                                                                                               |
| 10                     | Restore JSON file export alongside Copy to share. Report download initiation or a sanitized failure, without claiming browser download completion.                                                                                                                                                                                                                                                                                                                                                                         |
| 22, 26, 28, 29         | Authenticated shared Codex Responses and native Chat routes reuse actual-send token preparation and bound response restoration. Synthetic phone and checksum-valid identity fixtures cover JSON/SSE, sensitive requests and safe follow-ups carrying sensitive history. Unsupported conversions and explicit Block decisions still refuse sending. Classifier projections use bounded detector spans, the existing overlap planner and guarded JSON rebuilding, with a capacity-denial and allocation-lifetime regression. |
| 23                     | Suppress repeated historical audit notifications without exempting historical text from inspection, redaction or blocking. Cover read-only/mutable inspection and refusal summaries.                                                                                                                                                                                                                                                                                                                                       |
| 24                     | Use a local 150 ms tooltip for the tool-rule reset button, including the first hover, without resetting a rule on hover.                                                                                                                                                                                                                                                                                                                                                                                                   |
| 25                     | Capture, preview, apply and undo an owned Codex shared selection, including recognized legacy aliases. Preserve connection tokens and ownership metadata; skip unknown saved aliases.                                                                                                                                                                                                                                                                                                                                      |
| 27                     | Verify the existing WorkBuddy and WorkBuddy AI behavior: reselecting the checked model closes the picker without writing configuration or showing a new restart requirement.                                                                                                                                                                                                                                                                                                                                               |
| 30                     | Show an application-named completion notification after configuring a model. Desktop clients receive restart guidance; CLI clients receive new-session guidance. No client is restarted automatically.                                                                                                                                                                                                                                                                                                                     |
| 31                     | Verify the existing selection fix against unrelated route removal and republication for ten file-based Agent adapters. Assert both native model fields and visible canonical selection, with temporary client files.                                                                                                                                                                                                                                                                                                       |
| 32                     | The screenshot repeats the unsupported-restoration-route failure. Known Codex/WorkBuddy paths are covered above; the screenshot's Agent identity was requested separately.                                                                                                                                                                                                                                                                                                                                                 |
| 33                     | Add a direct single-Agent selector to profile creation; preserve the current-Agent default and existing multi-Agent capture.                                                                                                                                                                                                                                                                                                                                                                                               |
| 34                     | Reduce the shared switch's checked track intensity in the dark theme and preserve thumb visibility. Inspect actual rendered markup with the production CSS in a separate local preview.                                                                                                                                                                                                                                                                                                                                    |

Issues 5 and 6 remain deferred at the user's prior request. Earlier fixes for the other original issues remain in the base revision; no installer or running client was replaced for this batch.

## Regression and CI findings

The first full library run exposed a source-shape assertion that depended on a transformation function having private visibility. The function is now reused by the shared restoration finalizer. Update the assertion's delimiter to tolerate visibility while retaining its bounded, borrowed-tool-schema assertions.

The pushed base revision's CI run [43196570482](https://github.com/varium-ai/varyai/actions/runs/43196570482) passed frontend, Linux, Windows and Windows-to-WSL2 jobs. The macOS library test failed when rebinding the isolated Qoder TLS port after shutdown; consequently CI Required failed. Inspecting shutdown exposed an independently reproducible defect: timing out a moved JoinHandle detached the listener task. A focused test reproduces its retained port. Shutdown now aborts and joins a timed-out task and drops the listening socket before draining connections. This preserves the original certificate, trust, hostname and port-release assertions. A new local fix does not change the failed remote result.

## Verification record

Host: macOS 26.6.2 (25G83), Apple Silicon. Node 20.20.2, pnpm 10.12.3, Rust/Cargo 1.98.1. Rust commands use `CARGO_INCREMENTAL=0`.

The final renderer run passed 4,059 tests in 304 files. Type checking, renderer build, formatting, production dependency audit, and all eight development/client helper suites passed. Focused tests first reproduced the missing discovery/export/profile controls and completion notification, then passed after the changes. The native selection republication regression passed on the existing fix. A local browser preview confirmed light/dark checked and unchecked switch colors using the production stylesheet.

Final Rust formatting, compilation and Clippy checks passed. The complete library suite passed 7,473 tests with 21 existing ignored tests. The complete Cargo test run passed 7,638 tests with the same 21 ignored tests across 16 binary/doc-test groups. The original certificate/TLS assertion and the new shutdown, classifier-budget, shared-restoration, profile and model-republication regressions passed in the final tree. Local macOS checks do not establish Windows, Linux, installer, signing, notarization, live-account or release acceptance. Synthetic upstream responses do not establish compatibility with every provider or unsupported protocol conversion.

## Commands for this batch

| Command                                               | Final result                   |
| ----------------------------------------------------- | ------------------------------ |
| `pnpm typecheck`                                      | Passed                         |
| `pnpm build:renderer`                                 | Passed                         |
| `pnpm format:check`                                   | Passed                         |
| `pnpm audit --prod`                                   | Passed                         |
| `pnpm test:unit`                                      | Passed: 304 files, 4,059 tests |
| `pnpm test:dev`                                       | Passed                         |
| `pnpm test:macos-dmg`                                 | Passed                         |
| `pnpm test:trae-bridge`                               | Passed                         |
| `pnpm test:model-default`                             | Passed                         |
| `pnpm test:workbuddy-checkin-auth`                    | Passed                         |
| `pnpm test:doubao-checkin`                            | Passed                         |
| `pnpm test:agent-rewards`                             | Passed                         |
| `pnpm test:qoder-codec`                               | Passed                         |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed: 5 tests                |

The changed source/test/documentation snapshot contains 42 files, excluding the QA report and manifest. Its sorted-JSON SHA-256 is `dd1681f1d322b1378b556a6a1a3e76a57651cd7d3df4aa2fec4472e007661a07`; see [the source manifest](2026-11-12-follow-up-bugs-sources.json).

An intermediate bounded-classifier regression caught unsanitized Chat content when the sanitizer received native Chat fields. Classification now canonicalizes supported request fields first, applies the bounded redaction planner to that canonical copy, and preserves the original request for actual-send preparation. Passing a raw native Chat/Gemini envelope to that private classifier sanitizer is refused. Failed or superseded checks are not final acceptance.

| Command                                                            | Final result                             |
| ------------------------------------------------------------------ | ---------------------------------------- |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                   |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                   |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                   |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 7,473 tests; 21 existing ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 7,638 tests; 21 existing ignored |

Final source hashes matched the 42-file snapshot. Staged changes were empty; the working tree contains intended source, tests and documentation only. No commit or push was performed for this batch. The installed/running application was not replaced, and live client acceptance remains separate. The final visible pending filter still contained 18 records; no additional report appeared during the closing review.
