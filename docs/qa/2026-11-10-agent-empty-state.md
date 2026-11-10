# Workspace spacing and tool discovery empty state

## Scope

- Reduce the sidebar brand offset below the Windows native titlebar from 48 to 24 CSS pixels. Other native-titlebar platforms use the same compact default. Preserve the macOS 24-pixel drag strip and 24-pixel brand offset below it.
- Replace the unstructured empty-list paragraph with a compact icon, heading, explanatory copy, and visible browse/rescan actions. Reuse the existing tool disclosure and installation queries; browsing does not install or connect anything.
- Keep pending detection and failed detection distinct from an empty installation. Rescanning can replace the empty state with detected tool rows without reloading the page.
- Preserve the existing installed-tool cards and unrelated in-progress changes. Update the empty-state strings in English, Simplified Chinese, Japanese, and Traditional Chinese.

## Regression tests

Tests were added before implementation. The initial focused run reported four failing assertions: native-titlebar spacing, workspace and tray discovery actions, and an empty prompt incorrectly displayed after detection failed. After the changes, the focused navigation, spacing, and Agent tests passed (165 tests).

## Renderer inspection

An isolated Vite entry rendered the actual navigation and Agent components with synthetic API responses and no Tauri backend. It was removed after inspection. No installed application was launched or replaced.

- Chromium, 1150 x 780: native-titlebar layout brand top = 24 CSS pixels; simulated macOS drag-strip layout brand top = 48 CSS pixels.
- Light and dark desktop empty states: icon/copy/actions align horizontally; empty content height approximately 118 CSS pixels with English copy.
- 600-pixel narrow workspace and 360-pixel tray layout: controls wrap without horizontal overflow.
- English, Simplified Chinese, Japanese, and Traditional Chinese tray layouts: no horizontal document overflow.
- No browser page errors in the isolated preview. Native Windows titlebar and WebView2 behavior still require Windows verification; Chromium inspection is not Windows acceptance.

## Verification environment

- Date: 2026-11-10.
- Host: macOS / Darwin, arm64.
- Base revision: `23a4b2be`; checks ran against the current working tree, which includes unrelated pre-existing edits.
- Toolchain: Node.js 24.11.0, pnpm 10.12.3, Rust 1.98.1.
- Check logs and SHA-256 manifest of the eight relevant source/test files: `/tmp/vary-agent-empty-checks/` (local, non-deliverable).

## Results

- `pnpm typecheck`: passed.
- `pnpm build:renderer`: passed.
- `pnpm test:unit`: passed, 288 files / 3708 tests.
- `pnpm format:check`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml`: passed, 6281 tests / 19 ignored.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, 6444 tests (6281 library + 163 integration) / 19 ignored. The macOS linker emitted a compact-unwind size warning for the library test binary; the build and tests completed successfully.
- Helper suites: `pnpm test:dev` (8), `pnpm test:macos-dmg` (5), `pnpm test:trae-bridge` (57), `pnpm test:workbuddy-checkin-auth` (5), `pnpm test:doubao-checkin` (13), and `pnpm test:agent-rewards` (9) all passed.
- `pnpm test:qoder-codec`: 2 passed / 1 skipped (installed-Qoder codec test); no installed client was exercised for this UI change.
- `pnpm audit --prod`: no known vulnerabilities found.
- Changed test/document Prettier checks and `git diff --check`: passed. The checked source/test SHA-256 manifest still matched after all checks completed.

No commit, push, installer, signing, release, or remote CI verification is included. Linux, Windows, and WSL2 CI acceptance remains unverified.
