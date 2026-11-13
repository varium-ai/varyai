# Qoder TLS shutdown and readable Agent aliases

## Checked changes

The checked working tree combines remote revision
`9804cdb78f509a7b47ce2db90da1ce18adc496c8`, local commit
`19c73e0682a8debbae03e1170fe1ee94309326f4`, and the shutdown/alias regression
fixes. The SHA-256 of `git diff 9804cdb78 --` over `docs/development.md` and the
five changed Rust files is
`cabe0a11a6e580e501caa3fbaba0a68f5cb668fbc7e66100d7aa23fd96d57567`.
This verification record is excluded from that fingerprint.

The macOS CI failure occurred when a certificate test rebound its listener
address after `stop()` returned. A portable delayed-cleanup regression reproduced
the same address-in-use error with the former three-second timeout. Shutdown now
joins the listener task, which cancels and joins its accepted connections, before
returning. The immediate rebind and pending-handshake assertions remain enabled.

The local readable-alias commit conflicted with two newer tests expecting native
resource IDs. Their expectations now cover readable publication, hidden legacy
IDs, native configuration consistency and restoration. An additional regression
demonstrated that renaming/reordering a catalog changed already published aliases;
refresh now reuses an existing readable alias while preserving legacy mappings.

## Environment

- Worktree: `/Users/guitar/.codex/worktrees/qoder-tls-ci/varyai-open-source`.
- Host: macOS 15.3.1 (24D70), arm64.
- Rust/Cargo: 1.98.1, `stable-aarch64-apple-darwin`.
- Node: 22.12.0; pnpm: 10.12.3, matching the repository pins.
- Rust commands use `CARGO_INCREMENTAL=0` and the existing Cargo target directory.
  An initial compilation exhausted disk space; generated lib-test incremental
  caches and obsolete object files were removed before retrying.
- Final frontend logs:
  `/var/folders/q7/3zkkhzjs2vdb1xb_y2czj_hh0000gn/T/varyai-qoder-final-frontend-cpx08o1g/`.
- Final backend logs:
  `/var/folders/q7/3zkkhzjs2vdb1xb_y2czj_hh0000gn/T/varyai-qoder-acceptance-bvftply3/`.

## Local verification

Passed:

- `pnpm typecheck`
- `pnpm build:renderer`
- `pnpm format:check`
- `pnpm test:unit`: 304 files, 4,050 tests.
- `pnpm test:dev`
- `pnpm test:macos-dmg`
- `pnpm test:trae-bridge`
- `pnpm test:model-default`
- `pnpm test:workbuddy-checkin-auth`
- `pnpm test:doubao-checkin`
- `pnpm test:agent-rewards`
- `pnpm test:qoder-codec`
- `python3 -B scripts/tests/recover-shared-v23.test.py`
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 7,467 passed, zero
  failed, 21 existing ignored tests.
- `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`: all 16 test
  targets passed; 7,632 tests passed, zero failed, 21 existing ignored tests.
- `pnpm exec prettier --check docs/qa/2026-11-12-qoder-tls-ci-verification.md`
- `git diff --cached --check`

The alias rename/reorder regression, Agent legacy-selection/rename integration
test and T3 Code isolated-provider test also passed individually against the final
compiled crate. Both new regressions were observed failing before their fixes.
An exploratory `qoder_`-filtered run returned HTTP 503 in the unrelated custom-model
test; that unchanged test passed in the subsequent complete library run.

## Acceptance boundaries

Checks use synthetic fixtures, dynamic ports, temporary storage and in-memory
databases. No packaging, installation or system-trust changes were performed.
Remote Linux, Windows, macOS and applicable Windows-to-WSL2 checks must be assessed
for the exact pushed revision; local macOS results do not establish remote CI,
live-client, signing, notarization or installer acceptance.
