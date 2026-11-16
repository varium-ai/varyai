# Doubao selected-account reward summary

Date: 2026-11-16 (Asia/Shanghai). Host: macOS 26.6.2 arm64, Node 20.20.2, pnpm 10.12.3. Base revision: `3f1daf658fc92a492a03b3c9ec0becc009773e3f` plus the preserved working tree. Source hashes are recorded in `2026-11-16-doubao-selected-account-sources.json`.

The user observed a confirmed 400-credit claim inside the Doubao panel while its row still displayed automatic check-in. The row previously required every saved account to be checked in. It now follows the panel selection, initially the current native account, and displays only that account's reward or automatic/manual mode. Selection is retained when the panel is collapsed. No official-client account switch or claim is triggered by selecting a display account. The standalone panel remains supported, and disconnected saved-mode behavior is preserved. The implementation is shared by macOS and Windows; native Windows execution was not performed.

## Verification

- Updated regression failed on the previous all-account summary.
- Focused AgentConnectionsPage and DoubaoWorkCheckinPanel suite: 246 passed. Coverage includes selection changes in regular and compact layouts, mixed claim states, collapse/reopen retention, and absence of claim or automatic-mode mutations during selection.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`: passed.
- `git diff --check`: passed.
- Full `pnpm test:unit`: 4102 passed (4102). Logs: `/tmp/varyai-doubao-selection-*.log`.
- CUA verified the running renderer now displays the confirmed 400-credit reward in the Doubao row. No claim, application restart, configuration change or native account switch was performed for this UI fix.

Backend code did not change for this fix; previous backend results remain recorded separately. No commit or push.
