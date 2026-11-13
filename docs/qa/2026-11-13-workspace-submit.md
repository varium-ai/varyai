# Complete workspace submission verification

This record covers all completed source, test and documentation changes requested
for submission on 2026-11-13. The candidate starts at
`98a894d84ed2774c769db22757075777f4d5ef7a` and includes the files fingerprinted
below. It combines routing list simplification and minimum-window sizing,
provider loading placeholders, duplicate supplier labels, localized navigation
widths, and update download progress/cancellation. This record supersedes earlier
partial or concurrently changing-tree results for submission purposes.

The exact candidate was materialized under
`/private/tmp/vary-routing-submit-bmomv08z`, using the existing dependency and
Cargo caches. Source fingerprints were compared with the shared working tree
before staging. This verification document is the only subsequent addition;
implementation and test files were unchanged after their successful checks.
Generated renderer bundles, compiler output and local test logs are excluded
from the commit.

Host: macOS, Darwin 24.3.0 arm64. Node 24.11.0, pnpm 10.12.3,
rustc/cargo 1.98.1. Final command logs are under
`/private/tmp/vary-complete-submit-logs/`.

| Check                                                                                                | Result                                                        |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `pnpm typecheck`                                                                                     | Passed                                                        |
| `pnpm format:check`                                                                                  | Passed                                                        |
| `pnpm build:renderer`                                                                                | Passed                                                        |
| `pnpm test:unit`                                                                                     | Passed: 305 files, 4,061 tests                                |
| `pnpm test:dev`                                                                                      | Passed: 8 tests                                               |
| `pnpm test:macos-dmg`                                                                                | Passed: 5 tests                                               |
| `pnpm test:trae-bridge`                                                                              | Passed: 66 tests                                              |
| `pnpm test:model-default`                                                                            | Passed: 9 tests                                               |
| `pnpm test:workbuddy-checkin-auth`                                                                   | Passed: 5 tests                                               |
| `pnpm test:doubao-checkin`                                                                           | Passed: 13 tests                                              |
| `pnpm test:agent-rewards`                                                                            | Passed: 9 tests                                               |
| `pnpm test:qoder-codec`                                                                              | Passed: 2 tests; 1 installed-client test skipped by the suite |
| `python3 -B scripts/tests/recover-shared-v23.test.py`                                                | Passed: 5 tests                                               |
| `pnpm audit --prod`                                                                                  | Passed: no known vulnerabilities                              |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml`                                             | Passed                                                        |
| `cargo check --manifest-path src-tauri/Cargo.toml`                                                   | Passed                                                        |
| `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`                                   | Passed                                                        |
| Actual updater state module compiled and executed independently with `rustc --test` and cached Tokio | Passed: 4 tests                                               |
| Complete Rust library and integration suites                                                         | Incomplete locally: disk-space limit described below          |

The earlier candidate's full Rust test-binary compilation failed with
`No space left on device`. Its subsequent full-test attempt was interrupted.
Full Rust tests for the final updater changes could not run within the remaining
local disk space. Only unlocked abandoned compiler-session output was removed;
active builds and application data were preserved. Formatting, compilation,
Clippy and the four actual updater state-machine tests passed on the final
candidate. These results do not replace full library/integration acceptance.
The local Rust gap was disclosed before committing; remote CI must validate the
exact pushed revision on Linux, Windows, macOS and Windows-to-WSL2 when required.

The initial restricted frontend attempt could not complete registry signature
verification for the pinned pnpm release. Registry-enabled checks passed without
bypassing signature verification. Launcher tests used temporary loopback ports;
all fixtures used synthetic data. No installed application was launched,
replaced or restarted, and no live update package was installed. Installer,
signing and notarization acceptance is outside these checks.

## Candidate file fingerprints

| File                                                    | SHA-256                                                            |
| ------------------------------------------------------- | ------------------------------------------------------------------ |
| `docs/architecture/routing-observations.md`             | `1cb235bd88f3127e4cfabb619efc08f8facea57c11d371e14e933aa3d9b1a304` |
| `docs/guides/intent-routing.md`                         | `6ce1eba7cc857e5aba537e0cb72968da22f58a72045bddfcc550de0422658e14` |
| `docs/guides/subscription-providers.md`                 | `8e236deb80718338e45772f950b2cdc20ed747d2dace8c946e2fae8e48cc7035` |
| `docs/qa/2026-11-13-app-update-progress.md`             | `f18fd1132e10a666fd0acc92dd25c3071af35454ea04bdec7833d31fd4e08a14` |
| `docs/qa/2026-11-13-navigation-label-overflow.md`       | `2aa8fa3b1fa21fd0b4821dec498b2a996d1ba936b67463a9123f09ccee63f252` |
| `docs/qa/2026-11-13-provider-loading.md`                | `4cd0aa4ca2b686fa579c14764bd9a4c0d7cc0633b724a25166c4eddc75d026b5` |
| `docs/qa/2026-11-13-routing-list-simplification.md`     | `7b8b6031ac95d016d830f12dbb4cb7697d15602b405634756c27b8817cf16658` |
| `docs/qa/2026-11-13-routing-provider-names.md`          | `9204692addd4ca92fd8332e92b867ae7a0da395e0c11756bd9506e0b7df2791d` |
| `docs/troubleshooting.md`                               | `942cd72b7a2387d634d664a2036ecfee6c0bdd84172c5f9d3107ee32ed3ffa02` |
| `src-tauri/src/commands/settings.rs`                    | `7248e876fc1a343f3ad84361bddcddca14bc7243fdc4b0f5835afe6099af05f0` |
| `src-tauri/src/commands/settings/update_install.rs`     | `69ba0063fcd8103edd43c6100fe38dabafe770d7aaec0b2ab0e6ec3dc5a0caa5` |
| `src-tauri/src/lib.rs`                                  | `b4d23ae1d95b0306e6e6c32f043ff86ecbca065ddb00a03d12825715cf5ba5f4` |
| `src/App.tsx`                                           | `99c446f0ebca27ce08103b661182f4b64acc04bfeedc0fd4e80b5a2dbbf5d888` |
| `src/components/AppUpdateDialog.tsx`                    | `4fc9966164a7aea2168b4c45d58afb0ffd1995de907340dbe57d5fdd5fe8fa52` |
| `src/components/app-update-dialog.css`                  | `050ec56349f08d1d501a0b6b9c5c7aca62444692dc70135f79a23c350878d232` |
| `src/components/layout/workspace.css`                   | `f7e84b2920a64e419638a5ff01002acac398aa34e12fac41e7d460a872d840bd` |
| `src/components/providers/ApiProviderQuota.tsx`         | `9ca1d33fed8d6717addc33b82c58a5bf42a2e29dbfebdd2b3c6e1a56a221bc2b` |
| `src/components/providers/ProviderListQuota.tsx`        | `7171205c04d13faaf303552138dec9a5e1a318486cfe2d3ecdb912e5d2425e8a` |
| `src/components/providers/ProviderLoading.tsx`          | `fdb67a4a91f2a702e09e7b17e1c3835163f0c620e3d7dd38fe8d4884dbe2e3e2` |
| `src/components/providers/RoutingUpstreamsPanel.tsx`    | `1151646fff5447e86ae975e685537b294d2e9a8e59359ff49b417a3144ec078b` |
| `src/components/providers/SubscriptionAccountQuota.tsx` | `66eeeca131e42c70281e2e5d6e531979cf51e0e056ec37240add56cf59f5b03f` |
| `src/components/providers/SubscriptionQuotaSummary.tsx` | `e6ee0a523433afaa2b49a36da2bf91b4b8b4a3e551ff434993b95d779f972c4f` |
| `src/components/providers/provider-loading.css`         | `ae112c684a6569ac4c7c6454fb852a74564c800c84da3809b844b7d7f1ec11ee` |
| `src/components/routing/IntentRoutingPanel.tsx`         | `3f6c0ee23767cbbf31974bee9b746ebe99012d1298b3346dd324489c28c0031c` |
| `src/components/routing/RoutingStage.tsx`               | `3067effd2c6124d8241666be4fba8727cf4d5fb04c89cd66feeab0ca6a9e3200` |
| `src/components/routing/routing.css`                    | `a6021dc27127cc54bd16cfef895563fc0a5ae4f3e5e67e8a155d957b3946aa69` |
| `src/contexts/UpdateContext.tsx`                        | `b8c0d3f6bab620df8aba8d942304ecdd5938507f81c56bfbbd2777fc9c3fdb2b` |
| `src/i18n/locales/en.json`                              | `1b81c10d0f7f4402cebbcad0310eeee7af4d1d0204bed9159c29998b8e8d9761` |
| `src/i18n/locales/ja.json`                              | `ac0219f80faa66fbc8113d4617aa59e7188e6fd9310f75456ee211ba4dad2456` |
| `src/i18n/locales/zh-TW.json`                           | `6cf10e364cc154e41d569a1521315d99f0732deb4ad9741a1bd092e4bf4f8169` |
| `src/i18n/locales/zh.json`                              | `7c596b55117561a9867ebe12be52462b8db4f020cad96f535ecd949884542c34` |
| `src/lib/api/settings.ts`                               | `c868cbd4b87b7f358fba6d1090581e86e717f63bade99be842bbfd28ad58ad53` |
| `src/lib/updater.ts`                                    | `7d5fbae5172b843489b5ec863e78de1aa8ebb689cac09f3522892eaf7e8623f8` |
| `tests/components/ApiProviderQuota.test.tsx`            | `694cb482e80534af080e0959d8e52d6ee4252daf0d5436d71c4d0914362f4b0f` |
| `tests/components/AppUpdateDialog.test.tsx`             | `9518423b7110169ad26baee4704d9590a13bbdfbdf3351bafacc17b818189c7f` |
| `tests/components/CodexProviderEditor.test.tsx`         | `a414c423b427e6a7daf4d3619e3d011bfffbeea26c22b7b16efc022617ce7237` |
| `tests/components/IntentRoutingBugs.test.tsx`           | `d81193a6c17fc89634a1bb83359c48f1cf08d332ce1dcd167be57c06eaa5bc47` |
| `tests/components/IntentRoutingPanel.test.tsx`          | `8b89e05f4b88dc51d72e3e5bae8bf6dd4a3f4919da020d1f929592ab69084604` |
| `tests/components/ProviderListQuota.test.tsx`           | `c1a6d464bd175e5c18cbc66707d46592c60bbed46a658bfcf3323017f638e809` |
| `tests/components/RoutingPage.test.tsx`                 | `f99ca74e35d9f69d73a4a77755e316d4bcd0496712b5b6028c20916f3cf13bd5` |
| `tests/components/RoutingStage.test.tsx`                | `62e67600ac1cee38101f45942f9ffed853698324e497f67340716cf372612614` |
| `tests/components/RoutingUpstreamsPanel.test.tsx`       | `5851ecefc5de0ff16e43d9b13925d658af2dd95f83ee02dc0c192799bbd873cd` |
| `tests/contexts/UpdateContext.test.tsx`                 | `b8231c3a35b6f523a151b4dc81d70d3eaaac7ecc39cb0e35b9530d9f51285854` |
| `tests/integration/GlobalApp.test.tsx`                  | `ac2ec9f879c7bab32418bf35716c26d81554448a234b02ce2cc70545639dc946` |
| `tests/lib/updateInstall.test.ts`                       | `91007294b92ae03ec7ae41c76df75eb419c034e1d5dc58389333cf0fe5c7d4b1` |
