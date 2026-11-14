# PR 56 Chat conversion and Harness recovery verification

## Checked changes

Base revision: `663cb5c45ada2550610344af51f9e6b033266344`.
The final working tree changes generic Chat optional-search conversion and its
HTTP/SSE regressions, orphaned Harness credential recovery and its restoration
and foreign-ownership regressions, plus the corresponding documentation.
The seven-file source manifest is retained locally at
`/tmp/vary-pr56-fix-source.json`; its canonical JSON SHA-256 is
`e1c5808486a780ee65efbfaead27e95bd7b6c94814784eaf17f732807e28bbfb`. This record is excluded from that digest.

Both regressions were reproduced before their implementation changes. The
Harness regression also failed in PR 56's Linux and Windows CI logs at the base
revision. Recovery now requires matching generated routes and loopback URLs,
refuses foreign key references, and journals both native files for disconnect.

## Local checks

Host: macOS 15.3.1, Apple Silicon. Toolchain: Node 24.11.0, pnpm 10.12.3,
rustc 1.98.1. Final Rust source was checked with:

- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: passed;
  7,566 library tests and 165 integration tests, zero failures, 21 ignored.
  The full run includes the required library tests. Incremental compilation was
  disabled and package debug info was set to zero for the test run.

Rust outcomes are retained in `/tmp/vary-pr56-local-results.json` and
`/tmp/vary-pr56-rust-all.log`. The source stayed unchanged during these checks.

The frontend, helpers, dependency manifests and lockfile match the previously
checked snapshot `/tmp/vary-final-source.json` byte for byte; only the four Rust
files listed above differ among its non-Markdown entries. The passing frontend,
helper, renderer and production dependency audit results in the
[workspace verification record](2026-11-13-workspace-push.md) therefore remain
applicable. `pnpm format:check` passed. The updated documentation and this record were
checked with `pnpm exec prettier --check`; `git diff --check` also passed.

## Acceptance limits

Synthetic live requests confirmed that the active Chat converter completes
ordinary and optional-search turns with non-strict function tools. The affected
source separately rejects explicit strict function tools on Chat while accepting
them on Responses. Explicit strict requirements remain intact. A native UI
protocol-change draft was not saved: re-entering the existing key is required,
and the Mac locked before that confirmation. Native client acceptance remains
incomplete until the supported protocol is saved and the real turn is retried.

Linux, Windows, macOS and Windows-to-WSL2 acceptance requires the CI results for
the exact pushed revision. Local success alone does not establish that result.
No installer, signing or distribution acceptance is claimed.
