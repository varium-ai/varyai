# Consolidated pre-submit verification

This record covers the combined November 10 working tree based on
`82204c3fca2ed229f40fd5bfda5c4daf33967d4f`: native Codex authentication and
catalogs, Magpie conversation identity and concurrent forwarding, translated
deferred tools, macOS installer presentation, and version 0.3.0. It supersedes
intermediate acceptance results in the individual QA records.

## Test isolation corrections

The initial full Rust runs exposed two existing fixture defects:

- The escape-dense upstream-error test uses a helper that temporarily publishes
  the process-wide security runtime. It lacked the serial marker used by other
  callers of that helper, allowing another policy publication to interrupt its
  synthetic admission before the serialization assertions.
- The command-preview test assumed an initialized process-wide security runtime.
  In isolation, audit admission had no published policy, so command summaries
  correctly failed closed and no program matched. It now publishes a synthetic
  default policy under the serial test lock and restores the previous snapshot
  on scope exit.

Only test setup and isolation changed. Production security policy, redaction,
matching behavior and all existing assertions remain.

## Environment and evidence

- Checkout: `/Users/guitar/projects/varyai-open-source`.
- Host: macOS 15.3.1 (24D70), arm64.
- Rust/Cargo: 1.98.1.
- Node: 24.11.0; pnpm: 10.12.3. The local Node version differs from the
  repository's 22.12.0 pin; CI uses `.node-version`.
- Command logs, results and checked-source hashes:
  `/tmp/vary-desktop-picker-20261110-checks/`.
- Frontend results: `magpie-front-results.json`.
- Final backend results: `pr-final-back-results.json`.
- Backend commands use cached offline dependencies, disabled incremental/debug
  output, one build job, `/tmp/vary-magpie-parity-target`, fresh temporary test
  homes, and loopback proxy exclusions.

## Results

Frontend type checking, renderer build, all 3,588 unit tests, formatting,
development launcher, macOS DMG, TRAE bridge, WorkBuddy authentication, Doubao
check-in, agent rewards, Qoder codec and shared-storage recovery helper tests
passed. The production dependency audit passed with no known vulnerabilities.
Shell syntax checks passed for both packaging entry points.

`cargo test --offline --manifest-path src-tauri/Cargo.toml --no-fail-fast`
passed all 16 test targets: 6,403 tests passed, zero failed, and 19 existing
library tests were ignored. The previously failing preview test also passed
in isolation.

The separate library run passed 6,240 tests with zero failures and 19 existing
ignored tests:
`cargo test --offline --manifest-path src-tauri/Cargo.toml --lib`.

All final static checks passed:

- `cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings`
- `cargo check --offline --manifest-path src-tauri/Cargo.toml`
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`

The backend source/configuration hashes were unchanged during these final
checks. Subsequent edits only consolidated and formatted these QA records.
Changed documentation and helpers passed their applicable Prettier checks;
the final staged diff passed `git diff --cached --check`.

## Acceptance boundaries

Tests use synthetic fixtures, temporary storage, in-memory databases and dynamic
ports. The installed app was not replaced or restarted. The earlier DMG visual
and checksum checks validate presentation using a debug artifact. Local checks
do not establish live-client, installer release, signing, notarization or
cross-platform CI acceptance. Remote CI must be assessed for the pushed revision.
