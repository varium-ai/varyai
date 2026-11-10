# Codex disconnect recovery and Magpie confirmation — 2026-11-10

## Report and reference

The user reported a failed Codex disconnect on another Mac running Vary AI
0.2.5+88. The screenshot first showed the normal confirmation, then a missing
recovery-record explanation and an instruction to inspect tool configuration.
No configuration, credentials, logs or recovery files from that Mac were read.
The build suffix does not identify an exact public source revision. Public
0.2.5 source contains the same restore-before-detach ordering as the current
checkout, but this does not prove the precise remote-machine input.

The reference is Magpie revision
[`add0dadd4f1e2be0670618967469f1a4f399c4f0`](https://github.com/yetone/magpie/tree/add0dadd4f1e2be0670618967469f1a4f399c4f0),
resolved from upstream HEAD on November 10. Relevant source:

- [`internal/gui/assets/app.js`](https://github.com/yetone/magpie/blob/add0dadd4f1e2be0670618967469f1a4f399c4f0/internal/gui/assets/app.js):
  `askDisconnect` uses a dry-run change preview where available, otherwise its
  existing endpoint/key/models/effort restoration explanation. Codex keeps a
  provider-table note for existing sessions. An operation failure leaves the
  confirmation available and reenables its button. Magpie has neither the
  reported Vary configuration-conflict instruction nor the receipt-missing
  confirmation branch.
- [`internal/agent/codex.go`](https://github.com/yetone/magpie/blob/add0dadd4f1e2be0670618967469f1a4f399c4f0/internal/agent/codex.go):
  `Unwire`, `unroute` and `dropProvider` restore owned settings while retaining
  subsequent user edits and the provider definition needed by existing chats.

The confirmation changes here adopt that restoration explanation, button label
and Codex provider note. They remove the extra missing-receipt and inactive-intent
confirmation branches. They do not implement Magpie's general file-diff preview,
so this report does not claim parity with every Magpie disconnect feature.
The reported generic configuration-conflict instruction was deleted from all
four runtime languages. Connection and model-visibility actions use their
existing failure state rather than rendering an internal conflict code.

## Reproduction and correction

A synthetic temporary-home regression installs Vary over a partial Vary baseline
whose provider has an unrecognized identity. The initial reproduction used an
extra provider field; after separating supplemental fields from ownership, the
rejection regression uses an external endpoint instead. Before this change,
explicit disconnect first restored that baseline and deleted its receipt, then rejected
its remaining routing fields. The failed operation had already lost recovery
evidence. The regression failed on the missing receipt assertion before the fix.

Explicit disconnect now computes and validates the complete restored/detached
configuration before replacing the live document and deleting the receipt.
The regression verifies that rejection preserves both the installed configuration
and the original recovery evidence, without creating an orphan status. Existing
ownership checks, private backups, user edits, authentication, external gateway
baselines, retained providers and deferred conversation recovery remain in force.
Automatic recovery without a receipt still leaves the configuration untouched.
The common Rust path applies on macOS and Windows.

A separate explicit-disconnect regression covers a recognized local provider with
supplemental fields, both with and without a fresh connection receipt. Magpie
retains that table instead of treating extra fields as a reason to refuse the
disconnect. Vary now checks the known identity and endpoint fields for explicit
detachment while preserving the entire provider table, user settings and login.
Automatic orphan cleanup retains its strict whole-table recognition. Unknown
endpoints, altered identity fields and ambiguous foreign connections keep their
existing protection. Before implementation, the focused regression failed with
`intentRouting.clientConflict`; the preliminary full library run likewise
reported 6,280 passed, that one expected regression failure and 19 ignored.
After implementation, all 141 client tests passed, including both supplemental
field cases and the rejected-external-endpoint recovery check.

The UI regression initially failed because receiptless and inactive Codex used
their own confirmation. It now checks the shared confirmation and retry behavior,
including that an internal conflict is not displayed and failure is not reported
as success. Fixtures use synthetic values, temporary directories and a dynamic
loopback port. No installed application was launched, replaced or disconnected.

## Verification

Host: macOS 15.3.1 arm64; Node.js 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1
stable. Node differs from the repository's exact version pin.
The checkout is based on `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing
concurrent provider-import, Desktop and model-context work preserved. Exact logs,
reference source copies and before/after hashes for the changed connection files
are in `/tmp/vary-codex-disconnect/`. This is a dirty-tree check, not acceptance
for a release artifact. Installing the source fix on the reported Mac requires
a build containing it.

The first backend attempt was blocked by a concurrent provider-import compile
error, subsequently corrected in that work. A later build filled the shared
package cache: Rust reported ENOSPC, and 27 frontend files could not load or
write their test fixtures. Those runs are failures, not acceptance results.
Cargo's package cleanup released generated artifacts; one cleanup attempt
encountered an already-removed incremental file, and its retry removed 16.9 GiB.
Source and installed applications were preserved. Subsequent Rust checks use
`CARGO_INCREMENTAL=0`.

The frontend rerun passed all 287 files and 3,692 tests; the two connection files
also passed 152 focused tests. Typecheck, renderer build, formatting, all seven
Node helper suites, the Python shared-recovery suite and production dependency
audit passed. Qoder retains one existing skipped codec case; the audit reported
no known vulnerabilities. Renderer/test inputs remained unchanged between the
final source inventory and the successful full unit run.

Final checks against the corrected source:

| Command                                                            | Result                                           |
| ------------------------------------------------------------------ | ------------------------------------------------ |
| `pnpm typecheck`                                                   | Passed                                           |
| `pnpm format:check`                                                | Passed                                           |
| `pnpm build:renderer`                                              | Passed                                           |
| `pnpm test:unit`                                                   | 287 files, 3,692 tests passed                    |
| Focused connection UI tests                                        | 152 passed                                       |
| Focused Codex client tests                                         | 141 passed                                       |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                           |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,281 passed, 19 ignored                         |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                           |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                           |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 6,444 passed across 16 test programs, 19 ignored |

The final compile, full library, formatting, Clippy and complete Rust test runs
had no source changes between their before/after inventories. The earlier library
failure was the new supplemental-field regression before implementation; its
successful final rerun is in `recheck-cargo-lib.log`. The preliminary ENOSPC runs
were also replaced by successful reruns, rather than counted as passes.
`acceptance-summary.json` maps the successful commands to the preserved logs.
No task source changed after the final inventory.

Remote Linux, Windows, macOS and WSL2 CI, installer acceptance and recovery on
the reported Mac have not been run. No commit, push or application installation
was performed.
