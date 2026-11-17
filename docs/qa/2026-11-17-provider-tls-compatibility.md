# Provider TLS compatibility — 2026-11-17

## Scope and evidence

Model discovery for an authorized custom provider succeeded from Python and
Magpie but failed from VaryAI before an HTTP response. The same Rust client
could reach control HTTPS services. The installed Magpie binary identifies
itself as v0.1.4281, built with Go 1.26.8; the local reference source was
`yetone/magpie` at `58f24cc23527776b5f1958cfd4a8092a659bdfb5`.

The platform-native TLS client and ring-based Rustls client reproduced an early
connection close. AWS-LC's modern default ClientHello succeeded. Removing its
hybrid key-exchange groups reproduced the failure. A Go control still negotiated
classical X25519 successfully, so this evidence establishes ClientHello
compatibility with the affected network/provider gateway, rather than proving
that the upstream requires a hybrid negotiated group.

An additional regression reproduced failure on the second new connection: a
shared Rustls configuration remembered a classical key-exchange hint, replacing
its initial hybrid key share on subsequent handshakes. Keeping the modern initial
shares fixed resolved repeated requests and both forwarding paths.

## Implementation

- Reqwest discovery/detection and both Hyper forwarding paths share an explicit
  AWS-LC TLS configuration, independently of the ring provider installed for
  local listeners.
- TLS 1.3 hybrid and classical groups and TLS 1.2 remain available. Native system
  and Mozilla roots are loaded, with certificate and hostname checks enabled.
- A bounded session store retains TLS session/ticket caching while declining
  key-exchange hints. A complete second handshake verifies session resumption
  still works.
- Proxy selection, redirect restrictions, HTTP headers, model IDs, and API
  protocols are unchanged. The implementation is shared across macOS and Windows.

## Regression and live checks

Five synthetic regressions cover the ClientHello sent by all three application
client paths, hybrid/classical/TLS 1.2 negotiation, certificate and hostname
rejection, repeated connections after classical negotiation, and session
resumption. The original wire regression failed before the TLS change; the
repeated-connection regression failed with a HelloRetryRequest before the
session-store change. Both pass with the final configuration.

Temporary drivers compiled the actual repository HTTP and Hyper client modules
and TLS configuration. Using the user-authorized credential in memory only:

| Path | Result |
| --- | --- |
| Model discovery | HTTP 200; one model, `zai-org/GLM-5.3-Flash` |
| Hyper forwarding | HTTP 200; the same one-model catalog |
| Original-header forwarding | HTTP 200; the same one-model catalog |
| Chat Completions | HTTP 200; `OK`, normal completion |
| Three fresh connections using the shared configuration | All reached HTTP authentication rather than failing TLS |

The synthetic chat probe used a 1,024-token ceiling so reasoning could finish.
No credential, private configuration, raw reasoning, or raw response body is
included in this record. The installed application bundle and OS trust stores
were not replaced. No UI acceptance or signed installer acceptance is claimed.

## Initial checked tree and commands

Base revision: `1632471a043cb49c4e27d12b97407f8c1beb9dec` plus the working-tree changes in
`src-tauri/Cargo.toml`, `src-tauri/src/proxy/{http_client,hyper_client,mod,upstream_tls,tls_compat_tests}.rs`,
this record, and `docs/troubleshooting.md`. Pre-existing unrelated desktop-agent,
localization, and test changes were preserved and included in the local tree.

Source manifest SHA-256: `22388e9b210e497c3b03ef43365824d49c21c8b57e1761f30d6bbde7f2cab663` across
1753 source, test, script, workflow, manifest, and guide files,
including the two new TLS source files. The verification record itself is excluded.

Host: `macOS-15.3.1-arm64-arm-64bit-Mach-O`. Toolchain: `rustc 1.98.1 (48a229cea 2026-09-01)`,
Node `v24.11.0`, pnpm `10.12.3`.

| Command | Final local result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | 321 files; 4,276 tests passed |
| `pnpm audit --prod` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 7674 passed; 21 existing ignored tests |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Same library result; 165 additional integration tests passed |

Final Cargo checks used `CARGO_INCREMENTAL=0` and
`--config profile.dev.package.varyai.debug=0 --config profile.test.package.varyai.debug=0`
to limit disk usage without changing assertions or test selection. An initial
full-debug compilation ran out of space; only regenerable crate incremental
caches were removed, and the affected checks were rerun successfully.
The test build retained its pre-existing `quota_panel::Manager::expiring` dead-code
warning and the macOS linker compact-unwind warning.

The five new TLS regressions ran in the full library suite. The 21 ignored
library tests were not exercised. No commit or push was performed. Linux,
Windows, macOS remote CI, and the manifest-triggered Windows-to-WSL2 job have not
run against this working tree; cross-platform acceptance remains incomplete.

## PR integration verification

The submission includes confirmed desktop restart changes and the standalone
`pelican-bike.html` source.
It is integrated on `95a26608e4e1beed228ab51525372dc631d9331d` from `main`.
The stopped-application regressions and Windows installation candidates from
`main` are retained alongside bounded process resampling and verified relaunch
handling. The four locales retain upstream recovery guidance while internal
state-change diagnostics stay out of the UI. The full inventory assessment is in
[Agent feature coverage](agent-feature-coverage.md#provider-tls-compatibility-and-confirmed-desktop-restart-races).

Final checks passed against this integrated tree on macOS arm64, using the
same Rust 1.98.1, Node 24.11.0 and pnpm 10.12.3 toolchain noted above.
The source manifest SHA-256 is `1f40fe1f440cf6f459d0a8d585e2c693bbb485cce566a3faae796989fbfc45ab` over
1763 relevant files, excluding this verification record.

| Command | Integrated-tree result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm test:unit` | 322 files; 4,279 tests passed |
| `pnpm audit --prod` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 7,690 passed; 21 existing ignored tests |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Same library result; 165 integration tests passed |
| `pnpm test:dev` | Passed |
| `pnpm test:macos-dmg` | Passed |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:model-default` | Passed |
| `pnpm test:pi-session` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:agent-rewards` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `node --test scripts/tests/ci-policy.node.mjs` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `node --check` on the extracted pelican page script | Passed |

Cargo used the same non-incremental, package-debug overrides recorded above.
Windows native application execution and per-Agent native acceptance remain
unverified locally. Remote Linux, Windows, macOS and Windows-to-WSL2 CI results
must be checked for the exact pushed head before merge; they are not established
by the local results in this commit.
