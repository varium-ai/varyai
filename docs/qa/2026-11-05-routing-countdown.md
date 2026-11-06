# Routing reset countdown verification — 2026-11-05

Base revision: `a30e86d1d11641cf40dbbdaf02c11985d3e34df0`, plus the working-tree
changes below and the four runtime locale entries. This checkout also contains
unrelated concurrent changes. Host: macOS 15.3.1 arm64; Node.js 24.11.0,
pnpm 10.12.3, Rust/Cargo 1.98.1. Commands ran from this repository root.

## Behavior and reference

The shared workspace/tray reset display now follows Magpie's `dur` formatter in
`internal/gui/assets/plugins/routing/client.js` at
`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`. The reference file was clean in its
local checkout. Seconds and minutes round first; below ten hours, nonzero
minutes remain visible; below 48 hours, hours round; thereafter days round.
For the reported 88 hours 46 minutes, the label is **4 days**, localized as
`4 天后重置` in Simplified Chinese. Recorded reset timestamps and the live versus
historical observation-time selection are unchanged.

The formatter and `resetIn` wrapper support English, Simplified Chinese,
Traditional Chinese and Japanese. Both stage layouts and their hover text use
the shared formatter. Attribution is recorded in the routing reference notice.

## Verification

- Test-first stage reproduction failed in both layouts with the original
  `88 小时 46 分钟后重置` output.
- Focused tests: `pnpm exec vitest run tests/lib/routingDuration.test.ts
  tests/components/RoutingStage.test.tsx tests/components/TrayPanel.test.tsx
  tests/components/RoutingPackets.test.tsx` — 4 files, 77 tests passed.
- A direct comparison with the actual reference `dur` function matched for
  9,866 sampled durations, including the unit boundaries and a 30-day range.
- `pnpm typecheck` — passed.
- `pnpm build:renderer` — passed.
- Scoped `pnpm exec prettier --check` for the changed formatter, stage, tests
  and four locale JSON files — passed.
- Scoped `git diff --check` — passed.
- `pnpm test:unit` — 279 files, 3467 tests passed.
- `pnpm audit --prod` — passed; no known vulnerabilities found.
- Whole-checkout `pnpm format:check` — failed outside this patch in
  `AgentConnectionsPage.tsx`, `CodexProviderFields.tsx`,
  `SubscriptionAccountQuota.tsx`, `SubscriptionProviderFields.tsx`,
  `codexProviderQuota.ts`, `copilot.ts` and `types/subscription.ts`.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — failed outside this
  renderer patch; details are in the command log.
- `cargo check --manifest-path src-tauri/Cargo.toml` — passed after waiting for
  the shared build-directory lock.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml` — cancelled while
  waiting for another task's active compilation to release the shared target
  directory. Only this task's queued checks were stopped; the other build was
  left running. `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
  and `cargo test --manifest-path src-tauri/Cargo.toml` were not reached in that
  queue. Whole-checkout/native backend verification remains incomplete.

Raw command logs are in `/tmp/vary-routing-time-checks/`. The application was not
launched, installed or restarted. No commit or push was made, and no remote CI
or native release acceptance is claimed.

## Checked source hashes

- `src/lib/routingDuration.ts`: `5f824a331620263f8f8a6889359bfc75e3700d25dcc9a96c149434861f04b514`
- `src/components/routing/RoutingStage.tsx`: `f288500a84f8789b573c3a04ba5707b0da85a1f8ce159163e91fc59b02d1edd8`
- `tests/lib/routingDuration.test.ts`: `62a9cb94e7dca2744382e7bee9ac5e6527de494fcf603f09003ab713ea2aee84`
- `tests/components/RoutingStage.test.tsx`: `b4088a609c73a7585b0482711a41e2a11e00845fc14bf60aaf1bd1c9fdd6ee60`
