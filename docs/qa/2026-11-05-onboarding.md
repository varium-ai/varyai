# Onboarding verification — November 5, 2026

The change adds a two-step connection guide for API keys, supported subscription accounts and existing application imports. It reuses existing backend commands and the existing first-run acknowledgement setting. No Rust implementation, application installation or running desktop instance was changed by this task.

The UI follows the workspace's brand icon, colors, typography and controls. Browser inspection used a temporary renderer-only fixture with synthetic data, including light and dark themes, Key connection success, subscription selection and import selection. The smaller layout was inspected at 980 × 680. The fixture and preview server were removed after inspection. No real credentials or account authorization were used.

## Checked environment and scope

- Host: macOS (Darwin), Node.js 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.
- Working-tree source changes on top of `a30e86d1d11641cf40dbbdaf02c11985d3e34df0`; another task updated the checkout during verification. The source file hashes below identify this task's final files, including shared files that also contain pre-existing work.
- No commit, push or remote CI run was performed by this task. Linux, Windows, WSL2, installer, signing, notarization and live release acceptance are not established.

## Results

| Check | Result |
| --- | --- |
| `pnpm typecheck` | Passed on the final implementation. |
| `pnpm build:renderer` | Passed. The new guide is loaded lazily; the final build emitted no circular chunk warning. |
| `pnpm format:check` | Passed. |
| Full Vitest suite (`node node_modules/vitest/vitest.mjs run`) | 3,436 passed, zero failed or pending. This invokes the same runner as `pnpm test:unit`; the initial sandboxed pnpm version switch could not reach the registry. Subsequent pnpm checks ran with package manager cache/network access. |
| Final affected Vitest tests | 87 passed across onboarding, first-run persistence, upstreams, subscription addition, imports and the global application shell. This run covers the final account-policy preservation and header changes made after the full-suite run. |
| `pnpm test:dev` | 8 passed. |
| `pnpm test:trae-bridge` | 57 passed. |
| WorkBuddy, Doubao and Qoder helper tests | 20 passed, one existing conditional skip. |
| `python3 -B scripts/tests/recover-shared-v23.test.py` | 5 passed. |
| `pnpm audit --prod` | Passed; no known production vulnerabilities reported. |
| `cargo check --manifest-path src-tauri/Cargo.toml --offline` | Passed during verification. Concurrent backend changes mean this does not establish acceptance for a later backend tree. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --offline -- -D warnings` | Passed during verification, with the same concurrent-backend limitation. |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Failed on unrelated existing/concurrent Rust changes, including `claude_desktop_config/shared_recovery_tests.rs`. No Rust files were reformatted by this task. |
| Rust library tests | Initial sandbox run: 5,520 passed, 452 failed, 18 ignored; logs show temporary mock-server binding denied by the sandbox. A retry with local socket access emitted 20 storage/routing failures and did not complete; it was stopped. These results are not backend acceptance. |
| Full Rust integration command | Attempted, but interrupted while waiting for the shared build directory or rebuilding. Not accepted. |
| Isolated Rust failure diagnosis | Could not run: after waiting for the shared build lock, Cargo failed to write its test fingerprint with `No space left on device`. This does not establish the cause of every earlier test failure. |
| `git diff --check` for the task's files | Passed. |

The historical shell compatibility fixture now retains its original welcome dialog. Its tests continue to check historical behavior independently of the new first-run guide. New guide tests cover completion persistence, skipping, manual reopening, credential-header selection, safe failure messages, duplicate-submit protection, import filtering, selected subscription accounts and preserving model policy when reconnecting the same account.

Temporary disk exhaustion and simultaneous changes in the checkout interrupted verification. The final frontend checks above succeeded after recovery. Remaining Rust formatting/test failures and unavailable cross-platform results must be resolved before claiming full repository or release acceptance.

## Initial implementation source fingerprints

| File | SHA-256 |
| --- | --- |
| `src/App.tsx` | `2fcdbc73ecad7cc28cae20294c2e89c154eeff1cf1042fa33615fff0dfd580a2` |
| `src/components/FirstRunNoticeDialog.tsx` | `7ada1aea3255b9e3c31723e5e0146dc45f404eb48b43a6da2439653aa2ded0b8` |
| `src/components/onboarding/OnboardingWizard.tsx` | `92d846cb062ab83c374e8dd7097133b18fd4b3011e3c50115c94722f74679760` |
| `src/components/onboarding/onboarding.css` | `aea1cd3454c2d1bb9238047beff9268f9bcd6284670bf11b5fc1e07abea64140` |
| `src/components/providers/ProviderImportDialog.tsx` | `3521452c3c8cdff5754e13347b800b86ca120bb0bae4e6c7ab0db2c9b8e3818f` |
| `src/components/providers/RoutingUpstreamsPanel.tsx` | `d0ad4704366c169b7ca581c0de9995e5fa9495a3d039e2666f92e64550e447c7` |
| `src/i18n/locales/en.json` | `937e87b399bef076444ca4a031039a02aaa69f643338838a2a4638da3bc80cb4` |
| `src/i18n/locales/zh.json` | `dd02869eecb0980552f8be3349370b7839dcbf25f2a3c6cf457c82aef6a94a07` |
| `src/i18n/locales/zh-TW.json` | `e92df3f24a6a1cc39c85358e2afa976d9eeb9c5773ac9cecb41c432cd34fd112` |
| `src/i18n/locales/ja.json` | `995397875251aa0d394dce832d456d5fab35e1d622c0804ce5ce054efe617917` |
| `tests/components/OnboardingWizard.test.tsx` | `c0682a31abc18cdc7746a9a47281b40a30b1d4839a52422b82acf182c7fb6156` |
| `tests/components/FirstRunOnboarding.test.tsx` | `c70dcee3f8812f12f4f790b153460ea5d6a53f4d93e322763ae768d3a1867501` |
| `tests/fixtures/LegacyApp.tsx` | `26c4784cb785d0e4c56a59a821b93bf48f085ebc6ff26da3535c42ea23b07711` |
| `tests/fixtures/LegacyFirstRunNoticeDialog.tsx` | `622238e09cedfd576ed632ed9f94dab0581fe0ebafa33fc52e3fe2af7c6a7a46` |
| `docs/README.md` | `3ef37287aa85e3ebfd273c32e356c1cb858a58646e4a7b409e0bb10db999da25` |
| `docs/guides/onboarding.md` | `38d2e1778d575237425b9fff74cd7c32278736d1c17ab058dc998d87cd4b887f` |

## Follow-up: remove the permanent guide entry

At the user's request, removed the Models page's Setup guide button, the manual-open state and props, and the unused label in all four locales. The guide now opens only through the existing first-run condition. Updated the first-run tests and the usage guide to remove manual reopening. This supersedes the manual-reopening behavior described in the initial verification above.

Verified the follow-up working tree on macOS with Node.js 24.11.0, on top of `a30e86d1d11641cf40dbbdaf02c11985d3e34df0`:

- `node node_modules/typescript/bin/tsc --noEmit`: passed.
- `node node_modules/vite/bin/vite.js build`: passed.
- `node node_modules/vitest/vitest.mjs run tests/components/FirstRunOnboarding.test.tsx tests/components/OnboardingWizard.test.tsx tests/components/RoutingUpstreamsPanel.test.tsx tests/integration/GlobalApp.test.tsx`: 64 tests passed.
- Prettier source check and `git diff --check`: passed.
- Used the local package executables because sandboxed pnpm commands stalled during their package manager version check.
- Rust, the full frontend suite, helper tests, dependency audits and remote CI were not rerun for this frontend-only removal; the earlier results and limitations above are historical, not fresh acceptance for this tree. No commit or push was performed.

Follow-up SHA-256 fingerprints:

| File | SHA-256 |
| --- | --- |
| `src/App.tsx` | `f76cf2e0bc0c1d08a851d041d221861d7db9597112081b9e772d52b3b432caa9` |
| `src/components/FirstRunNoticeDialog.tsx` | `4f31f6193800f86989e506051aabcbc3dbe655ed969d919ab93af4668fb08b10` |
| `src/components/providers/RoutingUpstreamsPanel.tsx` | `383b8ee186be98844e86719ba99be54fe3bda5e9025b985fcd53126549998a96` |
| `src/i18n/locales/en.json` | `09be4aae72cf21d9c2dfecc3ff77f7cf7ced23375aae96edeab1785ed67fbd77` |
| `src/i18n/locales/zh.json` | `b37ef3f52153293e0c4f9ecd9e27f0498df3155c15aa09cbf7895a21a5b19b1d` |
| `src/i18n/locales/zh-TW.json` | `8678077af2c5fe7290a31b9fd7465241b2195e419800e7153382877de243714a` |
| `src/i18n/locales/ja.json` | `7779fbbbcd80b621a96d1a9cec64ac928c2989495eb65403c4c9a2065797f273` |
| `tests/components/FirstRunOnboarding.test.tsx` | `81b6a655e2dff830c12cab7e314757aef59febc663d5b5f710ac4cfbcae32520` |
| `docs/guides/onboarding.md` | `9627433c3935fbd7e1b1af1d3402e93657ee1b07673e84eeda08689ed5a10142` |
