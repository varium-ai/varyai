# Codex provider lifecycle alignment with Magpie

Date: November 9, 2026. Starting revision:
`fdeaa59822e118e7d62036aa7625565d2f1c2f4e`, plus existing uncommitted work.
Unrelated Desktop admission, settings UI, and documentation changes were preserved.

## Reference and reproduced differences

The reference is Magpie revision
[`9a00bfed14ee3aa56f7cfe78c5b90ad88b5f3757`](https://github.com/yetone/magpie/tree/9a00bfed14ee3aa56f7cfe78c5b90ad88b5f3757),
resolved from its main branch on November 9. Relevant implementation:

- [`internal/agent/codex.go`](https://github.com/yetone/magpie/blob/9a00bfed14ee3aa56f7cfe78c5b90ad88b5f3757/internal/agent/codex.go):
  `bringOver`, `bringBeside`, `lendOwn`, `giveLent`, `putProvider`, and
  `dropProvider`. The `lendOwn` comment records Codex displaying the same
  provider name on both sides of a mismatch while the internal provider IDs
  differ. Provider transitions do not depend on whether the saved model is
  currently published.
- [`internal/sessions/codex_provider.go`](https://github.com/yetone/magpie/blob/9a00bfed14ee3aa56f7cfe78c5b90ad88b5f3757/internal/sessions/codex_provider.go):
  paginated history changes its SQLite provider row, preserving rollout bytes,
  model identity, and pagination offsets. Writer locks and recent writes defer
  migration.
- [`internal/provider/codex_app_server.go`](https://github.com/yetone/magpie/blob/9a00bfed14ee3aa56f7cfe78c5b90ad88b5f3757/internal/provider/codex_app_server.go):
  route changes reload idle desktop-owned app-servers; replacement has its own
  30-second window after the old process exits. Each target is attempted even
  when another target fails.

Before the migration fix, the synthetic legacy-history regression failed with
an unpublished saved model: the provider remained `magpie` instead of becoming
`varyai`. The saved model gate skipped the conversation permanently rather
than treating provider ownership and model execution as separate concerns.
Before the watcher fix, an old process that exited 20 seconds after its signal
exhausted the entire replacement window immediately; the regression observed
`TimedOut` instead of continuing to wait for a new app-server.

## Implemented behavior

- Provider migration preserves saved model identities independently of the
  published model catalog. Paginated history changes only its provider row.
- Native-mode transitions retain a journal for the chats brought from a custom
  provider. OpenAI chats temporarily brought into a custom-provider takeover
  have a separate ID ledger, which survives Disconnect. The existing retry
  worker stays active while that ledger still has work, including after the
  main configuration receipt has been removed.
- Recognized retained VaryAI provider IDs are handled separately from the
  current provider. Each source and destination has its own journal, and each
  historical origin is restored before broad provider restoration. The
  immutable baseline proves the historical signature; new automatic moves
  stop when the live historical table changes or a profile claims it.
- Recognized OpenAI mirrors can join native sign-in. Provider publication adds
  the OpenAI actor header while preserving other headers and table shapes.
  Disconnect retains the historical provider definition. An inactive definition
  alone does not generate a connection-recovery warning, including when it has
  been customized; explicit cleanup still refuses unrecognized definitions.
- Desktop reload attempts every verified target and retries still-running old
  identities up to three times. A deferred final activity check does not consume
  the signal budget. Replacement identities are excluded, and a replacement
  receives 30 seconds after the old process disappears. Windows activity reads
  fresh file metadata for held-open rollout files.

The first complete client regression run passed 127 tests and exposed ten
remaining failures, including four existing historical-provider regressions.
The historical-provider tests established a separate same-label case beyond
migration of the immediately preceding provider; these drove the per-origin
journals. Assertions for provider-definition deletion and model-gated migration
were updated to the requested Magpie lifecycle contract while retaining checks
for unchanged authentication, model IDs, other fields, external edits, profile
ownership, and paginated bytes.

## Verification

Host: macOS arm64. Rust/Cargo 1.98.1, Node.js 24.11.0, pnpm 10.12.3.
Node differs from the repository's exact version pin.

Frontend checks completed successfully before backend changes stabilized:
`pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit`,
`pnpm format:check`, all documented Node helper suites, the Python shared-state
recovery suite, and `pnpm audit --prod`. Qoder retains one existing skipped
codec test. Vitest passed 281 files and 3,583 tests. Production dependency audit
reported no known vulnerabilities.
Logs are retained in the session temporary directory under
`varyai-codex-parity-frontend-m8vp0l4b`.

The first complete library run passed 6,220 tests with 19 ignored and exposed
four remaining service/menu assertions that still expected provider-definition
deletion. Those assertions now use the same strict restoration check as the
client tests: all baseline fields and comments must remain, and the retained
historical provider must still be a Responses definition.

The frozen final source inventory SHA-256 is
`5fccc38a2ad9b65621ff41d250470be109e42a3ada428012101f01cb1ccab125`.
It includes the working tree's tracked and untracked non-documentation inputs;
existing unrelated changes are part of this checked tree. Logs, exact commands,
per-file hashes, and per-command source comparisons are in the session temporary
directory `varyai-codex-parity-final-daf70b6z`.

| Check                         | Result                                           |
| ----------------------------- | ------------------------------------------------ |
| `cargo test --lib`            | 6,224 passed, 19 ignored                         |
| `cargo check`                 | Passed                                           |
| `cargo fmt --check`           | Passed                                           |
| `cargo clippy -- -D warnings` | Passed                                           |
| `cargo test`                  | 6,387 passed, 19 ignored across 16 test programs |

Cargo uses the offline dependency cache, one build job, disabled incremental
compilation, package debug information disabled, and the shared temporary target
cache. Test processes receive separate `CC_SWITCH_TEST_HOME` directories and
process-local numeric loopback exclusions in `NO_PROXY` and `no_proxy`; system
proxy and application settings are not changed. Earlier attempts encountered a
shared-cache lock and an ENOSPC build failure; they are not counted as successful
verification. No project source or private client data was deleted to recover.
All five final backend commands exited successfully, and their checked input
hashes remained unchanged before and after each command. Only this verification
report was completed afterward. Test builds retain the macOS large-unwind linker
warning and report one unused fixture binding; no warning suppression was added.
The required Clippy command passes with `-D warnings`.

Tests use synthetic credentials, temporary client homes, and dynamic test
listeners. No running desktop application has been launched, installed,
replaced, or signalled for this work. Local process replacement tests cannot
establish real-client authentication, inference, installation, or cross-platform
CI acceptance. Linux and Windows remote CI, Windows-to-WSL2 acceptance, and live
Codex takeover remain unverified.
