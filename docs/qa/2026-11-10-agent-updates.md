# Agent update target and channel verification

Date: 2026-11-10. Host: macOS / Darwin 25.6.0 arm64. Base revision:
`c092e24eb20c4f30e44b9b3f8c5d77f78fc85098`, with this task's uncommitted changes.
Node 22.22.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

## Findings and changes

- Managed Pi 1.5.0 was the active installation, while the update fallback wrote
  to a separate global npm installation. Managed Pi now uses its absolute
  launcher and native `update` command on macOS and Windows. It cannot fall back
  to installing a second npm copy. Existing package-manager installations keep
  their anchored updates.
- POSIX pnpm installations previously fell back to npm. Their update commands
  now use pnpm, matching the existing Windows behavior. Regression coverage
  includes all eight supported npm-distributed Agents.
- Official Homebrew formulae and casks use the release channel in their installed
  receipt to select Homebrew's public version metadata. This prevents advertising
  an npm release that has not reached the installation's Homebrew channel.
  Third-party taps retain their own installation and are never
  replaced by Homebrew core's similarly named package.
- Known desktop failure codes and Homebrew trust refusals receive localized,
  sanitized explanations in all four runtime languages. Unknown errors and raw
  command output are not displayed.
- Pi's fixed "Individual settings" label is removed. Its shared model selector
  and the actual settings action in tray details remain available.

Regression tests failed before the corresponding fixes: managed Pi anchoring,
Pi update fallback, pnpm anchoring, Homebrew channel identification, the fixed Pi
label, and failure-reason reporting.

## Local verification

Commands ran from the repository root. pnpm commands used
`PATH=/Users/baomi/.nvm/versions/node/v22.22.0/bin:$PATH`; Cargo used the repository's
default target directory. Logs are under `/private/tmp/vary-agent-update-*.log`.
The 14-file source/documentation fingerprint manifest is stored at
`/private/tmp/vary-agent-update-source-sha256.txt`, with SHA-256
`59286cd32915cd79d2d45582b914be4ba02eb8f7e5621c9ca28259144b6a8398`.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | 281 files, 3,605 tests passed |
| Focused Agent connection, update, and wording suites | 203 tests passed |
| `pnpm audit --prod` | No known vulnerabilities |
| Development, TRAE bridge, WorkBuddy auth, Doubao check-in, Agent rewards, and Qoder codec helper suites | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,239 tests passed, 19 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,239 library and 163 integration tests passed, 19 ignored |
| `git diff --check` | Passed |

The Rust library suite reports an existing unused test-fixture variable in
`services/proxy.rs` and the macOS linker reports a compact-unwind size warning.
Neither failed the tests; production Clippy passed with warnings denied.

## Live acceptance

- The explicitly ignored
  `official_package_can_be_staged_without_installing` acceptance test passed for
  Qoder CN, Doubao Work, and Kimi Code. It downloaded the official packages,
  verified their digests where supplied, safely unpacked them, and checked
  bundle identity, signing team, signature, CPU architecture, and version.
  It did not replace or launch any installed desktop application. The test binary
  used the final desktop-updater sources; subsequent changes only affected
  Homebrew metadata URL construction in another module.
- A separate, deliberate Pi update through the active managed launcher exited
  successfully. The active entry reported 1.5.1 afterward, up from 1.5.0. After
  rescanning, the running homepage displayed 1.5.1, omitted Pi's update button,
  and no longer displayed the obsolete label.
- Gemini's active Homebrew installation was deliberately upgraded from 0.66.0
  to 0.67.0 using its anchored Homebrew command. The command exited successfully,
  and the active CLI reported 0.67.0 afterward. Live Homebrew metadata already
  offered 0.67.0 during this check; the channel-comparison regression test uses
  synthetic lagging metadata and does not establish that channel lag caused
  this machine's earlier Gemini failures.
- A read-only Homebrew query refused the installed `anomalyco/tap/opencode`
  formula because that tap had not been trusted. After explicit user approval,
  `brew trust --formula anomalyco/tap/opencode` succeeded, followed by
  `brew upgrade anomalyco/tap/opencode`. OpenCode upgraded from 1.15.11 to
  1.19.5. The active `/opt/homebrew/bin/opencode` entry reported 1.19.5 with exit
  code zero, and its installed receipt still identified `anomalyco/tap`.
  Trust authorization was limited to that formula. The existing global npm Pi
  copy and the default executable selection were preserved.

No Vary AI installation, replacement, relaunch, commit, or push was performed.
The running development backend still needs its normal rebuild/restart to load
the Rust fixes; the renderer changes were verified through hot reload. Remote
Linux, Windows, and WSL2 CI, packaging, signing, and installer acceptance remain
unverified.
