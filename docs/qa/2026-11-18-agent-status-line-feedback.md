# Agent status-line feedback — 2026-11-18

The user clarified the exact location with an annotated screenshot: restart
feedback belongs in the status line immediately below the corresponding Agent's
name. This supersedes the footer placement from the previous report.

The phase-specific message now replaces that row's normal status in a neutral
capsule, exposes the full text in its title, and restores the current
normal status after eight seconds. Starting another action clears the message
for that Agent. Messages and expiry timers are independent per Agent, scoped to
the React Query session and retained across page navigation. A separate session
does not inherit another session's message. Both main and compact/tray views
use the existing Agent status line. The obsolete footer host was removed.

No backend, native adapter, protocol, update observation or localization logic
changed in this placement correction. Other working-tree changes were preserved;
no commit or push was made.

## Checked tree

- Base: `eb24a0e3b947ba89bd9aa668148e23b0e9bbe59c` with existing uncommitted work.
- macOS Darwin arm64; Node 24.11.0, pnpm 10.12.3. Pinned Node 22.12.0 and remote
  CI remain unverified.
- Source/test SHA-256 `7ee8dc24e9cbf9165a8fa58a0d7dfcfb1f6536d26b1d0dd0b8686c61bc33a4d1` covers each ordered path below encoded as
  path + NUL + contents + NUL:
  - `src/lib/agentFeedback.ts`
  - `src/components/agents/AgentConnectionsPage.tsx`
  - `src/components/layout/workspace.css`
  - `src/App.tsx`
  - `src/components/tray/TrayPanel.tsx`
  - `tests/components/AgentConnectionsPage.test.tsx`
  - `tests/hooks/useAgentFeedback.test.tsx`
- The status-line regressions failed first in 110 applicable presentation cases
  against the footer implementation, then passed after the placement change.
  The revised status expectations failed in all 44 applicable platform and view
  cases against the red alert before the neutral presentation was implemented.

## Verification

| Check | Result |
| --- | --- |
| Agent page, session feedback hook, tray and coverage-inventory suites | Included in the full frontend run |
| TypeScript | Passed |
| Renderer build / formatting / Git whitespace check | Passed |
| `pnpm test:unit --maxWorkers=6` | Passed all 4,545 tests in 325 files |
| Production audit | Passed with no known vulnerabilities |
| Backend formatting, check, Clippy, library and full integration tests | Previous full run passed 7,899 tests and ignored 21. This correction only changed the renderer. |

The per-Agent matrix was updated before implementation in the
[coverage ledger](agent-feature-coverage.md#agent-status-line-restart-feedback-and-update-verification-recovery).
It includes all twenty Agent IDs, domestic/international editions, CLI paths
without desktop restart and current updater/platform limitations. Synthetic
presentation tests cover all eleven desktop restart registrations, main/compact
views, macOS/Windows UI branches, enabled/restored state, phase results,
unsupported/malformed errors and sanitization. Hook tests cover independent
expiry, navigation, per-Agent retry clearing and separate query sessions.
Native self-tests remain unverified for every Agent; no native acceptance or
new Agent support is claimed.

An isolated browser fixture used the actual component and CSS with synthetic
API results. The light main view showed the neutral reopen-failure capsule
directly below Claude Desktop's name. Expiry restored its normal status. A dark
compact preview placed the neutral quit-refusal capsule on the same Agent's
status line, with the full message in its title. The browser checks confirmed
the muted color and status role in both views. The measured text contrast was
4.81:1 in the light main view and 7.06:1 in the dark compact view. Screenshots
live outside the repository.
