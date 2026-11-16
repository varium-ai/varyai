# Source labels in managed Agent model lists

## Requirement and scope

Every model VaryAI publishes to a managed Agent must identify its upstream
source. A provider heading such as `Vary AI` identifies the gateway and does not
replace the per-model source. The earlier React-picker change did not satisfy
this requirement for external clients.

The shared menu now qualifies direct-model display names before any Agent
adapter consumes it. It reuses readable source naming, consistent numbering
for equal source names and safe automatic Codex account labels. Routing groups
retain their group labels rather than inheriting an arbitrary member's source.
A raw model ID containing the source name still receives its source suffix;
custom labels already identifying the source avoid another suffix.

Native projections now allocate current display names independently of retained
request aliases. Existing connections can therefore refresh a display name
without reverting it to their older bare ID. Hermes and ZCode list IDs without
a separate display-name field, so their visible IDs follow current source labels
while historical aliases remain routable. An ID already identifying its source
stays stable when only the model's display name changes. WorkBuddy receives the current name
even if its older ID differs. Claude's role-name variables also receive the
display label. The React picker preserves already qualified backend labels
independently of account-management grouping.

This shared Rust/React logic applies to macOS and Windows. Native official model
lists outside VaryAI's managed catalog retain client behavior.

## Magpie reference

Reviewed Magpie revision `c5b9e448a8bc9bc2a1bf6704e5d8bb3384d084a6`:

- `internal/agent/route.go`: `buildModels` obtains `LabelsFor` once and stores
  each result in the shared model's `Name` before adapters consume it.
- `internal/provider/modelprefs.go`: `Entry.Label` and default `SuffixOn`
  publish `Model name · Provider name`, with the existing custom-name exception.
- `internal/agent/dsh.go`, `qoder.go` and `workbuddy.go`: native `name` or
  `displayName` fields receive that shared display name independently of `ID`.
- `internal/agent/hermes.go` and `zcode.go`: their native model lists use IDs.
  Magpie already encodes the provider in `Entry.ID` as `provider/model`
  (`internal/provider/models.go`, `entryFor`). VaryAI's retained bare aliases
  lack that information, so its ID-only adapters need qualified visible aliases
  while retaining compatibility mappings.

The implementation follows these publishing rules without adding Magpie's
optional per-Agent suffix-off settings. MIT attribution remains in
`third_party/routing-reference/NOTICE.md`.

## Verification method

- Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b`, with independent
  working-tree changes preserved.
- Host: macOS 15.3.1, Apple Silicon; Node 24.11.0, pnpm 10.12.3,
  rustc 1.98.1.
- Logs: `/tmp/vary-agent-sources-*.log`.
- Final task source/test hashes: `/tmp/vary-agent-sources-source.json`; all 11
  files matched the captured snapshot after the final checks.
- Two Rust regressions failed before implementation: missing shared-menu source
  labels and a historical ID overwriting the current display name.
- The first post-change matrix additionally exposed ZCode's ID-only menu, which
  required the same visible-alias handling as Hermes.
- The fixture matrix builds fresh and existing-connection projections for every
  portable Agent and for Claude Desktop on its supported host platforms. It
  checks source labels, retained historical aliases and serialized Harness YAML.
  It also checks Codex catalog metadata and both TRAE bridge payloads. The local
  discovery regression covers Gemini's `displayName`, other Agent catalog labels
  and forwarding through current and historical aliases on dynamic ports.
- A renderer regression first failed when an already qualified service label
  was expanded with its account-management group. The final picker suites pass
  41 tests across three files.
- Read-only inspection of the existing default Harness profile found 31 managed
  models, only three names containing an inline suffix, and no display name
  differing from its ID. The inspection returned counts only, without exposing
  names, endpoints or credentials. No native profile was written during this
  inspection.

## Results

| Check | Final result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | 312 files, 4,157 tests passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| Full `cargo test --no-fail-fast` | 7,707 passed: 7,542 library tests and 165 integration tests; 21 library tests retained their existing ignore gates |
| `git diff --check` | Passed |

The first complete library run had four failures. Three assertions still expected
old display names or their previous capability-table keys. The other regression
required preserving an already qualified ZCode ID through a model-label rename.
The latter behavior was fixed without weakening the existing alias-equality
assertion; expected label assertions were updated while retaining upstream,
capability and restoration checks. The final complete Cargo run includes the
passing library suite after these changes.

Rust runs used `CARGO_INCREMENTAL=0`. After waiting on the shared build directory,
verification moved to an APFS clone at
`CARGO_TARGET_DIR=/tmp/vary-agent-sources-target-20261113`. The final complete
test command additionally used
`--config profile.dev.package.varyai.debug=0`
and `--config profile.test.package.varyai.debug=0` to reduce primary-package
debug-symbol storage. Optimization, debug assertions and test assertions were
unchanged. The exact command outcomes are in
`/tmp/vary-agent-sources-backend-results.json`. The test build emitted the
preexisting quota-manager dead-code warning and large-DWARF linker warning;
the required Clippy command passed.
The temporary cloned build cache was removed after verification; logs and
source hashes were retained.

Development/helper tests and the production dependency audit passed earlier in
this chat, as recorded in the internal-picker report. They were not rerun in
this follow-up; their helper implementations and production dependency lockfile
were not changed by this task. No commit or push was requested.

## Runtime acceptance boundary

This task issued no application install, launch, quit or restart commands.
A second read-only inspection observed that the existing Harness profile had
refreshed: all 31 managed model names now included inline suffixes, and 28 had
display names distinct from their request IDs. The task did not directly edit
that native profile. This establishes the current file's updated labels, not
that every native client has reloaded its menu.

Native UI inspection was blocked because the Mac was locked and computer-use
inventory could not access native apps. No unlock or security bypass was
attempted. Clients retaining their own catalog may need their normal reload.
The active backend must contain the Rust fix; a renderer-only build cannot
update external Agent configuration. Cross-platform CI and live acceptance for
each installed client version remain unverified.
