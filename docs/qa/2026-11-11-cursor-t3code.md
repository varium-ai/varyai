# Cursor and T3 Code integration verification

This report records the initial verification. The subsequent TRAE fixture fixes
and complete local rerun are recorded in
[TRAE fixture verification](2026-11-11-trae-fixtures.md).

Date: 2026-11-11 (Asia/Shanghai).
Host: macOS 26.6.2 arm64; Rust/Cargo 1.98.1, pnpm 10.12.3.
The final frontend runs used Node 22.12.0, matching `.node-version`.
An initial frontend run also passed under Node 20.20.2.

## Checked changes

Base revision: `55a07d45444dbaf087cd67478419a4eb3e840951`.
The 65 changed source, asset and documentation files, excluding this report,
have aggregate SHA-256
`dcebf70f61f2e287e34c9f6b94c7f200cc8de84b0d513d7e6ac84fee180ac3ab`.
The sorted path/content digest manifest is retained locally in
`/tmp/vary-agents-source-manifest.json`. No commit or push was performed.

- Cursor CLI observes its native model and supports selecting Cursor Auto,
  preserving authentication, permissions and unrelated model parameters.
- Cursor Private Inference uses owned user environment values on macOS and
  Windows, with conflict-aware restoration and conservative macOS reboot
  recovery. The regular editor is excluded by product identity.
- T3 Code gets a separate Codex provider instance, home and full model catalog.
  Connection, refresh, gateway registration and disconnection are exercised
  through the same service used by the public Agent command.
- Client registration includes presence/version discovery, icons, visibility
  persistence, all four runtime languages, gateway policy capabilities and
  explicit exclusion from unsupported legacy integrations.

## Verification

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | Passed: 294 files, 3,872 tests on Node 22.12.0 |
| Final affected `GatewayKeysPanel.test.tsx` and `RoutingRulesEditor.test.tsx` runs | Passed: 19 and 9 tests after the last frontend changes |
| Focused Agent page, registry and icon tests | Passed: 238 tests |
| `pnpm audit --prod` | Passed: no known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml agent_connections` | Passed: 98 tests; one installed-OpenCode test ignored |
| New public-service Cursor/T3 Code lifecycle test | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Failed: unchanged TRAE fixture panics, then aborts during cleanup |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Initial attempt could not build the static archive because disk space was exhausted |
| `CARGO_INCREMENTAL=0 cargo test --manifest-path src-tauri/Cargo.toml --test '*'` | Built successfully; aborted in the unchanged TRAE profile fixture |
| `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` | Built successfully; two targets failed: library and profile bundle, both at unchanged TRAE fixtures. Twelve other integration binaries passed 132 tests |
| Windows-target check of the exact native environment module | Passed: `x86_64-pc-windows-msvc`; this is not full Windows backend acceptance |
| `git diff --check` | Passed |

The final affected frontend checks were repeated after the complete unit run;
the final renderer build, formatting and type check also cover those changes.
All native connection fixtures use temporary paths, in-memory databases and
dynamic ports. No test configured, installed, restarted or launched a real
Cursor, T3 Code or VaryAI application.

Helper checks passed for the development launcher, macOS DMG helper, desktop
model defaults, WorkBuddy credential helper, Doubao check-in, agent rewards,
Qoder codec and shared-v23 recovery. The TRAE helper suite failed three of its
58 tests; the other 55 passed.

## Existing TRAE failures

The four TRAE renderer/helper implementation and test files were compared with
HEAD and are unchanged. The Rust test body that aborts is also unchanged.

- The resident-helper test constructs a supposedly newer helper with
  `hookRevision: 12`, while the current bootstrap uses revision 18. Its
  reference-identity assertion fails after the bootstrap replaces that helper.
- Two renderer migration/cache tests seed legacy model rows without provider
  identity and, in some rows, without their original model name. The current
  identity guard rejects those rows with `model_identity_changed`. Real-client
  legacy migration acceptance requires separate evidence.
- The Rust reconnection fixture advertises revision 11, while the current
  bridge requires 18. The compatibility path sends `disable` to release an old
  hook. The fixture accepts no `disable` operation and panics with the message
  `configuration sync must not enable auditing`. Its destructor then unwraps
  the failed worker join, causing SIGABRT and preventing a normal suite summary.
- `trae_profiles_deliver_when_connected_without_binding_preview_to_runtime`
  in the profile-bundle integration target has the same revision-11 fixture and
  missing-operation failure, followed by a panicking destructor. Its test body
  was compared with HEAD and is unchanged. The full `--no-fail-fast` run
  continued through the other integration targets instead of omitting them.

These checks were not disabled or weakened to obtain a passing result.
Detailed command logs are retained locally under `/tmp/vary-agents-*.log`.

## Verification limits

The full Rust test retry initially exhausted available disk space. Only this
task's downloaded reference repositories, temporary Windows-check build cache,
failed incremental caches created during this task and orphaned task-created
compiler objects were removed. Compiler cleanup held the Cargo build lock;
source files, pre-existing caches and installed applications were preserved.
The retry disables incremental compilation to reduce temporary disk use.

The isolated Windows compilation checks the exact final registry and broadcast
implementation, using the repository's dependency versions. Unused-function
warnings belong to that standalone harness. It does not execute Windows APIs.

Remote Linux, Windows, macOS and WSL2 CI results are unavailable because this
working tree has not been pushed. Live Private Inference and T3 Code inference,
installer, signing and release acceptance were not tested. Overall verification
must not be described as fully passed while the TRAE checks fail.
