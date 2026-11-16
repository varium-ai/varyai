# Usage session source labels

The presentation described below was subsequently replaced by
[plain usage source names](2026-11-16-plain-usage-sources.md). This report retains
the verification evidence for the earlier grouped presentation.

The Data Center source selector previously displayed session-import placeholders
such as `Codex (Session)` alongside supplier names without explaining the
difference. Imported local usage is now grouped under **Session imports**, with
labels such as `Codex · Session import`. Source statistics and request logs also
identify these entries as **Provider unattributed**.

Classification requires the internal placeholder ID and its backend fallback
name together, including the legacy Pi gateway fallback. Saved suppliers with
similar names retain their names. A name shared by a saved supplier and a session
placeholder keeps its raw filter label because that filter includes both.
Selection continues to send the original backend name, and a selected import
retains its readable label after leaving the current data range. Usage storage,
attribution, totals and provider configuration are unchanged.

## Checked tree and environment

- Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`.
- Branch: `codex/intent-recognition`; uncommitted working-tree changes.
- This correction covers `UsageDashboard`, `ProviderStatsTable`,
  `RequestLogTable`, the shared source-label helper and component, four locale
  files, four component test files, and usage/troubleshooting documentation.
- The final checks also include the preceding quota UI changes in this tree.
  The 21 changed source and test files are recorded in
  `/tmp/varyai-usage-source-verification/checked-tree.json`.
- SHA-256 of the sorted, compact JSON mapping of those file paths to their
  SHA-256 checksums:
  `46c2d144bda611000655af225413a8e83184727ef9c896faf10791ba86aa72e6`.
- Host: macOS 15.3.1 (24D70), arm64; Node 22.23.5 and pnpm 10.12.3.
  The exact CI Node 22.12.0 version was not used.

## Verification

Three regression assertions failed before implementation: dropdown
classification, request-log labeling and the provider/source statistics header.
The shared label component was also absent at that point. All 27 focused tests
passed after implementation, including 18 additional test cases covering the
reported behavior, all four runtime languages, real provider names, legacy Pi
records, mixed name filters, selection retention and the `all` sentinel.

| Check                                | Result                         |
| ------------------------------------ | ------------------------------ |
| `pnpm typecheck`                     | Passed                         |
| `pnpm format:check`                  | Passed                         |
| `pnpm build:renderer`                | Passed                         |
| `pnpm test:unit`                     | Passed: 316 files, 4,213 tests |
| `git diff --check`                   | Passed                         |
| Isolated Chromium and WebKit preview | Passed: 32 cases               |

The browser cases cover 880/1440-pixel widths, light/dark themes and all four
runtime languages. They use the real selector and tables with synthetic usage,
confirm menu containment, select the localized Codex import, verify the raw
backend filter value and unchanged cost, then clear the filter. English menu
screenshots in both themes and the dark statistics table were visually reviewed.
The isolated Vite server used a dynamically allocated loopback port and was
stopped after verification. Logs are in
`/tmp/varyai-usage-source-verification/`.

Rust, helper, manifest, lockfile and CI inputs have no changes from the prior
[quota verification](2026-11-16-tray-quota-magpie.md). Its passing Cargo
check, library tests, formatting, Clippy, integration tests, helper checks and
production dependency audit are reused for those unchanged inputs; they were
not rerun for this renderer-only correction. Rust and Cargo were 1.98.1.

No installed application or client was launched, replaced or restarted. No live
provider data or credentials were used. The renderer implementation is shared
by macOS and Windows, but these local checks do not establish installed-webview,
Linux, Windows, packaging or release acceptance. No commit or push was made;
remote CI has not run for this working tree.
