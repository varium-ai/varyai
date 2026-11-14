# Generic Chat strict compatibility verification

## Scope and reference

Base revision: `d05b940ed35a395967674fec5b7ad69f3dbfb95f`.
Magpie reference: `c5b9e448a8bc9bc2a1bf6704e5d8bb3384d084a6`
(local reference checkout; see `internal/gateway/chat.go`, `buildChat`, and
`internal/gateway/ir.go`, `strictSaid`). Generic Chat conversion omits strict
tool flags while retaining complete schemas, required fields, additional-property
constraints and tool choices. Native Responses and Anthropic retain their strict
behavior. The prepared Codex/DLP converter removes the flag only at the Chat wire
boundary, preserving its shared tool metadata for Anthropic.

Both newly added regression tests failed before implementation changes and now
pass. The HTTP/SSE regression upstream also rejects strict flags. The full suite
caught an initial shared-metadata regression; the final implementation fixes it
at the Chat boundary, and the unchanged Anthropic enforcement test passes.

The five-file checked source manifest is retained locally at
`/tmp/vary-pr56-strict-source.json`, with canonical JSON SHA-256
`ba100d3453d37f7b77d7d0ca1c97f4d1513ca2986cb06b996523adf5252a425c`. This verification record is excluded.

## Checks

Host: macOS 15.3.1, Apple Silicon. Toolchain: Node 24.11.0, pnpm 10.12.3,
rustc 1.98.1. On the final Rust source:

- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: passed;
  7,568 library tests and 165 integration tests, zero failures, 21 ignored.
  Incremental compilation was disabled and package debug info was set to zero.
- `pnpm format:check`, updated Markdown Prettier checks and `git diff --check`:
  passed.

Results and logs are retained at `/tmp/vary-pr56-strict-final-results.json`
and `/tmp/vary-pr56-strict-final-*.log`. Frontend, helper and dependency inputs
are unchanged from the base revision, whose frontend, Linux, Windows, macOS,
Windows-to-WSL2 and CI Required jobs all passed in run `43448786589`.
The new revision requires its own CI acceptance after pushing.

## Live diagnostics and unresolved desktop acceptance

Synthetic local gateway requests with strict namespace tools, with and without
optional search, completed with HTTP 200; streaming also reached
`response.completed`. Ephemeral read-only greetings through the installed CLI
and the desktop-bundled CLI both completed through the affected source. These
isolated checks do not reproduce the desktop's private configuration and workspace
context. A subsequent real desktop greeting still failed with HTTP 400.
Desktop acceptance therefore remains unresolved, despite the confirmed fixes.

A proposed diagnostic that loaded private workspace configuration was rejected by
automatic approval review and was not executed. Subsequent isolated parameter
checks used synthetic inputs only. No private configuration or raw tool inputs
are included in this record. Existing source credentials and protocol were not
changed. The unsaved protocol-change workaround is not required for this code fix.
