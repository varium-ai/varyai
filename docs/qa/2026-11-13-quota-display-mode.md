# Tray quota display mode

The reported Antigravity screenshots compared Vary AI's used percentages with
Magpie's remaining percentages. An upstream remaining fraction of 0.968 maps to
3.2% used and 96.8% remaining; a rounded remaining display may show 97%. A fully
available window maps to 0% used and 100% remaining. No change to upstream quota
conversion is required for those readings.

The tray now defaults to remaining percentages when there is no saved display
preference. Explicit used/remaining preferences remain respected. Percentages
retain their existing decimal precision. This React change applies to macOS and
Windows.

A new test failed before changing the default. It verifies all five synthetic
windows from the comparison, including an independent Gemini 4 window at 0%
remaining. Switching to used shows that same window at 100% and the Gemini week
at 3.2%, and saves the explicit preference. Existing tests cover restoring the
used preference. The renderer does not cap the number of returned quota windows.
This does not establish why a live account response omitted an independent
window; no live account quota request was made for this check.

Checked on macOS arm64 with Node 24.11.0 and pnpm 10.12.3, on
`codex/intent-recognition` based on `c1fc6ab8`, with pre-existing working-tree
changes preserved. Relevant implementation: `TrayQuotaPanel.tsx`; regression:
`tests/components/TrayQuotaPanel.test.tsx`; contract:
`docs/architecture/tray-panel.md`.

- `pnpm typecheck`: passed.
- Seven focused quota/tray test files: 56 tests passed.
- `pnpm test:unit`: 4,136 tests passed across 310 files.
- `pnpm build:renderer`: passed.
- `pnpm format:check`: passed.
- `git diff --check`: passed.

Command logs are `/tmp/vary-quota-display-{unit,build,format}.log`; source/test
fingerprints in `/tmp/vary-quota-display-sources.json` remained unchanged after
the checks. No Rust source was changed for this display correction. Cargo checks, native application
installation, live account comparison and cross-platform CI were not run for
this correction. No commit or push was made. These results do not establish
acceptance of the other unfinished quota-control changes in the working tree.
