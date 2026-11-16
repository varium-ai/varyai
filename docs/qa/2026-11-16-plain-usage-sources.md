# Plain usage source names

The source selector lists Codex alongside ordinary suppliers without the
Session imports group, suffix or explanatory paragraph. Source statistics and
request logs use the same plain `Codex` name without an unattributed-provider
subtitle. Other known client placeholders use their app names as well. Obsolete
presentation strings were removed from all four runtime locales.

This is a display change. Selection still sends the original backend name, and
refreshing the options retains the selected label. Real saved supplier names,
including session-like names, keep their original spelling. Usage totals,
storage and supplier attribution are unchanged.

## Verification

Thirteen focused assertions failed before the change; all 27 focused cases
passed afterward. Tests cover the flat menu, plain labels in all four languages,
selection and refresh behavior, unchanged costs and saved supplier names.
The recorded frontend inputs stayed unchanged throughout the final checks.
Unsupported Testing Library `exact` options were also removed from the source
selector tests and the concurrently added pagination tests; role queries already
match string names exactly, so their assertions retain the same meaning.

| Check                           | Result                         |
| ------------------------------- | ------------------------------ |
| `pnpm typecheck`                | Passed                         |
| `pnpm format:check`             | Passed                         |
| `pnpm build:renderer`           | Passed                         |
| `pnpm test:unit`                | Passed: 317 files, 4,223 tests |
| Isolated Chromium/WebKit checks | Passed: 32 cases               |
| `git diff --check`              | Passed                         |

Browser checks use synthetic records with the real selector and tables at
880/1440-pixel widths, light/dark themes and all four runtime languages. They
verify a flat option list, plain names, menu containment, the original backend
filter value, unchanged costs and clearing the filter. English light-menu and
dark-table screenshots were visually reviewed. The isolated Vite server used a
dynamic loopback port and was stopped after verification.

## Checked tree and limits

- Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`, with shared
  working-tree changes preserved.
- Host: macOS 15.3.1 (24D70), arm64; Node 22.23.5 and pnpm 10.12.3.
  Node differs from the exact 22.12.0 CI pin.
- Logs, browser results and source snapshots are under
  `/tmp/varyai-plain-usage-source-verification/`. `checked-tree.json` records
  frontend inputs; `scoped-source-hashes.json` records the 14 scoped source,
  test, locale and documentation files. The SHA-256 of its sorted compact JSON
  mapping is `2f430800e6ae34fb73ada90faf143b7d738b3bb4d6b24c38fde234f4bea56bf7`.
- No Rust, helper, manifest or lockfile changes were made for this presentation
  correction. Helper and production dependency audit results from the
  [provider-name verification](2026-11-16-provider-name-validation.md) apply to
  their unchanged inputs. Cargo checks were not repeated for this frontend
  change. Independently edited desktop Agent files are outside this verification
  scope, so earlier backend results do not establish acceptance of their newer
  state.
- The renderer implementation is shared by macOS and Windows. Installed-webview,
  Linux, Windows and remote CI checks were not run. No installed application or
  AI client was launched, replaced or restarted; no commit or push was made.

This report supersedes the grouped presentation in
[the earlier session-source report](2026-11-16-usage-session-sources.md).
