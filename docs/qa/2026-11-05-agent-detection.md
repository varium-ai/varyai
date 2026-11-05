# Agent detection and loading verification

Date: 2026-11-05. Host: macOS / Darwin arm64. Base revision:
`a30e86d1d11641cf40dbbdaf02c11985d3e34df0`, with existing working-tree changes
preserved. Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.
No commit, push, application installation, or native app launch was performed.

## Behavior

- Each Agent has independent connection and native-model queries. Pending
  connection reads show Checking in that Agent's row instead of Not connected.
- Completed rows remain usable while another Agent is pending or fails.
- Background refresh retains confirmed data while requests remain pending.
- Installation discovery shows compact row placeholders. The page-wide loading
  paragraph and its extra vertical gap are removed.
- Pending and unavailable rows stay above the compact Not connected group.
- Connection repair refreshes only the affected Agent's catalog. Existing batch
  commands remain available for other callers.
- The checking indicator respects reduced-motion preferences. All four runtime
  languages include the checking label.

## Verification

Regression tests were added before implementation. The initial per-Agent tests
failed against the previous aggregate UI. A further failing compact-mode check
reproduced pending Agents appearing inside the Not connected group before the
sorting fix.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed on final renderer sources |
| `pnpm build:renderer` | Passed on final renderer sources |
| `pnpm format:check` | Passed on final renderer sources |
| `pnpm test:unit` | 278 files, 3,439 tests passed |
| Focused Agent connection, installation, and update suites | 161 tests passed after the final compact-mode adjustment |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 5,981 library tests and 163 integration tests passed; 18 library tests ignored |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| Development, TRAE bridge, WorkBuddy auth, Doubao check-in, Agent rewards, Qoder codec helper suites | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `git diff --check` | Passed |
| Browser preview with synthetic fixtures | Verified pending Codex/Gemini rows beside usable completed rows, without the top loading gap |

The full Rust invocation includes the required library suite. Rust commands used
`--offline --target-dir /private/tmp/vary-magpie-parity-target` and
`--config build.incremental=false --config profile.dev.package.varyai.debug=0`.
The pinned pnpm executable was invoked through Node because the host's global
pnpm bootstrap was unreliable. Logs are in `/private/tmp/vary-agent-*.log`.

The full frontend suite ran before the final compact-group adjustment; all three
affected Agent suites, type checking, renderer build, and formatting were rerun
afterward. An initial formatting run reported `src/App.tsx`; a subsequent run on
the final tree passed. Existing runtime/test diagnostics did not fail the suites.
The browser used an isolated Vite preview with English labels and synthetic data,
without connecting to a running Tauri instance.

No remote CI revision was submitted. Linux, Windows, WSL2, packaging, installer,
signing, and notarization acceptance remain outside this local verification.

## Checked source fingerprints

These hashes identify the final relevant source files within the existing dirty
working tree; they do not claim ownership of earlier edits in those files.

| File | SHA-256 |
| --- | --- |
| `src/components/agents/AgentConnectionsPage.tsx` | `75a33b421ad6b66aa65fadbdecbb9a5393e620e6d2de19b44b3faa8845eee38b` |
| `src/components/layout/workspace.css` | `b64af1f7ff01b5c92e7eaf69e8e18e02bce5a9c3b1112b5c982c2b77cddb16ed` |
| `src/lib/api/agentConnections.ts` | `b98bb13b6beb6024def9e0d2dc3cc46b0b3a98f581b43b513d8cd99daa21ff5d` |
| `src/i18n/locales/en.json` | `e4b5a760f27857e59a7b78f18c9a5711f182d384248ac4a79fe29e9a017f0f83` |
| `src/i18n/locales/zh.json` | `e4f57c3187d8425bce811dde7f2d3d9d2c66ddb5cc6b0981404fce126f4eb267` |
| `src/i18n/locales/ja.json` | `3f1ccd5c0354dadb4019ff3c8dd6cbb16065c30b59694a3013c0fca576801dc6` |
| `src/i18n/locales/zh-TW.json` | `1b30a2a9b1f36853111216eb59676749fa20a18a79646aba76119aae7a447008` |
| `src-tauri/src/commands/agent_connections.rs` | `41c87d172f9583e0b76600f32967170b0bd2307693e030f0b007380e06d8730a` |
| `src-tauri/src/services/agent_connections/native.rs` | `5a901a327f4894cf82501a6f1191b54a09ca7cd9406e7a51d15193761bff3d9e` |
| `src-tauri/src/services/proxy.rs` | `929d7d87dc2b5f43acfcef30200ad78a8d602d12d3a846c17d17abfe773dad60` |
| `src-tauri/src/lib.rs` | `9ad05da4e16b259394621b56e8756d895df53acc6c567f48e3fac7268976f05f` |
| `tests/components/AgentConnectionsPage.test.tsx` | `9a5410b7f1e582b297a4dc6c48b031cc4a764f3ec6969282e77ad8aaf75f65e4` |
| `tests/components/AgentInstallation.test.tsx` | `bd35d17667994736767219334cbedde7b8c4d5ea459f128f063f422ad466a2a9` |
| `tests/components/AgentUpdates.test.tsx` | `eb2bbec565884851b51de611165931f846a5ad70865c1b6d0f6d63fc0ef6fc4e` |
| `docs/architecture/shared-catalog.md` | `e4ee134f531ebd7a2bfeb85f6ea14d14b34d7a29b5062eb3ccafeedcf01c3042` |

## Follow-up: remove the workspace TRAE disclosure

The TRAE workspace row no longer renders its details arrow or expandable details
region. Its model selector, check-in controls, and connection switch stay in the
flat row. Compact tray details remain available. Regression assertions cover
connected, disconnected, and bridge-pending workspace rows, including clicking
the heading without exposing a details region. Shared-catalog browsing coverage
now uses the compact TRAE details surface.

The three new workspace assertions failed before the rendering change. The
updated Agent page suite passes all 115 tests. Type checking, renderer build,
source formatting, and `git diff --check` pass on this follow-up. The full
frontend suite passes all 278 files and 3,439 tests on the final renderer sources.
Backend sources are unchanged by this follow-up; their previous verification
still applies.
Host and toolchain are unchanged. No commit, push, or app installation occurred.
Logs use `/private/tmp/vary-trae-arrow-*.log`.

Updated source fingerprints for this follow-up:

| File | SHA-256 |
| --- | --- |
| `src/components/agents/AgentConnectionsPage.tsx` | `e42d33758166281191848b325f370d3ee4e3e1004afdeaf1aa3abaf66b4ed2b4` |
| `tests/components/AgentConnectionsPage.test.tsx` | `83abc36adbb1c24f6754490df7e2d16a6ef567fe01316d6a64df29a8f574e245` |
| `docs/architecture/shared-catalog.md` | `fd05594ba0237ec3165f75e216ce110ae0e5d09dec3f83325e0d868be1e61a7a` |
