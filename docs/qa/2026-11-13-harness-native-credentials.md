# Harness native credential alignment

VaryAI now publishes its dedicated local gateway credential through Harness's version-1 `.credentials.yaml` store and references it as `VARYAI_HARNESS_GATEWAY_KEY` through `apiKeyEnv` on all three managed protocol routes. Harness can therefore resolve authentication and report its native configured-credential indicator. This indicator is not a connection test or a current-conversation selection signal.

## Scope and restoration

The desktop profile and gateway credential are journaled together. Intact header-based receipts migrate during catalog refresh, preserving the original disconnect baseline and native model choice. A first connection refuses a conflicting named credential. Ordinary refresh refuses externally changed credentials, references and authentication headers. Explicit reconnect validates both native documents, saves private backups, and releases only recorded ownership before rebuilding the connection. Other references, account records and credentials added after connection are preserved. Empty native stores, including comment-only documents and null reference sections, remain supported. Unix writes use mode 0600; Windows follows Harness's native behavior, which does not interpret POSIX modes.

The implementation uses the same portable Rust path on macOS and Windows. The narrow `control_dir` visibility adjustment allows an existing sibling Qoder regression to compile; it does not change its runtime behavior. Existing unrelated working-tree changes were retained.

## Checked source and environment

Base revision: `c1fc6ab85781986e24d77fb1d298c25fac86d36b` plus the current working tree. Host: macOS 15.3.1, arm64. Toolchain: Rust 1.98.1, Node 24.11.0, pnpm 10.12.3. The nine relevant Rust file hashes are recorded before and after each check in `/tmp/vary-harness-final-results.json`; their sorted-inventory SHA-256 is `737336666db30eb0d9aced2fe314331dd0d713c7207c6027fb04f84fb3467495`. Full backend source stability is recorded separately in that report.

Rust commands use cached offline dependencies, `/tmp/vary-magpie-parity-target`, disabled incremental compilation and debug information, and two build jobs. Tests use temporary files and synthetic credentials; the complete Rust suites run outside the filesystem/network sandbox so loopback fixtures can bind. Logs are `/tmp/vary-harness-final-*.log`.

## Verification

- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` and `pnpm test:unit` passed. Unit tests passed: 4,110 tests across 308 files. This task changes no renderer source.
- Final Rust formatting, `cargo check`, and Clippy with `-D warnings` passed.
- `cargo test --lib` passed: 7,518 passed, 21 ignored, zero failures. All eight native credential regressions and the model-name migration regression passed. The complete `cargo test` run passed: 7,683 passed, 21 ignored, zero failures across 16 test harnesses. All relevant file hashes and the complete backend source inventory remained unchanged during each final Rust check.
- Regression coverage includes native credential references, three protocols, legacy migration, restoration, added native keys, externally edited or missing credentials, invalid stores, explicit recovery and install races. The initial test-first build encountered the unrelated Qoder visibility blocker. The complete suite subsequently reproduced an empty-flow-mapping write failure and a legacy model-name fixture that treated the new credential file as a model document. Native YAML encoding now falls back to validated full serialization when section editing cannot represent the input, and model assertions remain scoped to the Cordis profile.
- Public protocol-library functions from the installed Harness 0.2.2-rc.1 bundle (pi-ai 1.3.0) completed synthetic streams for Chat Completions, Responses and Anthropic with a resolved native API key. Missing authentication was rejected. Network responses were supplied by a local fetch stub; no real upstream was contacted.

No installed application was launched, restarted, installed or modified, and no live client profile or credential store was changed. The on-screen green indicator and real upstream conversations have not been accepted on an installed rebuilt application. Linux/Windows CI, installer and signing acceptance are not established by these macOS checks. No commit or push was performed.
