# Combined bug-fix merge verification

The local fix commit `23afbe7afdc3d6c9fb96643891e4fe74081f3655` could not be pushed because the remote branch advanced to `4ad92a29f76356aa062a811eb90c368d03a4af06`. Both histories are retained. Five conflicts were resolved in the DLP restoration admission fixture, provider key editor, routing panel, and their renderer tests.

The merge retains the remote routing table, per-row pending operations, explicit removal of automatic-discovery controls, share/copy action, native DLP protocol ownership, and provider UI updates. Local fixes retain key validation, disabled unavailable members, provider-name masking, visible unavailable template actions, removed-group recovery, and explicitly selected generic-key hiding actions. Regression tests use the current row-removal and share/copy controls.

Two inherited backend assertions still expected Gemini/T3 Code to use display labels as request IDs. Their final tests require stable native IDs across display-label changes, retain legacy aliases, verify the visible T3 name separately, and still check native selection and restoration. Production behavior was not changed to satisfy these assertions.

## Checked tree

- Host: macOS 26.6.2 (25G83), Apple Silicon.
- Node 20.20.2, pnpm 10.12.3, rustc/cargo 1.98.1.
- Changed source/test files relative to `ab5cf647`: 239.
- Source fingerprint: `99c248f5783b0901fa92d53c8dd4d5350ccdb8f99be123a9f8c47b8311400630` (SHA-256 of the sorted JSON source-hash mapping).
- Source hashes are recorded in [the merge source manifest](2026-11-12-bug-fix-merge-sources.json).
- Rust commands use `CARGO_INCREMENTAL=0`.

## Local results

Frontend typecheck, renderer build, formatting, production dependency audit and all helper suites passed. The complete renderer suite passed 4,050 tests across 304 files. The shared-v23 recovery helper passed 5 tests.

The initial backend library run passed 7,463 tests with 21 ignored and exposed the two obsolete assertions above. Superseded runs were stopped when their test fixtures changed. The corrected library run passed 7,465 tests, with 21 existing ignored tests and no failures. Final compilation, formatting and Clippy checks passed. The complete `cargo test --manifest-path src-tauri/Cargo.toml` run passed 7,630 tests with 21 existing ignored tests across 16 binary/doc-test groups.

Logs are under `/tmp/varyai-merge-check-*.log`, `/tmp/varyai-merge-accepted-check-*.log`, and `/tmp/varyai-merge-recovery.log` on this host.

## Scope and limits

The user-approved Chinese routing-design document remains Chinese. All sessions' committed source and documentation changes are retained. The running application was not launched, installed, or replaced. Local checks do not establish installer, signing, notarization, live-account, or cross-platform acceptance. Remote CI must validate the exact pushed merge revision, including Linux, Windows, macOS and the applicable Windows-to-WSL2 contract.

Final verification repeated `cargo fmt --check`, `cargo check`, `cargo clippy -- -D warnings`, `cargo test --lib` and `cargo test`, each with the manifest path above. All source hashes still matched the recorded 239-file snapshot afterward. All 13 frontend/helper commands, the recovery helper, production dependency audit and final formatting check passed. The initial failures and superseded runs do not count as final acceptance.
