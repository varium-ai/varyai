# UI visual consistency verification

The application-mode audit compares working-tree renderer changes over
`868cf24b3df4f8c087635009a8d8c41642a8921c` with the
[seven Figma comparisons](https://www.figma.com/design/DgaQbqyYAsgUi5n40HloXF/Vary-AI?node-id=1076-1288)
and the [v1.2 execution manifest](https://www.figma.com/design/DgaQbqyYAsgUi5n40HloXF/Vary-AI?node-id=1085-1046).
Application-mode browser fixtures used the actual React components and
synthetic Tauri responses, with screenshots and separately captured computed
styles before and after the changes. Dimensions below are logical CSS pixels.

| Issue | Baseline | Renderer verification | Implementation |
| --- | --- | --- | --- |
| VA-06 | The unavailable row had 0.5 opacity, including its reason and status. | Passed. Text ancestors have opacity 1. Names resolve to `#273443` / `#E5E7EB` and reasons to `#526277` / `#B4BCC8` (light / dark). The unavailable checkbox is disabled. The lowest non-exempt import text samples measured 5.43:1 / 5.72:1. | [Import styles](../../src/components/providers/providers.css) |
| VA-08 | Import cancel was ghost, 30 px tall. Custom cancel had an outline and was 36 px tall, with 6 px radius, 12 px type and an 8 px gap. | Passed. Both cancel buttons render as Secondary with 36 px height, 8 px radius, 1 px border and 13/20 px type at weight 500. Both footers measured 12 px between actions and a 20 px right inset. The import primary is disabled when zero items are selected. | [Custom editor](../../src/components/providers/ProviderEditorDialog.tsx), [import dialog](../../src/components/providers/ProviderImportDialog.tsx) |
| VA-09 | The tools directory was already 40 px tall with a 16 px search icon. The vendor picker was 32 px tall with a 14 px icon. | Passed. Both fields render at 40 px with a 16 px icon, 12 px trailing padding, 8 px corner radius and 13/20 px type. The two container widths and the focused border were measured independently. The picker accepts a query and clears it on Escape while the dialog stays open. | [Tools](../../src/components/agents/AgentConnectionsPage.tsx), [vendor picker](../../src/components/providers/ProviderPickerDialog.tsx) |
| VA-11 | Request-log numeric columns were left-aligned. The source and model summaries already right-aligned numeric columns. | Passed. Input, output, costs, timings and headers have matching right edges. The cache subvalue inherits right alignment. Time, source, model and status were measured as left-aligned. | [Request log](../../src/components/usage/RequestLogTable.tsx) |
| VA-12 | The gateway example and profile capture used native HTML selects. | Passed in the browser fixture. Both menus have a 36 px trigger, 12 px trigger padding, a 6 px trigger radius, a 2 px open border, an 8 px menu radius, a 6 px viewport inset and an 8 px trigger-to-menu gap. Tests checked the selected swatch, keyboard open and Escape focus return, four generated protocol examples and twenty profile entries. Synthetic tests exercised capture of each available Agent and the unavailable Doubao Work option. | [Gateway](../../src/components/proxy/GatewayKeysPanel.tsx), [profile picker](../../src/components/profiles/ProfileSwitcher.tsx), [menu styles](../../src/index.css) |
| VA-13 | The Chinese severity header wrapped over two lines at the 760 px fixture viewport. | Passed. Category and severity text occupy one line in the header and in each of three populated rows. Chromium and WebKit covered light and dark, English, Simplified Chinese, Traditional Chinese and Japanese, at 760 or 900 px viewport widths with long scope text and an operations column. | [Tool risk rules](../../src/components/security/ToolRiskRulesPanel.tsx) |
| VA-15 | The tools, routing and provider titles used 22/26 px at weight 700. Statistics, security and settings used 18 px at weight 600, with 28 or 30 px line height. | Passed for renderer typography. All six existing headings measured 18/28 px at weight 500. The stack requests Inter and Noto Sans SC and then a platform fallback. Chromium reported PingFang SC Medium for the mixed-script tools title on this host. | [Page title role](../../src/components/layout/workspace.css) |
| WCAG 2.2 AA text contrast | The sampled active primary button measured 3.99:1 in both themes. The sampled selected badge measured 3.48:1 / 2.74:1. | Passed for sampled states. The active primary measured 5.97:1. The selected badge measured 5.21:1 / 5.07:1. Body, helper, placeholder, read-only, badge, link, filled control and hover samples passed 4.5:1, with minimums of 4.83:1 / 4.94:1. Stable solid-background text on all six sampled page views cleared the applicable threshold. | [Application palette](../../src/design-tokens.css), [contrast regression](../../tests/config/textContrast.test.ts) |

Browser verification passed 32 groups across Chromium and WebKit on macOS
26.6.2 arm64. The contrast calculation used computed color, actual fixture
backgrounds, and ancestor opacity. Inactive controls were excluded as specified
by WCAG SC 1.4.3. Gradients, images, production data states and native webviews
require separate acceptance. See [Agent feature coverage](agent-feature-coverage.md#ui-consistency-and-text-contrast)
for native verification status.

## Local repository checks

- Final renderer verification used Node 22.22.0 and pnpm 10.12.3.
  `pnpm typecheck`, `pnpm build:renderer`, and `pnpm format:check` passed.
  `pnpm test:unit --maxWorkers=6` passed all 4,839 tests. The focused run
  passed 235 tests.
- The production dependency audit, development launcher, CI policy, macOS DMG,
  TRAE bridge, desktop model defaults, Pi session identity, WorkBuddy auth,
  Doubao Work check-in, Agent rewards, Qoder codec and database recovery
  helper checks passed.
- Rust 1.98.1 passed `cargo check`, `cargo fmt --check`, and
  `cargo clippy -- -D warnings` against the working-tree backend. The final
  `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` run
  passed 7,825 library tests and 165 integration tests, with 21 library tests
  ignored. The standalone library run had child-process failures during the
  reduced-debug retry. The final full run passed those cases, and the
  isolated child-process probe also passed independently.
- Native macOS and Windows webview acceptance and platform CI remain pending.
