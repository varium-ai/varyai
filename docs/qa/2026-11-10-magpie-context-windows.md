# Magpie context window presentation

## Scope and reference

The November 10, 2026 report showed duplicate Claude Desktop entries and missing
1M tags in API provider model chips. Reference behavior comes from Magpie
revision `a45dd9c6573ab19cf19afd621501c9aef2e3f462`: `desktopLongestLine` and
`contextTag` in `app.js`, the `DesktopLongest` setting, `desktopModels` in
`internal/gateway/desktop.go`, native profile projection in
`internal/gateway/desktop_list.go`, and the longest-window tests.
The public reference is https://github.com/yetone/magpie/tree/a45dd9c6573ab19cf19afd621501c9aef2e3f462.

## Final behavior

Following the final request, the list exposes neither the largest-window switch
nor the Claude Desktop connection switch. Largest-window deduplication is always
enabled. Discovery publishes one long `[1m]` ID for capacities of at least 1M,
and managed native profiles publish `supports1m` and `prefer1m`. Actual input
limits, display labels, reasoning limits, smaller models and existing
plain/long/historical conversation aliases remain valid. The legacy discovery
endpoint publishes the same single long representation, and both plain and long
requests still map to the original upstream model. Installed Desktop
connects by default when the catalog is available. Existing active connections
and external ownership/recovery decisions are preserved; failed default attempts
are not retried repeatedly by rendering. No Desktop process is launched or
restarted. The connection flow and profile/discovery path are common to macOS
and Windows.

API provider chips merge sparse selected rows into rich discovery/catalog rows
without losing capacity metadata. Explicit selected capacities retain priority.
ID-only models can use the existing cached public model facts and a reduced
licensed models.dev snapshot. Unknown capacities remain unknown. This lookup is
read-only and does not save a fallback as a context override. It does not renew
credentials, probe inference or access provider allowance endpoints.

Tags exactly follow Magpie's threshold and formatting: only 1,000,000 tokens or
more; integral millions use `1M` or `2M`; fractional millions use one decimal,
such as `1.0M` for 1,048,576. Names already containing a token such as `1M` do
not receive a duplicate tag. The tooltip retains the exact token count.

The public input was fetched from https://models.dev/api.json on November 10,
2026. Its SHA256 is `a110847264aa6b935947ee15c1f94c65dea6ad7ff1812769797e881773326969`.
The reduced file retains 2,611 model suffixes and integer capacities. Source,
reduction rules and license are documented in `third_party/models-dev/NOTICE.md`.

## Test-first and visual evidence

The original switch/discovery tests and capacity merge/tag tests were written
before the initial implementation and failed on missing implementations. The
final default-on/no-switch request added workspace and tray tests that failed
against the switch-based UI before changing it. Coverage also checks public
ID-only capacity lookup, explicit smaller vendor limits, unknown models,
native historical `[1m]` aliases, and preservation of recovery ownership.

Synthetic English renderer checks use production React components with local
API fixtures. Evidence for model chips is
`/tmp/vary-longest-preview/model-badges.png`. Final Agent-list evidence is
`/tmp/vary-longest-preview/desktop-default-list.png`: Desktop is configured by
default and exposes only its model-list picker, with neither removed switch. No real credentials, provider inference or
installed client launch is used for these previews.

## Checked tree and environment

Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with the preceding
allowance/provider-form work and unrelated dirty changes preserved. Final source
hashes are recorded in `/tmp/vary-desktop-default-final-sources.json`.
Host: macOS arm64; Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.
CI uses Node 22.12.0, so local frontend results use a different installed version.

## Verification

All 18 required local commands passed against the final applicable source changes.
The initial library run found one existing takeover-profile assertion still
expecting only `supports1m`; it now explicitly expects `prefer1m` as well. The
complete library suite was rerun successfully after that fixture update. Earlier
failed-run logs remain available separately.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Passed |

The workspace received concurrent provider-page edits during verification. The
six affected component suites were refreshed successfully (252 tests), followed
by a fresh full frontend run. No source/test/script files changed during that
final refresh; its snapshot is `/tmp/vary-desktop-default-validation-snapshot.json`.

The frontend suite passed **3,691 tests in 287 files**. The final explicit Rust
library run passed **6,274 tests**, with **19 ignored**. The complete Rust run
passed **6,437 tests across 16 suites**, with **19 ignored**. The renderer,
production dependency audit, helper suites, Clippy and both format checks passed.
macOS test linking emitted the existing compact-unwind-size warning without
failing the command. Idle generated compiler caches were cleared to maintain
local disk space; no source or test assertion was removed to bypass a failure.

Final frontend logs are indexed in
`/tmp/vary-desktop-default-final-front-results.json`; backend results are in
`/tmp/vary-desktop-default-final-back-results.json`. The earlier library assertion
failure remains in `/tmp/vary-desktop-default-back-1.log` and the successful
rerun is `/tmp/vary-desktop-default-final-lib.log`.

Windows, Linux and Windows-to-WSL2 CI, installed-client reload behavior and
installer/signing acceptance are not established by these local checks. No
commit, push, installed-application launch or replacement was performed.
