# Magpie visible subscription imports

The import list previously treated endpoint-free subscription model settings as
incomplete API providers and exposed unrelated login-history rows. Only Codex
authorization had an account-copy implementation.

This change combines subscription settings with saved authorization for Codex,
Claude, Copilot, ChatGPT API, Gemini CLI and Antigravity. Subscription model picks
and catalogs are retained, and the preview counts their models. Single-account
entries use the provider name. Hidden providers and removed logins are omitted;
policy-only entries and unrecognized plugin login history do not become rows.
Ordinary API providers retain the earlier basic-connection import behavior.

Authorization remains backend-only. Google account copies retain their project,
expiry and installed OAuth client. Claude tokens remain in a new private CLI
credential home. Copilot imports only accept GitHub or GHE destinations; imported
opaque account IDs survive a later verified numeric GitHub identity. Existing
local sessions win over repeated imports. No vendor requests or source/native
client writes are performed while previewing or copying accounts.

Source changes, including hiding a provider or rotating its authorization after
preview, invalidate the selected fingerprints. Native, Copilot and Codex account
copies are staged around the provider transaction and rolled back if it rejects
the batch. This is not a cross-file crash-atomic transaction.

## Verification scope

Base revision: `23a4b2be4b582f892691f5c7feb5fe88e79e6b42` plus the working-tree
importer, native/Copilot account-copy helpers, command injection, four locale
strings and related tests/documentation. Concurrent provider-form, provider-ID,
model-kind and agent-connection changes in this checkout were preserved.

Host: macOS 15.3.1 arm64; Node 24.11.0; pnpm 10.12.3; Rust 1.98.1 stable.
Tests use synthetic identities, temporary account stores and in-memory databases.
The visible-subscription regression failed against the original implementation
before the import changes. Initial focused verification passed 48 backend tests
and 10 import-dialog tests. Further tests cover Claude credential ownership and
mixed-store rollback after the registry reaches its provider limit.

The host ran out of disk space during a subsequent Rust build and six frontend
test-file setups. These were infrastructure failures, not passing checks. Test
retries disable incremental compilation and debug symbols for the application
package only, preserving the test assertions and debug assertions:

```sh
CARGO_INCREMENTAL=0 cargo test --lib --manifest-path src-tauri/Cargo.toml --config profile.test.package.varyai.debug=0 --config profile.dev.package.varyai.debug=0
CARGO_INCREMENTAL=0 cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml --config profile.test.package.varyai.debug=0 --config profile.dev.package.varyai.debug=0
```

The first full library run also caught temporary import stores sharing the
runtime account mutex with a renewal-isolation test. Explicit temporary stores
now use per-store coordinators; the runtime store retains its existing mutex.
The original renewal assertion is unchanged, and a new test holds publication
in one temporary store while proving another can finish independently.

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm format:check` | Passed |
| `pnpm exec vitest run tests/components/ProviderImportDialog.test.tsx` | 10 passed |
| `pnpm test:unit` after disk recovery | 3,642 passed; 5 failed in ProviderAddSheet and RoutingUpstreamsPanel form tests. The import-dialog tests passed. |
| `pnpm audit --prod` | No known vulnerabilities |
| Development, DMG, TRAE, WorkBuddy, Doubao, agent-rewards and Qoder helper tests | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | 5 passed |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Final retry passed |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Final retry passed |
| Resource-adjusted `cargo test --lib` above | 6,258 passed, 0 failed, 19 ignored; both the original renewal-isolation test and the new store-isolation/mixed-batch tests passed |
| Resource-adjusted complete `cargo test --no-fail-fast` above | 6,421 passed, 0 failed, 19 ignored, including 163 integration tests |
| Provider-import tests within the final library run | 51 passed |

The verified implementation/test snapshot did not change during the final checks.
Its SHA-256 (`path + NUL + contents + NUL`, in the following order) is
`e78b062c6c6f87194d0b79b2988e60930ce9203b4ccfb884653756fa7d3cb1e5`:

1. `src-tauri/src/commands/routing_upstreams.rs`
2. `src-tauri/src/services/provider_import/mod.rs`
3. `src-tauri/src/services/provider_import/readers.rs`
4. `src-tauri/src/services/provider_import/subscriptions.rs`
5. `src-tauri/src/services/provider_import/tests.rs`
6. `src-tauri/src/services/native_subscription.rs`
7. `src-tauri/src/services/native_subscription/provider_import.rs`
8. `src-tauri/src/proxy/providers/copilot_auth.rs`
9. `src-tauri/src/proxy/providers/copilot_auth/provider_import.rs`
10. `tests/components/ProviderImportDialog.test.tsx`

Passing rows describe the snapshots observed by those commands; the five frontend
form-test failures and continued concurrent edits prevent whole-checkout
acceptance. Linux, Windows and macOS CI, any required WSL2 job, and live
subscription acceptance have not run. No installed application was
launched/replaced, and nothing was committed or pushed.
