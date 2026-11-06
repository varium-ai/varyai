# Vary AI contributor and coding-agent guide

Vary AI is a Tauri application using Rust, React, and TypeScript. The historical crate/library and storage names still include `cc-switch`. Preserve compatibility unless a separately reviewed migration changes it.

## Invariants

- Never use `unwrap()` or `expect()` in the proxy hot path (`src-tauri/src/proxy/`). Handle errors without terminating local forwarding.
- Auxiliary usage accounting, audit logging, latency checks, and tool observations must not interrupt normal forwarding when their own work fails. Preserve intentional security decisions and safe-redaction failures.
- Preserve protocol keys, model parameters, headers, and response semantics unless a required protocol conversion changes them. Extract only the supported text fields for inspection.
- Never echo credentials, matched sensitive plaintext, private configuration, or raw tool inputs in client errors, diagnostics, or test reports. Use synthetic fixtures.
- The shared and extension SQLite connections are separate mutex-protected connections. When both are needed, lock shared first, extension second. Do not hold database locks or execute blocking transactions in request forwarding. Send audit/usage persistence through the asynchronous queues.
- The local proxy defaults to `127.0.0.1:15722`. Tests must use temporary directories, in-memory databases, and dynamic ports instead of competing with a running application.
- The single-instance plugin may foreground an existing app and exit a new process. Do not launch, install, or replace the user's running application as a routine check.
- Use test-first changes for new behavior and reproducible bugs. Keep implementation changes focused and update the applicable documentation.
- Write commit messages in English.

## Modules

| Path                                | Responsibility                                                       |
| ----------------------------------- | -------------------------------------------------------------------- |
| `src/`                              | React UI, API bindings, provider forms, localization and queries.    |
| `src-tauri/src/proxy/`              | Local routing, protocol adaptation, streaming and retry/failover.    |
| `src-tauri/src/proxy/security/`     | Text extraction, DLP rules, redaction and asynchronous audit events. |
| `src-tauri/src/services/tool_risk/` | Tool semantics, applicable risk rules and sanitized observations.    |
| `src-tauri/src/services/`           | Provider/configuration operations and client takeover/recovery.      |
| `src-tauri/src/database/`           | Shared/extension storage, migrations, backup and import.             |
| `scripts/`                          | Public development builds and generic manifest utilities.            |

User-facing text uses `src/i18n/locales/`: `en.json`, `zh.json`, `ja.json`, and `zh-TW.json`. Preserve these runtime languages. New or rewritten VaryAI documentation, design assets, screenshots and code comments must use English. Unchanged inherited CC Switch material and its original history are outside that language cleanup scope.

## Documentation

- [Development](docs/development.md)
- [DLP architecture](docs/architecture/security-audit.md)
- [Tool observations](docs/architecture/tool-risk.md)
- [Shared and extension storage](docs/architecture/storage.md)
- [Client integration and build guides](docs/README.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Release responsibilities](docs/guides/release-distribution.md)

Company release orchestration, upload and notification adapters live in separate private facilities. Ordinary contribution checks must not require those facilities or production credentials. The public macOS packaging helper creates local artifacts; its `--install` option explicitly replaces the installed application and is not a routine verification step.

## Required verification

From the repository root:

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml
```

Before submitting, also run the CI formatting, Clippy and integration-test checks:

```bash
pnpm format:check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

### Commit and CI acceptance

- Treat `.github/workflows/ci.yml` and [Development checks](docs/development.md#checks) as the verification contract. Before committing, run the applicable local checks against the final changes, including helper tests and the production dependency audit. Focused tests during development do not replace the required checks.
- Record the checked revision or working-tree changes, host OS, toolchain, commands, results and any checks that could not run. If relevant files change afterward, rerun the affected checks. Results from another checkout do not validate a different final tree.
- Fix relevant local check failures before committing. If a required check cannot run, disclose the gap before committing; do not silently treat it as passed.
- After pushing, inspect CI for the exact pushed revision and follow it through completion. A push is not verification acceptance. Report pending, skipped and failed jobs explicitly; do not describe CI as passed until all required jobs pass.
- Backend acceptance requires the applicable Linux, Windows and macOS jobs, plus the Windows-to-WSL2 contract when required by the workflow. Local macOS success does not establish cross-platform success. Investigate failures using job logs before assigning a cause; do not disable checks or weaken assertions merely to obtain a green run.
- Before merging, all required CI checks must pass for the revision being merged. If remote results are unavailable, report verification as incomplete.

Before committing, inspect staged and unstaged changes, track only intended source/documentation changes, and exclude generated binaries, debug files and local secrets. Preserve unrelated work. A passing unit test does not establish installer, signing, notarization or live release acceptance.
