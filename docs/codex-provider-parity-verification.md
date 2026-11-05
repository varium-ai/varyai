# Codex provider compatibility verification

## Scope and source

The Codex connection adapter is compared with
[`yetone/magpie` at `465928bccea4fec3537edd6280f7cabb9a0c5bad`](https://github.com/yetone/magpie/tree/465928bccea4fec3537edd6280f7cabb9a0c5bad).
Relevant source areas are `internal/agent/codex.go`,
`internal/sessions/codex_provider.go`, and the provider transition regression
tests. The implementation uses VaryAI's existing Rust gateway, authentication,
configuration ownership, and private recovery storage.

The checked checkout is based on `25d6b901658d4fd96a7d783542670ab7c53667d7`
with preexisting uncommitted changes plus this task's changes. It is not a clean
revision or a release build. No installed application, live Codex configuration,
or real conversation history was used as a test fixture.

Other work continued editing the shared checkout during verification, including
scheduling, shared catalogs, imports, and onboarding. The source manifest at the
start of final verification was
`134909f94295f78f17bcf2e048e966e7758343d61bd69ccba5a8ab38969ade71`.
The local manifest is `/private/tmp/varyai-magpie-final-tree.json`; later changes
were detected and the affected build and test checks were rerun. These results
are observations of the checked working trees, not acceptance of an immutable
commit or of subsequent changes made by another task.

## Environment

- Date: November 5, 2026 (Asia/Shanghai).
- Host: macOS 15.3.1, Apple Silicon.
- Rust/Cargo: 1.98.1.
- Node: 22.22.2; the repository's exact 22.12.0 was not installed locally.
- pnpm: 10.12.3, matching `package.json`.
- Tests use synthetic credentials, temporary directories, in-memory or temporary
  SQLite databases, and dynamic ports for listeners.

## Results

| Check | Result |
| --- | --- |
| Initial provider-mode regressions | Four expected failures before implementation |
| Initial conversation lifecycle regressions | Two expected failures before integration |
| Codex client, native gateway, and connection status tests | 107 passed in the final full Rust run (92 client, 13 native, 2 status) |
| `AgentConnectionsPage` tests | 112 passed in the final focused run |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Completed; circular chunk warning remains in onboarding/shared chunks |
| `pnpm test:unit` | Latest run: 3405 passed, 17 failed across 3 files; the earlier 3406-pass run does not validate the later shared-tree edits |
| `pnpm format:check` | Passed |
| `pnpm test:dev` | 8 passed |
| `pnpm test:trae-bridge` | 52 passed |
| `pnpm test:workbuddy-checkin-auth` | 5 passed |
| `pnpm test:doubao-checkin` | 13 passed |
| `pnpm test:qoder-codec` | 2 passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | 5 passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Ran; superseded by the full run below after correcting the native fixture's expected request count |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed after replacing post-MSRV file-lock APIs with platform lock calls |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Library: 5960 passed, 4 failed, 18 ignored; integration target `profile_bundle` aborted, remaining integration targets passed |

The initial sandbox denied local loopback listeners and npm DNS resolution.
The affected helper tests and dependency audit passed when rerun with the
required local networking or public registry access.

The latest full Rust failures are the `pace_uses_budget_per_hour...` and
`smart_ranks_quota_reset...` scheduling tests, plus
`parity_saved_group_keeps_available_members_when_a_provider_is_disabled` and
`shared_catalog_keeps_account_models_and_routes_without_access_token_refresh`.
The TRAE profile integration fixture rejected an unexpected bridge operation,
then panicked again while joining its fixture worker. None is a failing Codex
provider transition or native ingress regression.

The latest renderer failures are in `WorkBuddyHome.test.tsx`,
`rendererBuild.test.ts` (circular chunks), and `integration/App.test.tsx`
(the legacy fixture expects the previous onboarding confirmation button).
They were observed after concurrent changes to those areas. Full repository
verification therefore remains incomplete. Logs are retained under
`/private/tmp/varyai-magpie-final-*.log`; no assertions were disabled or weakened
to report a passing repository run. The macOS test linker also emitted its
existing large `__eh_frame` warning.

## Acceptance limits

Local checks do not establish Linux, Windows, WSL2, installer, signing,
notarization, or live desktop acceptance. This task has not pushed a revision,
so cross-platform CI acceptance is incomplete. An API-key-only configuration
and a supported ChatGPT connection intentionally use different provider modes.
Ordinary connect and background refresh retain ownership checks for existing native gateway URLs. Explicit reapply can replace a saved gateway URL and retain its restore baseline.

This verifies the Codex provider connection path, not a complete port of every
Magpie feature. Native ingress currently accepts the tested Codex 0.114, 0.165,
and 0.166 families with file-backed ChatGPT authentication. Keyring/auto auth
stores and WebSocket forwarding are not enabled by this change; native clients
must use HTTP. Quiet history migration currently limits a decoded rollout to
64 MiB and a synchronization pass to 256 MiB of file data. Oversized files,
packing conflicts, and paginated metadata that cannot fit its original byte
length remain pending with recovery records retained. Live client behavior
outside those boundaries still requires separate compatibility work.

## Correction: background conversation migration status

The conversation-provider pending label and its recovery instructions were a
Vary-only UI addition. They were not present in the pinned Magpie reference.
Magpie's Codex `Check` inspects configuration health, while
`codexThreadsLater` schedules busy conversation migration in the background.

The correction removes that label, its four translations, and the mapping from
the private migration receipt to a public configuration issue. Background
migration and retries continue; deferring a busy conversation does not turn an
otherwise valid connection into a recovery state or disable model selection.
Actual missing-provider, profile-override, and missing-catalog checks still apply.

The active-conversation regression failed before the correction because status
returned `ThreadProviderPending` instead of no configuration issue. After the
correction it verifies that the private pending flag remains set, the public
configuration warning is absent, and migration completes when the file becomes
quiet.

Follow-up checks on the shared working tree:

- Codex client library tests: 92 passed.
- Agent connections UI tests: 110 passed.
- TypeScript typecheck, renderer build, and Prettier check: passed.
- The three changed Rust files pass focused formatting checks. The whole-tree
  formatting check reports a concurrent difference in
  `services/agent_connections/desktop_config.rs`.
- Whole-tree Clippy is blocked by `collapsible_if` in that concurrently edited
  `desktop_config.rs`; no Clippy suppression was added.

Logs are `/private/tmp/varyai-remove-custom-sync-status-*.log`. Earlier full-suite
results above remain historical; complete repository and cross-platform
acceptance is still incomplete.

## Correction: explicit reapply with deferred recovery

The previous service implementation disconnected Codex before applying the
selected projection again. A busy conversation could keep the old recovery
receipt alive after the configuration had been restored. The subsequent connect
then rejected that same receipt as a configuration conflict. This behavior did
not match Magpie's direct configuration update and deferred conversation moves.

Explicit reapply now updates the projection without first disconnecting. If the
live configuration differs from its old ownership record, the explicit action
archives that record privately, preserves external choices in the next restore
baseline, and publishes a fresh projection. Conversation backups remain intact.
Ordinary connect and background refresh retain their ownership checks.

The synthetic service regression reproduced `intentRouting.clientConflict`
before the fix. It now reapplies successfully with a busy conversation, preserves
that conversation and authentication file, adopts external settings on explicit
reapply, rejects an empty catalog without changing configuration, and restores
the saved choices after shutdown. A separate client regression covers a recovery
receipt already stuck in deferred restoration and verifies that its archived
record and conversation backups survive.

Verification on macOS with the toolchain above, based on
`a30e86d1d11641cf40dbbdaf02c11985d3e34df0` plus working-tree changes:

- Reapply regressions: 6 passed.
- Codex client tests: 93 passed.
- Native Codex service lifecycle tests: 10 passed.
- `cargo check`, `cargo fmt --check`, Clippy with `-D warnings`, and
  `git diff --check`: passed.
- Scoped source manifest:
  `508c8ee9136a3a3387460d04b9441b2322b9f11e76727fef5798f85e8e757a41`,
  recorded in `/private/tmp/varyai-codex-reapply-tree.json`.

Logs are `/private/tmp/varyai-codex-reapply-*.log`. The running application and
real conversation files were not modified by these checks. These focused results
do not replace the earlier full-repository and cross-platform acceptance gaps.


## Correction: replacing a saved native gateway on explicit reapply

The pinned Magpie `set` path writes `openai_base_url` directly when applying a
model through its native Codex gateway (`internal/agent/codex.go`, around line
1604). Vary previously rejected an existing unowned URL even during explicit
reapply, leaving the old gateway address in place.

Explicit reapply now replaces an existing string URL. The private receipt stores
the replaced URL with its acquisition baseline, carries it across refreshes, and
restores it on disconnect. Ordinary connect and background refresh still reject
unowned URLs. Authentication files are not changed. Reapply is available when
native compatibility is unavailable, and the manual configuration/restart hint
has been removed from all four runtime languages.

Generated model display names no longer append `VaryAI`. Upstream model IDs and
routing aliases remain unchanged.

The client regression failed with `intentRouting.nativeConflict` before this
change. The UI regression failed in all four unavailable native states because
the reapply button was absent and the manual hint remained. The service
regression covers ordinary activation preserving an external URL followed by
explicit reapply acquiring it and disconnect restoring it. All fixtures use
synthetic authentication and temporary configuration directories.


Live verification exposed another mismatch: the Vary process inherited
`OPENAI_BASE_URL` from its launcher. Treating that environment as evidence of
an independently launched Codex client's configuration incorrectly rejected
native setup as unsupported. Native setup and reconciliation now validate the
selected Codex configuration, authentication file, ownership receipt, and
incoming authentication directly. They do not infer the client's environment
from Vary's process. The service regression reproduces the inherited launcher
variable using a synthetic URL; it failed as unsupported before the correction.


Verification for this correction on macOS / Rust 1.98.1, based on
`a30e86d1d11641cf40dbbdaf02c11985d3e34df0` plus shared working-tree changes:

- `pnpm typecheck`, `pnpm format:check`, and the bundled renderer build passed.
- `pnpm test:unit`: 278 files and 3,439 tests passed.
- Isolated `cargo test --manifest-path src-tauri/Cargo.toml`: 6,145 passed,
  18 ignored across library and integration binaries before the launcher
  environment correction. After that final Rust change, the affected
  `native_codex_` tests were rerun: 12 passed.
- The explicit native URL replacement regression passed, including a subsequent
  refresh and restoration of the previous URL on disconnect.
- `cargo fmt --check` and `git diff --check` passed.
- The first final build and Clippy attempts ran out of disk space. Three inactive
  Rust incremental caches were removed before retrying; source and user data
  were retained.

The final scoped source hashes are recorded in
`/private/tmp/vary-native-reapply-final-tree.json`. Test logs use the
`/private/tmp/vary-native-reapply-*` and
`/private/tmp/vary-native-launcher-*` prefixes. No commit, push, or remote CI
verification was performed. The 18 ignored Rust tests and platform CI remain
outside this local acceptance.


The retried host debug app bundle and Clippy (`-D warnings`) passed after cache
cleanup. Vary AI was quit and relaunched from the newly built bundle
(`tauri://localhost`, rather than the old development server). Explicit Codex
reapply was performed through the UI. Read-only comparisons confirmed that the
live native URL matches the installed receipt, the replaced URL is retained for
restoration, the authentication file is byte-for-byte unchanged, and unrelated
top-level settings are unchanged. The UI reports a configured connection and
allows model selection; its selected model label is `Qwen3.8-27B · gs`, without
the generated brand suffix. This verifies configuration application and the
running UI; no live model completion was sent as part of this check.

Final `cargo check --manifest-path src-tauri/Cargo.toml` passed against the
recorded source hashes after the launcher correction and live reapply.
