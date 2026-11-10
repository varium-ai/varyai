# TRAE editions and Doubao Work campaign discovery

Base revision: `d7b7eb2b` on `codex/intent-recognition`.
Host: macOS arm64. Toolchain: Rust 1.98.1, Node.js 22.22.0, pnpm 10.12.3.

## Changes

- Corrected the CN website to `https://www.trae.cn/` and added a separate `trae-intl` Agent for `https://www.trae.ai/`.
- Kept existing CN IDs, consent, paths and saved outboxes compatible. International catalog delivery, starting-model selection, lifecycle checks, consent and cleanup use independent state.
- Added edition-specific macOS and Windows installation paths and lifecycle identities. Windows helper communication uses an authenticated named pipe with credentials in a protected current-user directory. No international client was installed or authorized during this change.
- Added international macOS update metadata from the official manifest, excluding CN packages. Windows automatic update acceptance is not established.
- Kept check-in specific to the officially supported CN integration. No official international daily reward capability was confirmed.
- Changed Doubao Work campaign SDK discovery to inspect exports and deduplicate service objects. A facade repeating the campaign marker no longer causes an immediate module-count failure.

## Evidence and limits

A read-only diagnostic against the already authorized Doubao Work 2.36.3 connection found two matching module factories. Only one exported the three required campaign methods. The old helper failed at campaign API discovery before querying a reward. The diagnostic did not export account identifiers or credentials and did not execute a claim.

Synthetic regression fixtures were updated for the duplicated campaign marker and independent TRAE outboxes. They were not executed: the user explicitly requested no further tests or CI and asked for a combined commit and push.

Compiler checks cover source integration only. They do not establish a successful live Doubao claim, international native model delivery, Windows execution or release acceptance. The running development backend was not replaced for verification.

## Final source checks

- `pnpm typecheck`: passed for the final renderer and locale changes.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed for the final Rust changes.
- Focused formatting and whitespace inspection: completed before commit.
- Tests, full required checks, production dependency audit and remote CI: skipped at the user's explicit request. Verification remains incomplete.

Concurrent check-in record persistence changes in this checkout are outside this commit and remain preserved. Compiler checks ran against the shared working tree; no test result is claimed for either change set.
