# Browser bug review — 2026-11-06

## Scope and evidence

Source: the user's existing Chrome Bug Management page. The initial filtered
view contained 18 records. The user continued operating the tracker and asked
the review to use read-only observations. Subsequent filtering/status changes
are therefore not evidence of acceptance. No tracker comments or status updates
were submitted by this review.

The user explicitly excluded the two discussed Chikham records about Codex phone
redaction and identity-card detection. Restoring their original plaintext is not
requested. These records remain outside this review's acceptance claims.

The initial checkout was `c88861d8`; a concurrent merge advanced it to
`5984530cbe725447dc68628431ecef7b4b96b00a`. The only intervening changes are the
already-present provider-notes fixture repair and its verification report. This
review preserves those changes and makes no runtime changes. Renderer sources
are identical across those revisions. Backend verification uses the merged tree.

## Record-by-record findings

Numbers identify the initial visible order, not permanent issue IDs. Existing
implementation and passing source regressions do not establish acceptance of a
reporter's installed 0.2.4 build.

| Row | Report | Current source and acceptance boundary |
| --- | --- | --- |
| 1 | Snapshot tooltip is delayed | Read the record and its three comments. Management focuses its heading, all three action hints use immediate portal tooltips, and opening/hovering does not capture a snapshot. Existing interaction tests cover initial focus, keyboard traversal and consistent hints. |
| 2 | Qwen Responses errors and duplicate models | Read the duplicate-identity comment. Existing root/prefixed Responses route handling and managed-catalog replacement cover these paths. Prior isolated Qwen acceptance is documented in the November 3 review; this review sends no native inference request. |
| 3 | TRAE CN provider switching fails | Read the record; it has a video and no comments. Existing managed-model replacement, cache refresh, rollback and sanitized diagnostics have bridge regressions. The historical generic failure does not establish a remaining cause on the reporter's client. |
| 4 | WorkBuddy creation rejects overlapping models | Existing save-only creation avoids inspecting/changing native models. Lifecycle tests cover overlapping saved alternatives, switching and rejection of unrelated native conflicts. |
| 5 | TRAE CN cannot connect to security auditing | Existing renamed-converter discovery, target-discovery handshake, helper revision selection and inspection regressions cover the previously reproduced causes. Reporter-client reconnection still needs current-build acceptance. |
| 6 | TRAE CN does not show supplier models | Existing native model delivery/replacement tests and the November 3 recorded live observation establish prior recovery, not a new live acceptance in this review. |
| 7 | WorkBuddy installation is not detected | Current macOS discovery recognizes domestic/international bundle identities and renamed bundles. Isolated discovery tests cover both. Read-only local metadata identifies WorkBuddy 5.9.3 with the recognized domestic identity; the reporter's screenshot/comment and other platforms still need comparison. |
| 8 | Qwen, Kimi and TRAE lack provider notes | The shared supplier editor includes notes. Renderer and database regressions cover saving/editing notes and excluding them from executable upstream configuration. The concurrent fixture repair is preserved. |
| 9 | New installation reports a newer database | Current split storage includes released shared-v23 compatibility and independent VaryAI extension migrations. Frozen-schema startup, edits, import/restore and recovery-helper tests cover these paths. No user database is reset or migrated by this review. |
| 10 | Codex phone text becomes `[REDACTED]` | Excluded explicitly by the user. No restoration change. |
| 11 | Identity-card input is not hidden/notified | Excluded explicitly by the user. No rule change. |
| 12 | Redis password URI passes the sandbox | Existing regression covers an empty username with a password, outbound masking, masked findings and a password-free negative. The original correction is documented in the October 22 review. |
| 13 | Saving while tool observation is off advances the version | Identical settings return `not_needed`, create no history, retain the version and produce no update toast. A real policy change still advances the version even if observation is off; disabling observation does not discard configuration edits. |
| 14 | Policy-history refresh has no completion feedback | Current refresh disables duplicate actions while busy and reports success or a sanitized failure. Both outcomes have renderer regressions. |
| 15 | Policy history does not name the changed rule | Current history displays localized built-in names and current custom-rule names beside stable IDs. Deleted rules retain IDs; historical names are not fabricated. |
| 16 | Advanced keyword/regex modes are hard to discover | The Shell list editor now exposes all five match scopes directly. Existing tests cover direct selection, validation and scope confirmation. |
| 17 | Risk labels cannot be related to a Shell command | Event rows and details show backend-processed command summaries and matched rule IDs. Tests prohibit falling back to raw inputs. Old records without summaries explicitly remain unavailable. Exact comparison with the original attachments is still needed before declaring reporter acceptance. |
| 18 | Data-exfiltration warnings have no system notification | Current notifier has one delivery path for all in-scope risky categories, with policy/permission checks, deduplication and bounded queues. Permission-denial and delivery-failure tests preserve in-app feedback. No OS permission is changed here; the original machine's permission and event metadata remain unverified. |

No new failing reproduction was established by this review. Existing fixes were
retained instead of adding speculative changes based only on old issue titles.
Rows requiring reporter evidence remain open for acceptance.

## Verification

Host: macOS 26.6.2 arm64. Final renderer verification uses Node 22.22.0 and pnpm
10.12.3; Rust/Cargo 1.98.1. An initial Node 20 run also passed, but the final
Node 22 run is the renderer acceptance result.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | 279 files, 3,479 tests passed on Node 22 |
| `pnpm test:dev` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,055 passed, 18 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,218 passed, 18 ignored across the full run |
| `git diff --check` | Passed |

Logs use the `/tmp/varyai-bug-review-20261106` prefix; the final Node 22 helper
results and logs are in the directory with that name. Rust linking emitted the
existing nonfatal compact-unwind size warning. No installed application
was launched, replaced or restarted; no native provider or security preference
was changed. No commit or push is part of this review. Remote Linux, Windows,
macOS and WSL2 CI, packaging and reporter-client acceptance are not established
by these local checks.

## Related evidence

- [November 3 pending bug review](2026-11-03-pending-bug-review.md)
- [October 22 regression record](2026-10-22-baomi-bugs.md)
- [October 29 pending bug retest](2026-10-29-pending-bug-retest.md)
- [Provider-notes fixture verification](2026-11-06-pr44-provider-notes-fixture.md)
- [Tool observation behavior](../architecture/tool-risk.md)
