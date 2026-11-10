# Compact routing form and reference styling verification

Date: November 10, 2026 (Asia/Shanghai).

## Checked scope

Base revision: `72ce390fa473087505934f91ed77728fac51ef01`, plus the working-tree
files identified below. Existing and concurrent routing-dialog, toolbar,
provider, locale and backend changes were preserved. No commit or push was
requested.

The visible form contains name, routing strategy and model members, in that
order. Every other field, including detailed member configuration, lives in a
closed Advanced settings disclosure. Controls remain mounted to preserve
changes and template-provided configuration. Template cards retain one Add
action that opens the populated dialog; only Save persists it.

The dedicated form stylesheet replaces the old editor-specific layout block.
It follows Magpie's system font, neutral light/dark palette, 27px input height,
7px input corners, 22px segmented options, 10px/14px label-grid spacing and framed
sheet dimensions. Pattern inputs have one border. Vary's own blue is inherited
for Save, focus rings and interactive highlights. Menus opened from the editor
share its typography and neutral surfaces. An existing delayed-refresh focus
regression was fixed by queuing the group identity in state, so restoration
also runs when the close event arrives after pending work finishes.

Reference: Magpie `internal/gui/assets/app.css` (`.editor`, `.segs`, `.opt`,
`.modal .editor.framed`) and `plugins/routing/routing.css` (`.rt-gedit`,
`.rt-patadd`, model rows). Attribution remains in
`third_party/routing-reference/NOTICE.md` and the new stylesheet.

## Environment

macOS 15.3.1, Apple Silicon; Node v24.11.0; pnpm 10.12.3.
The implementation is shared by macOS and Windows.

## Verification

- Browser baseline confirmed the old blue-gray track (`rgb(229, 237, 252)`),
  Inter/Noto font stack and redundant pattern frame before the rewrite.
- Test-first order assertion failed before routing was moved below the name.
- Test-first Advanced assertion failed before the disclosure was introduced.
- Final focused run: `pnpm exec vitest run tests/components/IntentRoutingPanel.test.tsx tests/components/RoutingPage.test.tsx tests/components/RoutingExecutionSettings.test.tsx` passed, 103 tests.
- Coverage includes the three primary fields, initially hidden Advanced content,
  changed execution values surviving collapse and Save, all three templates'
  populated model/rule/classifier values, cancellation without persistence,
  pending-save protection and focus restoration after delayed list refresh.
- A test-first editor masking assertion caught a tooltip containing a synthetic
  account address in the new member icon. The icon now uses the shared account
  masking context, and the expanded masking regression passes.
- Final `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`: passed.
- Full `pnpm test:unit`: 288 files passed, 1 failed; 3,764 tests passed and 4 failed.
  The four failures are existing navigation-label expectations in
  `WorkspaceNavigation.test.tsx` (en, zh, ja and zh-TW). This full run preceded
  the final disclosure-arrow and expanded-layout CSS adjustments; the final
  focused suites, typecheck, formatting and renderer build cover those updates.
- `git diff --check`: passed.

This follow-up changes the renderer, tests and documentation. Rust sources,
helper scripts and dependencies are unchanged by this work; their earlier local
checks are recorded in [routing page and form verification](2026-11-10-routing-page-and-form.md).
No installed application, real vendor request, client configuration, installer,
signing/notarization or remote CI acceptance was exercised. Linux/Windows/macOS
remote CI was not run.

## Browser verification

An isolated Vite preview used synthetic models and stubbed native APIs.

- At 1280 by 720, the roomy dialog measured 716.8px wide and remained centered.
- Light form input height: 27px; segmented option height: 22px; track:
  `rgb(241, 241, 244)`; enabled Save: `rgb(50, 120, 255)` (`#3278ff`).
- Name and pattern inputs had matching left edges; the pattern wrapper border
  measured 0px and its padding 0px.
- Dark surface: `rgb(35, 35, 39)`; dark track: `rgb(45, 45, 51)`;
  inherited Vary accent: `#78a8ff`.
- At 520 by 740, the dialog measured 472 by 684 at x=24, y=28. The form stacked
  into one column and had no horizontal overflow.
- Add on the Smart template opened the populated dialog with Advanced closed.
  Its four rules and two detailed member sections appeared on expansion. After
  collapsing, exactly the three primary fields were visible, all four rules
  remained in the DOM, and Save stayed enabled. No save was issued in the browser.
- The disclosure has an explicit right-pointing chevron when closed and a
  downward chevron when expanded.
- Expansion previously broke label/control alignment because native details
  and fieldset rendering boxes interrupted nested subgrids. Every section now
  defines the same 132px label column and flexible control column independently.
  All ten advanced row types aligned at control x=445.6 at 1280 by 720. At
  520 by 740, the advanced grid stacked into one 436px column without overflow.
- Rule textareas retain their compact row sizing and display the complete
  multiline rule text; the generic textarea minimum height no longer clips it.
- Final local screenshots: `/tmp/vary-routing-expanded-aligned.png` and
  `/tmp/vary-routing-expanded-rules.png`.

## Checked files

| File                                            | SHA-256                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------ |
| `src/components/routing/IntentRoutingPanel.tsx` | `f51c6f6fc2f18c9160cb06cc2660af6f7b280a5d6dde27fbbe18824d2b79376b` |
| `src/components/routing/routing.css`            | `e329cc8f56b168721c33fa2ae366774ce7d9304271f535324ce258a3c2d7d005` |
| `src/components/routing/routing-group-form.css` | `6150deb8f68a380f4a1048f60d4cd33788fec3b3fcd3d0a091bb6363f0bac0df` |
| `tests/components/IntentRoutingPanel.test.tsx`  | `2b7079bf300902b0d472277b209446fb92956ac8e2a3cf61bd74b1a39d1977ca` |
| `src/i18n/locales/en.json`                      | `4d3ac0c0783be15373d19aed00583d7dad1e6ed188a169a9b02d186812b635fe` |
| `src/i18n/locales/zh.json`                      | `67de898c5ad3956913631e0bb629bbc88bf3055fb8a8004f5afe3e04da27366e` |
| `src/i18n/locales/ja.json`                      | `23f32213b766584598c3b50ecbf4c42de185ce675198cede4d4aac03e2786a07` |
| `src/i18n/locales/zh-TW.json`                   | `dd302905dee63cdc71fbb876ae6e660c4428105bf4192a31346fa936842570ce` |
| `docs/architecture/routing-observations.md`     | `a8eb9b58407ec19ddb6d490717f6ecba14f789490e996d664f8154c11b4e58f4` |
