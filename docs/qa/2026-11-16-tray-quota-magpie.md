# Tray quota cards and sparklines

The tray allowance cards now follow Magpie's compact ring geometry, short pool
labels, amber exhaustion color, and 30-point current-cycle sparkline. The chart
adds the green/amber gap against an even-consumption line. Pooled window suffixes
are recognized, and small reset-deadline drift no longer discards earlier
readings. Independent model windows remain visible; more than four rings use a
full-width row below the account information, wrapping from the left if needed.

Relay API balance cards omit the quota sparkline, including its reference line
and shaded band. Their balance, supported quota rings and refresh action remain
available, and they no longer read or write renderer quota-history samples.
This is the requested compact presentation for the reported sub2api card;
the pinned Magpie source supports conditional quota sparklines and is not
evidence that every Magpie relay card always omits them.

Reference: [`yetone/magpie` at
`58f24cc23527776b5f1958cfd4a8092a659bdfb5`](https://github.com/yetone/magpie/tree/58f24cc23527776b5f1958cfd4a8092a659bdfb5),
`internal/gui/assets/app.js` (`panelQuotaCard`, `quotaSpark`, `sparkGap`,
`quotaCycleBreak`) and `internal/gui/assets/app.css`.

## Checked tree and environment

- Base revision: `be6095af15e4bfc6c0358dc613a00069ad2bbe26`.
- Working-tree changes: `ApiProviderQuota.tsx`, `ProviderQuotaCard.tsx`, `QuotaSparkline.tsx`,
  `tray-panel.css`, all four locale files, and the four quota component test
  files. Documentation describes the resulting behavior separately.
- SHA-256 of the sorted JSON map of those 12 file paths to their SHA-256 hashes:
  `c4ddc7683129822ed7bbd399529261eafd6b92db7f72935fbe5377c1f495fd51`.
  The final map and repeated frontend checks are in
  `/tmp/varyai-quota-verification/relay-correction/`; the original helper,
  dependency-audit and Rust logs remain in `/tmp/varyai-quota-verification/`.
- Host: macOS 15.3.1 (24D70), arm64.
- Full frontend checks: Node 22.23.5, pnpm 10.12.3. Initial focused tests also
  passed on Node 24.11.0. CI's exact Node 22.12.0 version was not used for the
  completed suite; the initial attempt was rejected by the host pnpm launcher.
- Rust and Cargo: 1.98.1, using the repository's stable toolchain.

## Verification

Seven new sparkline assertions failed before implementation, covering pooled
window recognition, reset tolerance, refill boundaries, interval shading, and
rendering after history failure. They pass with the final changes. Additional
tests retain stale status, full pool identities, zero balances, and the
used/remaining toggle.

Two additional tray regressions failed before removing the relay's sparkline
binding. They verify unused and partly used daily windows, even with available
history, retain the balance, ring and refresh without mounting a chart or issuing
history calls. All 37 focused quota tests pass. The relay correction changes
only renderer code, its regression tests and documentation; helper, dependency
and Rust inputs are unchanged from the earlier passing checks below.

| Command                                                            | Result                                                            |
| ------------------------------------------------------------------ | ----------------------------------------------------------------- |
| `pnpm typecheck`                                                   | Passed                                                            |
| `pnpm format:check`                                                | Passed                                                            |
| `pnpm build:renderer`                                              | Passed                                                            |
| `pnpm test:unit`                                                   | Passed: 314 files, 4,195 tests                                    |
| `pnpm audit --prod`                                                | Passed: no known vulnerabilities                                  |
| `pnpm test:dev`                                                    | Passed                                                            |
| `pnpm test:macos-dmg`                                              | Passed                                                            |
| `pnpm test:trae-bridge`                                            | Passed                                                            |
| `pnpm test:model-default`                                          | Passed                                                            |
| `pnpm test:workbuddy-checkin-auth`                                 | Passed                                                            |
| `pnpm test:doubao-checkin`                                         | Passed                                                            |
| `pnpm test:agent-rewards`                                          | Passed                                                            |
| `pnpm test:qoder-codec`                                            | Passed                                                            |
| `python3 -B scripts/tests/recover-shared-v23.test.py`              | Passed                                                            |
| `cargo check --manifest-path src-tauri/Cargo.toml`                 | Passed                                                            |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml`            | Passed: 7,592 tests, 21 ignored                                   |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`           | Passed                                                            |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | Passed                                                            |
| `cargo test --manifest-path src-tauri/Cargo.toml`                  | Passed, including 165 integration tests; 21 library tests ignored |

An isolated Vite renderer served synthetic relay, Antigravity, and Codex data on
a dynamically allocated loopback port. Chromium and WebKit passed 32 layout
combinations: 320/440-point widths, light/dark themes, and English, Simplified
Chinese, Traditional Chinese, and Japanese. Checks covered all ten meters,
card/page overflow, ring overlap, and refresh-button containment. English
screenshots were visually inspected in both themes and at the narrow width;
the Codex options remained reachable in the fixed-height scrolling panel.
The shared used/remaining toggle and an empty-history case also passed in the
initial run. The final 32-case browser run additionally confirms that a relay's
unused daily allowance retains its 100% meter without a chart, while the two
subscription charts remain mounted.

No installed Vary AI application or AI client was launched, replaced, or restarted. No live
credentials or provider responses were used. The implementation is shared by
macOS and Windows, but this local verification does not establish Windows,
Linux, installed-webview, packaging, or release acceptance. Remote CI was not
run because this work was not pushed.
