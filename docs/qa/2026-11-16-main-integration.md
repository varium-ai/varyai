# Main integration and acceptance handoff

The user requested committing all current source/documentation changes, integrating remote main into `codex/intent-recognition`, and pushing that branch. The initial workspace commit is `89f44593`. The merged main revision is `0437976cb18b32e938c5a055be2ec70e2330e5d4`. Its exact main CI run `43814510970` was observed as successful in GitHub.

## Conflict resolution

- Desktop restart retains PID-exit verification and stale LaunchServices filtering, together with main's stricter replacement-instance verification.
- Profile rollback preserves the native runtime snapshot and main's published-menu type. The new semantic Codex startup receipt is included alongside the legacy receipt in profile backup/restore paths. The existing scheme undo test now verifies that the semantic receipt changes and is restored byte-for-byte.
- Agent mutations retain main's navigation-safe operation handling and the selected-account Doubao reward summary. Restart errors are asserted on the row/toast after the confirmation closes; duplicate retries remain prevented.
- Request logs use main's completed-page anchoring, placeholder identity, retry/error handling and source labels; both branches' relevant regression coverage is retained.
- Qoder's common model name already includes the supplier after main integration, so the test now requires one supplier suffix. Codex freshness validation uses main's semantic receipt rather than the former whole-file fingerprint schema.

## Local verification

Host: macOS 26.6.2 arm64; Node 20.20.2, pnpm 10.12.3, Rust 1.98.1. This local Node version differs from CI's pinned version.

- Frontend typecheck, renderer build and formatting passed. Final full frontend suite: 318 files, 4,240 tests passed. An earlier run loaded the pre-adjustment synchronous retry assertion and failed that one test; its corrected asynchronous assertion passed separately and in the final full run.
- All development/packaging/client helper tests, shared-v23 recovery tests and production dependency audit passed after integration.
- The first full Rust run exposed two stale expectations (duplicated Qoder suffix and legacy Codex receipt). These were corrected while adding semantic-receipt rollback coverage. Final Rust formatting, cargo check and Clippy with warnings denied passed. The interrupted full run was restarted and completed successfully: 7,796 tests passed across 16 result groups, including 7,631 library tests and 165 integration tests; 21 existing ignored tests, zero failures. The existing linker unwind-size and test-only dead-code warnings were nonfatal.
- All 219 recorded source/documentation hashes were unchanged after final verification. Staged and unstaged whitespace checks passed, with no unresolved merge entries.
- Logs: `/tmp/varyai-main-merge-*.log`; helper results: `/tmp/varyai-main-merge-helper-checks.json`; final Rust results: `/tmp/varyai-main-merge-final-rust-checks.json`.

The browser's current filtered view has 24 reports; see `2026-11-16-current-24-bugs.md`. Native-client and packaged-build acceptance remain separate from automated checks. No external tracker fields, native accounts or running application configuration were modified for this integration. Final branch push and exact-revision CI status will be reported in the conversation.
