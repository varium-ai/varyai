# Windows Agent installation discovery

Branch: `codex/fix-windows-agent-detection`. Base revision:
`81763452a347cd527c1ca068c8cd5cf1f2b7333a`. The checkout was clean before this
work. Verification below applies to the working-tree discovery changes, tests,
coverage ledger and troubleshooting documentation over that revision.

## Cause and implementation

Most desktop Agents previously depended on macOS `.app` discovery, which returns
no result on Windows. The only Windows exceptions were TRAE and T3 Code; their
fixed default roots missed registered custom locations. Read-only inspection
confirmed TRAE CN installed under `D:\Program Files\Trae CN`, with a matching
Windows uninstall registration. OpenCode's npm launcher was discoverable through
the existing CLI search, explaining its different behavior.

The shared Windows adapter now reads a registry snapshot for each installation
or desktop-version query. It checks known executable layouts, user/machine
uninstall registrations in both registry views, current-user AppX registrations
for Codex/Claude, and Claude's Squirrel launcher/version directories. Installer
paths are parsed as data. Detection never executes registry commands, launches
Agents, creates configuration or changes security policy. Registered installations
must retain the expected executable; resource-only or malformed AppX packages
are rejected. Codex packages also require its known publisher ID.

Both installation presence and desktop-version state use this adapter. Missing
version metadata does not make an installed Agent disappear. Existing CLI
fallbacks, macOS discovery and Linux behavior remain intact. This change does not
expand Windows native restart, takeover or update authorization to arbitrary
registered paths. Those adapters retain their own identity checks.

## Magpie comparison

Reviewed public Magpie revision `69233d6bcfda248a17b613a199e0f2735b0a3e08`:

- [desktopapp.go](https://github.com/yetone/magpie/blob/69233d6bcfda248a17b613a199e0f2735b0a3e08/internal/agent/desktopapp.go)
  separates CLI and desktop presence and reads installer-chosen locations from
  `InstallLocation`, `DisplayIcon` and `UninstallString`. Its DeepSeek adapter
  supplies the electron-builder installation GUID used by this implementation.
- [desktopapp_windows.go](https://github.com/yetone/magpie/blob/69233d6bcfda248a17b613a199e0f2735b0a3e08/internal/agent/desktopapp_windows.go)
  reads current-user AppX package registrations without PowerShell or traversing
  protected WindowsApps directories, and checks both registry views.
- [workbuddy.go](https://github.com/yetone/magpie/blob/69233d6bcfda248a17b613a199e0f2735b0a3e08/internal/agent/workbuddy.go)
  still accepts WorkBuddy configuration as presence outside macOS. VaryAI retains
  its stricter existing executable-based rule to avoid uninstall leftovers.

The Rust implementation is independently written; no Magpie source is copied.
Magpie's Agent inventory differs from VaryAI's and is not used as the coverage
inventory. Its reference checkout hit Windows long-path errors in unrelated
session fixtures; all source files cited above were available and reviewed.

## Verification

Host: Windows 10 build 19045 x64. Node 24.22.1; available pnpm 11.25.0 (repository
declares 10.12.3); isolated Rust/Cargo/Clippy/rustfmt 1.98.0 and 1.100.0 GNU. The toolchain
and comparison repository live outside the source checkout. No application was
launched, installed, stopped or replaced.

- The exact Windows adapter source compiled in an isolated harness using
  `winreg 0.52.0`; six regression groups passed. They exercise all twelve desktop
  Agent IDs and each listed layout, registered custom locations from each path
  field, missing metadata, edition separation, stale/malformed registrations,
  leftover configuration, Squirrel layouts and AppX identity/version selection.
  Harness fixtures use `tempfile 3.10.1`; the application's dependency files are
  unchanged. Initial attempts without an explicit Cargo target failed to execute
  build scripts; specifying `--target x86_64-pc-windows-gnu` resolved this.
- Isolated adapter tests and Clippy with `--tests -- -D warnings`: passed on
  both 1.98.0 and 1.100.0. Repository-wide `cargo fmt --check`: passed on both.
  The added version-state integration regression requires the full application
  test build and has not run; adapter tests are not a substitute for it.
- Native read-only scan with that compiled adapter found Claude Desktop, Codex,
  WorkBuddy and TRAE CN. Other desktop IDs returned absent on this host; their
  real installations remain unverified. This is installation evidence only,
  not native connection, launch, forwarding or updater acceptance.
- `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`, production
  dependency audit and the focused installation UI/hook/inventory suite
  (287 tests): passed. Commands used `--config.verify-deps-before-run=false`
  after pnpm 11's automatic dependency check attempted to purge the freshly
  installed modules without a TTY. Its generated workspace configuration change
  was removed; dependency manifests and lockfile are unchanged.
- Helper checks passed: development launcher (outside the restricted process
  environment), macOS DMG, TRAE bridge (71 passed, one skipped), model-default,
  Pi session, Doubao check-in, Agent rewards (outside the restricted environment)
  and CI merge policy.
- Helper failures remain explicit: WorkBuddy check-in auth has five failing
  assertions on Windows; Qoder codec's trust test invokes `/usr/bin/openssl`,
  which is unavailable here (one failed, one passed, one skipped). Storage
  recovery has one Windows directory-open `PermissionError` out of five tests.
  These files are unchanged by this discovery fix.
- Full renderer verification is incomplete: default fork workers timed out;
  the thread-pool retry reported failures in unchanged macOS signing tests and
  then exited with an unexpected worker exit, without an aggregate pass result.
  The focused 287-test suite completed successfully in the same checkout.
- Full `cargo check`, `cargo test --lib`, `cargo clippy -- -D warnings` and
  `cargo test` were attempted with the isolated toolchain and `--offline`;
  dependency resolution stopped at missing `aho-corasick`. This machine has no
  application dependency cache or installed MSVC build prerequisites. The full
  backend checks remain incomplete. The standalone adapter harness uses cached
  dependencies and does not validate the entire Tauri application.
- Cross-platform CI and native macOS/Linux runs have not been performed. No
  commit, push or merge has been made. Required checks have unresolved gaps;
  this is not a CI or all-Agent acceptance claim.

## Remaining acceptance

The all-Agent inventory is recorded in the coverage ledger. Synthetic discovery
coverage does not establish vendor filename accuracy for every installed build.
Portable apps outside known or registered locations, untested vendor layouts,
native macOS/Linux behavior and full application acceptance remain unverified.

## Submission verification after Windows toolchain setup

The user reported successful local verification on 2026-11-18 before requesting
the commit, push and PR. The exact client scenarios were not supplied, so this
does not broaden the per-Agent native acceptance recorded above.

The final working tree was checked on Windows 10 x64 with the installed MSVC
Rust/Cargo 1.100.0 toolchain, Visual Studio 2022 Build Tools and Windows SDK
10.0.26100.0, Node 24.22.1 and the declared pnpm 10.12.3. This supersedes the
earlier missing-toolchain limitation for the commands rerun below.

- Passed: `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`,
  `pnpm audit --prod`, `cargo fmt --check --manifest-path src-tauri/Cargo.toml`,
  `cargo check --manifest-path src-tauri/Cargo.toml`,
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and
  `git diff --check`. Cargo reported an existing unused `tower-http` manifest
  dependency warning; compilation and Clippy exited successfully.
- Passed again: installation UI/hook/inventory regressions (287 tests),
  development launcher, macOS DMG helper, TRAE bridge, model-default, Pi session,
  Doubao check-in, Agent rewards and CI merge-policy tests.
- Unchanged helper failures reproduced: five WorkBuddy credential-helper
  assertions; Qoder codec trust test's unavailable `/usr/bin/openssl`; storage
  recovery's Windows directory-open `PermissionError`. No tests or assertions
  were disabled to hide these results.
- Full `pnpm test:unit` was retried with pnpm 10.12.3; default fork workers again
  stalled without an aggregate result. A two-worker thread-pool retry also
  stalled and was interrupted. These runs are incomplete, not passing; the
  focused 287-test suite completed on this final tree.
- Full MSVC library and integration tests were started before submission. The
  library test binary's first code-generation/link step was still running when
  the PR was prepared; integration tests were queued behind it. Their results
  and the exact pushed revision's three-platform CI must be checked before
  acceptance. A successful build, focused suite or user manual check does not
  replace those pending results.
