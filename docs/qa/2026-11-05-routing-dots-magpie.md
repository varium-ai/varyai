# Magpie dot-only routing animation verification — 2026-11-05

Base: `3839ed09adeaa36b6712a07f3c2c5266c92fac59`, with the current changes to
`RoutingPackets.tsx`, `RoutingStage.tsx`, `routing.css`, their packet tests and
routing/reference documentation. Host: macOS 15.3.1 arm64; Node.js 24.11.0,
pnpm 10.12.3, Rust/Cargo 1.98.1. Commands ran in the repository checkout.
This report records pre-commit validation. No application install or native
application restart was performed.

The requested presentation is small circular packets only. Numeric badges,
aggregation, halo discs, per-request offsets and bird markup/styles are absent.
Each request retains its own outgoing, wait, fallback and return lifecycle.
Concurrent requests at the same destination overlap at the same measured tip.

Reference: Magpie `1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`,
`internal/gui/assets/plugins/routing/client.js` (`holdAll`, `pose`, `frame`,
`pLayout`, `pTick`, `pPlay`) and the matching routing styles. The workspace uses
its 6px or quarter-wire endpoint clearance and cosine easing. The compact tray
uses its wire-tip positions, 3.5px dots, quadratic easing, 420/460ms outgoing
legs and green/red replies. Existing neutral cancellation semantics are retained.

- Test-first regression: nine packet assertions failed against the previous
  badge/aggregation implementation before the change.
- Focused tests: packet, stage, tray, flight lifecycle and countdown suites —
  5 files, 81 tests passed.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` — passed.
- `pnpm test:unit` — 279 files, 3471 tests passed.
- Browser fixture: the real components and production styles rendered twelve
  synthetic waiting requests in each stage at viewport widths 1100, 650 and
  360px. All independent dots remained visible and matched Magpie's measured
  wire-tip formula within 0.01px after resizing. Main/tray radii were 4.5/3.5px;
  no badge, halo disc, bird, SVG text or rectangle was present. No page errors.
  The resulting screenshot was visually inspected.
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — passed.
- `cargo check --manifest-path src-tauri/Cargo.toml` — passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml` — 6025 passed,
  18 ignored, no failures.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — passed.
- `cargo test --manifest-path src-tauri/Cargo.toml` — passed, including its
  library and integration-test executables.
- `pnpm audit --prod` — passed; no known vulnerabilities found.
- Scoped Prettier check including the packet test, and `git diff --check` — passed.
- Pre-commit helper checks: `pnpm test:dev`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec` and
  `python3 -B scripts/tests/recover-shared-v23.test.py` — passed.
- Pre-commit `pnpm audit --prod` — passed again.

All required renderer and Rust commands listed above passed on this local tree.
Remote CI must be checked separately for the exact pushed revision.

Raw logs and the synthetic browser preview screenshot are under
`/tmp/vary-routing-magpie-checks/`. This validates the local components and
checkout, not native WebKit, cross-platform CI, packaging or release acceptance.

## Checked implementation hashes

- `src/components/routing/RoutingPackets.tsx`: `2746a9477d7bf2e76311fd6590cfcca8259e3d4211d2066414d7f16fe13bd34f`
- `src/components/routing/RoutingStage.tsx`: `0ee1ae42d2072d2943dde6d0b73ebf6b766bab18577c13965e0763657d52ff7d`
- `src/components/routing/routing.css`: `446af9e2b46c1d02a4a1fe7c1595ff55290bccd56318f6a7a803d62e27235ced`
- `tests/components/RoutingPackets.test.tsx`: `b0311c315efda4fd93fa850af466914296f23f0d48c5fc2636ce7e04a3a03bde`
