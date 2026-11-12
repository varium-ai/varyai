# Model picker identity hints and provider icons

## Reference and behavior

Same-name model hints follow Magpie revision
[`138d806f7a04559015f5591e9540522c917829bf`](https://github.com/yetone/magpie/tree/138d806f7a04559015f5591e9540522c917829bf),
specifically `modelTells`, `openPicker`, `tellTag` and `renderList` in
`internal/gui/assets/app.js`, and `.pop li .mtell` in `app.css`.

Models with equal displayed names in the same provider group retain separate
rows. Their public references have common leading and trailing path segments
removed to produce a muted hint, such as `gs` and `gs-2`. If shortening would
leave ambiguous hints, the complete references are used. Unique labels and
equal labels in different groups receive no hint. Hints remain available when
filtering, and search includes the public references.

The Agent catalog adds a `reference` field for this display purpose, while
retaining its existing `id`, name, capability metadata and provider group.
Native client configuration, persisted routing aliases and upstream request
model IDs are unchanged. The routing classifier picker supplies the equivalent
reference without changing its encoded selection value. The Rust change is
shared by macOS and Windows.

Provider icon edits were already saved, published and followed by catalog
query invalidation. The model row only read `logo`, however, while the catalog
supplied the provider image as `icon`. Rows therefore kept the generic box.
Rows and the provider rail now reuse `UpstreamIcon`, supporting built-in icons,
uploaded images, letter icons and failed-image fallback. Explicit native model
logos retain priority, and routing groups retain their routing mark. This fix
changes only renderer code; the verified Rust files remained unchanged.

## Verification

- Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`, with unrelated
  working-tree changes preserved.
- Host: macOS 15.3.1, Apple Silicon.
- Node 24.11.0, pnpm 10.12.3, rustc 1.98.1, cargo 1.98.1.
- Source hashes, working-tree fingerprint and outcomes:
  `/tmp/vary-model-picker-verification.json`.
- Logs: `/tmp/vary-model-tells-*.log`, `/tmp/vary-model-icons-*.log` and
  `/tmp/vary-model-picker-unit-final.log`.

Test-first identity regressions initially had four failures; the provider-icon
regressions initially had two failures. Subsequent tests cover common ID path
segments, ambiguous shortened hints, unchanged selection values, filtered
copies, built-in icon changes, uploaded image changes, failed images, letter
icons and native-logo priority. Fixtures use synthetic data and in-memory
storage and do not print credentials.

| Check | Result |
| --- | --- |
| Final core picker, Agent and icon regression suites | 261 passed across 5 files |
| Final `IntentRoutingPanel.test.tsx` rerun | 98 passed |
| `pnpm typecheck` | Earlier runs passed; latest rerun failed in independently edited release-notes tests, described below |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| Complete `pnpm test:unit` rerun | All 302 files and 4,019 tests passed before subsequent independent edits |
| Development, DMG, bridge, model-default, WorkBuddy, Doubao, rewards and Qoder helper tests | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed, no known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `CARGO_INCREMENTAL=0 cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `CARGO_INCREMENTAL=0 cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `CARGO_INCREMENTAL=0 cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,338 passed, 2 failed, 19 ignored |
| `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Same library failures; all 164 integration tests passed |
| `git diff --check` | Passed |

Earlier frontend runs encountered notarization fixture failures and four
routing-group Actions-menu failures while independent working-tree updates
were in progress. The standalone notarization suite subsequently passed all
21 tests, and the final routing-panel rerun passed all 98 tests. The complete
frontend rerun passed. The picker implementation files remained unchanged;
independent routing-page and guide edits detected during final bookkeeping
received an additional routing-panel, typecheck, build and formatting rerun.

That additional routing-panel run passed 98 tests, and the renderer build and
format check passed. The additional typecheck reported TS2305 in
`tests/components/AppUpdateDialog.test.tsx:13` and
`tests/lib/releaseNotes.test.ts:7` because `fetchReleaseHistory` was no longer
exported from `src/lib/releaseNotes.ts`, plus TS7006 for the resulting untyped
`release` callback at `tests/lib/releaseNotes.test.ts:41`. These are separate
release-notes changes; this task did not edit those files. Further independent
routing-page and guide edits were also observed after the rerun. The unchanged
picker and Rust hashes establish this task's checked scope; these results do
not establish acceptance of the combined, still-changing working tree.

The successful helper commands were `pnpm test:dev`, `pnpm test:macos-dmg`,
`pnpm test:trae-bridge`, `pnpm test:model-default`,
`pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
`pnpm test:agent-rewards` and `pnpm test:qoder-codec`.

The remaining Rust library failures are the previously recorded model-label
assertions in
`reasoning_catalog_projects_into_native_agent_files_and_restores_them` and
`provider_backup_imports_saved_reasoning_preferences_into_shared_agent_catalogs`.
They expect older provider-suffixed labels and do not exercise the new picker
metadata or icon renderer. The new backend picker-reference test and the
existing provider-icon persistence/catalog test passed.

Rust validation initially hit disk exhaustion, then a missing generated
fingerprint. Failed and inactive crate incremental caches were removed, and
validation was retried with incremental compilation disabled. The completed
retry compiled and ran the library and integration targets; these environment
errors are not counted as successful checks.

## Visual verification

An isolated Vite preview on a dynamic port rendered the actual picker with
synthetic providers. Browser checks confirmed that changing one provider's icon
updates its rows while retaining the other provider's icons; an uploaded SVG
loaded in all three relevant rows and in the provider rail. Light and dark
themes retained the identity hints. Selecting the copy still returned its
original internal routing alias.

The final English preview is `/tmp/vary-model-tells-preview/icons-light.png`;
the dark preview is `/tmp/vary-model-tells-preview/icons-dark.png`. The preview
server was stopped afterward. No installed application was launched, replaced
or installed, and no real provider credentials or client configuration were
used for these checks.

Linux, Windows, WSL2 and remote CI were not run. Whole-repository acceptance
remains incomplete while the Rust and release-notes typecheck failures remain.
No commit or push was made.
