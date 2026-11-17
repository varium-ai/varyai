# Catalog refresh and continuation ownership

## Evidence and scope

A Claude Desktop conversation received local HTTP 409
`routing_unknown_continuation` after a catalog synchronization. The last preceding
request reached the selected upstream successfully; all 22 rejected retries had
zero routing candidates and no upstream attempt. A catalog synchronization
completed at 17:04:47 Asia/Shanghai; the first rejection started at 17:04:57.
The logs do not identify the exact changed configuration field.

The existing destination comparison and durable fingerprint include the complete
provider configuration, including `modelCatalog`. Therefore a catalog-only edit
can discard the in-memory owner and make the saved owner incompatible. The
all-Agent assessment is in [the coverage ledger](agent-feature-coverage.md#catalog-refresh-and-continuation-ownership).

Magpie revision `cd89ccdb4003847e26716be99f70f98dec1ca6a3`,
`internal/gateway/affinity.go`, treats a missing affinity as a new routing choice.
This fix retains VaryAI's existing ownership/security checks and addresses
catalog-only invalidation instead of broadly allowing unknown tool or opaque
continuations.

## Implementation

Only the top-level provider setting `modelCatalog` is excluded from destination
comparison and the persisted identity. The same normalization applies to Gemini's
account comparison. Every other setting and metadata field remains exact,
including protocol, credentials, endpoint, upstream model and fixed effort.
The stored provider configuration and forwarded request are not edited. Catalog
values are skipped without cloning the catalog itself on the forwarding path.

Legacy fingerprints are evaluated lazily when a stored destination does not
match the stable identity. An exact current legacy match restores its observed
owners and rewrites the record with stable keys after a normal observed answer.
Unknown legacy fingerprints remain rejected: an old hash does not establish
which endpoint/account produced it. This deliberately cannot reconstruct a
conversation whose legacy record was already invalidated before the update.

No policy is relaxed for unknown calls, ambiguous owners, foreign opaque state
or actual destination changes. The WorkBuddy recovery fixture now changes a
request-shaping cache setting as well as its catalog, so it continues testing an
actual configuration change and preserves its original refusal assertion.

## Regression coverage

The two initial regressions failed before the implementation: a catalog-only
edit changed live destination identity, and restoring the saved tool/opaque
owner returned `routing_unknown_continuation`.

Eight new regressions cover live identity, durable restart, exact legacy
migration, unchanged security boundaries, and four native protocol HTTP matrices.
The matrices derive all shared identities from the backend registry and add both
TRAE editions. Codex has separate managed-menu and native Responses requests.
They exercise JSON and SSE, named and unnamed conversations, real structured
tool-call observation, two tool-result follow-ups, catalog refresh and a new
runtime loading the encrypted store. Every forwarded history and its request
parameters are checked for preservation. Disabled connections and malformed JSON
are rejected before forwarding. These fixtures make 888 successful upstream
requests across the four matrices, using temporary homes/databases and dynamic
ports. They never launch installed applications or use production credentials.

The frontend ledger inventory regression checks all 20 product Agent IDs.
Doubao Work has its separate audit/launch integration and no shared catalog
destination affected by this change. Native applications, both desktop/CLI
variants and supported operating systems still need separate acceptance; shared
HTTP fixtures are not reported as that acceptance.

## Verification

Base revision: `e84ae0dc90ca5e3b77821f121e41347f8f3d4ad0`, plus working-tree changes.
Host: macOS Darwin 24.3.0 arm64; rustc/Cargo 1.98.1, Node 24.11.0,
pnpm 10.12.3. Node differs from the repository's pinned 22.12.0.
Unrelated concurrent working-tree changes are preserved.

The first default-debug compilation exhausted disk space before executing tests.
Stale generated Rust object caches and failed test objects were removed; source,
application executables, databases and linked libraries were retained. Rust
verification uses disabled debug information and incremental compilation, matching
the CI debug-information policy. The two regression failures were then observed,
and the first six regression tests passed after implementation. The final eight
regressions passed within the full library run. The first full integration build
also exhausted disk space while other builds were using the shared target;
inactive generated objects were removed and the retry used two Cargo jobs.
That retry passed all 165 integration tests but failed a concurrently added
startup-probe fixture: it changed the target model without the corresponding
routing-group member, leaving no valid candidate. The corrected fixture updates
both. Its focused rerun and the final complete Rust suite passed without changing
the continuation fix or weakening the fixture's assertions.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 322 files and 4,282 tests passed |
| `pnpm format:check` | Passed |
| `pnpm audit --prod` | Passed; no known vulnerabilities |
| Development launcher, CI policy, macOS DMG helper, TRAE bridge, model-default, Pi session, WorkBuddy auth, Doubao check-in, Agent rewards, Qoder codec and storage recovery helpers | All passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 7,706 passed, 21 intentionally ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` | Final run passed: 7,711 library tests and 165 integration tests; 21 intentionally ignored library tests |

The ignored library tests are existing explicit live-client/download, performance,
external S3 and private-corpus checks. They are not native acceptance passes.
Rust checks use `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
`CARGO_INCREMENTAL=0`; the final full retry additionally uses `CARGO_BUILD_JOBS=2`.
Focused/library runs used the equivalent `profile.test.debug=0` and
`profile.test.incremental=false` Cargo configuration overrides.

Logs: `/tmp/varyai-continuation-*.log`. Verification source hashes are recorded in
`/tmp/varyai-continuation-checked-tree.json` and
`/tmp/varyai-continuation-final-owned-source.json`. The final Rust run, check,
Clippy and formatting logs have a `-final.log` suffix; their source snapshot is
`/tmp/varyai-continuation-final-check-source.json`. The continuation sources did
not change during those checks. Concurrent desktop-update changes to the
frontend, locale files, tests and `commands/misc/agent_updates` started after the
recorded source snapshot; those later edits are outside the results above.
Windows, Linux, Windows-to-WSL2, installed-client continuation acceptance and
remote CI have not run in this change. No commit, push, application installation
or client restart was performed as part of this continuation repair.
