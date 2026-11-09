# Bug-list follow-up — November 6, 2026

## Observed scope

The user clarified that the task concerns the currently open Lark Bug Management
page, not just unfinished changes in the checkout. A read of its existing
follow-up view showed 11 records: WorkBuddy installation detection, provider
notes for Qwen/Kimi/TRAE, and nine security/tool-observation reports. Filters,
record text, status and comments were not intentionally changed. The two
phone/identity-card reports excluded in the earlier review remain outside this
follow-up's acceptance claims.

The browser tab was already attached to another session and browser focus also
changed during inspection. Native Chrome observations established the visible
list and the disabled-observation save report's details. The Lark CLI URL resolver
worked, but block lookup required missing user authorization; no authorization
or tracker write was attempted. Attachment contents were not fully reviewed.

## Reproduced fixes

### History refresh retained stale rule names

The policy-history panel refreshed only history rows. Its separate rules query
could retain an old custom-rule name after a rename, while the panel announced a
successful refresh. Refresh now reloads both datasets, keeps refresh and clear
disabled until completion, and reports a sanitized failure if either read fails.
Stable IDs and the existing current-name rather than historical-name boundary
are retained.

Two regressions failed before implementation because the name lookup never
refreshed. They now cover a delayed successful rename and a failed name lookup,
including the busy state and absence of private error text. All 12 panel tests
pass after the fix.

### Deflate overflow lost its size-limit classification

During the initial request-decoding investigation, a synthetic zlib-wrapped
deflate stream exceeding the output limit reproduced a separate defect: raw
deflate fallback replaced `TooLarge` with an I/O decoding error. The decoder now
retains the size-limit error when both formats fail, while allowing a successful
raw-deflate fallback. This preserves the callers' explicit oversized-body
rejection instead of their ordinary decoding-failure behavior.

The regression failed on the original implementation. All 16 content-encoding
tests pass after the fix, including wrapped/raw deflate, exact limits, overflow,
invalid data and existing stacked-encoding cases. An isolated Rust harness
compiled the actual source against existing dependency artifacts to obtain the
before/after result without waiting on another session's Cargo build lock; the
regression also passed through Cargo.

## Verification boundary

Base revision: `5984530cbe725447dc68628431ecef7b4b96b00a`, with pre-existing
uncommitted WorkBuddy and native Codex changes preserved. Host: macOS 26.6.2 arm64;
Node 22.22.0, pnpm 10.12.3, Rust/Cargo 1.98.1. Logs use the
`/tmp/varyai-continue-` prefix. Other sessions were also using this checkout, so
these checks do not establish an immutable release revision. The tracked patch
for this follow-up's decoder, history panel/test and architecture documentation
has SHA-256 `df3145ebee0621fde14c57a21edd5a8fe80550abd0fee6873bf731bc5bcdd84d`.

The first complete frontend run passed 3,489 tests. A later run, while concurrent
installation-card regressions were being added, failed six of those new cases.
After the concurrent implementation was updated, the focused installation suite
passed all 22 tests. A fresh complete frontend run on that tree passed all 279
files and 3,495 tests.

| Check | Result |
| --- | --- |
| History panel before/after | Two new cases failed before; all 12 pass after |
| Content-encoding before/after | New overflow case failed before; all 16 pass after |
| `pnpm typecheck` | Passed on the updated installation-card tree |
| `pnpm build:renderer` | Passed on the updated installation-card tree |
| `pnpm format:check` | Passed on the updated installation-card tree |
| `pnpm test:unit` | 279 files and 3,495 tests passed on the updated tree |
| `pnpm test:dev`, `pnpm test:trae-bridge` | Passed |
| WorkBuddy, Doubao, agent-rewards and Qoder helper suites | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed, no known production vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 6,069 passed, 18 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 6,232 passed, 18 ignored across the full run |
| `git diff --check` | Passed |

No commit, push, installed-app replacement or restart was performed. Reporter
acceptance, attachment comparison, operating-system notification delivery and
cross-platform CI remain separate from local source verification. Existing
coverage for the other reports is summarized in
[the earlier review](2026-11-06-browser-bug-review.md).
