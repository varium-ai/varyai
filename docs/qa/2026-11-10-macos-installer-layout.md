# macOS installer layout verification

For acceptance of the combined submission, see the
[consolidated pre-submit record](2026-11-10-pre-submit-verification.md).
The results below describe this task's intermediate working trees.

Date: November 10, 2026 (Asia/Shanghai).

## Change under review

The macOS DMG now uses a white 660 x 420 Finder window with the Vary AI app
on the left, an Applications shortcut on the right, a curved arrow, blue
accent strokes, and the instruction `Drag "Vary AI" to the Applications folder`.
The background is a 1320 x 784 PNG at 144 DPI, below Finder's 28-point title bar.
Its editable source is `scripts/render-dmg-background.swift`.

`src-tauri/tauri.conf.json` supplies the background and icon positions. Both
`package-macos.sh` and `scripts/notarize-macos.sh` use
`scripts/create-macos-dmg.mjs` to reuse Tauri's generated create-dmg tooling
and that configuration. The helper copies the existing signed/stapled app
without rebuilding it and verifies the new image before replacing an older
artifact. Temporary staging is removed on success and failure.

The change also adds `scripts/tests/macos-dmg.node.mjs`, its package command
and CI step, updates the notarization test fixture, and documents packaging
in `docs/guides/local-macos-build.md` and `docs/development.md`.
This is a macOS disk-image presentation change; it adds no application logic
or Windows installer behavior.

## Environment and boundaries

- Base revision: `82204c3f`, with the packaging working-tree changes above.
- Host: macOS 15.3.1 (24D70), Apple Silicon.
- Toolchain: Node 24.11.0, pnpm 10.12.3, rustc 1.98.1, Cargo 1.98.1.
- Existing and concurrent routing/backend, version, and documentation edits
  were retained and are outside this change. Broad checks exercise the shared
  working tree; they do not establish acceptance for later concurrent edits.
- No app was installed, launched, or replaced in `/Applications`. No release
  was uploaded, committed, or pushed. Signing, Apple notarization, updater
  acceptance, and remote cross-platform CI were not performed.

## Packaging checks

The five new regression tests failed before implementation and then passed.
They cover the native background/layout contract, shared repackaging settings,
preservation of the source app, verified replacement, failure cleanup, missing
Tauri tooling, and use of the helper by both shell entry points.
The existing notarization suite passed all 21 tests after its fixture was
updated to stub the new helper; no test mounts an image or contacts Apple.

An image was produced from an existing debug app with:

```bash
LC_ALL=C LANG=C LC_CTYPE=C node scripts/create-macos-dmg.mjs \
  'src-tauri/target/debug/bundle/macos/Vary AI.app' \
  'src-tauri/target/debug/bundle/dmg/Vary AI-layout-preview.dmg'
```

The image passed `hdiutil verify`, was mounted read-only at an isolated
temporary mount point, and was visually inspected in Finder. The background,
aligned app and Applications icons, visible names, arrow, instruction and
volume icon rendered correctly. The temporary mount was detached and the
preview image removed. This uses an existing debug binary and validates DMG
presentation, not the runtime or a distributable release build.

The first packaging attempt exposed an unsupported `C.UTF-8` locale in this
terminal's system Perl; using `C` allowed Tauri's packaging tooling to finish.
The first broad check attempt also encountered a full disk. Those failed
attempts are not passes; checks were retried after space became available.

The native Tauri path also completed successfully with the final DMG settings:

```bash
LC_ALL=C LANG=C LC_CTYPE=C pnpm tauri bundle --debug --bundles dmg --no-sign \
  --config src-tauri/tauri.no-updater.conf.json
hdiutil verify 'src-tauri/target/debug/bundle/dmg/Vary AI_0.3.0_aarch64.dmg'
```

The resulting debug image passed checksum verification. The `0.3.0` version
was a concurrent working-tree change, not part of the installer layout edit.

## Repository checks

| Command                                                            | Result                                                                                                                                                      |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                                                                                                                      |
| `pnpm build:renderer`                                              | Passed                                                                                                                                                      |
| `pnpm test:unit`                                                   | Passed: 281 files, 3588 tests                                                                                                                               |
| `pnpm format:check`                                                | Passed                                                                                                                                                      |
| `pnpm test:dev`                                                    | Passed                                                                                                                                                      |
| `pnpm test:macos-dmg`                                              | Passed: 5 tests; rerun after the final test-only refinement                                                                                                 |
| `pnpm test:trae-bridge`                                            | Passed                                                                                                                                                      |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                                                                                                                                      |
| `pnpm test:doubao-checkin`                                         | Passed                                                                                                                                                      |
| `pnpm test:agent-rewards`                                          | Passed                                                                                                                                                      |
| `pnpm test:qoder-codec`                                            | Passed                                                                                                                                                      |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                                                                                                                                      |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                                                                                                                            |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed, with an unused `Duration` import warning in the concurrently edited `proxy/intent/native.rs`                                                        |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Failed to compile the concurrently edited native intent tests: missing `Lease`, `Credentials`, `read_credentials`, and Native methods, among 80 diagnostics |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                                                                                                                      |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Incomplete: stopped this task's runner while it was waiting for another build's shared target-directory lock                                                |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Incomplete: the final bounded attempt reached compilation and was stopped after 30 seconds                                                                  |

Changed JavaScript, JSON, YAML and Markdown files also passed Prettier checks;
both modified shell scripts passed `bash -n`, and `git diff --check` passed.
The isolated notarization suite passed 21 tests. The test-only refinement
changed coordinate assertions to be independent of JSON property order.

The Rust test diagnostics are in existing native intent source files that this
installer change does not modify. No unrelated backend work was changed to
make these checks pass. Backend and cross-platform verification remain
incomplete; local packaging success is not release or CI acceptance.

Local command logs are under `/tmp/varyai-dmg-frontend-retry-*.log`,
`/tmp/varyai-dmg-rust-retry-*.log`, `/tmp/varyai-dmg-rust-final-contract.log`,
`/tmp/varyai-dmg-notary-retry.log`, and `/tmp/varyai-dmg-native-*.log`.
