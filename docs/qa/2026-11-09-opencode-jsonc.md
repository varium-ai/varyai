# OpenCode JSONC publication and model limits

Checked base: `0cb8e387fd972ac3f04c0956f8c4b6df5fd64af9` plus this task's
agent-connection writer, model-limit, regression-test and troubleshooting changes.
Other ongoing subscription, login and proxy changes were present in this shared
checkout. Host: macOS 15.3.1 arm64, Node 24.11.0, pnpm 10.12.3, Rust 1.98.1.
Installed native consumer: OpenCode 1.19.5.

## Findings

The OpenCode connection reused the OpenClaw JSON5 writer. When adding a missing
root `model` field, that writer emitted an unquoted identifier. OpenCode accepts
JSONC comments and trailing commas but rejects the JSON5 identifier with
`InvalidSymbol` and `PropertyNameExpected`. The previous parity fixture used
unquoted keys and single-quoted values and only checked with a JSON5 parser,
so it did not enforce the native consumer's syntax contract.

A second defect was hidden behind the syntax failure: a model with known context
and unknown output received `limit.context` without `limit.output`. An isolated
`opencode debug config` probe rejected that synthetic context-only fixture
(exit 1), then accepted the same fixture with `output: 0` (exit 0).

The reference comparison is pinned to
[Magpie 465928bccea4fec3537edd6280f7cabb9a0c5bad](https://github.com/yetone/magpie/tree/465928bccea4fec3537edd6280f7cabb9a0c5bad):
`internal/edit/json.go` and `jsonc_valid.go` enforce JSONC output;
`internal/agent/agents.go` emits both limit fields and uses zero for unknown output.
These two contracts were not previously aligned. This fix does not establish
complete Magpie feature parity.

## Changes

- OpenCode receives its own JSONC publication path. It retains compatible legacy
  receipt reads but validates emitted syntax using JSON tokens plus
  `serde_json`, allowing comments and valid trailing commas only.
- New root keys are double quoted, including legacy bare model keys. Nested
  object edits preserve unrelated provider comments and uniform CRLF line endings.
- Receipt serialization keeps the existing `Json5` representation for backward
  compatibility. OpenCode upgrades the in-memory writer on load. Refreshes repair
  old syntax even when the selected model and catalog have not changed.
- Model limit objects always contain both `context` and `output`, using zero
  for an unknown value and retaining known output ceilings.
- Regression fixtures cover strict JSON, JSONC, native-provider preservation,
  connection refresh/disconnect, legacy receipt repair, comments, CRLF, invalid
  JSON5, malformed commas and complete model limits. An optional installed-CLI
  regression feeds production-generated JSONC and model metadata into OpenCode.

## Verification

Both regressions failed before their implementation changes:
the projection failed strict JSON decoding with `key must be a string`;
the limit regression found an absent output field where zero was required.
The legacy refresh regression also detected a semantic no-op that skipped syntax
repair; the no-op check was corrected without weakening the assertion.

- Focused `services::agent_connections` run: 64 passed, one optional installed-CLI
  test ignored in the ordinary run.
- Explicit `installed_opencode_accepts_generated_jsonc -- --ignored` run:
  one passed. Production JSONC encoding and model-limit generation were used;
  the installed OpenCode 1.19.5 `debug config` command exited successfully.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm test:unit`: 3,577 passed in 281 files.
- `pnpm audit --prod`: no known vulnerabilities.
- Node helper checks (`test:dev`, `test:trae-bridge`,
  `test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`,
  `test:qoder-codec`): 94 passed; one optional installed-Qoder check skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,186 passed,
  19 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 6,186 library tests and
  163 integration tests passed; 19 ignored. The ignored tests include the
  installed-OpenCode regression executed explicitly above and 18 existing ignores.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- Prettier checks for troubleshooting and this report: passed.
- `git diff --check`: passed.

Rust verification uses the existing target directory with
`--config profile.dev.package.varyai.debug=0 --config build.incremental=false`
to keep build artifacts within the host's available disk space. This affects
debug information and caching, not test assertions or runtime behavior.
The macOS linker reported a large `__eh_frame` section warning during test
linking; the test commands completed successfully.
Stale library-test codegen objects from before this task were removed;
source files and live application data were not cleaned.

Native checks use temporary XDG configuration, data and cache directories,
synthetic credentials, disabled project configuration and disabled remote model
discovery. Output is captured rather than exposing client configuration. No
vendor inference, application installation, launch/replacement of VaryAI,
provider cleanup or credential rewrite was performed. A read-only live check
found that the user's OpenCode file had already returned to valid strict JSON
and no VaryAI provider was present, so no live repair was necessary.

The writer and model projection are shared between macOS and Windows.
CRLF behavior is covered by fixtures. Remote CI, installed Windows acceptance
and live inference acceptance are outside these local checks. No commit or push
was performed.

## Checked file fingerprints

| File                                                       | SHA-256                                                            |
| ---------------------------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/services/agent_connections.rs`              | `a8f2f8e01411cae127be833934663cdbed21ae643796ef3a3fcf4d177387a82b` |
| `src-tauri/src/services/agent_connections/document.rs`     | `a199a6514688ca61265e3ec94d7bb0f7afaa1d711d00d59c13d97aa8ea380870` |
| `src-tauri/src/services/agent_connections/jsonc.rs`        | `1659a1605b683b843bd15cf191916c1f0533dada1bb16a74658835e23303016d` |
| `src-tauri/src/services/agent_connections/native.rs`       | `821fb4912c0f0942de9a2c7536910c2001a448b9501dc4481043efb46433ed39` |
| `src-tauri/src/services/agent_connections/parity_tests.rs` | `24ee737eb6901b3ae7b02630fc158a0f2e7d77ce76dbc827453e142c519d3029` |
| `src-tauri/src/services/agent_connections/reasoning.rs`    | `c69b441d7168a5d7c46074d920285595eaecd4d3bc7e23409488a2724fc1be24` |
| `docs/troubleshooting.md`                                  | `13a44a6217423e581b225cd4dce15b59e61fe54139c11cc239ac741bce93b4ec` |
