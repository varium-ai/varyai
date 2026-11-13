# Model picker branding

Checked the working tree based on `ae334540f4840854110792d61157fededb5e5117` on macOS (Darwin 24.3.0, arm64), Node v24.11.0 and pnpm 10.12.3.

## Changes

The Agent model picker identifies managed models and routing groups as using Vary AI in English, Simplified Chinese, Traditional Chinese and Japanese. Native models retain their separate native-settings label. Import-source names and compatibility identifiers are unchanged.

The shared model popover inherits the application's accent instead of overriding it with the reference implementation's purple. Soft badge and selection backgrounds derive from that accent at 10% opacity in light mode and 16% in dark mode.

Changed source files are `src/components/models/model-picker.css` and the four locale files. `tests/components/SharedModelPickerBranding.test.tsx` exercises the rendered labels using the actual locale resources.

## Verification

- Test-first reproduction: all four locale cases failed before the text change.
- Focused picker tests passed: 30 tests across the branding, shared picker and popover placement suites.
- `pnpm typecheck`, `pnpm format:check`, and `pnpm build:renderer` passed.
- `pnpm test:unit` passed: 306 test files and 4,065 tests (161.67 seconds).
- Pre-commit helper checks passed: `pnpm test:dev`, `pnpm test:macos-dmg`, `pnpm test:trae-bridge`, `pnpm test:model-default`, `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`, `pnpm test:agent-rewards`, `pnpm test:qoder-codec`, and `python3 -B scripts/tests/recover-shared-v23.test.py`. The Qoder suite retained its existing installed-client skip.
- `pnpm audit --prod` passed with no known vulnerabilities.
- `git diff --check` passed.
- An isolated browser preview of the actual component verified English labels and computed colors: `#3278ff` in light mode and `#78a8ff` in dark mode. Managed badges use the theme accent; native settings remain neutral. The preview uses synthetic models and was stopped after inspection.

No backend files changed, so Rust checks were not rerun for this presentation-only change. No native application was launched, installed or restarted. The user requested a commit and push after local verification. Remote CI results must be checked separately for the pushed revision; local results do not establish remote acceptance.
