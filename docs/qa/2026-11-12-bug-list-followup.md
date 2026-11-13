# Bug list follow-up — 2026-11-12

The existing bug-list tab was inspected through computer-use without refreshing
it. The list initially contained 20 reports; a later read showed a new security
report in row 21. Attachments were viewed without downloading them. No account,
installed application or production database was modified. An accidental blank
row created during inspection was immediately undone; the list returned to 20
records before the separate, substantive row 21 appeared.

## New reports

| Row | Report                                                                                | Change and acceptance boundary                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| --- | ------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 15  | Request logs show VaryAI as a supplier                                                | Pi records the gateway adapter name, not the real upstream. New imports map this adapter to the Pi session placeholder. Historical detail logs with explicit Pi session provenance display `Pi (Session)`, even if a legacy provider record shares the adapter ID. Supplier statistics separate session detail from a real supplier with that ID. Historical rollups have no source field and retain their recorded attribution. Costs and credentials are unchanged. |
| 16  | Configured model names differ from statistics; used models are absent from the filter | Return distinct observed request and response IDs with model statistics. Offer those IDs and billing IDs in the filter, including historical rollup aliases. All usage queries match those three dimensions; costs remain grouped by billing model. Logs display the billing ID plus differing request/response IDs. Do not rewrite upstream protocol identifiers or invent usage for unused configured models.                                                       |
| 17  | Request-graph icons differ from supplier settings                                     | Add optional public source-provider metadata to trace candidates. Resolve the configured icon with that public ID while retaining immutable execution identity for scheduling, capacity observations and continuation. Old trace records retain their existing fallback.                                                                                                                                                                                              |
| 18  | Show/hide accounts appears ineffective                                                | Include configured supplier names in masking scopes on the request graph and classic routing-group panel. Nested scopes merge inherited names. Mask names in summaries, selectors and tooltips; retain models and configured brand icons. Initial-based fallback icons are masked. Showing accounts restores the original labels without saving configuration.                                                                                                        |
| 19  | A successful selected group appears to send to an unrelated failed supplier           | Reproduce concurrent trace branches being merged into the focused graph. Scope graph branches to the focused request's Agent, model and session. Keep multi-route rendering available for other stage consumers. This is a display isolation fix; it does not change upstream dispatch.                                                                                                                                                                               |
| 20  | Provider search lacks scope and appears incorrect                                     | Show search scope beside the field and in its accessible description. Search name, endpoint host and model IDs/display names. Vendor-mode sources search the active catalog. Display matching model IDs when a model caused a match, explaining why a supplier whose name lacks the query still appears. Whitespace-separated terms remain conjunctive.                                                                                                               |
| 21  | Generic `sk-` notice lacks redaction/restoration buttons                              | The user explicitly requested extending this previously restricted rule. Expose both actions and permit their persistence, runtime activation, span detection, preparation and audit projection. Restoration remains subject to existing supported-route and authority checks; ordinary redaction creates no mapping. Default allow remains unchanged. Regression covers both actions, local resolution, policy revision changes and unsupported-route rejection.     |

## Earlier reports and classic UI

The first round's implementation and evidence remain in
[the original report](2026-11-12-bug-list.md). The experimental routing-policy UI
was withdrawn by the other task; this follow-up verifies the retained classic
panel and preserves that work.

| Row | Report                                              | Current status                                                                                                                                                                                                                                                                                                                                                                          |
| --- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Invalid/excessive provider keys lack save feedback  | Existing validation retained; regression included in renderer suite.                                                                                                                                                                                                                                                                                                                    |
| 2   | Automatic discovery has no progress feedback        | Classic switch retains busy/loading feedback. Network latency is not represented as eliminated.                                                                                                                                                                                                                                                                                         |
| 3   | Clicking the discovery heading toggles it           | Plain heading and explicit switch retained.                                                                                                                                                                                                                                                                                                                                             |
| 4   | Gemini-compatible model discovery fails             | Same-origin/prefix catalog fallback retained; synthetic HTTP regression covers it.                                                                                                                                                                                                                                                                                                      |
| 5   | Claude CLI model list is incomplete                 | Published models are included in existing allowlists. The local installed CLI reports `2.1.322`. Acceptance of the updated build inside the user's client is still pending. The user asked to skip further investigation for now.                                                                                                                                                       |
| 6   | Account login cannot obtain available models        | Gemini's missing-quota-metadata handling is fixed. The recording confirms successful callback followed by a generic model-discovery failure, without a diagnostic code. Real Gemini/Antigravity authentication, entitlement and provider responses remain unverified; other platforms are not declared fixed from this recording. The user asked to skip further investigation for now. |
| 7   | Discovery switch is distant from its label          | Adjacent placement retained.                                                                                                                                                                                                                                                                                                                                                            |
| 8   | Selected Agent model resets                         | Receipt adoption from managed native selections retained, including legacy aliases and edited-file precedence.                                                                                                                                                                                                                                                                          |
| 9   | First template has no Add action                    | Visible disabled action and prerequisite retained.                                                                                                                                                                                                                                                                                                                                      |
| 10  | Export has no outcome feedback                      | Download-start feedback and safe failure reporting retained. Browser completion is not claimed.                                                                                                                                                                                                                                                                                         |
| 11  | Editor deletion is absent from removed groups       | Transactional archive/restore retained, with removed groups available while discovery is off.                                                                                                                                                                                                                                                                                           |
| 12  | Provider model removal is absent from manual groups | Unavailable saved members are marked and cannot be selected or re-added. Saved definitions remain available for review/removal; they are not silently deleted.                                                                                                                                                                                                                          |
| 13  | Only one member can be added                        | Sequential multi-member picker regression retained.                                                                                                                                                                                                                                                                                                                                     |
| 14  | Billing model text is truncated                     | Wrapping retained and now applied to the actual billing model, with distinct observed IDs below it.                                                                                                                                                                                                                                                                                     |

## Verification

Base revision: `ab5cf647b304d080370af4838f96778f063ca5f0` plus the shared,
uncommitted working tree. Host: macOS arm64, Node 20.20.2, pnpm 10.12.3,
Rust/Cargo 1.98.1. Rust checks use `CARGO_INCREMENTAL=0`. Other tasks share this
checkout and build directory; source hashes are compared after verification.

The checked changes are recorded in
[the follow-up source manifest](2026-11-12-bug-list-followup-sources.json).
Its canonical sorted-JSON SHA-256 is
`fab306d3419e5fd0256f79e655fb30a23d467050205f29650ac4c7e01a2e133c`.
The manifest contains 68 changed source/test files, including preserved work
from the other task. All 68 source hashes matched after the final checks, with no additional changed source files. HEAD remained the base revision above.

| Command                                                            | Result                                                              |
| ------------------------------------------------------------------ | ------------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                              |
| `pnpm build:renderer`                                              | Passed                                                              |
| `pnpm format:check`                                                | Passed                                                              |
| `pnpm audit --prod`                                                | Passed                                                              |
| `pnpm test:unit`                                                   | Passed: 299 files, 3,906 tests                                      |
| `pnpm test:dev`                                                    | Passed                                                              |
| `pnpm test:macos-dmg`                                              | Passed                                                              |
| `pnpm test:trae-bridge`                                            | Passed                                                              |
| `pnpm test:model-default`                                          | Passed                                                              |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                                              |
| `pnpm test:doubao-checkin`                                         | Passed                                                              |
| `pnpm test:agent-rewards`                                          | Passed                                                              |
| `pnpm test:qoder-codec`                                            | Passed                                                              |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed; repeated after the final test edit                          |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                              |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                              |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed after validation-fixture correction: 6,360 tests; 21 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed: 6,525 tests; 21 ignored across 16 binaries                  |

The new regressions were first observed failing for graph isolation, configured
name masking, public-source icon lookup, catalog search, model-filter options and
request/response display. Focused renderer checks then passed. Backend statistics
tests exposed a nonexistent rollup source-column reference; the SQL now accepts
source metadata only for detail logs. No migration or fabricated rollup source
was added. Classic masking expectations were updated to assert the new intended
privacy behavior, including restored labels when unmasked.

The first full renderer run found a search/sorting regression for older sources
with saved models but no cached catalog. Search now falls back to those saved
models and also covers subscription catalogs. The two targeted suites passed
all 66 tests, followed by the complete successful renderer run above. New
localization keys were checked at their actual root namespaces in all four
runtime languages.

After the user explicitly requested extending the generic `sk-` rule, its two
new action controls were first observed disabled in the updated regression.
All 45 Security Center renderer tests passed after the extension. Six focused
backend tests passed for persistence, detection, irreversible redaction, local
resolution and audit projection. The default HTTP gateway activation and policy
generation tests also passed. The final library run includes an additional
overlap regression preserving specific irreversible key rules.

The initial full library run exposed an older validation fixture that still
classified the newly permitted generic-rule actions as invalid. It now checks
malformed action names before entering the policy transaction. The normalization
fixture also verifies that generic-rule updates immediately publish their
effective runtime action, including the existing legacy block-to-redact mapping.

Remote Linux, Windows and macOS CI, including the conditional Windows-to-WSL2
contract, remain unverified for this uncommitted tree. No installed-client,
signing, installer or live-provider acceptance is inferred from unit tests.

The corrected command-write suite passed all 16 tests. The final library run,
full integration run, formatting, compilation and Clippy checks all passed.
Renderer checks were repeated after the final UI changes. The helper-test and
production-audit inputs remained unchanged after their successful runs.
`git diff --check` and the follow-up report/manifest formatting checks passed.
The existing macOS debug linker compact-unwind warning remains nonfatal. The
21 ignored tests comprise 19 existing live/service/corpus checks and two manual
timing fixtures from the other task; none were newly ignored for these fixes.
No commit or push was requested, and the external bug-list statuses were not
changed to imply installed-build acceptance.

## Commit preparation

The user subsequently requested one commit containing all current changes,
including the other chat's source and documentation work. Before staging,
all 68 changed source/test hashes still matched the verified manifest above;
no additional source files had appeared. The local checks recorded here
therefore also cover the source contents being committed. Historical reports
retain their original checked snapshots and results.

The additional development-guide check
`python3 -B scripts/tests/recover-shared-v23.test.py` passed all five tests on
the same macOS host. Staged and unstaged file inventories were reviewed for
generated binaries, debug files and local credentials; none were included.
The Chinese routing design document is retained under the user's explicit
translation request in the other chat. Remote platform CI remains unverified.
