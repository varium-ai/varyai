# Desktop update exit feedback

## Request and reference

The user requested removal of the red global notice saying that Claude Desktop's
update failed because the application had not quit. The selected response is
addressed by treating this condition as a deferred update rather than a global
failure. The target application is never counted as successfully updated merely
because its failure notice is hidden.

The local Magpie reference at `58f24cc23527776b5f1958cfd4a8092a659bdfb5`
was reviewed in `internal/agent/cliupdate.go` (`Agent.CLI`) and
`internal/gui/assets/app.js` (`updateCLI`, `paintUpdateAll`, `cliTag`). That
reference updates Agent CLIs and displays desktop application versions; it does
not implement VaryAI's desktop bundle replacement path. It therefore does not
establish native desktop-update parity. VaryAI keeps its desktop updater and
adapts the requested unobtrusive per-row feedback without masking real failures.

## Implementation

The renderer records the recognized desktop `running` result as `deferred`.
Deferred Agents retain a muted version-row hint and an enabled update button.
Their state is separate from batch results, so it survives navigation and updates
to other Agents during the renderer session. Retrying clears only the selected
Agent's hint. Deferred-only results do not render the global result banner;
mixed batches describe completed or genuinely failed operations without counting
a deferred update as either success or failure. CLI errors, unknown/malformed
error codes and actual desktop installation failures retain their prior handling.
All four runtime languages have a localized waiting hint.

The macOS updater now waits for kernel absence of each captured process ID.
AppKit's cached termination flag is no longer the exit signal. An invalid PID,
permission failure or other unknown state retains the wait; an application that
stays open times out with the same backend code, presented as a deferred update.
Replacement still follows exit, identity/signature checks, staging and rollback.
No force quit, automatic retry or installed-client launch is added.

Windows/Linux automatic desktop bundle replacement remains unsupported as before;
the renderer behavior and portable wait-contract tests are shared. The full Agent
inventory and native acceptance gaps are recorded in
[Agent feature coverage](agent-feature-coverage.md#desktop-update-exit-feedback).

## Verification

Base: `e84ae0dc90ca5e3b77821f121e41347f8f3d4ad0` plus the combined working tree.
Host: macOS 15.3.1 (24D70), Apple Silicon; Rust/Cargo 1.98.1, Node 24.11.0,
pnpm 10.12.3. Node differs from the pinned 22.12.0. Prior routing, continuation
and startup-probe work remains intact; these checks cover that combined tree.

Before the implementation, all nine new desktop-path regressions failed because
they received the global error notice rather than an inline deferred state.
The focused final frontend run passed 88 tests across the Agent update, localized
message and full-inventory coverage suites. The focused backend update suite
passed 13 tests; one explicit publisher-download/staging test stayed ignored.
An initial typecheck identified the new field missing from a message-test
fixture; the fixture was updated before final checks.

The first full backend library run found an existing native-agent redaction
fixture that neither initialized nor isolated its required process-global policy.
It failed its unchanged synthetic redaction assertion in the full run and also
failed when run alone without policy initialization. The test now takes the
shared serial guard, explicitly installs its redaction policy and restores the
previous policy on drop; production native-agent forwarding is unchanged.
Final verification passed on the frozen source snapshot recorded in
`final-source-v3.sha256`:

| Check | Result |
| --- | --- |
| `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 322 files, 4,297 tests passed |
| Development launcher, CI-policy, macOS packaging, client bridge, model-default, Pi identity, check-in, rewards, codec and storage-recovery helpers | All passed; exact commands in `frontend-results.jsonl` |
| `pnpm audit --prod` | Passed; no known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 7,712 passed, zero failed, 21 manual tests ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Full library and 165 integration tests passed; same 21 manual tests ignored |
| Isolated native-agent redaction fixture after policy isolation | Passed without changing its assertions |
| `git diff --check` | Passed |

An intermediate full run exhausted the filesystem and reported `StorageFull` /
`No space left on device`, causing further fixture failures. Build space was
recovered and the affected final full runs above were repeated successfully.
Exact commands, source hashes and logs are in
`/tmp/vary-desktop-update-feedback-verification/`. Backend tests use temporary
homes, synthetic process-state observations, in-memory storage and dynamic ports.
Unused generated test executables and duplicate top-level static-library build
outputs were removed to recover build space; no source, installed application or user data was removed.

No running application was quit, installed, replaced or launched for update
acceptance. Native installation/quit acceptance and Windows/Linux/macOS remote
CI remain unverified. No commit or push is part of this change.
