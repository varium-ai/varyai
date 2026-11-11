# TRAE fixture verification

Date: 2026-11-11 (Asia/Shanghai).
Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1, Node 22.12.0,
pnpm 10.12.3. Node matches `.node-version`.

Base revision: `55a07d45444dbaf087cd67478419a4eb3e840951`.
The 67 changed source, asset and documentation files, excluding the two QA
reports, have aggregate SHA-256
`0f0a0406ada6f04f407402e0a88097cea1fd701c33cb838f8c4713d3ab4d4ea2`.
The local manifest `/tmp/vary-trae-source-manifest.json` records each content
hash and the aggregate encoding (sorted UTF-8 path, NUL, content hash, LF).

## Scope and reproduction

This follow-up preserves the Cursor and T3 Code integration changes and fixes
the TRAE fixtures identified in the
[initial verification](2026-11-11-cursor-t3code.md).

- Derive the resident helper's newer revision from its current revision.
- Supply the original provider and model name in positive legacy model fixtures.
- Advertise the current bridge revision, 18, in the Rust reconnection and profile
  fixtures. Both still reject unexpected operations, including audit enablement.
- Join each mock worker during cleanup without a second panic during unwinding.
  A worker failure still fails normal cleanup.
- Add eight negative identity tests: missing or changed provider and model name,
  through both legacy-label discovery and stored-ID ownership. Each rejects the
  apply operation before any model mutation, preserves native models and reports
  a configuration mismatch.

Before fixture corrections, the helper suite reproduced three failures. The
eight new identity regressions passed against the unchanged implementation while
those three original tests still failed. The Rust reconnection fixture also
reproduced its panic and SIGABRT before correction. Both focused Rust fixtures
were rerun after correction.

## Identity protection

The production identity check remains unchanged. The following complete files
are byte-for-byte identical to base revision
`55a07d45444dbaf087cd67478419a4eb3e840951`:

| File | SHA-256 |
| --- | --- |
| `src-tauri/resources/trae-bridge/renderer.cjs` | `4a66367a454f25a6149142510e9644c36fa592e6a93e75e7bfb869e6a085fca5` |
| `src-tauri/resources/trae-bridge/agent.cjs` | `8b1980534f26f6ddaba9a136da336379a236ce41792ec4ba5d7ca068877b568d` |
| `src-tauri/src/services/trae_bridge.rs` | `3b13c28f5e0a94973684050b4c70a652b312a771ed28111ce9b76df48f167a20` |

## Complete local rerun

The commands below cover the final non-report working tree. Rust checks use
`CARGO_INCREMENTAL=0` to limit temporary disk use. Fixtures use synthetic data,
isolated paths and dynamic ports; no installed application was launched or
replaced.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed: 294 files, 3,872 tests |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| `pnpm test:trae-bridge` | Passed: 66 tests, none skipped |
| `pnpm test:dev` | Passed: 8 tests |
| `pnpm test:macos-dmg` | Passed: 5 tests |
| `pnpm test:model-default` | Passed: 9 tests |
| `pnpm test:workbuddy-checkin-auth` | Passed: 5 tests |
| `pnpm test:doubao-checkin` | Passed: 13 tests |
| `pnpm test:agent-rewards` | Passed: 9 tests |
| `pnpm test:qoder-codec` | Passed: 3 tests, none skipped |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed: 5 tests |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed: 6,326 tests; 19 ignored |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Passed: 6,490 tests total, including 164 integration tests; 19 ignored |
| `git diff --check` | Passed |

The focused reconnection library test and profile-bundle integration test each
passed. Rust test compilation emitted a macOS linker warning that the test
binary's `__eh_frame` section exceeds the compact unwind encoding limit.
The new identity regression block also matches Prettier range
formatting, preserving unrelated inherited formatting in the helper file.

All 32 profile-bundle tests passed in the full integration run. The 19 ignored
library tests retain their original annotations and cover explicit live-client,
download, external-service, private-corpus and performance acceptance. They are
not counted as passed. Their names and reasons are retained locally in
`/tmp/vary-trae-ignored-tests.log`.

## Limits

This report records local verification before commit and push. Remote Linux,
Windows, macOS and Windows-to-WSL2 CI results were unavailable at that point and
must be checked against the pushed revision. Local macOS verification does not
establish cross-platform CI acceptance. Installed-client inference,
installer, signing and release acceptance remain outside this test run.

Command logs are retained locally under `/tmp/vary-trae-fixtures-*.log` and
`/tmp/vary-trae-full-*.log`.
