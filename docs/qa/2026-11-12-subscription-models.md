# Subscription model discovery and form parity

Confirming a Codex subscription could leave the visible account unchecked and skip automatic model loading. The backend had merged a native login and a managed subscription into one row with proven aliases. Confirmation selected the managed ID, while the editor still rendered the native representative. The readiness check searched for the selected ID as a primary row ID and found no match.

The editor now projects the explicitly selected credential source onto that proven identity row. Readiness comes from the selected managed credential's own status. A ready native alias cannot make an expired managed credential ready. The confirmed managed source remains selected, its allowance is read under that ID, and the existing missing-catalog effect automatically fetches its models. Reconfirming a proven alias on an existing provider replaces that identity's prior source in place, preserves unrelated accounts, and retains saved model preferences. Account identities are never merged by matching email addresses.

Codex and other subscription model toolbars now use the API form's shared `ProviderCheckbox` and `pv-model-select-all` styling. One tri-state checkbox replaces the separate select-all/select-none buttons. Model refresh sits on the same toolbar and uses the shared form button styling. Bulk toggles affect visible chat models and preserve hidden models, image selections, model order and metadata. Refresh is disabled when the selected Codex credential is unavailable. Model parameters and catalog transport remain unchanged. The implementation is shared React code for macOS and Windows.

## Verification

- Base revision: `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`, plus the working tree; existing and concurrently updated unrelated work was preserved.
- Host: macOS 15.3.1, Apple Silicon; Node 24.11.0, pnpm 10.12.3.
- Implementation: `CodexProviderFields.tsx`, `SubscriptionProviderFields.tsx` and the Codex confirmation branch in `RoutingUpstreamsPanel.tsx`.
- Tests: `CodexProviderEditor.test.tsx`, `SubscriptionProviderFields.test.tsx` and `SubscriptionAddFlow.test.tsx`.
- Final checked-file hashes and results: `/tmp/vary-subscription-models-verification.json`.
- Logs: `/tmp/vary-subscription-models-*.log`.

The initial regression run failed five cases against the prior implementation while 60 existing checks passed. It covered both native-readiness values for a selected managed alias and the shared bulk-selection controls. A subsequent existing-provider confirmation regression failed before its binding replacement was fixed. Final coverage includes new and existing confirmed accounts, account-specific discovery and quotas, rejection of unavailable selected credentials, mixed/all/none checkbox states, filtered selection, hidden/image model retention, ordering and saved metadata.

| Check                      | Result                                                                                 |
| -------------------------- | -------------------------------------------------------------------------------------- |
| Final three focused suites | 3 files, 85 tests passed                                                               |
| `pnpm test:unit`           | 301 files, 3,997 tests passed; affected suites rerun after the final source adjustment |
| `pnpm typecheck`           | Passed                                                                                 |
| `pnpm build:renderer`      | Passed                                                                                 |
| `pnpm format:check`        | Passed                                                                                 |
| `git diff --check`         | Passed                                                                                 |

The full unit run overlapped a final type annotation and toolbar element-order adjustment. The final affected suites, type check, renderer build and formatting check passed afterward.

The isolated browser harness rendered the actual `ProviderEditorDialog` with a synthetic native/managed alias, mocked local auth status and synthetic model responses. The account remained checked, three models appeared automatically, the bulk checkbox showed a mixed state, and clicking it selected then cleared all models. The refresh action shared its row. Review included the light desktop view, a 420 px dark window and all four runtime languages at 420 px. Each locale showed all three model controls without horizontal dialog overflow and stayed within the viewport.

English preview: `/Users/guitar/.codex/visualizations/2026/11/12/01a1cdea-3782-7243-b4c7-9abdffdb0954/subscription-models-preview.jpg`.

Tests and browser checks used synthetic accounts and API responses. No real credentials or client configuration were read, and the installed application was not launched or replaced. Live authentication and vendor API acceptance were not exercised. Backend, helper-script, dependency-audit and remote CI checks were not repeated for this renderer-only change. The earlier two Rust model-label assertion failures remain documented in `2026-11-12-provider-add-page.md`; this report does not claim whole-repository or cross-platform CI acceptance. No commit or push was made.
