# Live Agent update verification

Date: 2026-11-10. Host: macOS / Darwin 25.6.0 arm64. Source baseline:
`35bc9e2f28e26f041aedaee72099bf6b328362bd`, plus this report, the CLI observation
retry, its component regressions, the Qoder codec binding, and the corresponding
Agent update and Qoder guides. Node 22.22.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

## Actual updates

The user authorized updating all currently offered Agents. The running homepage's
Update all action performed these updates. Its Grok multiple-installation notice
was accepted within that scope; the other installation was not removed.

| Agent | Before | After |
| --- | --- | --- |
| Claude Code | 2.1.318 | 2.1.322 |
| Grok Build | 1.0.70 | 1.0.77 |
| Hermes | 0.22.7 | 0.23.1 |
| Qoder CN App | 0.5.3 | 0.5.7 |
| Doubao Work App | 2.35.19 | 2.36.3 |
| Kimi Code App | 1.1.0 | 1.3.2 |
| Qwen Code | 0.26.8 | 0.27.1 |

Claude Code, Grok Build, Qwen Code, and Hermes subsequently returned their new
versions with exit code zero. The three desktop applications' installed
Info.plist product and build versions matched the table. The final homepage
displayed all seven new versions and no update buttons. Vary AI was not
installed, replaced, or relaunched.

The batch initially reported six verified updates and one unverified Hermes
result, with no Hermes version in its cache. A subsequent direct Hermes probe
succeeded, and a fresh homepage probe displayed 0.23.1. The original probe's
specific error was not retained; a timeout is not established as its cause.

## Regressions and fixes

- CLI update confirmation previously made only one version observation. New
  component tests reproduced both a missing version and a rejected observation,
  followed by a healthy new version; both failed before the fix. Successful
  updates now retry missing observations twice, one second apart. Installer
  execution is not retried. Readable unchanged versions, failed update commands,
  and persistent observation errors retain their conservative outcomes. The
  persistent-error regression verifies the three-observation bound.
- The newly installed Qoder 0.5.7 worker changed its contents, so the existing
  hash allowlist rejected it. Both the Node installed-runtime helper and the Rust
  decode/redact/sign regression failed before the fix. The binding now recognizes
  this exact worker SHA-256:
  `0d42d8e68ec7fe45ab04d4c9fda8111952b7ca4f82fe89364813afe42cdb618e`.
  The same synthetic payload tests pass after the binding change. The unknown
  worker rejection test remains unchanged and passes. No vendor module bytes or
  live account credentials were added to the repository.

The Qoder binding is embedded in the native backend and requires a normal Vary
restart after rebuilding to affect that running backend. The live homepage check
establishes installation/version acceptance, not a real upstream conversation or
acceptance of the new binding in the previously running backend.

## Model selection

The shared starting-model selector is implemented for Claude Code, Codex, Gemini,
Grok Build, OpenCode, OpenClaw, Hermes, Pi, Kimi, Qwen, ZCode, and DeepSeek Harness.
TRAE uses its bridge to apply a starting selection. Claude Desktop, WorkBuddy,
WorkBuddy AI, and Qoder expose a synchronized model catalog and use their native
model pickers. Doubao Work is audit-only and retains official model selection.
These are current Vary adapter boundaries, not a claim that upstream clients
cannot support additional default-model integration.

## Verification

Logs are under `/private/tmp/vary-update-recheck-*.log`. The checked source hashes
are recorded in `/private/tmp/vary-update-recheck-source-sha256.json`.

- Renderer type checking, production build, formatting, and all 3,625 unit tests
  passed. The focused Agent update suite passed all 48 tests. The two added
  recovery tests failed before implementation.
- Production dependency audit: no known vulnerabilities.
- Development, TRAE bridge, WorkBuddy check-in auth, Doubao check-in, Agent rewards,
  and Python recovery helpers passed. The Qoder helper passed all three tests
  after the binding fix, following its initial installed-runtime failure.
- Final Rust formatting, check, and Clippy with warnings denied passed. The
  focused installed-Qoder decode/redact/sign test passed after the binding fix.
- Final `cargo test --lib --manifest-path src-tauri/Cargo.toml`: 6,240 passed,
  19 existing ignored tests, zero failures.
- Final `cargo test --manifest-path src-tauri/Cargo.toml`: 6,403 passed across
  library and integration suites, 19 existing ignored tests, zero failures.
- Additional Prettier checks for the codec helper and component tests, plus
  `git diff --check`, passed. The existing unused test-fixture variable and macOS
  compact-unwind linker warning did not fail the Rust tests; Clippy passed with
  warnings denied.

Remote cross-platform CI and packaging/signing acceptance are not established by
these local checks. The additional fixes are not part of the previously pushed
baseline revision.
