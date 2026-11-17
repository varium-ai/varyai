# WorkBuddy existing conversations follow the default model

The user explicitly requested that changing the Vary AI default and restarting WorkBuddy also update existing conversations, without creating a new chat. This supersedes the earlier new-task-only selection behavior.

## Native contract and change

Read-only inspection of the installed WorkBuddy 5.11.3 and WorkBuddy AI 5.9.4 application resources established the same native conversation interface: `wb:conversations:list` accepts paginated local-transport queries; `wb:conversations:configSetModel` persists a conversation model through the native config manager; `wb:conversations:get` exposes the config-manager model for readback. Both versions persist model changes through their existing conversation patch mechanism. No installed file, account setting or live conversation was modified during this inspection.

The existing private-pipe helper now applies the ordinary new-task default and awaits a separate existing-conversation pass. It snapshots bounded paginated metadata before any writes, because model writes can reorder the list. Only local conversation IDs are passed to the native model setter, followed by exact model/identity readback. No prompt is sent, no messages are written, and no history, account credentials, reasoning settings or vendor files are edited by the helper. Cloud conversations retain their native selection.

Overlapping helper polls share the same pending operation. Successful application is remembered for that renderer and plan, allowing subsequent manual selection during the same run. A new renderer or a changed default applies the selected default again. Disconnect/restoration cancels further migration work and does not rewrite existing conversation selections. Snapshot validation, a 10,000-record metadata budget, native refusal or failed readback retain pending status rather than falsely reporting complete application. Errors expose only the fixed existing unavailable code.

Both desktop platforms embed the same helper resource. WorkBuddy adapter revision advances to 2; Qoder retains revision 1 and its previous new-conversation behavior. WorkBuddy-specific localized hints and consent explain existing-local-conversation changes in all four runtime languages.

## Regression evidence

Before implementation, the new helper tests failed for both WorkBuddy editions because existing models remained unchanged; failure handling also incorrectly returned success. Before the UI change, both WorkBuddy consent-description regressions failed while Qoder passed.

Passing focused checks: `pnpm test:model-default` (16 tests); Agent connection consent cases (3 tests); complete renderer tests (4,257 tests in 318 files), TypeScript, renderer build and renderer formatting. The fixture uses 103 local conversations across pages, preserves synthetic history and thought levels, excludes a cloud conversation, verifies a subsequent default change, shares overlapping writes, and rejects partial/oversized/duplicate/unrecognized snapshots and failed native writes/readback. The agent test evaluates the serialized functions in an isolated VM and verifies that readiness waits for the native update and readback.

The combined final Rust checks passed with the Codex restart-notice and stopped-Agent restart repairs: check, Clippy, formatting and full tests (7,821 passed, 21 ignored). See [combined verification](2026-11-17-stopped-agent-restart.md#verification) for host, checked tree and platform limits. Logs: `/tmp/varyai-workbuddy-existing-before.log`, `/tmp/varyai-workbuddy-existing-helpers.log`, `/tmp/varyai-workbuddy-existing-ui-before.log`, `/tmp/varyai-workbuddy-existing-ui-final.log`, `/tmp/varyai-two-model-bugs-frontend.log`.

No installed application was rebuilt, restarted or replaced for verification. The user subsequently reported that both requested model/restart fixes passed self-testing with their build. No push or remote CI validation is included in this record.
