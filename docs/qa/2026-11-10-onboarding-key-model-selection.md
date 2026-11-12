# API key model selection during onboarding

Checked base: `a0001b89c8b9b1e7b5fe7bc276d910e447a20c27` plus the working-tree
changes to `OnboardingWizard.tsx`, its component tests and the onboarding guide.
Host: macOS 15.3.1 arm64; Node 24.11.0, pnpm 10.12.3,
Rust and Cargo 1.98.1.

API key onboarding previously saved the discovered catalog with an empty model
selection and vendor mode. It now selects the discovered models in picked mode,
matching new subscription connections. The first 256 models are selected to
respect the existing editor and backend limit; the complete catalog and model
metadata are retained. Existing subscription selection and reconnect policies
are unchanged. The shared React implementation applies to macOS and Windows.

## Verification

- Before implementation, the updated and new regression assertions reproduced
  the bug: six failed and 41 passed.
- Focused onboarding component tests: 52 passed. Coverage includes all four API
  protocols, selected model metadata, full catalog retention and the 256-model
  selection limit, alongside the existing subscription tests.
- `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` and additional
  Prettier checks for the test and guide: passed.
- `pnpm test:unit`: 3,766 passed across 289 files.
- `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards` and `pnpm test:qoder-codec`: passed. The optional
  installed Qoder codec test was skipped.
- `python3 -B scripts/tests/recover-shared-v23.test.py`: five passed.
- `pnpm audit --prod`: no known vulnerabilities.
- `cargo check --manifest-path src-tauri/Cargo.toml`,
  `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
- `cargo test --lib --manifest-path src-tauri/Cargo.toml --quiet`:
  6,291 passed, 19 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --quiet`: passed on the
  final pre-commit run: 6,291 library and 163 integration tests passed, with
  19 library tests ignored. Two earlier attempts were interrupted by SIGINT
  without a failing assertion; the completed run resolves that verification gap.
- `cargo test --manifest-path src-tauri/Cargo.toml --test '*' --quiet`: passed
  when the integration targets were run separately.

Tests use synthetic model catalogs and mocked provider operations. No running
application was launched, installed or replaced. Remote CI, Windows execution
and packaged application acceptance were not performed. These results describe local verification before the requested commit and push.

## Checked file fingerprints

SHA-256 of the final implementation, tests and guide:

| File                                             | SHA-256                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------ |
| `src/components/onboarding/OnboardingWizard.tsx` | `e4f6f85113d4f7cf97d84ff6233780c86f18f375bd704b2bcb43869763081332` |
| `tests/components/OnboardingWizard.test.tsx`     | `241dae59215296545cdef2c421fa868fc9adfb9833904842eda2a59c136a7efb` |
| `docs/guides/onboarding.md`                      | `e01a9d972745f11ac0359963bb59ca39beef478fad1efdcb4a9633f6cb384cd3` |
