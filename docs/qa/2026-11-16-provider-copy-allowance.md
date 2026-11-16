# Copied API providers and relay allowance discovery

Reference: [Magpie revision `58f24cc`](https://github.com/yetone/magpie/tree/58f24cc23527776b5f1958cfd4a8092a659bdfb5),
`internal/gui/assets/app.js` (`duplicateProvider`, `draftOf`, `renderProviders`),
`internal/provider/balance_probe.go`, and `internal/provider/balance.go`.

## Behavior

API providers and their saved copies no longer mount allowance queries in the
Providers list. Subscription summaries remain unchanged. Tray cards continue to
read API allowances. A copied provider does not gain an allowance query just
because its ID or revision is new.

The backend shares relay interface discovery by a SHA-256 digest of the normalized
origin and credential. It serializes concurrent discovery for that identity and
keeps found interfaces for 24 hours, missing interfaces for six hours, and
unreachable/unavailable discovery for five minutes. Found and missing results
survive restart in the private device-local `relay-balance-probes.json` file.
The file stores only digests, fixed interface identifiers and expiry times. It
contains no endpoints, credentials, balances or quota responses. Cache failures
remain auxiliary, and all disk work runs outside request forwarding and database
locks. The shared Rust implementation and private atomic writer support macOS
and Windows.

Each balance observation still reads a fresh amount from a discovered interface.
Manual card/header refresh bypasses discovery expiry. An unsuccessful first
discovery hides the tray card; a known interface's later failures remain visible,
retaining the last successful amount with stale status. Explicit authentication
rejection clears the amount. Renderer retry timing honors the remaining backend
cache interval, including the shorter five-minute retry for discovery failure.
The configurable Balance URL and Auto-detect controls from Magpie are outside
this focused change.

## Regression evidence

The new list regression failed before implementation: both API rows mounted an
allowance status/query. The initial backend regressions failed with duplicate
interface discovery for the same endpoint/key, both on a miss and on subsequent
balance reads. Final focused checks passed 123 frontend assertions across the
provider panel/parity and quota suites, and all 11 backend allowance tests.
Additional coverage checks restart reuse, credential/origin isolation, expiry,
manual retry, temporary discovery failure, known-interface errors, explicit
rejection, corrupt/unwritable cache recovery, and five-minute tray polling.
Tests use synthetic data, dynamic loopback ports and temporary cache directories.

## Checked tree and environment

- Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`, plus the shared working-tree changes.
- Host: macOS 15.3.1 (24D70), arm64; Node 22.22.2, pnpm 10.12.3, Rust/Cargo 1.98.1 stable.
  The local Node 22.22.2 runtime differs from the exact 22.12.0 CI pin.
- Scoped source hashes: `/tmp/vary-provider-quota-verification/scoped-source-hashes.json`.
  SHA-256 of the sorted compact JSON mapping: `afb741c2b48dfa5d2b15ce206c1dfbc57aaa4c09b3ea6e66da467a1bc7a1ffe0`.
- Per-command results and initial/final tree snapshots are in
  `/tmp/vary-provider-quota-verification/`. The final frontend runner is
  `frontend-final-tree`, backend runner is `backend-final`, and integration
  runner is `integration`. `fmt-final.log` records the final formatting check.
- Existing tray, usage-source and provider-name edits were preserved. Earlier
  full checks overlapped independently applied name-validation changes. The
  final frontend pass uses the recorded frontend source. Independent Rust
  name-validation fixtures and `CodexProviderEditor.test.tsx` subsequently
  changed, so these full-suite results do not establish acceptance of that
  newer complete tree.
  The scoped allowance implementation remained unchanged through the final
  checks; all eleven allowance tests passed in both backend full runs.

## Verification results

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed: 316 files, 4,218 tests |
| `pnpm test:dev`, `test:macos-dmg`, `test:trae-bridge`, `test:model-default` | Passed |
| `pnpm test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`, `test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | Passed: no known vulnerabilities; the initial connection reset recovered on retry |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed, including the final repeat |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Failed: 7,595 passed, 6 failed, 21 ignored |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Same six library failures; does not proceed to integration targets |
| `cargo test --manifest-path src-tauri/Cargo.toml --test '*'` | Passed: 14 targets, 165 integration tests |
| `git diff --check` | Passed |

The six backend failures belong to the independent provider-name uniqueness
change, not the allowance service:

- `services::intent_routing::catalog::tests::missing_catalog_discovery_reuses_stored_connection_cache_without_changing_selections`
- `services::intent_routing::tests::automatic_groups_default_to_off_until_explicitly_enabled`
- `services::intent_routing::tests::automatic_groups_follow_catalogs_and_preserve_user_overrides_and_hides`
- `services::intent_routing::tests::discovery_tracks_catalogs_independently_of_transport_eligibility`
- `services::intent_routing::tests::normalized_discovery_preserves_legacy_manual_overrides_and_hidden_groups`
- `services::provider_import::tests::native_import_is_atomic_deduplicates_a_batch_and_retains_redacted_secrets`

Five report `routingUpstreams.nameExists` while importing fixture providers;
the sixth expects a same-name copy to succeed under the earlier contract.
No checks were disabled or assertions weakened for this change. Overall backend
verification is incomplete while these failures remain. Linux, Windows, WSL2
and remote CI were not run. No commit or push was made, and no installed
application or AI client was launched, installed, replaced or restarted.
