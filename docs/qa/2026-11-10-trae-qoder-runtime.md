# TRAE startup compatibility and Qoder credential reuse

Date: 2026-11-10. Host: macOS / Darwin 25.6.0 arm64. Baseline:
`5ba154ca1991b19d926b451d43ae73a5e896719c`, plus the existing check-in presentation
work, concurrent Agent-installation icon changes, and this task's runtime changes.
Toolchain: Node 22.22.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

## TRAE CN

The installed version was 3.4.17 and its existing workbench window was open.
Read-only diagnostics found a listening random loopback debugger endpoint and
the resident revision-10 bridge. Its health reported `ready: false`,
`enabled: false`, and `modelReplacement: false`. The installed runtime scripts
matched the repository's pre-fix scripts. No application was launched, quit, or
replaced during inspection.

Static inspection of the installed chat bundle confirmed that the workbench
initializer's module IDs 20536, 94656, and 1053 were absent. The legacy selection
mapper's ID 46623 was also absent. A unique initialized-container export was
present in module 19208, and the matching legacy mapper factory was in 7494.
Those observations establish an incompatibility in the old initializer; they do
not establish that every possible startup delay has the same cause.

The workbench initializer now discovers the unique container export, verifies
initialization, and resolves model services through native service identifiers.
The native helper and extension renderer share this resolver. Legacy selection
readback discovers the unique matching mapper instead of requiring its old
module number. Unknown and ambiguous candidates remain unsupported. Revision 11
requests replacement of the resident helper while preserving the rule that an
older helper cannot replace a newer renderer. Existing conversation selections,
audit consent, model ownership checks, and client-launch behavior are preserved.

Node regressions cover renumbered containers, initialization, ambiguous/missing
bindings, no unrelated module loads, refresh notifications, legacy persistence
and readback, and older/newer resident hooks. The initial mapper regression failed
before implementation; the new service-resolver contract was initially absent.
All 58 TRAE helper tests pass after implementation.

## Qoder CN

The native reader previously cleared its in-memory Safe Storage secret on every
explicit Read client login action. The refactored cache test reproduced that
behavior before the fix: two reads instead of the expected one for repeated
interactive reads followed by a background read. The fixed test passes and also
checks invalidation, changed Safe Storage services, uncached failed reads, and a new process's
empty cache. It uses synthetic secrets and never invokes the real Keychain.

Explicit refresh and claims now reuse a valid process-local secret. Current
native login data is still read independently; account opt-ins and expected
account identity checks are unchanged. A native decryption rejection clears a
supplied cached secret so a later explicit read can request fresh authorization.
Background reads retain their no-interaction guard. No password, account token,
Keychain access rule, or native authentication file was changed for acceptance.
The user confirmed that the prompt is the macOS Keychain/login-password window,
not a Qoder website login. The prompt itself was not reproduced or accepted.

The secret is not persisted across Vary restarts. If macOS requires authorization
again on the next process's first read, that remains an OS permission boundary.
This change does not promise to eliminate system authentication across launches.

## Verification

Logs: `/private/tmp/vary-trae-qoder-*.log`. Source hashes:
`/private/tmp/vary-trae-qoder-source-sha256.json`.

- Type checking, production renderer build, formatting, production dependency
  audit, Rust formatting/check, and Clippy with warnings denied passed.
- All six Node helper suites and the Python storage-recovery helper passed.
  Extra Prettier checks cover the changed TRAE helpers.
- Full frontend suite: 281 files and 3,634 tests passed, including the concurrent
  installation-icon cases.
- Initial broad runs exposed two fixtures still advertising revision 10 to
  revision-11 code: the library reconnection case and the profile-bundle
  integration case. Both health fixtures now advertise 11. Their operation and
  audit assertions remain unchanged. The focused reconnection regression passed.
- The profile-bundle integration suite passed all 32 cases after its fixture
  revision update.
- Final `cargo test --manifest-path src-tauri/Cargo.toml` passed all 6,244 library
  and 163 integration tests, with 19 existing ignored tests and zero failures.
  This final full run also revalidated the library cases from the earlier aborted
  `--lib` run. No tests were skipped to obtain acceptance.
- Final Rust formatting and `git diff --check` passed. Existing test-fixture and
  macOS linker warnings remained nonfatal; production Clippy passed with warnings
  denied.

The running Vary backend and native clients were not rebuilt/relaunched for live
acceptance. Loading the changes needs a normal Vary rebuild/restart. A real TRAE
post-upgrade connection, upstream conversation, and macOS password-prompt count
after deployment remain unverified. Remote cross-platform CI and packaging are
separate from these local checks.
