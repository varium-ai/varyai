## Summary

<!-- Describe the VaryAI behavior changed, its purpose, and any compatibility impact. -->

## Related Issue

<!-- Link the related issue. Use "Fixes #123" to auto-close it when merged. -->

<!-- Link an issue in this repository, or state "Not applicable". -->

## Validation

<!-- Record commands actually run, their results, and the OS/architecture. -->

| Check                                                   | Result or reason not run | Environment |
| ------------------------------------------------------- | ------------------------ | ----------- |
| `pnpm typecheck`                                        |                          |             |
| `pnpm build:renderer`                                   |                          |             |
| `pnpm test:unit`                                        |                          |             |
| `pnpm format:check`                                     |                          |             |
| `cargo check --manifest-path src-tauri/Cargo.toml`      |                          |             |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` |                          |             |
| Manual verification and affected platforms              |                          |             |

<!-- For Rust changes, also record cargo fmt --check and cargo clippy with -->
<!-- --manifest-path src-tauri/Cargo.toml; describe platform coverage and remaining limits. -->

## Screenshots

<!-- If applicable, add sanitized before/after screenshots. -->

| Before | After |
| ------ | ----- |
|        |       |

## Checklist

- [ ] Recorded actual validation results and platform limits
- [ ] Updated documentation for behavior or configuration changes
- [ ] Updated applicable i18n files for user-facing text changes (zh/en/ja/zh-TW)
- [ ] Removed secrets and private data from code, logs, screenshots, and fixtures
