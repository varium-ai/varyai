# Routing packet alignment verification — 2026-11-05

## Scope and checked tree

Base revision: `a30e86d1d11641cf40dbbdaf02c11985d3e34df0`, with the working-tree
changes listed below. The checkout contains unrelated concurrent changes; this
report does not claim acceptance of those changes. No commit or push was made.

Host: macOS 15.3.1, arm64. Toolchain: Node.js 24.11.0, pnpm 10.12.3,
Rust/Cargo 1.98.1. Commands were run from the repository root on this checkout.

| Changed file | SHA-256 at verification |
| --- | --- |
| `src/components/routing/RoutingPackets.tsx` | `8ff16058eb34fc8e20b75ca8aec8392540c83697d12843a2c24b30b9425510f3` |
| `src/components/routing/RoutingStage.tsx` | `e53c1fad32aa3fae45f477007c529af6b4903ca03675ce1d758665b6114d29b9` |
| `src/components/routing/routing.css` | `774af008039ab710fe4dd20cddaf5025c334b5b50426e529335085da55cca37d` |
| `tests/components/RoutingPackets.test.tsx` | `180a4f4716b7af71c9fd37eeaf00217571624a8bb2428a2f2eacd0905895c623` |
| `docs/architecture/routing-observations.md` | `6133b049abf46e5520518dad7f28498b43b9c7d98151e205afc1eb8764212336` |

## Behavior

- Remove per-request backward spacing and off-wire vertical overflow offsets.
- Keep waiting markers on their measured wire, inset 14 pixels from the
  destination or centered when the wire is shorter than 28 pixels.
- Aggregate only arrived, waiting packets on the same wire within one stage.
  Show one marker with a numeric badge for two or more waiting requests.
- Preserve independent outgoing, retry and return animations, completion
  callbacks and request lifecycles. Recompute counts after departures,
  removals and geometry changes without restarting other requests.
- Apply the shared behavior to both the workspace and compact tray.

## Passing checks

- Test-first reproduction: the updated packet suite initially failed seven
  assertions against the old implementation before the fix.
- Focused regression suite: `pnpm exec vitest run
  tests/components/RoutingPackets.test.tsx tests/components/RoutingStage.test.tsx
  tests/components/TrayPanel.test.tsx tests/hooks/useRoutingFlights.test.tsx`
  — 4 files, 58 tests passed.
- `pnpm test:unit` — 278 files, 3444 tests passed.
- `pnpm build:renderer` — passed.
- Scoped `pnpm exec prettier --check` for the three changed renderer files and
  the packet test file — passed.
- `pnpm test:dev`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec` — passed.
- `python3 -B scripts/tests/recover-shared-v23.test.py` — passed.
- `pnpm audit --prod` — passed; no known vulnerabilities found.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — passed on the retry.
- Scoped `git diff --check` — passed.

## Browser geometry and visual checks

A temporary Vite fixture rendered the real `RoutingStage`, `RoutingPackets`
and production styles with synthetic requests only. Headless Chromium verified
12 arrived requests in each of two independent stages. At viewport widths
1100, 650 and 360 pixels, the workspace stage measured 1052, 602 and 312 pixels;
the compact tray stage remained 440 pixels wide. Each stage had exactly one
visible waiting dot and one badge displaying `12`. Every held marker matched
its measured SVG path position within 0.01 pixels after resizing. Light and dark
marker screenshots were inspected; the fixture produced no browser page errors.
The temporary preview servers were stopped after verification.

This is component/browser verification, not native macOS WebKit, packaging,
installer, signing or live release acceptance. The user's installed/running
application was not launched, installed, replaced or restarted.

## Non-passing whole-checkout checks

These errors occur outside this patch; the affected files were left untouched.

- `pnpm typecheck` — failed in `SubscriptionProviderFields.tsx` because a
  `catalog` property is not accepted by the Codex `UpstreamSource` branch, and
  in `src/lib/query/copilot.ts` because quota fields may be null/undefined.
  No errors remained in the changed routing files or tests.
- `pnpm format:check` — failed for `SubscriptionAccountQuota.tsx` and
  `SubscriptionProviderFields.tsx`.
- `cargo check --manifest-path src-tauri/Cargo.toml` and
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — failed
  because an initializer in `src/services/subscription.rs` is missing the
  `SubscriptionQuota.codex` field.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml` — failed to compile
  for that missing field and unresolved `parse_claude_cli_usage` references in
  `src/services/native_subscription.rs` tests.
- `cargo test --manifest-path src-tauri/Cargo.toml` — the later snapshot compiled
  and ran the library suite: 5992 passed, 2 failed, 18 ignored. The failures were
  `commands::agent_connections::codex_status_tests::external_takeover_turns_the_switch_off_even_with_saved_intent`
  (`codex_status_tests.rs:73`, the switch was still enabled) and
  `services::agent_connections::selection::tests::native_model_switches_keep_connections_active_without_reapply`
  (`selection.rs:370`, external ownership did not turn the switch off).
  Both are outside this patch. Cargo stopped at those library failures, so the
  remaining integration-test executables were not accepted. Concurrent edits
  to unrelated files mean the earlier compile-check failures and this later
  test result describe different whole-checkout snapshots, not a single fully
  validated revision. The routing file hashes above stayed unchanged.

The initial broad run also encountered insufficient disk space during Rust
compilation; only that attempt's incomplete incremental cache was removed.
The reported frontend results and subsequent Rust diagnostics are from reruns.
Raw command logs are in `/tmp/vary-routing-checks/` on this host.

No remote CI was run because no revision was pushed. Linux, Windows,
Windows-to-WSL2 and native release acceptance remain unverified; the overall
checkout must not be described as verification-complete.
