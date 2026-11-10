# Provider icon field placement

Date: 2026-11-10 (Asia/Shanghai).
Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42`, with existing unrelated changes preserved.
Host: macOS, Darwin 24.3.0, arm64; Node v24.11.0 and pnpm 10.12.3.

The existing custom API provider icon field now appears immediately above the
provider name. The component, visibility condition, handlers and disabled state
are unchanged. Existing name autofocus remains explicit. Existing editor, icon,
creation, focus and save tests were rerun.

| Check | Result |
| --- | --- |
| Focused tests: RoutingUpstreamsPanel, ProviderAddSheet, ProviderParity, ProviderEditorModels | Passed: 106 tests |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | Passed: 3691 tests across 287 files |
| `git diff --check` | Passed |

Tested SHA-256 of `src/components/providers/ProviderEditorDialog.tsx`:
`34c567a4fdd82860f7f42ec59ad2f326f22af72ac3e334fb896232bb06753269`.
Logs are under `/tmp/vary-icon-position-*.log`.

The follow-up question about the new custom provider's comma-separated model-ID
input was checked against Magpie's reference `internal/gui/assets/app.js`:
`drawEditor` creates the input at line 9969, `addFormModels` places it next to
Fetch at line 10848, and `sync` fills it from chip selections at line 10859.
This input already exists in the reference and was not changed by this task.

This follow-up changes one renderer component and documentation. Backend,
helper-script and production dependency checks from the preceding
[batch deletion verification](2026-11-10-provider-batch-deletion.md) were not
repeated for this field move. No application launch, installation, commit or push
was performed. Remote CI and cross-platform release acceptance were not run.
