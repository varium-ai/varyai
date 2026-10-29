# Pending bug review — 2026-10-29

The source is the user's open Chrome Bug Management view. Row numbers below refer to the 15 pending records in the Baomi group and one jointly assigned record read during this review, not a permanent issue ID. Tracker statuses and comments were not changed. Fixes await user retesting; automated and local acceptance are distinguished below.

## Client replacement scope

| Client | Result |
| --- | --- |
| Kimi, Qwen, ZCode | Explicit supplier switches and applied-provider edits replace VaryAI-managed model groups, including recognizable groups missing from the current database. File validation and routing restoration are staged before writing. Official selection withdraws managed models and preserves native configuration. |
| Qoder | Supplier switching replaces reserved VaryAI custom-provider groups; official selection removes them. Unrelated providers, plugins and native credentials remain intact. |
| WorkBuddy | Supplier switching replaces managed models, with a separate private ownership file surviving database deletion. Profile undo includes that file. Unmarked historical native models still require import/conflict resolution. |
| TRAE CN | Native replacement removes owned groups, applies the complete target catalog and attempts restoration after failure. Recovery data is required before destructive changes. Older helper revisions are refreshed automatically before model replacement. |
| Claude Desktop, Grok Build | Existing writers already replace complete generated model snapshots; no additional catalog rewrite was needed. |
| Doubao Work | Official native login only; the integration exposes no custom supplier catalog. |
| OpenCode, OpenClaw, Hermes, Pi | Multiple-provider mode retained, as explicitly confirmed by the user. |

## Pending-view findings

| Row | Report | Findings and acceptance status |
| --- | --- | --- |
| 1 | Codex GLM requests include a Qwen usage record | Unresolved origin of an additional client request. Concurrent-request attribution regression passes without relabeling legitimate requests. |
| 2 | Codex catalog fetch fails | Not reproduced with the three saved presets: 7, 7 and 4 models returned. The recording omits the endpoint and detailed response. No speculative protocol-detection change was introduced. |
| 3 | Copying a gateway key has no success feedback | Fixed. Success toast follows clipboard completion; rejection follows the error path. Creation also no longer automatically reveals the secret. |
| 4 | Qoder routing/certificate rebuild/clear fail | A real cleanup failure was reproduced and fixed: macOS returned a generic deletion failure for an already absent certificate. Confirmed absence is now idempotent success. Local routing and supplier selection also succeeded. Other tester-machine certificate failures remain unconfirmed; approved authorization is not treated as proof that later operations succeeded. Fixed-stage diagnostics identify subsequent failures without exposing raw output. |
| 5 | Kimi switching fails and withdraws routing | Reproduced after deleting shared storage and recreating a supplier under a new ID: a native same-name group survived the deletion. Replacement now succeeds; seven target models replace fourteen prior managed aliases, with native login preserved. Rejected configuration validation leaves the prior file and routing recovery backup intact. |
| 6 | TRAE CN remains disconnected from auditing | Recovery from an existing random debug port without launch-backup metadata is fixed, including numeric/string zero. Old resident helpers are refreshed automatically. Local 3.4.11 reconnect and a synthetic phone-redaction audit event passed; tester 3.4.9 acceptance remains pending. |
| 7 | TRAE CN configured models do not appear | Local native API and client dropdown verification passed: one custom model, seven custom models, official zero custom models, then seven restored. All labels used Model name · Provider name. GLM-5.3 also completed a synthetic request. Real 3.4.9 delivery still needs tester retesting. |
| 8 | Qoder models survive database deletion/reinstall | Native storage is independent. Explicit switching now replaces recognizable VaryAI-owned groups. No startup-wide deletion of client settings was introduced. |
| 9 | Kimi models survive database deletion/reinstall | Same storage distinction. Explicit switching now clears recognizable stale managed aliases and rebuilds the target catalog. |
| 10 | Manual tool-risk preview works with observation disabled | Manual preview is intentionally independent of live observation and executes no tools or writes live observations. The UI now explains the distinction. |
| 11 | Qwen CLI shows models outside the selected supplier | Full replacement now removes recognizable VaryAI-managed aliases from prior suppliers. Recording review and installed CLI retesting remain pending. |
| 12 | Adding a WorkBuddy supplier immediately activates it | Fixed the creation mutation to save inactive alternatives, matching Qoder and other replacement clients. Regression reproduced the omitted false flag and now passes. |
| 13 | OpenAI write_file sandbox does not match a rule | The screenshot uses file_path, but the registered OpenAI adapter requires path. Added an explicit field example and explanation that unchecked does not establish safety; the adapter trust boundary is unchanged. |
| 14 | Codex official token refresh fails | Skipped at the user's request. No ChatGPT credentials were replaced during this verification. |
| 15 | Qwen switching reports a duplicate protocol/endpoint/model | Newly read report; the managed-catalog replacement fixes have automated coverage, but the new recording still requires review. |
| Joint row | Sandbox accepts malformed JSON | Fixed. JSON-looking malformed input now produces a visible validation error without invoking the sandbox. Free-form patch text is preserved. |

## Upgrade/delete/reopen reproduction

CC Switch was updated from 4.0.10 to official 4.0.21 after signature and installer SHA-256 verification. Its real startup migrated shared storage from schema 20 to 23. Per the user's instruction, the database-compatibility implementation was withdrawn; that solution is being handled by another colleague on main. No schema-version change remains in this branch.

The upgraded database was retained, and only the active shared database was moved out to reproduce the tester's reset. The independent extension database and native client configuration were preserved. Saved supplier configurations were recreated under new IDs in the disposable shared database. Kimi then reproduced the invalid-configuration error; the repaired backend subsequently switched successfully and replaced the stale aliases. Qoder routing and supplier selection succeeded locally. Clearing the newly created test certificate reproduced the keychain deletion error; the repaired backend cleared its metadata/files successfully after confirming the certificate was absent.

Original user data was restored after verification: 35 saved providers, original settings, and native Kimi/Qoder/TRAE launch files. Extension table contents were unchanged. Codex authentication and configuration files matched their pre-retest backups. Recoverable application/database/test artifacts are retained locally under `~/.cc-switch/backups/compat-upgrade-20261029-152642/`.

## Verification

- Renderer typecheck, renderer build and formatting passed; 2,585 unit tests across 209 files passed.
- TRAE Node tests: 33 passed, including replacement of orphaned groups, preservation of native models, rollback and missing recovery data.
- Full Rust suite: 4,103 passed and 15 ignored, including 3,940 passing library tests. Follow-up focused checks cover old-helper capability preflight.
- Profile bundle integration tests: 32 passed, including native model selection during takeover and TRAE offline delivery.
- WorkBuddy focused suite: 46 passed before the additional ownership rollback regression was included in the full run.
- Follow-up renderer checks, formatting, Clippy and development build passed and are recorded in `/tmp/varyai-final-followup-*.log`; full Rust results are in `/tmp/varyai-trae-refresh-full-rust.log`.

The protocol matrix exposed a repeatable converted-stream failure: an HTTP reset could discard already buffered frames. Converted Anthropic streams now emit a sanitized protocol error on that failure path; native passthrough and successful content are unchanged. The sixteen-edge matrix passed in the full run. No provider capability was disabled based on theoretical protocol assumptions.

Installed-client inference, signing/installer acceptance, and tester-machine Qoder/TRAE behavior are not established by unit tests. No commit, push, tracker comment or status update was performed.

## Native TRAE follow-up

The first isolated test directory exceeded the macOS Unix socket path limit. Shortening that disposable directory allowed the helper to start. This was a local test-environment failure and does not establish the tester-machine root cause. The upgraded helper reported revision 2, model replacement available, audit enabled and IDE/agent scope. Native switching restored the original seven-model supplier after testing. The isolated data copy was retained, and the development app was then relaunched with the original 35-provider database at the user’s request.

The restored original database also reconnected successfully: current helper revision 2, native model replacement available, audit enabled. A disabled legacy helper did not supersede the current connection.
