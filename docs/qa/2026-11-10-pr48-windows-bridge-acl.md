# PR 48 Windows bridge directory ACL verification

Date: November 10, 2026 (Asia/Shanghai).
Base revision: `b58296acd60d1d5676e9eb92ac49d288ffe54017`, plus the source
fingerprint below. Checked in `/tmp/varyai-pr48-ci-20261110`.

## CI evidence and change

[Windows backend job](https://github.com/varium-ai/varyai/actions/runs/42820347643/job/129844380573)
failed six existing TRAE bridge tests. Every failure returned the generic
bridge-unavailable error while preparing a temporary directory or installing
its runtime. The common Windows-only directory-protection operation constructed
a new security descriptor from a DACL-only SDDL string and applied it with
`Set-Acl`. Child-process diagnostics were suppressed, so the logs do not expose
a more specific PowerShell exception.

The updated operation reads the existing security descriptor and sets only its
Access section, retaining the owner and primary group. It keeps the protected
current-user-only full-control DACL and object/container inheritance. The path
continues to be passed as environment data and accessed through `-LiteralPath`.
Microsoft documents the section-specific overload in
[SetSecurityDescriptorSddlForm](https://learn.microsoft.com/en-us/dotnet/api/system.security.accesscontrol.objectsecurity.setsecuritydescriptorsddlform?view=netframework-4.8.1).

A Windows-only regression protects a temporary path containing spaces and square
brackets, then checks the actual DACL, inherited child-file permissions and
unchanged owner/group. Assertions and process errors do not print account SIDs,
owner names, paths or credentials. The existing six preparation/runtime tests
remain enabled. Unix permission behavior is unchanged and remains covered by
the existing runtime test. No installed client is launched or authorized.

## Local verification

Host: macOS 26.6.2 arm64. Rust/Cargo 1.98.1; Node 22.22.0; pnpm 10.12.3.
Cargo used the existing cache through
`CARGO_TARGET_DIR=/Users/baomi/Work/varium-ai/varyai/src-tauri/target` while
compiling this worktree's sources.

| Command                                                                                               | Result |
| ----------------------------------------------------------------------------------------------------- | ------ |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                              | Passed |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                    | Passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                    | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml services::trae_bridge::tests:: -- --nocapture` | Passed |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`                                       | Passed |
| `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml --quiet`                              | Passed |
| `git diff --check` and changed-document Prettier check                                                | Passed |

The focused bridge suite passed 14 tests. The complete library suite passed
6,303 tests with 19 ignored; the full run additionally passed 163 integration
tests. Frontend, helper and dependency files did not change in this follow-up;
their local results are recorded in
[the first CI fix note](2026-11-10-pr48-ci-fixes.md). Remote frontend, Linux,
macOS and WSL2 jobs passed for the base revision.

The Windows-specific regression cannot execute on this macOS host. Windows
acceptance remains pending for the new pushed revision; this note does not
claim that the ACL change has already resolved the remote failure. Follow all
required jobs for the new revision through completion before accepting CI.

## Source fingerprint

| File                                    | SHA-256                                                            |
| --------------------------------------- | ------------------------------------------------------------------ |
| `src-tauri/src/services/trae_bridge.rs` | `41eff64530de82db0eb61403b2caec81fda303e3c6077b5f2dc53be1b80ecc68` |
