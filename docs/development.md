# Vary AI development guide

This guide covers development from the current source checkout.

## Toolchain and setup

Use the Node version in `.node-version`, the pnpm version in `package.json`, and the stable Rust toolchain in `rust-toolchain.toml`. Install the native Tauri prerequisites for your operating system. The crate's declared minimum Rust version is recorded in `src-tauri/Cargo.toml`.

```bash
pnpm install --frozen-lockfile
pnpm dev
```

`pnpm dev:renderer` starts only the browser renderer. Native commands, the proxy and database operations require the Tauri runtime. Exit an existing app before starting a separate development instance.

The optional `setup.sh` changes the host environment: it can install system packages, activate global tools and select a Rust toolchain. Review it before running it. Manual setup is sufficient for contribution checks; this guide does not claim that every installer path has been validated.

## Checks

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
pnpm test:trae-bridge
pnpm test:qoder-codec
pnpm audit --prod
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

Use `pnpm exec vitest run <test-files>` to select frontend tests. There is no `pnpm lint` command. `pnpm format:check` checks `src/`; apply formatting tools to other changed files as appropriate.

Provider tests that read Hermes configuration must isolate `HERMES_HOME` and
`LOCALAPPDATA` alongside the temporary home directory, and restore their previous
values afterward. Hermes uses `LOCALAPPDATA` on Windows, so changing `HOME` alone
can leave providers from earlier tests visible to later tests.

The Qoder TLS listener owns its accepted connection tasks and cancels and joins
them during shutdown. Its certificate test keeps both an immediate same-address
rebind assertion and pending TLS handshakes to verify that stopping the listener
releases active sockets. The test uses an isolated certificate and never installs
system trust.

## Toolchain dependency review

The Windows-to-WSL2 CI contract keeps the distribution running with a detached Linux process while Windows accesses its UNC filesystem. Preparation, contract execution, and cleanup have separate timeouts. Preparation logs distinguish Linux directory setup from Windows UNC access so a stalled runner can be diagnosed without waiting for the whole job timeout. Both the PR workflow and the manual WSL2 workflow use the same lifecycle.

Run `pnpm audit --prod` for application dependencies and `pnpm audit` for the complete development tree. The checked-in overrides set security floors for build and test dependencies; review compatibility when removing or changing them.

Zod is pinned to 4.6.7 to address GHSA-mrhx-c7xc-m3w5. Its pnpm patch rewords two explanatory comments that Rollup mistakes for misplaced pure annotations; it does not change executable code or actual optimization annotations. Recheck whether the patch is needed when upgrading Zod. Validation code has a separate renderer chunk to retain the existing bundle size budget without suppressing build warnings.

The remaining known development dependency advisory is [GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm) in `braces` 3.0.3, used by the Tailwind 3 globbing toolchain. As of October 22, 2026, no patched version is published. This package is absent from the production dependency tree. The documented build consumes repository-controlled patterns rather than a public glob-processing API; this limits exposure but does not fix the dependency. Maintainers must revisit the exception when a patch or a compatible toolchain replacement becomes available, or when a workflow starts accepting untrusted patterns. Do not suppress the advisory as though it were fixed.

## Optional statistics destination

Ordinary source builds have no statistics destination and do not report installation statistics. To build a distribution with this optional integration, set `VARYAI_ANALYTICS_BASE_URL` at compile time to an HTTPS base URL operated under your published privacy policy. Runtime reporting still requires explicit opt-in in Settings. Do not include credentials, query parameters or fragments in this URL. See [data and network behavior](data-and-network.md) for the payload and consent lifecycle.

## Development boundaries

- Follow [AGENTS.md](../AGENTS.md) for proxy, protocol, database and secret-handling invariants.
- Keep tests isolated from real client configuration, credentials, running apps and the default proxy port.
- Use synthetic provider endpoints, keys and sample content. Sanitize logs and screenshots before sharing.
- Keep the four runtime locale files aligned; the application remains multilingual.
- Read the [storage design](architecture/storage.md) before changing migrations or backup formats.
- Use the [platform build guides](README.md) for local packaging. Production release infrastructure is separate.

Contribution and reporting expectations are in [CONTRIBUTING.md](../CONTRIBUTING.md) and [SECURITY.md](../SECURITY.md).
