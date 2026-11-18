# Restart feedback and update verification — 2026-11-18

Placement superseded by [Agent status-line feedback](2026-11-18-agent-status-line-feedback.md)
after the user identified the exact row in a screenshot. Backend and update
verification results below remain historical evidence for their unchanged paths.

This supersedes the earlier inline restart guidance presentation at the user's
explicit request. Restart results use Magpie-style footer capsules: 12.5px text,
red foreground, a lightly tinted background and thin inset border, single-line
ellipsis with the full message available on hover, and an eight-second lifetime.
A newer message replaces the previous one. Main and tray surfaces host the shared
capsule, and navigation in the main window does not move it into an Agent row.

The native restart orchestrators now distinguish a rejected quit request,
an app that remains running, and failure to reopen after the original app exits.
Both macOS and Windows map these phases; identity/configuration conflicts retain
their existing handling. Native discovery, graceful exit, Windows tree cleanup,
launch authority and request/response handling are unchanged. Unknown errors
remain sanitized through the existing renderer allowlist. No client was actually
restarted during verification.

The Codex screenshot exposed related UI and observation issues:

- The update banner treated every non-success outcome, including `unverified`,
  as a failure and coloured it red. Only failed/preflight-failed updates now use
  failure styling; an unverified result clearly says its new version is unknown.
- Post-update observation stopped at any nonempty version, even the old version.
  It now retries stale versions as well as missing/failed probes (three reads,
  one second apart). The update command itself is never retried.
- A **Recheck version** action only reads versions. A subsequent successful
  version refresh also reconciles an earlier unverified result automatically.
  Versions that remain unchanged/unreadable do not become successful updates.

This does not prove the screenshot's particular Codex installation was upgraded.
Multiple installations or a failing version command remain possible causes;
no installation, update or replacement was performed on the user's clients.

## Checked tree and environment

- Base: `eb24a0e3b947ba89bd9aa668148e23b0e9bbe59c`, with existing unrelated work preserved.
  No commit or push was made.
- Host: macOS Darwin arm64; Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.
  Repository-pinned Node 22.12.0 and remote CI remain unverified.
- Backend checks use `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`
  and `CARGO_INCREMENTAL=0` to avoid the earlier disk-space failure. Debug
  stripping matches CI; disabling the incremental cache changes build storage,
  not runtime logic. Only the earlier failed incremental cache created by this
  work was removed; source, user data and running applications were preserved.
- Frontend regressions were reproduced before implementation. A native test
  first reproduced the generic-error result where a quit-phase result was needed.
- Changed source/test SHA-256: `a68167bc0e20b8bf64e9c53409389a3df7a5635a7ddb0f49e80731efc1f7d213`, over each ordered path below encoded
  as path + NUL + contents + NUL:
  - `src/lib/agentFeedback.ts`
  - `src/components/agents/AgentFeedbackStatus.tsx`
  - `src/App.tsx`
  - `src/components/tray/TrayPanel.tsx`
  - `src/components/layout/workspace.css`
  - `src/components/agents/AgentConnectionsPage.tsx`
  - `src/components/agents/AgentUpdates.tsx`
  - `src/hooks/useAgentUpdates.ts`
  - `src-tauri/src/services/agent_connections/desktop.rs`
  - `src-tauri/src/services/agent_connections/desktop/windows.rs`
  - `src-tauri/src/commands/agent_connections/codex_status_tests.rs`
  - `src/i18n/locales/en.json`
  - `src/i18n/locales/zh.json`
  - `src/i18n/locales/ja.json`
  - `src/i18n/locales/zh-TW.json`
  - `tests/components/AgentConnectionsPage.test.tsx`
  - `tests/components/AgentUpdates.test.tsx`
  - `tests/components/AgentUpdateMessages.test.tsx`
  - `tests/components/AgentFeedbackStatus.test.tsx`

## Checks

| Check | Result |
| --- | --- |
| Focused Agent page, update workflow/messages, capsule lifetime, locale and inventory suites | Passed: 6 files, 527 tests |
| TypeScript / renderer build / formatting | Passed |
| Full frontend unit suite / production dependency audit | Passed: 325 files, 4,544 tests; no known production vulnerabilities |
| Rust formatting / check / Clippy | Passed |
| Rust library tests | Passed: 7,734 passed, 21 manual tests ignored |
| Full Rust integration tests | Passed: 7,899 total passed, 21 ignored across library and integration suites |

The unchanged development, CI policy, DMG, TRAE, model-default, Pi session,
WorkBuddy/Doubao, rewards, Qoder and recovery helpers passed earlier in this
working tree; their sources were not modified by this follow-up.

## Coverage and native acceptance

The [coverage matrix](agent-feature-coverage.md#restart-outcome-capsules-and-update-verification-recovery)
records all twenty frontend/connection-registry entries before implementation,
including desktop/CLI and domestic/international variants. Per-Agent synthetic
UI checks cover restart outcome allowlisting, footer placement, enabled/restored
connections, main/compact views, macOS/Windows UI branches, unsupported results,
malformed errors and private-error sanitization. Update message tests cover all
Agent IDs; workflow tests separately cover stale, missing, throwing and later
recovered version observations and read-only rechecking. Existing native updater
limitations remain explicit; no new updater support is claimed.

Synthetic macOS and Windows restart orchestration tests distinguish failed
phases and retain refusal/timeout, process-replacement and configuration-conflict
checks. These are not installed-client self-tests or Windows native API tests.

An isolated browser fixture rendered the actual React/CSS. The main preview
showed a neutral Codex verification banner with **Recheck version** and a footer
capsule naming the reopen failure. The capsule disappeared after eight seconds;
a compact dark preview also showed the quit-refusal wording and capsule colours.
Screenshots use synthetic model/version data and live outside the repository.

Installed-Agent self-tests, native Windows/Linux runs, Windows-to-WSL2, remote CI
and release acceptance were not performed. Native acceptance remains unverified
for every Agent. No all-Agent native acceptance claim is made.
