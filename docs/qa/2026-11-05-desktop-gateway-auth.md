# Claude Desktop gateway credential repair

## Change

Magpie reference: `yetone/magpie` revision
`1d0fc2bb08c51d8711759694189b8a1d92ee3a1e`, particularly
`desktopKeyApplied`, `desktopKeyTaken`, `desktopOpen`, and `desktopProbe`.

Vary retains independent random connection credentials. A new connection must
write its new credential even when the existing profile contains an opaque
Desktop-encrypted key for the same address. An ordinary refresh can preserve
that ciphertext only when retaining the journaled connection token.

Synchronization repairs an invalid readable key in the same managed profile.
It does not bypass authentication or overwrite changes to the gateway address,
provider, authentication scheme, model list, or applied profile. The update's
no-change shortcut now also checks the live documents, so it cannot skip a
necessary credential repair just because the desired receipt is unchanged.

Existing connection-probe behavior already follows Magpie: use Desktop's native
menu choice or visible default and raise the one-token probe to 16 output tokens.
The probe continues to call the real upstream.

## Regression evidence

Tests were added before implementation. Initial test execution was blocked by
concurrent Rust compilation errors, so no pre-fix assertion result was obtained.
The tests cover replacement of a retired opaque key, preservation of the same
connection's sealed profile, readable-key repair, rejection of the retired key,
retained user settings and recovery baseline, and refusal to overwrite external
gateway changes. Fixtures use temporary homes, in-memory storage and dynamic ports.

The compiled Desktop suite passed **95 tests**, with **4 explicitly ignored
acceptance tests**. This includes the command-level credential repair and the
existing native-menu/probe routing tests. Initial socket-dependent runs were
denied by the sandbox and were rerun with loopback access.

## Verification context

- Date: November 5, 2026.
- Checkout: `/Users/guitar/projects/varyai-open-source`.
- Base revision: `25d6b901658d4fd96a7d783542670ab7c53667d7` plus working-tree changes.
- Host: macOS 15.3.1 (24D70), Apple Silicon.
- Toolchain: Rust/Cargo 1.98.1, Node 24.11.0, pnpm 10.12.3. Node differs from
  the repository's pinned 22.12.0; results establish this host's checks only.
- The checkout contains substantial unrelated, concurrently edited Codex work.
  A required `String` to `&str` argument conversion in `thread_lifecycle.rs` was
  minimally corrected to unblock compilation; the other work was retained.
- The six Desktop Rust source/test file hashes were recorded in
  `/private/tmp/vary-desktop-auth-verification/checked-desktop-source-hashes.json`.
  The SHA-256 of its canonical sorted JSON is
  `2e2a9b0eae91793e1d430f102c1a2e7bfcc374905236c115c664b17a89b02692`.
  These six files remained unchanged through verification. Other Rust modules,
  including `thread_lifecycle.rs`, continued changing after the Desktop test
  binary was compiled and after later checks. Passing individual runs below do
  not establish acceptance of that moving working tree.

| Command | Result |
| --- | --- |
| `pnpm typecheck` | Passed |
| `pnpm format:check` | Passed |
| `pnpm build:renderer` | Passed |
| `pnpm test:unit` | 275 files, 3406 tests passed |
| `pnpm test:dev` | 8 passed after rerunning with loopback access |
| `pnpm test:trae-bridge` | Passed |
| `pnpm test:workbuddy-checkin-auth` | Passed |
| `pnpm test:doubao-checkin` | Passed |
| `pnpm test:qoder-codec` | Passed |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | Passed |
| `pnpm audit --prod` | No known vulnerabilities |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Passed on rerun |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Passed on final rerun |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Failed in unrelated `client/threads.rs`: unnecessary lazy evaluation and boolean assignment |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | Blocked by unrelated `intent/native.rs` test compilation: `Body::data` is unavailable |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Blocked by the same unrelated test compilation errors |
| Compiled test binary with `desktop_` filter | 95 passed, 4 ignored |

Local logs are under `/private/tmp/vary-desktop-auth-verification/`. Full backend
acceptance remains incomplete. Linux, Windows, Windows-to-WSL2 and remote CI were
not run for these uncommitted changes. No application was installed, replaced,
launched or restarted, and the screenshot's live connection was not retested.
