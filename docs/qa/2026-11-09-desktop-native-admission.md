# Claude Desktop native gateway admission

## Scope and reference

Claude Desktop 4.23118 reports a Gateway credential failure when its startup
`config-health-probe` receives HTTP 403. A read-only comparison against the running
VaryAI gateway found that authenticated model-list requests succeeded without
Fetch Metadata, while the same requests with Electron Fetch Metadata and absent,
`null`, or `app://localhost` origins failed with `routing_local_clients_only`.
No upstream model request was needed for that comparison.

Magpie reference revision:
[`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`](https://github.com/yetone/magpie/tree/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e).
Its [origin guard](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/gateway/cors.go#L44-L145)
allows native app schemes and requires an authenticated caller for `Origin: null`.
Its [DNS-rebinding guard](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/gateway/lan.go#L490-L517)
does not reject Fetch Metadata alone for the gateway's own loopback host.
[Upstream tests](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/gateway/cors_test.go#L313-L386)
exercise both native and authenticated null origins. VaryAI retains its own
independent connection-token model and scopes this compatibility to Desktop's
supported native origins and endpoints. This change does not adopt Magpie's
permissive loopback authentication behavior or all of its native origin schemes.

The [Desktop probe implementation](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/gateway/desktop.go#L914-L950)
and [forwarding path](https://github.com/yetone/magpie/blob/1d0fc2bb08c51d8711759694189b8a1d92ee3a1e/internal/gateway/gateway.go#L2236-L2260)
recognize the one-token `.` request, apply a 16-token output floor, and call the
real upstream. VaryAI already implements those behaviors; the new end-to-end
regression verifies they remain intact after native request admission.

## Implementation and coverage

The shared Rust ingress admits Desktop's native model-list and Messages requests
from a verified loopback peer with an absent, `null`, or exact `app://localhost`
origin. Native admission still leads to the existing connection-token check.
Web origins, duplicate origins, forwarding metadata, unrelated routes, and
unsupported method combinations are rejected. No browser CORS grant is added.
The implementation is shared by macOS and Windows.

Regression coverage uses synthetic providers, in-memory storage, temporary
configuration, and dynamic loopback ports. It checks IPv4/IPv6 peers, exact native
origins, model discovery, both supported credential headers, missing and invalid
credentials, revoked connections, real upstream rejection, and the existing
16-token probe conversion. The installed applications are not restarted,
installed, or replaced for verification.

## Verification

The existing admission regression failed before the implementation change. The
end-to-end Electron probe also failed before the fix: expected HTTP 200, received
HTTP 403. An initial broad `desktop_native_` test filter additionally selected a
legacy gateway policy test that failed with HTTP 500 without an explicit test
home; subsequent backend runs use an isolated temporary `CC_SWITCH_TEST_HOME`.
The first post-fix end-to-end run reached the synthetic upstream and exposed an
incorrect preexisting test expectation of pass-through HTTP 403. The shared
router already sanitizes this rejection to HTTP 502; this fixture uses automatic
routing with one candidate, so its established error is
`routing_candidates_unavailable` after that upstream refuses the probe
(`intent/mod.rs`). The regression asserts both that status/code and actual
upstream receipt. Production upstream error handling was not changed.

Verification context: November 9, 2026; base revision
`fdeaa59822e118e7d62036aa7625565d2f1c2f4e` plus these working-tree changes;
macOS 15.3.1 (24D70), Apple Silicon; Rust/Cargo 1.98.1, Node 24.11.0, pnpm
10.12.3. Node differs from the repository's pinned 22.12.0. The checkout also
contains unrelated settings UI changes, which were retained.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 281 files, 3583 tests passed |
| `pnpm test:dev` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed initially; a later whole-tree run failed on concurrently added Codex migration tests in `services/intent_routing/client/mode_tests.rs` |
| `rustfmt --check --edition 2021 --config skip_children=true` on the three Desktop Rust files | Passed |
| `cargo test --offline --lib --manifest-path src-tauri/Cargo.toml proxy::connections::` | Final source: 16 passed, including the Electron probe, authentication, upstream failure, Desktop selection, and TRAE admission regressions |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --offline --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |

The initial full `cargo test --lib` run passed 6202 tests and found only the
incorrect Desktop upstream-error code assertion described above. The next full
`cargo test` run also picked up four new, concurrently edited Codex historical
migration tests, which failed independently of the Desktop ingress. Its compiled
Desktop test still contained the earlier error-code expectation. The final
Desktop suite was rebuilt after that expectation was corrected and passed all
16 tests.

The subsequent final full Cargo invocation remained blocked on another task's
build-directory lock and was cancelled. All integration targets were therefore
not completed in that pass. The freshly rebuilt library test executable is also
run directly with an isolated home, recording its exact executable hash, to
avoid contending for the shared Cargo lock. That run finished with **6204 passed,
12 failed, and 19 ignored**. The Desktop Electron probe passed. The failures were
two Codex app-server lifecycle assertions (an incomplete synthetic termination
sequence and a replacement timeout), four historical Codex migration assertions,
three Codex provider-parity assertions, and three Codex thread-lifecycle migration
assertions. Their failure output was inspected; this change does not assign a
single cause to those failures or alter their assertions. The executable SHA-256
was `9d11d679687ff6d31f58bf35ff8b227a1a73bcb409c6bba7b1aa59432ed57eac`.

Whole-tree backend verification remains incomplete while these failures and
concurrent edits are present; the scoped Desktop result is not a claim that the
entire moving checkout passed. Windows and Linux CI were not run.

Logs, source hashes, and executable hashes are under
`/private/tmp/vary-desktop-native-verification/`. Backend checks use cached offline
dependencies, `/tmp/vary-magpie-parity-target`, incremental compilation disabled,
debug information disabled, temporary test homes, and loopback additions to the
child processes' `NO_PROXY`/`no_proxy` values.

Local macOS checks do not establish Windows/Linux CI acceptance or live
installed-application acceptance. No commit, push, application installation, or
running client restart is part of this verification.
