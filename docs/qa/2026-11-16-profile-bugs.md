# November 16 scheme-mode follow-up

The user requested another read of the open bug list, a status for every record, and continued repairs without committing or pushing. The existing browser tab was inspected without reloading, changing filters or writing tracker fields. It initially showed 32 pending records; the final read showed 29 pending records. Three reports left the visible view during this work; their acceptance status is not inferred. The [latest 29-row mapping](2026-11-16-current-29-bugs.md) preserves the relationship to the [historical 32-row snapshot](2026-11-13-current-32-bugs.md). Report numbers below use that historical snapshot: 17 and 19 are now 14 and 16.

## Newly reproduced cases

- **17, scheme preview does not match configuration:** while Codex uses an owned shared route, previewing a saved native-supplier scheme displayed the last native supplier and its model rather than the current route. Preview now identifies Vary AI and the owned catalog model. Other agents and other preview mismatches remain unaccepted until their specific cases are reproduced.
- **19, cannot switch scheme:** applying that native Codex scheme reached the ordinary provider-switch guard while routing ownership was still active. The result matched the report's sanitized Codex provider-apply failure. An explicitly reviewed native selection now flushes queued routing continuity, uses the existing ownership-checked disconnect, and then enters the ordinary guarded provider-switch path. It records the disconnected intent only after provider/model/resource changes succeed.
- **Rollback and undo:** the previous native-ingress projection is captured alongside the existing local file, authentication, provider, receipt and menu snapshots. Restoring a shared connection restores that projection as well as its menu. Provider-switch guards, external-edit checks and pending thread-recovery protection remain in place.

A fifth regression uses the production relationship between the provider adapter and routing manager: both share the same temporary Codex configuration file. The generic unit-test proxy constructor intentionally uses a separate file, so the fixture overrides that test-only choice. With an explicit native baseline and a synthetic conversation database, Undo initially restored configuration but left existing conversations assigned to the native provider. Restore now invokes the existing guarded conversation reconciliation after configuration and routing metadata are restored. Legacy rollout metadata and paginated SQLite-authoritative histories both complete the routed → native → routed cycle without changing their model identity.

The first focused run reproduced the wrong supplier label and the guarded native-switch refusal. A forced failure after a successful native switch checks rollback rather than merely checking that an early refusal left everything untouched. Successful apply followed by Undo also verifies the original authentication bytes and routing ownership. Tests use an in-memory application database, a temporary client home, synthetic credentials and a dynamic unused listener; they do not start or replace installed clients.

The row 10 protocol-detection recording was also inspected. It shows an explicitly selected Gemini format and completion of the detection spinner, but no HTTP error status or detailed failure result. No live request was made to the recorded endpoint and no credential was retrieved. That reported case remains unfixed.

## Verification

Base revision: `3f1daf658fc92a492a03b3c9ec0becc009773e3f` plus the current local tree, including the earlier uncommitted repairs. Host: macOS 26.6.2 arm64; Rust 1.98.1; Node 20.20.2; pnpm 10.12.3. The final source/config/test fingerprints are recorded in the [source manifest](2026-11-16-profile-bugs-sources.json). All 43 fingerprints were verified unchanged after the final checks. Validation logs use `/tmp/varyai-profile-fixes-*`; Rust used a private APFS-cloned target directory. Final checks ran against the fingerprinted sources after the conversation-history repair, with `CARGO_INCREMENTAL=1`.

| Check                                                              | Final result                                                                        |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| Focused scheme-mode regressions                                    | 5 passed; includes legacy and paginated conversation restoration                    |
| `pnpm typecheck`                                                   | Passed                                                                              |
| `pnpm build:renderer`                                              | Passed                                                                              |
| `pnpm format:check`                                                | Passed                                                                              |
| `pnpm test:unit`                                                   | 4,091 passed across 307 files                                                       |
| `pnpm audit --prod`                                                | Passed; no known vulnerabilities reported                                           |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                                              |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                                              |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                                              |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 7,680 passed, 21 ignored across 16 groups; library subset: 7,515 passed, 21 ignored |

The macOS linker emitted a non-fatal compact-unwind size warning for the large test binary. No assertions or required checks were weakened. The final full Cargo run includes the library and integration checks.

Native application acceptance and Linux/Windows/Windows-to-WSL2 CI remain outstanding. No commit, push, configuration reset, installed application restart/replacement or tracker-status write is included in this batch. The unrelated design research and earlier repairs remain preserved.
