# Magpie provider upper-form alignment — 2026-11-10

Scope: every API preset, custom API providers, Codex, and the six supported
subscription providers. The implementation follows Magpie's existing upper form;
Advanced and the separate Endpoints section are excluded by request. No installed
application was launched, installed or replaced.

## Form behavior

- Existing API providers expose Name and an editable ID. ID uses Magpie's lowercase
  ASCII slug normalization and falls back to the saved ID when empty. Stored notes
  are retained, but the invented Notes row is removed.
- Saving an ID change atomically migrates owned routing-group members, classifier
  targets and auto-group exclusions with revision checks and collision rejection.
  A private execution identity preserves published Agent model aliases, ordering,
  conversation ownership and the immutable transport snapshot. Reusing a retired
  public ID creates a distinct owner. Storage failure rolls back every reference.
- New API providers use Magpie's single API-key field and Show/Hide control.
  Primary/preferred-key controls, Done, Add another key and routing-policy hints
  appear only in the saved editor. The storage hint names Vary AI's actual local
  database rather than Magpie's JSON path. Custom Name, ID, Base URL and model
  placeholders follow the reference; URL examples change with the protocol.
- Preset forms use their preset connection settings. New presets show the relevant
  account/region/plan fields; another instance can expose Name. Existing presets
  show Name, ID, accounts and model controls. Custom forms retain protocol detection,
  test-model controls, Base URL and icon editing. New forms use Add; existing forms
  use Save.
- New custom forms place the comma-separated model input and Fetch above the list,
  with a full-width filter below and 6px separation. Saved forms place filter/bulk
  actions above chips and add/fetch/test/preferences below them. Model search matches
  model IDs and names. The saved form has no invented selected-count row.
- Image-generation models have their own field. Chat filtering and bulk selection
  do not alter image choices. Image input alone does not classify a chat model as
  an image generator. Shared TypeScript/Rust fixtures cover explicit kind, output
  modalities, known model families and opaque vendor IDs. Agent chat publication
  and chat-model tests exclude image-generation models while retaining editor data.
- Model-footer actions share Magpie's `text action` appearance across API,
  Codex and subscription forms: 27px height, 12px text, the same 1px border,
  background and shadow. Disabled actions retain the same frame and use faint
  text. Browser computed-style checks covered all actions, including the native
  Names & levels toggle, so component variants cannot change their appearance.
- The extra Codex Agent row is removed. Dialog dimensions, field order, input
  heights, neutral palette, chips, spacing and fixed bars follow the reference.
  Accent colors follow the Vary AI theme after the brand-color follow-up below.

Reference source snapshot SHA-256:

- `internal/gui/assets/app.js`: `3f8b7916157b0549474addfa6adac35948a5168450f89ede383b1803b3d3ef8a`
- `internal/gui/assets/app.css`: `8d329c34592c06983c396fa2f99d5b52ffca53159566810f6b313f508e296af0`

The reference functions are `drawEditor`, `addFormModels`, `detectAPIs` and
`renderModels`, with `.models`, `.mbulk`, `.mfoot` and `.dialog.roomy` styles.
The implementation does not add unsupported TLS, pricing or endpoint capabilities.

## Verification

Test-first regressions reproduced mixed chat/image lists, ID-renaming failure,
reference rollback, unstable model ordering, vendor-mode Select none, and provider
names incorrectly matching every model. A later three-failure regression run
reproduced the incorrect new-provider account pool and missing placeholders
before the single-key form and examples were implemented. The final focused UI run passed 164 tests
across seven files; both backend rename regressions passed. Credentials and
transport assertions use synthetic fixtures and in-memory databases.

Browser verification used a temporary synthetic renderer harness in English.
Custom new/saved, preset new/saved, Codex and Copilot forms were inspected. ID
editing normalized `My New ID!` to `my-new-id` and showed the reference-migration
hint. The new model/filter gap measured 6px. Before the brand-color follow-up, selected-chip background was
`rgb(238, 237, 253)`, matching Magpie's light accent. Checks at 1280×900 and 390×844
showed no document horizontal overflow. A final empty-new-form inspection also
verified the single key field, Show/Hide behavior and per-protocol URL placeholders
at the default desktop viewport and 390×844. Dialog bars remained fixed while fields
scrolled. The preview tab, harness and temporary server were removed afterward.
Screenshots and detailed logs are under `/tmp/vary-magpie-form-complete/`.

Host: macOS 15.3.1 arm64; Node 24.11.0; pnpm 10.12.3;
Rust/Cargo 1.98.1 stable. Base revision:
`23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, plus the working-tree changes.
Existing importer, subscription, key-pool, Agent connection and locale edits in
this shared checkout were preserved. Final checks apply to this tree.

The final full frontend attempt reported one 5,000ms timeout in the existing
`PiProviderForm` thinking-map test (3,651 other tests passed). Its isolated rerun
passed in 1.24s. No Pi source, timeout, concurrency setting or assertion was
changed. The unmodified standard full-suite rerun passed all 3,652 tests; the timeout is retained in
`unit-acceptance.log` and the isolated result in `pi-recheck.log`.

| Command                                                            | Final result                                        |
| ------------------------------------------------------------------ | --------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                              |
| `pnpm format:check`                                                | Passed after the final button styles                |
| `pnpm build:renderer`                                              | Passed after the final button styles                |
| `pnpm test:unit`                                                   | 283 files, 3,652 tests passed on the standard rerun |
| `pnpm audit --prod`                                                | No known vulnerabilities                            |
| `pnpm test:dev`                                                    | Passed                                              |
| `pnpm test:macos-dmg`                                              | Passed                                              |
| `pnpm test:trae-bridge`                                            | Passed                                              |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                              |
| `pnpm test:doubao-checkin`                                         | Passed                                              |
| `pnpm test:agent-rewards`                                          | Passed                                              |
| `pnpm test:qoder-codec`                                            | Passed                                              |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                              |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                              |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | 6,258 passed, 19 ignored                            |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                              |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                              |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | 6,421 passed across 16 binaries, 19 ignored         |

Rust checks set `CARGO_INCREMENTAL=0` to limit generated build-cache growth after
an earlier build exhausted local disk space. Package build artifacts were cleaned
with Cargo; source and installed applications were preserved. The macOS test
linker emitted an `__eh_frame` size warning; compilation and tests succeeded.

Final code-manifest SHA-256:
`d1e6f3797ceeb0a14ff81a84479a236f578e4d3ab3e332a354bbd3d7b12d55ce`.
The path/hash manifest is `source-manifest-final.txt` in the log directory. It
covers source, tests, helpers and selected build/test configuration. The final
button change only affects CSS; its build/format and 102 focused tests passed.
All backend and helper checks apply to unchanged files at this manifest.

No commit or push was requested. Linux/Windows/macOS remote CI and the Windows
WSL2 contract have not been run for this uncommitted tree; cross-platform CI
acceptance remains incomplete. Installer, signing, notarization and live provider
acceptance are outside these checks.

## Brand-color follow-up

At the user's request, the form now inherits Vary AI's shared accent and focus
variables instead of overriding them with Magpie purple. Save/Add buttons, links,
selected chat/image chips, enabled switches and account/key selection use the
same brand accent. Soft selection backgrounds derive from that accent. Neutral
surfaces, layout and model-action button geometry are unchanged.

Browser computed-style checks verified light `#3278ff` and dark `#78a8ff`, with
focus tokens `219 100% 60%` and `219 100% 74%`. Save, model chips, switches and key
selection matched the shared theme. The dark primary text also uses the shared
contrast color. English screenshots are `/tmp/vary-provider-brand-light.png`
and `/tmp/vary-provider-brand-dark.png`; the temporary harness and server were
removed after inspection.

This follow-up changes only `providers.css` and this record. `pnpm format:check`,
`pnpm build:renderer` and `git diff --check` passed on the final CSS. Existing
behavioral and backend test results above precede this color-only follow-up;
those suites were not repeated. No application was installed or launched.

The subsequent key-row alignment and removal of the extra API allowance field
are recorded in [Magpie API-provider key rows](2026-11-10-magpie-key-rows.md).
