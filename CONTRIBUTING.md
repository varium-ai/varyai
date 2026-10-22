# Contributing to VaryAI

Contributions to code, documentation, translations, and reproducible bug reports are welcome. Read the [Code of Conduct](CODE_OF_CONDUCT.md) and use the [Security Policy](SECURITY.md) for vulnerabilities.

Use this repository's Issues and Pull requests tabs for VaryAI changes and reports. CC Switch's tracker is for CC Switch; direct VaryAI-specific questions and contributions here.

## Development workflow

### Set up the toolchain

- Use the Node.js version in [.node-version](.node-version), currently 22.12.0.
- Use the pnpm version in [package.json](package.json), currently 10.12.3. Follow the [pnpm installation guide](https://pnpm.io/installation). If using Corepack, update it before running `corepack enable`: the version bundled with Node 22.12.0 has outdated signing keys and cannot install this pnpm version.
- Install Rust and Cargo. [rust-toolchain.toml](rust-toolchain.toml) selects stable with rustfmt and Clippy; [Cargo.toml](src-tauri/Cargo.toml) declares the minimum Rust version.
- Install the native dependencies required by [Tauri](https://v2.tauri.app/start/prerequisites/) for your operating system.

From the repository root:

```bash
pnpm install --frozen-lockfile
pnpm dev
```

`pnpm dev` runs the desktop application. `pnpm dev:renderer` runs only the Vite renderer; native Tauri commands require the desktop runtime.

### Validate a change

Run the checks from the repository root. Building the renderer also verifies that the frontend assets can be produced.

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

On macOS, the Windows-runner parser test uses a temporary Cargo project, temporary frontend assets, a synthetic target inventory, and a mock runner. It requires Cargo on `PATH` but does not install a Windows target or build a Windows release.

For files you changed, run Prettier or rustfmt as appropriate. `pnpm format:check` checks `src/`; it does not validate every document or script. There is no `pnpm lint` script in this repository.

Ordinary contribution checks require no private release infrastructure or company credentials. Optional signing and notarization use the builder's own explicit configuration; see the [platform build guides](docs/guides/release-distribution.md#development-builds). Run them only when preparing those local artifacts, separately from contribution checks.

### Submit a focused pull request

1. Discuss substantial features or architectural changes before implementation. Small, self-contained fixes and documentation corrections can be proposed directly.
2. Branch from the public repository's `main`. Keep one problem or feature per pull request. Contributors without write access can work in a fork.
3. Describe the problem, resulting behavior, verification commands, and platforms tested. Mark untested platforms and remaining limitations explicitly.
4. Add a failing regression test before a bug fix when the behavior can be tested meaningfully; then implement the smallest sufficient change.
5. Update documentation and translations when behavior or user-facing text changes.

Use concise commit subjects such as `fix(proxy): preserve upstream error details` or `docs: clarify local development setup`. Keep credentials, local databases, model conversations, internal URLs, and generated build output out of commits and reports. Use synthetic data for fixtures.

The public `main` branch is the application development line. Submit changes through a pull request and wait for review and the `CI Required` check. That check verifies path detection and all applicable frontend, backend, and WSL2 jobs; failures, cancellations, and unexpected skips fail the check. Documentation-only pull requests may intentionally skip the application suites.

Frontend checks and the Linux, Windows, and macOS backend suites run for relevant PR changes and every push to `main`. The WSL2 filesystem contract runs only when storage/configuration code, backend dependencies, tests, or its workflows change, on both PRs and main pushes. A manual run of `CI` runs all checks regardless of changed paths.

Before a release or when investigating WSL compatibility, maintainers can run [WSL2 Full Suite (Manual)](.github/workflows/wsl2-manual.yml) from the Actions tab. This longer suite has no scheduled trigger and is separate from the PR gate.

Dependabot opens PRs for security fixes only. Ordinary version-update PRs for npm, Cargo, and GitHub Actions are disabled with `open-pull-requests-limit: 0` in [.github/dependabot.yml](.github/dependabot.yml). Security alerts and automated security fixes remain enabled in repository settings; they use a separate PR limit and are not delayed by the schedule required in the configuration file.

Maintainers update dependencies in small, focused batches when needed and validate compatibility locally before submitting a PR. Security fixes still require normal review and CI; dependency PRs are not automatically merged.

Maintainers triage issues and label PRs directly. Issues are not automatically closed because of inactivity.

Maintainers can squash a pull request when merging it. Preserve commits already published on `main`, including the original CC Switch history. Keep release credentials and company release orchestration in private facilities; do not merge private development history into this repository.

## Code and review expectations

- Preserve request and response semantics unless a conversion explicitly requires a change.
- Do not use `unwrap()` or `expect()` in the local proxy hot path. Auxiliary accounting and audit failures must not interrupt forwarding.
- Do not echo secrets or matched private content in errors, logs, screenshots, tests, or API responses.
- Follow the database lock order, asynchronous audit persistence, and port isolation rules in [AGENTS.md](AGENTS.md).
- Explain third-party code origins and retain applicable copyright and license notices. See [upstream provenance](docs/upstream-provenance.md).
- AI-assisted contributions receive the same review: the contributor must understand the change, verify it, and accurately describe what was tested.

## Internationalization

User-facing text belongs in `src/i18n/locales/`. Keep keys aligned across `en.json`, `zh.json`, `ja.json`, and `zh-TW.json`, and use the existing i18n helpers. These are the application's four locale files; they are separate from translated README and manual files.

New or rewritten VaryAI documentation, design assets, screenshots and code comments use English. Unchanged inherited CC Switch content can retain its original language. Runtime localization and synthetic language-coverage fixtures are preserved.
