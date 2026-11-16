# Editing providers with unchanged duplicate names

The editor previously excluded the current provider ID but still rejected an
unchanged name when another historical record had the same normalized name.
The database already allowed that edit, so the frontend prevented valid updates
to other fields before a save could reach the backend.

The editor now compares the draft name with the saved original name after
trimming outer whitespace and lowercasing. An unchanged normalized name does
not trigger a local collision error. A new provider, a copy or an actual rename
still checks every other provider ID. Backend collision errors remain visible,
including conflicts introduced after loading the list. Copy drafts do not inherit
the original provider's unchanged-name exception.

## Regression evidence

Both new regression cases failed before the fix. The final focused run passed
148 tests across the provider panel, provider parity and Codex editor suites.
Coverage verifies opening historical duplicates, preserving names with case or
whitespace differences, rejecting a different conflicting name, reverting that
name, saving changed model selections and rejecting a same-name copy.

The isolated browser fixture contains two identically named synthetic providers
and a third distinct name. All 16 Chromium/WebKit cases passed across light/dark
themes and four runtime languages. They check the initial enabled Save button,
a conflicting rename, blocked Enter submission, reverting to the original
normalized name, saving changed models and retaining the same provider ID.
English screenshots were visually reviewed. The preview used a dynamic loopback
port and was stopped after verification.

| Check                   | Result                         |
| ----------------------- | ------------------------------ |
| `pnpm typecheck`        | Passed                         |
| `pnpm format:check`     | Passed                         |
| `pnpm build:renderer`   | Passed                         |
| `pnpm test:unit`        | Passed: 317 files, 4,225 tests |
| Focused component tests | Passed: 148 tests              |
| Isolated browser checks | Passed: 16 cases               |
| `git diff --check`      | Passed                         |

## Checked tree and limits

- Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`, plus the shared
  working-tree changes. Unrelated edits were preserved.
- Host: macOS 15.3.1 (24D70), arm64; Node 22.23.5, pnpm 10.12.3.
  The local Node version differs from the 22.12.0 CI pin.
- Logs and source snapshots are in
  `/tmp/varyai-provider-name-edit-verification/`. `checked-tree.json` records
  frontend check inputs. `scoped-source-hashes.json` records the changed source,
  test and documentation files and the unchanged backend validation and tests.
  SHA-256 of its sorted compact JSON mapping:
  `545102c558b995cb2d4c3a2ab595028e2f044dbfc0af407cae4b7151b16dda37`.
- The backend name-validation implementation and regression tests are unchanged
  from the successful [provider-name verification](2026-11-16-provider-name-validation.md).
  Helpers, manifests and lockfiles also retain their checked contents. Those
  earlier results remain applicable to those inputs; Cargo checks were not
  repeated for this frontend correction. Concurrent native Agent changes are
  outside this verification scope.
- The renderer code is shared by macOS and Windows. Installed-webview, Linux,
  Windows and remote CI checks were not run. No installed application or AI
  client was launched, replaced or restarted, and no commit or push was made.
