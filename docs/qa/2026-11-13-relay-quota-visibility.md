# Relay quota card visibility

## Problem and behavior

The tray showed custom relay providers such as synthetic New API sites as empty
cards saying that a subscription had no quota interface. The Rust reader already
probes `/v1/usage`, followed by `/api/usage/token`, using the selected saved key.
The provider-list view already omitted unsupported replies, but the tray kept
both their cards and provider headings.

The tray now omits an unsupported relay's card and heading together. A reported
zero balance stays visible. Loading, explicit key rejection and temporary query
failure remain visible for retry. Successful prior readings survive temporary or
unrecognized replies with a stale status; key rejection clears the old reading.
If all relay candidates are unsupported, the ordinary empty state is displayed.

API provider views share the existing revision-scoped React query cache through
`src/lib/query/apiProviderQuota.ts`. The tray keeps query observers mounted when
cards are hidden, allowing the header's refresh to retry them. A missing-interface
result lasts six hours in renderer memory. Reopening the tab does not extend the
original observation's expiry. At expiry an active tab probes again; closing the
tab stops polling. A changed saved revision creates a fresh query. Provider
configuration and keys are not removed when cards are hidden.

Reference: Magpie `c5b9e448a8bc9bc2a1bf6704e5d8bb3384d084a6`,
`internal/provider/balance_probe.go`, `balance.go`, `balance_probe_test.go` and
`internal/gui/assets/app.js` (`renderPanelQuota`). This change aligns unsupported
card visibility and missing-interface expiry using Vary AI's existing probe; it
does not introduce Magpie's custom balance-query configuration form or persistent
probe file.

## Verification

Five new regression cases failed before the implementation. The focused set of
four provider/tray test files now passes 53 tests. Tests cover unsupported cards
and headings, a real zero balance, initial errors and key rejection, preservation
of prior data, the all-hidden empty state, explicit retry, cached reopening,
provider revision changes and six-hour expiry across tab reopening. Fake timers
also verify that unmounting stops automatic requests. All fixtures are synthetic;
no live supplier credentials or quota requests are used.

Host: macOS arm64; Node 24.11.0, pnpm 10.12.3. Branch:
`codex/intent-recognition`, based on `c1fc6ab8`, with unrelated working-tree edits
preserved. Final implementation/test fingerprints are recorded in
`/tmp/vary-relay-quota-sources.json`. Command logs use the
`/tmp/vary-relay-quota-` prefix.

- `pnpm typecheck`: passed.
- Focused provider/tray suite: 53 tests passed.
- `pnpm build:renderer`: passed.
- `pnpm format:check`: passed. The five changed source/test files also passed an
  explicit Prettier check.
- `pnpm test:unit`: 4,123 passed and 30 failed across 311 files. Failures are in
  `AgentModelVisibility.test.tsx`, `SharedModelPicker.test.tsx` and
  `SharedModelPickerBranding.test.tsx`, involving model-picker roles, labels and
  branding. Those areas were being edited separately during the run and were
  not modified for this correction. The first file subsequently passed all
  eight tests in an isolated run. The full suite is not accepted as passing;
  see `/tmp/vary-relay-quota-final-unit.log` and
  `/tmp/vary-relay-quota-visibility-check.log`.
- `git diff --check`: passed.

The six recorded quota source/test fingerprints matched after the complete
frontend run. No quota test file failed in that run.

No Rust source changed for this correction. Cargo checks, cross-platform CI,
installer checks and live-account acceptance were not run for this correction.
No running native application was launched, installed or replaced. No commit or
push was made. These frontend results do not establish acceptance of the other
unfinished quota-control backend changes in this working tree.
