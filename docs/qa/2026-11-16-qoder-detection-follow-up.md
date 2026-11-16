# Qoder labels and protocol detection follow-up

This follows the [23-row tracker snapshot](2026-11-16-current-23-bugs.md).
The existing browser tab and attachments for rows 10, 12, 14 and 22 were read
without reloading or editing tracker fields. Rows 1, 3 and 4 remain deferred at
the user's request. No commit, push, client restart or installation was performed.

## Row 14: Qoder CN supplier names do not match

The shared projection reused stable historical wire aliases as display labels.
Consequently, renaming a model could leave its old name visible, and duplicate
model names did not identify their configured supplier. Qoder now receives the
current display name with the supplier suffix; routing group names remain intact.
The request aliases, selected model and alias-to-route map remain unchanged.

A synthetic regression failed before the fix because a direct model lacked its
supplier suffix. It now checks two suppliers with identical model names, a
routing group, and refreshed names with unchanged aliases. All 98 active Agent
connection tests passed; one opt-in test was ignored. The installed Qoder menu
and original recording still require acceptance testing.

Manual retest: use two suppliers with the same model name, refresh the shared
connection, and reload Qoder. Its menu should distinguish both suppliers. Rename
one model or supplier, refresh and reload again, and verify the new label. Existing
selections must still resolve to the same route.

## Row 12: Gemini Base URL protocol detection fails

The attachment shows a populated model catalog with no selected models. The
editor nevertheless fetched the catalog again before probing. A failed repeat
discovery prevented detection; an empty result ended silently. Detection now
uses an available chat model from the displayed catalog before trying discovery.
An empty chat catalog reports the existing sanitized detection error. Neither
path changes the saved model selection, credentials or protocol.

Both regressions failed before the fix. All 85 provider-panel tests then passed,
including stale-result handling and stored-key endpoint restrictions. This fixes
the reproduced editor failure; the original endpoint's inference response has
not been retested and is not declared accepted.

Manual retest: open a Gemini-compatible supplier with a populated catalog, leave
all models unselected, and click Detect protocol. Expect a result for an available
chat model without changing the selection. With no discoverable chat models,
expect a visible failure instead of a silently completed button spinner.

## Remaining reports

Row 10's attachment reports protected native Qoder credentials. Background refresh
must not bypass Keychain access approval. The user was asked whether the explicit
Read client login action succeeds after the system prompt; this report remains
unresolved until that result is available. No credential storage was changed.

Row 22's attachment uses a mode-4755 permission change. Existing synthetic rule
coverage does not explain the missing event in that original client session; its
status remains unresolved. Row 11's existing WorkBuddy session selection is also
unresolved; native conversation history was not rewritten.

## Verification

Checked base: `3f1daf658fc92a492a03b3c9ec0becc009773e3f` plus the uncommitted
working tree. Existing changes from other tasks were preserved. The
[source manifest](2026-11-16-qoder-detection-sources.json) records 66 changed
source/configuration/test file hashes; they were verified again after the checks.
Host: macOS 26.6.2 arm64, Rust 1.98.1, Node 20.20.2, pnpm 10.12.3.

| Check                                                              | Result                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                        |
| `pnpm build:renderer`                                              | Passed                                                        |
| `pnpm format:check`                                                | Passed                                                        |
| `pnpm test:unit`                                                   | 4,096 passed in 307 files                                     |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                        |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                        |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                        |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 7,688 passed, 21 ignored, zero failed across 16 result groups |

The full Cargo run includes the library suite: 7,523 passed and 21 ignored.
Its first run exposed an older menu-preference assertion that expected a model-only
Qoder label. That assertion now expects the supplier-qualified display name and
also verifies the unchanged request model. The focused regression and final
backend checks were rerun successfully. No checks were disabled or assertions
removed. The macOS linker emitted its nonfatal compact-unwind size warning.

Command results and logs are under `/tmp/varyai-qoder-followup-frontend.json`,
`/tmp/varyai-qoder-final-backend.json` and their matching command log paths. The
final frontend source did not change after its full suite; only the backend test
assertion and documentation changed afterward. No helper implementation or
dependency manifest changed in this follow-up. Remote Linux/Windows/macOS CI,
Windows-to-WSL2 execution, packaging and installed-client acceptance were not run.
