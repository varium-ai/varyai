# Native select styling

Checked tree: `26a69df7056461c4f64a04c6739f29a76fed5042` plus this task's
shared select CSS, global import, provider/routing CSS and development guide
changes, together with the preceding uncommitted onboarding import-order change.
Host: macOS 15.3.1 arm64; Node 24.11.0, pnpm 10.12.3, Rust 1.98.1.

Single-choice native selects now share a flat surface, theme-aware chevron,
reserved arrow space, text truncation, disabled feedback and keyboard focus.
Existing page-specific heights remain. The compact login proxy's existing SVG
indicator and sizing now apply outside the provider dialog too. Sized and
multiple list boxes retain native rendering. Forced-color mode restores the
system indicator for ordinary selects. The CSS applies to macOS and Windows.

## Browser regression

Before implementation, a local fixture using the real application styles
reported `appearance: auto` and no custom indicator in onboarding, provider
authentication, connection presets, gateway, check-in and routing controls.
The compact login proxy already used a separate SVG indicator.

After implementation, these ordinary controls report `appearance: none`, a
single chevron and 36px of reserved end padding. Onboarding, provider, check-in
and routing heights remain 42px, 27px, 30px and 28px respectively. Browser checks
verified light/dark colors, a 2px keyboard focus outline, protocol selection,
disabled controls and ellipsis in a 190px control with no horizontal page
overflow. The compact proxy remains 24px high with one SVG indicator; the
multiple list box remains native. Preview fixtures load no real account data.

Screenshots are retained in the chat's visualization artifacts. These checks
used Chrome and application CSS fixtures, not the packaged desktop application.
Windows WebView2, macOS WebKit and forced-color mode still require native visual
acceptance; no application was installed, replaced or launched for these checks.

## Local verification

- Focused onboarding, import, login, preset, gateway and check-in tests: 70 passed in seven files.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `pnpm test:unit`: 3,533 passed in 280 files.
- Launcher, TRAE bridge, WorkBuddy authentication, Doubao check-in, agent rewards and Qoder codec helper tests: passed; the optional installed Qoder codec test was skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`: 6,165 passed, 18 ignored.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`: 6,165 library and 163 integration tests passed; 18 library tests ignored.
- `git diff --check` and formatting checks for the updated documentation: passed.

The macOS test linker emitted an `__eh_frame` size warning; linking and both
test runs completed successfully. Final edits after verification only updated
this report, and the source/guide fingerprints below were checked again.

## Checked SHA-256 fingerprints

| File                                     | SHA-256                                                            |
| ---------------------------------------- | ------------------------------------------------------------------ |
| `src/index.css`                          | `4fff773731cdb45da494bdd4cf78a0b418faad13598781451faa7a1327717084` |
| `src/components/ui/native-select.css`    | `055a60f78aef87b3dd2713d0d6613781dcb68dd7f0a7ff7d4039bc32e9281f7d` |
| `src/components/providers/providers.css` | `7cc347c91343ca774dfef2dd78d8ec5f6fb91310e5704d54441dba84f949bee6` |
| `src/components/routing/routing.css`     | `73e9d9cef04f06c3b7b3f172d9590ec9b70ee0af0f302841c37ad4a5d0a4e749` |
| `docs/development.md`                    | `7f17885408436e27b11b4fe916c89dfc084f7baf22c8fb880206720d334b2651` |

No commit, push or remote CI was performed. Local checks do not establish
cross-platform CI acceptance.
