# Magpie API-provider key rows — 2026-11-10

The screenshot comparison exposed missing key-row metadata and actions, a custom
Preferred/Set as preferred presentation, and an extra API allowance field.
The upper form now places each key's model-scope badge, concurrency/spend status,
Enable/Remove actions and In use/First marker alongside that key. Disabled keys
have no Make first action. The separate API allowance field is removed; provider
list allowance and subscription-account meters remain available. All API presets
and custom providers share the same key editor. Vary AI brand colors are retained.

Per-key model scopes are saved separately from provider-wide picks. The inline
picker supports per-key choices, all available models, All models and Cancel.
An absent scope means unrestricted; an explicit empty scope serves nothing.
Magpie imports preserve explicit account-model scopes by the source's SHA-256
key fingerprint, or per-key catalog membership. Merge imports retain scopes.
Normal editor responses remain redacted; copying still retrieves only the
explicitly selected credential.

Scope enforcement covers direct models, routing groups, classifiers and saved-key
probes. An end-to-end regression caught reusable connection templates retaining
the first model's key for a different model. Templates now retain their scope
until retargeting checks it; executable snapshots discard that metadata. The
regression still verifies the exact synthetic key and single-target count for
each model, plus saved scope round-trips and unrestricted/empty distinctions.
No proxy hot-path database operations or lock changes were introduced.

Concurrency and spend text report the current engine's unrestricted configuration.
This change does not add configurable limit editors or Magpie's advanced spend
accounting. Those advanced capabilities are not imported, as the import contract
already states. No fabricated model counts, balances or limits are displayed.

Browser verification used a synthetic English renderer harness, with three
independent model scopes of 25, 10 and 13. All key rows measured 39px high at the
default desktop viewport; controls stayed alongside their own keys. The model
picker, scope save, enabling, Make first and removal were exercised. At 390×844,
the document width remained 390px and the dialog width 366px. Screenshots are in
`/tmp/vary-magpie-key-rows/`. The temporary harness, tab and server were removed.
No installed application was launched or replaced.

The checkout is based on `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing
concurrent Desktop, context, allowance and importer work preserved. Host: macOS
15.3.1 arm64; Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1 stable. Generated package
artifacts filled the disk during concurrent builds; Cargo package cleanup removed
36.3 GiB, preserving source and installed applications. Rust checks use
`CARGO_INCREMENTAL=0` to bound subsequent cache growth.

Initial UI regressions failed before implementation. The first integration run
also caught the connection-template scope bug described above. An initial full
frontend run failed 14 tests in concurrently changing Agent connection and
subscription-website fixtures. Their focused rerun passed all 145 tests after
connection switches were selected by name and the website fixture used the real
query provider. No tested product assertion was removed.

| Command                                                            | Result                                      |
| ------------------------------------------------------------------ | ------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                      |
| `pnpm format:check`                                                | Passed                                      |
| `pnpm build:renderer`                                              | Passed                                      |
| Provider form focused tests (five files)                           | 112 passed                                  |
| `pnpm test:unit`                                                   | 288 files, 3,687 tests passed               |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                      |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,276 passed, 19 ignored                    |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                      |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 6,439 passed across 16 binaries, 19 ignored |

The earlier broad frontend/library runs were followed by further concurrent
Desktop edits. The final provider-focused tests, renderer build, typecheck and
format check were repeated against `source-manifest-final.txt`. The subsequent
manifest changed only `AgentConnectionsPage.test.tsx`; its affected tests and
typecheck were rerun separately. These records identify the checked states and
do not imply remote CI acceptance. Helper tests and dependency audit from the
earlier provider-form verification were not repeated: their scripts, dependency
files and implementation inputs did not change in this follow-up.

Latest recorded source/test/helper manifest SHA-256:
`15a33a0dbec1a08f64500344167f6465222fe948c9b80e8579773cdf08d0f369`.
The path/hash manifest is `/tmp/vary-magpie-key-rows/source-manifest-end.txt`.
The local macOS linker emitted the existing `__eh_frame` size warning; builds
and tests succeeded. Logs are under
`/tmp/vary-magpie-key-rows/`, with earlier focused failures also under
`/tmp/vary-key-row-*.log` and `/tmp/vary-key-scope-*.log`.

No commit or push was requested. Remote Linux, Windows and macOS CI, including
any required WSL2 contract, has not run for this uncommitted tree. Installer,
signing, notarization and live vendor acceptance are outside these checks.

## Inline key replacement follow-up

Clicking a key's pencil now replaces that row's masked key label with its editor.
The row no longer displays both the old label and a separate replacement field.
Done restores the masked label. Focus, primary/backup draft values, Show/Hide,
key names, model scopes and account actions retain their existing behavior.
Opening the editor still does not retrieve the stored credential.

The test-first regression failed while the old label remained visible. After
the change, the three provider/key component suites passed all 54 tests. The
regression covers replacing both primary and additional key labels, focus,
switching between rows, draft retention and restoring a masked label after Done.

Browser verification used the actual React component with synthetic accounts and
an isolated API stub. The primary input's left edge matched the old label's
position exactly (323px); the other row retained its label. Both row editors and
Done were exercised, and the final state contained two masked labels and no key
input. The password field stayed masked in the screenshot. The preview server
and tab were closed without launching the installed app.

Only ProviderKeysField, its component test and the obsolete editing-label CSS
rule changed in this follow-up. Both desktop platforms use the same React code.
Base revision and host/toolchain are as recorded above; other shared-checkout
work was preserved. Logs, baseline copies, a synthetic screenshot and the final
three-file hash manifest are under `/tmp/vary-inline-key-replacement-20261110/`.

Type checking, renderer build, source/test formatting, Rust formatting and
`cargo check` passed. The full frontend run passed all 3,691 tests across 287
files. No helper, dependency, native-platform or forwarding implementation was
changed by this follow-up.

The Rust library suite passed 6,274 tests with 19 ignored, and Clippy passed with
warnings denied. These runs used `CARGO_INCREMENTAL=0`; they do not establish
remote CI, installer or live-provider acceptance.

The final full `cargo test --manifest-path src-tauri/Cargo.toml` run passed 6,442
tests across 16 binaries, with 19 ignored. It includes five backend test cases
added concurrently after the standalone library run. The three changed files
still matched the recorded hash manifest, and Rust formatting plus
`git diff --check` were checked again at the end. The final Rust formatting
recheck found unformatted concurrent additions in
`src-tauri/src/services/intent_routing/client/tests.rs`; that file was preserved.
The earlier Rust format check passed, but final whole-tree formatting acceptance
is incomplete while that other work continues. No commit or push was requested.
