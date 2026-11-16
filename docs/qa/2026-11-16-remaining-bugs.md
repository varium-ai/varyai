# November 16 remaining-bug follow-up

The user requested further fixes, including Codex restart, with no commit or push. The existing browser view was inspected without reloading or changing filters. It still contains 32 records. No tracker status was written. Row numbers refer to the [current 32-row list](2026-11-13-current-32-bugs.md), not earlier snapshots.

## Changes and acceptance boundaries

| Rows       | Change                                                                                                                                                                                                                                                                                                                                                                  | Acceptance still required                                                                                                                                  |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 11, 21, 22 | Desktop restart waits for the original PID to exit, treats loss of launch metadata as an unknown live process, ignores dead LaunchServices entries before reading their metadata, launches a fresh verified macOS instance, and confirms that an instance reappears. Retry feedback remains visible while pending and duplicate clicks cannot dispatch another restart. | Installed Codex and other supported desktop clients on macOS and Windows. No running client was quit or launched for verification.                         |
| 30         | Shared routing selects tool semantics using the verified Agent identity independently of the wire protocol. Native Pi Chat no longer loses its `bash` definition by being classified as generic OpenAI. Pi event attribution is retained.                                                                                                                               | Live Pi/Codex warning delivery for the original calls. Unknown schemas, custom namespaces and unsupported conversions remain outside established coverage. |
| 32         | Client-provided session IDs propagate through response observations, the asynchronous worker, tool-event storage and unified security-event projection. Internal routing affinity is separate and cannot stand in for a client session.                                                                                                                                 | Create new tool events with clients that actually supply a session ID. Historical rows and requests without a valid ID remain empty.                       |

Extension schema version 8 adds the nullable tool-event session column and session/time index. Native and internal staging pairs are backed up before the transactional upgrade. Existing settings, routing preferences, pair identity and event history remain intact. Interrupted native version 7 transactions remain recoverable. The shared schema version is unchanged.

Row 31 remains unaccepted for its reported live call. Its attachments show a permission-changing command with mode `4755` and a model claiming success; they do not by themselves establish the complete structured tool-call payload. The synthetic HTTP regression covers that mode, symbolic setuid and an ordinary permission mode with Pi `bash` and Codex `exec_command` / `shell_command`. It asserts applicable policy results without executing any command. Ordinary `chmod 777` retains the existing uncertain verdict; this change does not broaden the privilege rule to every permission change.

## Reproduction evidence

Before the restart changes, two lifecycle tests failed: missing launch metadata was treated as process exit, and an accepted open request was reported as a completed restart without observing an application. The UI regression failed because retry removed its error feedback. The tool-event DAO regression failed because the supplied session ID was discarded.

The Pi HTTP regression then reached a successful tool-bearing response but produced no applicable shell event because its schema was collected as generic OpenAI. After schema attribution was corrected, the converted Codex path exposed a second identity defect: an internal routing affinity was returned as the event session. A separate optional observation-session value now preserves the actual client session or its absence. Dynamic ports, in-memory databases and synthetic identifiers are used throughout.

Apple documents that [NSRunningApplication](https://developer.apple.com/documentation/appkit/nsrunningapplication) properties can depend on main-run-loop progress and that [terminate](https://developer.apple.com/documentation/appkit/nsrunningapplication/terminate%28%29) does not wait for process exit. The lifecycle changes address these observable races; they do not establish the exact cause of every installed-client failure.

## Verification

Base revision: `3f1daf658fc92a492a03b3c9ec0becc009773e3f` plus the local working tree, including the preceding fixes for rows 25–28. Host: macOS 26.6.2 arm64; Rust 1.98.1; Node 20.20.2; pnpm 10.12.3. No commit, push, application installation, live configuration reset or tracker-status update was performed. Unrelated design research remains preserved.

Validation logs are under `/tmp/varyai-remaining-*`. Rust uses a private target directory so concurrent work cannot clean its executable. The [39-file source/config/test fingerprint manifest](2026-11-16-remaining-bugs-sources.json) was verified unchanged after the final checks. Final backend checks used `CARGO_INCREMENTAL=0`.

| Check                                                         | Result                                                                       |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `pnpm typecheck`, `pnpm build:renderer`                       | Passed on the final renderer sources                                         |
| `pnpm test:unit`                                              | 4,091 passed across 307 files                                                |
| Agent connections component suite                             | 237 passed                                                                   |
| Final `pnpm format:check`, `git diff --check`                 | Passed                                                                       |
| `cargo fmt --check`, `cargo check`                            | Passed                                                                       |
| `cargo clippy -- -D warnings`                                 | Passed                                                                       |
| Complete `cargo test` including library and integration tests | 7,675 passed, 21 ignored across 16 groups; library: 7,510 passed, 21 ignored |
| Development helper suites                                     | 119 passed across eight suites                                               |
| Storage recovery Python helper                                | 5 passed                                                                     |
| `pnpm audit --prod`                                           | No known vulnerabilities found                                               |

The helper suites were `test:dev`, `test:macos-dmg`, `test:trae-bridge`, `test:model-default`, `test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards` and `test:qoder-codec`. A nonfatal macOS compact-unwind linker size warning was emitted for the large application/test binaries. No test assertion was weakened to suppress a failure. Test fixtures were corrected to initialize the synthetic policy gate and use supported Pi Chat / Codex Responses endpoints before interpreting their results.

Linux, Windows and Windows-to-WSL2 CI were not run: this is an uncommitted local batch and the user explicitly excluded pushing. Portable Windows restart contract tests passed in the macOS library suite; native Windows process control and cross-platform database acceptance still require their platform jobs. Native application quit/reopen, packaging and live warning presentation remain unverified. No installed application was replaced or restarted.
