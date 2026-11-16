# Codex catalog reuse — November 16, 2026

## Diagnosis and change

Offline inspection of the supplied test artifacts found an otherwise valid
connection identity with exactly 64 catalog paths. Its retained local credential
matched the VaryAI provider definition. The database passed integrity and foreign
key checks; its provider and routing-group references were intact. The application
log recorded a failed Codex model-menu refresh. These observations identify a
local publication failure independently of upstream account authentication.

The old writer allocated another UUID catalog path whenever the contents changed
and returned `intentRouting.limit` at 64 recorded paths. Disconnect preserved the
same identity and paths, so disconnecting and reconnecting did not clear the
condition. Automatic refresh failure can leave the runtime menu inactive; that
can produce `routing_session_expired` on a subsequent request. The artifacts do
not establish that every reported sign-in error had this cause.

The writer now updates the latest owned catalog path. Changed contents and a
missing current file reuse that path; unchanged contents do not rewrite it.
Existing historical paths and model aliases remain available. The legacy identity
reader retains its validation of configuration ownership, credentials, alias
count and historical-path structure; new refreshes do not grow the path list.
An exhausted 64-path identity therefore reconnects without manual cleanup.

The catalog is replaced through the existing guarded file operation. If receipt
or configuration publication fails, the previous catalog is restored or a newly
created catalog is removed. Concurrent external edits win over rollback.
Malformed model-list contents receive a private recovery copy before replacement.
Cache invalidation follows content changes, including when the filename stays
the same. The shared Rust implementation applies on macOS and Windows.

The reference is Magpie revision
[`e97271f02741ec12111e7641c0e669062a6e9b04`](https://github.com/yetone/magpie/blob/e97271f02741ec12111e7641c0e669062a6e9b04/internal/agent/codex.go),
where `codexIn` uses `magpie-models.json` and `writeOwnList` preserves malformed
contents before replacement. This comparison concerns catalog publication, not
the absence of all limits elsewhere in Magpie.

## Scope and environment

The base revision is `f9bbcb16b3f7d1e2d4b6ca8ce48840e245ca7e4f`. The checkout
already contained changes for Codex restart, tool discovery, source labels and
protocol conversion. This task changes only:

- `src-tauri/src/services/intent_routing/client/mod.rs` for path reuse,
  publication rollback and content-based cache invalidation.
- The new `catalog_file.rs` and `catalog_publication_tests.rs` in that directory.
- This report, `docs/troubleshooting.md` and the routing reference notice.

The test host is macOS Darwin 24.3.0 on arm64 with Rust/Cargo 1.98.1 and pnpm
10.12.3. Broad frontend and helper checks use the installed Node 22.22.2; the
repository pins 22.12.0, so these are not exact pinned-version results. Cargo
checks use cached dependencies with `--offline` and `CARGO_INCREMENTAL=0`.
The check runner sets `CC_SWITCH_TEST_HOME` to a temporary directory and appends
loopback exclusions to its own `NO_PROXY`/`no_proxy` environment.

All new fixtures are synthetic. Tests use temporary directories and a non-listening
loopback URL; they never request a real account or bind the application's port.
Private test artifacts and credentials are not included in this repository.

## Verification

Before implementation,
`cargo test --offline --lib --manifest-path src-tauri/Cargo.toml catalog_publication_tests`
failed both initial regressions: the legacy 64-path case returned
`intentRouting.limit`, and changed contents produced two files instead of one.

After implementation, the focused client suite passed all 174 tests, including
five new catalog tests and existing cache-expiry, ownership and restoration
tests. The final legacy test additionally exercises a missing catalog after
disconnect and a newly generated runtime token adopting the retained identity.
The complete checks below include those final assertions.

All local required checks completed successfully on the final source:

| Check                                                                           | Result                                                      |
| ------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`                    | Passed                                                      |
| `pnpm test:unit`                                                                | 4,235 passed in 317 files                                   |
| `cargo fmt --check`, `cargo check`, `cargo clippy -- -D warnings`               | Passed                                                      |
| `cargo test --lib`                                                              | 7,618 passed; 21 ignored                                    |
| `cargo test --no-fail-fast`                                                     | 7,783 passed: 7,618 library and 165 integration; 21 ignored |
| `pnpm test:dev`, `test:macos-dmg`, `test:trae-bridge`, `test:model-default`     | Passed                                                      |
| `pnpm test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards` | Passed                                                      |
| `pnpm test:qoder-codec`                                                         | 2 passed; 1 installed-client case skipped                   |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                           | Passed                                                      |
| `pnpm audit --prod`                                                             | No known vulnerabilities                                    |
| Documentation formatting and `git diff --check`                                 | Passed                                                      |

Cargo commands use `--manifest-path src-tauri/Cargo.toml`; compilation and test
commands also use `--offline`. Existing ignored tests include installed-client,
live-service and manual profiling cases. No live acceptance was opted into.
The test build retained the existing unused quota-helper and macOS compact-unwind
linker warnings; production Clippy passed with warnings denied.

Command logs, the baseline and per-file source hashes are stored locally under
`/tmp/vary-codex-catalog-fix/`. The 1,755-file source/check-input manifest has
SHA-256 `78246fc8dcc985e690e4f3cfb0d009ca797c1d7ed097597ed2c90bd870423196`.
Its before/after hashes match, including pre-existing working-tree changes.

No installed VaryAI or Codex application was launched, quit or replaced. The
tester's installed binary has not been verified with this fix. At completion of
local validation, no commit, push or remote Linux, Windows or macOS CI run had
been performed. The submission starts from main revision
`0437976cb` whose source tree matches the checked base revision above; remote
results are tracked on the submitted PR. The Windows-to-WSL2 workflow is not
triggered by these changed paths. Local tests do not establish installed-client,
packaging, signing or release acceptance.
