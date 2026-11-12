# Claude Desktop and Codex picker: Magpie parity

For acceptance of the combined submission, see the
[consolidated pre-submit record](2026-11-10-pre-submit-verification.md).
The results below describe this task's intermediate working trees.

## Reference

The requested behavior is Magpie's implementation at
[`7c0a103eb1c4975343456ad4906b21f01fa741f5`](https://github.com/yetone/magpie/tree/7c0a103eb1c4975343456ad4906b21f01fa741f5):

- `internal/provider/codex_models.go`: `CodexListedBeside`.
- `internal/gateway/codex_backend.go`: `codexMerged`.
- `internal/codexcat/codexcat.go`: `Entries` and `FitOwn`.
- `internal/gateway/responses.go`: `responsesTool`.

The prior proposal to keep both official entries and merely separate their
priority ranges has been superseded by this reference behavior.

## Diagnosis

On November 10, 2026, Claude Desktop's model probe and title generation completed
through the local gateway while its conversation retried eleven times. Sanitized
routing history showed upstream HTTP 400 followed by local
`502 routing_upstream_rejected`. Successful probes did not establish acceptance
of the conversation's tool declarations.

Controlled requests used synthetic text and tool definitions. Existing credentials
were read into process memory without printing or persisting them. No original
conversation content or tool inputs were replayed.

| Synthetic request through the existing Desktop connection | Result                                   |
| --------------------------------------------------------- | ---------------------------------------- |
| Greeting without tools                                    | HTTP 200 and `message_stop`              |
| Greeting with Anthropic metadata                          | HTTP 200 and `message_stop`              |
| Greeting with an ordinary function                        | HTTP 200 and `message_stop`              |
| Same function with `defer_loading: true`                  | HTTP 502 and `routing_upstream_rejected` |
| Greeting with adaptive thinking and high effort           | HTTP 200 and `message_stop`              |

A direct official Responses request containing the synthetic deferred function
without native tool search received HTTP 400. Its error referenced both
`defer_loading` and `tool_search`; the raw error body was not logged.
Anthropic's client-side `ToolSearch` function does not supply the Responses
native search contract.

The Codex catalog snapshot contained native model IDs alongside VaryAI's
provider-labeled aliases. Both catalogs started their priorities independently.
The merge deduplicated only identical IDs, leaving duplicate official direct
entries, while overlapping priorities caused the catalogs to interleave.
Magpie instead compares the underlying official model ID against the native
backend's list. Display-name equality does not decide whether to omit an alias.

## Correction and regressions

The shared Responses builder follows `responsesTool`: translated function
definitions omit `defer_loading`. Names, descriptions, schemas, explicit
strictness, tool choice, reasoning settings and discovery history remain.
Anthropic requests sent to Anthropic retain deferred loading. A native Responses
function retains its native deferred-search contract. The fix is not an
official-account-only special case.

Native Codex catalog merging follows `CodexListedBeside`: direct official models
already listed by the backend are omitted from the appended list. Official
models absent from that backend, routing groups and third-party models remain.
The check uses actual execution targets and retained wire-to-canonical mappings,
not labels. Historical mappings remain executable even when omitted from model
discovery. `codexMerged` supplies the appended priorities; `FitOwn` moves hidden
entries last, renumbers the result and retains native models in the first
100 entries read by the desktop app. Capabilities and execution targets remain.
Both corrections use shared Rust code on macOS and Windows.

The isolated Desktop regression uses in-memory storage, dynamic loopback ports,
synthetic credentials and an upstream fixture that rejects deferred functions
without native tool search. It failed before the correction with HTTP 502 instead
of 200. Streaming and non-streaming cases require a complete answer, preserved
function schema and tool choice, and the correct termination marker.

An initial version of this fixture supplied unowned prior tool-call history and
hit the existing HTTP 409 continuation guard. That fixture was corrected to
start a fresh conversation before recording the HTTP 502 regression. A separate
unit test checks converted discovery history directly without bypassing the
continuation guard.

The shared-builder regression failed before the correction because the
translated function retained `defer_loading`; the hidden-order regression failed
because the first row was a hidden native model. Logs are
`magpie-deferred-red.log` and `magpie-picker-red.log`.

Picker regressions cover official direct-model deduplication, missing models
from another account, distinct same-label groups and third-party routes,
historical alias resolution, hidden-entry ordering, consecutive priorities and
the desktop's first-page limit. The old grouping-only regression was replaced.

## Verification context

- Checkout: `/Users/guitar/projects/varyai-open-source`.
- Base revision: `82204c3fca2ed229f40fd5bfda5c4daf33967d4f`.
- Host: macOS 15.3.1 (24D70), Apple Silicon.
- Toolchain: Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3.
  Node differs from the repository's exact version pin.
- Logs and source fingerprints:
  `/private/tmp/vary-desktop-picker-20261110-checks/`.
- Backend checks use cached offline dependencies,
  `/tmp/vary-magpie-parity-target`, disabled incremental compilation and debug
  information, temporary test homes, and explicit loopback proxy exclusions.
- The checkout also contains concurrent native-authentication, conversation
  identity and packaging work. Those changes are preserved.

| Check                                                                                                 | Result                                                                                   |
| ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                                                      | Passed                                                                                   |
| `pnpm build:renderer`                                                                                 | Passed                                                                                   |
| `pnpm test:unit`                                                                                      | 281 files, 3,588 tests passed                                                            |
| `pnpm format:check`                                                                                   | Passed                                                                                   |
| Development, macOS DMG, TRAE bridge, WorkBuddy auth, Doubao check-in, rewards and Qoder helper suites | Passed                                                                                   |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                                 | Passed                                                                                   |
| `pnpm audit --prod`                                                                                   | Passed                                                                                   |
| Focused native gateway, official bridge, Desktop integration and shared IR builder/parser regressions | 56 passed                                                                                |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                              | Passed                                                                                   |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml`                                          | Passed                                                                                   |
| `cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings`                          | Passed                                                                                   |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml --no-fail-fast`                            | Failed: 6,239 library tests passed, 1 failed, 19 ignored; all integration targets passed |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib`                                     | Failed: 6,239 passed, 1 failed, 19 ignored                                               |

The frontend rerun (`magpie-front-results.json`) passed on the 0.3.0 working tree
without changes to its checked inputs. Source and result manifests record changes
observed during each backend check. The focused binary and relevant source
fingerprints are in `magpie-focused-source-and-binary.json`.
All five final backend commands recorded no changes to their checked inputs
while running (`magpie-back-results.json`). Final relevant source hashes still
match the focused test run. The retrieved Magpie checkout resolved to
`0842bc517e87027f1a34c76d722dee16ee1c0225`; the four
reference files are byte-identical to the pinned `7c0a103e` revision
(`magpie-reference.json`).

### Remaining full-suite failures

- The first full run failed
  `proxy::security::upstream_error_tests::upstream_error_passthrough_escape_dense_serialization_is_linear`
  with `synthetic_admission_failed`, before its serialization assertions.
  Its isolated retry passed, and it also passed in the subsequent library run.
  See `magpie-back-0.log` and `magpie-upstream-isolated.log`.
- The subsequent library run failed
  `services::tool_risk::service::command_list_tests::command_lists_preview_uses_only_twenty_latest_sanitized_summaries_and_never_echoes_them`:
  zero matched summaries instead of ten. Its isolated retry also failed.
  The retained earlier 0.2.5 test executable reproduced this failure; it is not
  evidence from a clean base checkout. No command-risk implementation or
  assertion was changed in this work. See `magpie-back-4.log`,
  `magpie-command-preview-isolated.log`, and
  `magpie-command-preview-earlier-binary.log`.

Full-suite acceptance remains incomplete. These failures are retained explicitly;
the focused passes do not replace them. No assertions or checks were disabled.

The running installed VaryAI application was not installed, replaced, restarted
or signalled. The live synthetic probes diagnose its pre-fix behavior; they do
not establish post-fix acceptance of the original Desktop conversation or its
visible Codex picker. No commit, push, remote CI, cross-platform execution,
installer, signing or release acceptance is claimed.
