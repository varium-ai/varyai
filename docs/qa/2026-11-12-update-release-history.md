# Update release history

Date: 2026-11-12

## Problem and behavior

The previous update dialog displayed only the target release. When the update
changed from `0.3.3` to `0.3.4`, the `0.3.3` changes disappeared even for a client
still running `0.3.0`. Inline version labels alone did not implement the requested
history across skipped releases.

The dialog now independently requests published releases in the range
`installed < release <= target`, sorts by semantic version, and displays the
latest 40 versions in descending order. Each release has its own heading and each
list item retains the version of the release supplying its description. Manifest
notes remain immediately available and override the public description for the
target version. Every description is selected using the existing interface
language and translation fallback rules. Relative links use that release's page.

Drafts, out-of-range versions, duplicate versions and prereleases for stable
targets are excluded. History requests have an eight-second total timeout and a
five-page limit (100 releases per page). Incomplete history is explicitly labeled
when requests fail or the limits omit records. Cancellation is supported. Update,
dismissal and portable download actions remain usable independently of history.
An unknown installed version preserves the target-only fallback.

## Checked source and environment

Working-tree changes on `a0bda872a8c5c97711eab1dd2d0a661d09201ebc`, including
existing concurrent provider, routing and model-catalog edits. This task changes
the release-note helper, update dialog and styles, three keys in each of the four
runtime locale files, focused tests, and release documentation. Unrelated edits
were preserved.

Host: macOS 15.3.1 (Darwin 24.3.0, arm64), Node.js 24.11.0, pnpm 10.12.3,
Rust and Cargo 1.98.1. Command logs: `/tmp/vary-update-history-checks/`.

## Test-first and visual verification

Eight new expectations failed before implementation: missing history retrieval,
range filtering and version attribution, and incomplete-history handling. All
focused release-note and dialog tests passed after implementation (32 tests).
Coverage includes numerical and prerelease version ordering, pagination,
deduplication, range bounds, cancellation, timeout, localization, target manifest
precedence, correct older-release link resolution, and installation availability.

An isolated Vite fixture rendered the actual dialog using a snapshot of the
public GitHub release descriptions. Native update operations were mocked. At
1280 by 720, the Chinese view showed `0.3.4` and `0.3.3` simultaneously, with
`0.3.2` and `0.3.1` in the scrollable body and installation actions fixed below.
The installed application was not launched, installed, updated or replaced.
No remote release descriptions were edited.

## Check results

| Command | Final result |
| --- | --- |
| `pnpm typecheck` | Passed on final rerun |
| `pnpm build:renderer` | Passed on final rerun |
| `pnpm format:check` | Passed on final rerun |
| `pnpm test:unit` | Passed on final rerun: 302 files, 4,042 tests |
| Focused release-note and update-dialog tests | Passed: 32 tests |
| Prettier for the two modified test files | Passed |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Passed: 6,340 tests, 19 ignored by existing suite |
| `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` | Passed: library and all 164 integration tests |
| `git diff --check` | Passed |

An initial full frontend run passed 4,029 tests and failed five routing drag
preview assertions while that separate code was being edited. A focused rerun
then passed all 44 tests across the two release-note suites and the routing-drag
suite. The complete frontend checks were restarted against the updated tree.

## Limits

No commit or push was requested. Remote CI, Linux, Windows and Windows-to-WSL2
checks were not run. These shared renderer changes apply on macOS and Windows,
but local macOS success does not establish cross-platform or installer acceptance.

All hashes below were rechecked after the final verification run.

## Checked file hashes (SHA-256)

```text
bce3546210ce5a9f7f1bb215c494fea853b63a5254457c9b2ab7e2c7c3401c3e  src/lib/releaseNotes.ts
9709fc383adbd497aa586954164e8f13092055f7f215eecf0f27d27c04210372  src/components/AppUpdateDialog.tsx
b89f38109c95a84f511da31977bcae37b3e30113997c4bc693ca2e7250b4d553  src/components/app-update-dialog.css
c482aa4b8837ce86f9a10ad53694c52e44110588379093a468708d33ae154b25  src/i18n/locales/en.json
8620f724ea0f3a7bd4edde914011b6db6f7dc32f8434af344bd912a0ec3e0f7f  src/i18n/locales/zh.json
aa2ba0d9c4aee0a3fec512d21a1250d9573c012c57a99818bc9f8358af393a6c  src/i18n/locales/ja.json
249c1cef013c81fcbd2d29ec21f561c6efc8ddd1d30dd5d7548ec8431d3135dd  src/i18n/locales/zh-TW.json
15d9332e7cf2747f7f4984c53aec215697c6c857be8f6ae9f3e854b98ad85276  tests/lib/releaseNotes.test.ts
33cdfe285b5231706d837bcb2dba55b449cac053b67c9f7d4ce879ed57a9bb04  tests/components/AppUpdateDialog.test.tsx
a4820ad84c06210cae521b54c81e80e1e84a9f1c0b18d505fd7973ee4fcd94b9  docs/guides/release-distribution.md
```
