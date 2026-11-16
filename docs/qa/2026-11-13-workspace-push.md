# Workspace integration before push

## Scope and source identity

The requested commit includes all pending source, test, localization and
documentation changes in the workspace: Agent model source labels, native
configuration freshness and recovery, installation rescanning, quota views and
controls, desktop statistics, text selection and update checks.

The checkout started at `c1fc6ab85781986e24d77fb1d298c25fac86d36b`.
Before committing, it fast-forwarded to the existing remote branch revision
`c519afcb98197d5a2514d98d4153916aa6fae0d3` and restored the pending changes from a retained temporary stash.
Five conflicting files were reconciled. The combined behavior retains remote
Unicode-safe model identities and per-Agent errors, adds durable restart
mutation state, and journals Harness credentials separately from its profile.
Existing native Harness profiles remain adoptable, as required by the remote
restoration change.

A regression first reproduced the loss of a legacy Harness model preference
when reconnecting a receipt that predates the separate preference cache. The
credential-release path now remembers that model before removing its receipt.
The existing recovery fixture removes the preference cache to cover this case.

The checked source snapshot is stored locally at
`/tmp/vary-final-source.json` (1,758 non-Markdown files). Its canonical JSON
SHA-256 is
`4bf08f3837caa4af54adf9a6b3c4b080cbf6ef1da69621c13dae83684afd5eff`.
The verification record itself is excluded from that source digest.
No generated binaries or debug artifacts are included in the intended commit.

## Environment and checks

Host: macOS 15.3.1, Apple Silicon. Toolchain: Node 24.11.0, pnpm 10.12.3,
rustc 1.98.1. Logs use `/tmp/vary-final-*.log`; structured outcomes use
`/tmp/vary-final-results.json` and `/tmp/vary-final-backend-results.json`.

Completed frontend and helper checks on the combined source snapshot:

- `pnpm typecheck`, `pnpm format:check` and `pnpm build:renderer`: passed.
- `pnpm test:unit`: 312 files and 4,179 tests passed.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`,
  `pnpm test:doubao-checkin`, `pnpm test:agent-rewards` and
  `pnpm test:qoder-codec`: passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: passed.
- `pnpm audit --prod`: no known vulnerabilities found.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`,
  `cargo check --manifest-path src-tauri/Cargo.toml` and
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.

Initial sandbox-only attempts to listen on loopback ports or contact the npm audit service were
retried with the required access. No application was installed, launched,
replaced or restarted for these checks.

## Acceptance boundary

Local checks establish behavior on this macOS source snapshot. Linux, Windows,
and the Windows-to-WSL2 contract require the corresponding remote CI jobs.
Native client UI, installer, signing and live vendor acceptance are outside
this commit verification.
