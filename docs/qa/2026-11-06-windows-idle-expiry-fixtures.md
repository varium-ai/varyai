# Windows idle-expiry timestamp fixtures

## Observed CI failure

[CI run 42192220290](https://github.com/varium-ai/varyai/actions/runs/42192220290) checked pushed revision `05507ae663503edcba0bbe64fa6a4ae68987f2e0`. Frontend, Linux, macOS and the Windows-to-WSL2 filesystem contract passed. The Windows backend's full library run reported 6,063 passed, two failed and 16 ignored; both failures returned OS error 5 (`PermissionDenied`, `Access is denied`) during fixture setup:

- `services::codex_app_server::tests::abandoned_rollouts_expire_only_after_their_last_file_activity`
- `services::codex_app_server::tests::abandoned_database_turns_expire_by_database_and_wal_activity`

These tests open existing temporary files with `File::open`, then backdate their modification time with `set_times`. The read-only handle works on the local macOS filesystem, but Windows requires a handle with write attributes for timestamp changes. The intended inactivity assertions cannot finish after the fixture operation fails.

## Correction

Open the same existing temporary files with write access before changing timestamps. The files are neither created nor truncated by this operation. Keep the active-file, stale-file and fresh-WAL assertions, and keep their original time windows. This change touches test fixtures only; production activity detection and restart behavior are unchanged.

## Verification

On Darwin arm64 with Rust/Cargo 1.98.1, all five backend checks passed: formatting, type checking, strict Clippy, library tests (6,151 passed; 18 existing ignored) and the complete 16-binary test run (6,314 passed; 18 existing ignored). Source inventories match before and after every command; the canonical inventory SHA-256 is `bb83c331adcbc3faab66a6e3d3a62302dfb3a26856bbee33cd0e366af46eb778`. Exact offline commands and results are retained under `/tmp/vary-ci-windows-time-20261106/`.

Frontend sources still match the previously checked inventory (3,516 unit tests and production build/type/format/audit checks passed), and the original pushed revision's frontend CI passed. No frontend behavior changes in this correction. Windows acceptance still requires CI for the corrected pushed revision; the local macOS result alone does not establish it. Test processes used temporary homes and dynamic ports; no installed client was launched or replaced. This report was completed after the source checks without application-source changes.
