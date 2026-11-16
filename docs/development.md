# Vary AI development guide

This guide covers development from the current source checkout.

## Toolchain and setup

Use the Node version in `.node-version`, the pnpm version in `package.json`, and the stable Rust toolchain in `rust-toolchain.toml`. Install the native Tauri prerequisites for your operating system. The crate's declared minimum Rust version is recorded in `src-tauri/Cargo.toml`.

```bash
pnpm install --frozen-lockfile
pnpm dev
```

`pnpm dev` owns the renderer and Tauri processes together. Vite selects an available port starting at 3001, and the launcher passes its actual URL to Tauri without starting a second renderer. An occupied port, including one held by an older development process, is skipped without stopping the existing listener. `Ctrl+C`, termination, or a failed Tauri startup closes the launcher's renderer and Tauri process group. Additional Tauri options can be passed directly, for example `pnpm dev --no-watch`.

`pnpm dev:renderer` starts only the browser renderer on port 3001 and requires that port to be available. Native commands, the proxy and database operations require the Tauri runtime. Exit an existing app before starting a separate development instance.

For manual acceptance on the normal installation paths, run `pnpm dev` without `CC_SWITCH_TEST_HOME` or a test Tauri configuration. VaryAI uses its normal application identity and independent storage, while existing native client directories remain in place. Installer replacement acceptance uses the normal packaged application: install the selected older build, then replace it with the new package and verify storage ownership, native configuration and route recovery. Do not clear user data to simulate an installation.

The renderer and Tauri development URL use `http://localhost:3001`. The application gateway defaults to `127.0.0.1:15722`. A separate application identifier and temporary home isolate storage for acceptance tests, but additional instances still need distinct ports when their listeners run concurrently.

Console and file logs display application module targets as `vary_ai_lib` or `vary_ai`, including their module paths. Internal crate names remain compatible. Third-party targets, message text, timestamps and runtime level filtering retain their existing behavior.

The optional `setup.sh` changes the host environment: it can install system packages, activate global tools and select a Rust toolchain. Review it before running it. Manual setup is sufficient for contribution checks; this guide does not claim that every installer path has been validated.

A temporary test home also redirects native client paths. A nonempty `CC_SWITCH_TEST_HOME` takes priority over an inherited `DSH_HOME` for Harness configuration so tests launched inside Harness stay isolated. An empty test home represents a machine without configured clients, not just a fresh VaryAI installation. To verify an existing-client installation, prepare private client configuration copies or synthetic existing-client fixtures inside that home. Do not copy rotating OAuth sessions into parallel test clients; use a test API-key provider or a separate authorization. Changing the VaryAI application data root must leave native client paths unchanged. Do not create empty production client files as a substitute for setup, and do not grant OS trust or connection authorization as part of routine checks.

## Desktop text selection

The renderer defaults to `user-select: none` on `body` so navigation, labels,
and tool lists behave like desktop controls instead of a selectable web page.
Keep the WebKit-prefixed declaration alongside the standard property for macOS;
Windows WebView2 uses the same shared stylesheet. Inputs, textareas, editable
content, CodeMirror content (including read-only editors), and `pre`/`code`
blocks explicitly retain text selection. Add `select-text` to other content
that users need to copy; do not enable selection on an entire page or disable
it with a universal selector or a JavaScript selection handler.

## Checks

```bash
pnpm typecheck
pnpm build:renderer
pnpm test:unit
pnpm test:dev
pnpm test:macos-dmg
pnpm test:trae-bridge
pnpm test:model-default
pnpm test:workbuddy-checkin-auth
pnpm test:doubao-checkin
pnpm test:agent-rewards
pnpm test:qoder-codec
python3 -B scripts/tests/recover-shared-v23.test.py
pnpm audit --prod
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml
```

Before submitting, also run the CI formatting, Clippy and integration-test checks:

```bash
pnpm format:check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

### Commit and CI acceptance

Before committing, run the applicable checks above on the final changes and compare them with `.github/workflows/ci.yml`. Include the helper tests and production dependency audit, rather than reporting only renderer and Rust unit tests. Record the checked revision or working-tree changes, OS, toolchain, commands, outcomes and unavailable checks. Fix relevant failures before committing; disclose any required check that cannot run. Changes made after verification require rerunning the affected checks.

After pushing, inspect the workflow for the exact pushed revision and follow it until the required jobs finish. Report pending, skipped and failed jobs explicitly. Backend acceptance includes the applicable Linux, Windows and macOS jobs and the Windows-to-WSL2 contract when the workflow requires it. Local success on one platform is not cross-platform CI acceptance. Use failed job logs to establish the cause; do not bypass a check or weaken an assertion merely to make CI green. All required checks must pass for the revision being merged. If remote results cannot be obtained, report verification as incomplete.

Use `pnpm exec vitest run <test-files>` to select frontend tests during development; focused tests do not replace the required checks before committing. There is no `pnpm lint` command. `pnpm format:check` checks `src/`; apply formatting tools to other changed files as appropriate.

Claude Desktop, WorkBuddy, WorkBuddy AI and Qoder CN process inspection and confirmed restart support macOS and verified Windows installations. Keep native helpers behind their platform guards; pure freshness, selection and lifecycle orchestration compile under `cfg(test)` so synthetic fixtures run on every CI platform. Desktop integrations on Linux report `unsupported`. macOS requests a normal quit; Windows asks windows to close before ending verified remaining processes when necessary. Restart confirmation text shows only the current operating system's guidance, including the Windows-only Codex recovery warning. Tests must use fake runtimes and never inspect, quit, signal or launch installed applications. Codex has a separate idle-aware app-server worker: route changes reload automatically, while model-catalog changes require confirmation. Its gateway activity guard covers the response body lifetime and a ten-second quiet period without modifying frames or trailers. Run Clippy on all three CI platforms because a macOS-only check cannot detect unused native helpers in other platform builds.

For Rust compatibility changes, run the backend checks with the exact CI toolchain as well as the previously supported local toolchain; passing on one version does not establish compatibility with another. DLP accounting uses the private `src-tauri/src/proxy/security/atomic_update.rs` compare-and-exchange helper to avoid both the deprecated `fetch_update` API and a replacement newer than the declared minimum Rust version. Its tests cover retry observations, refusal without mutation, budget bounds and saturated owners. API compatibility alone is not evidence that the entire dependency graph has been tested on the minimum version.

Codex rollout activity reads the modification time and bounded event suffix from
the same open file handle. Windows can retain an old modification time even on
that handle while a writer keeps the rollout open, so freshness also considers
timestamps recorded in the bounded suffix. Recent events keep active turns
protected; completed turns and expired archives remain idle. Malformed recent
suffixes block restart, while malformed expired archives remain expired. Keep
the held-open writer and stale-metadata regressions in the cross-platform suite.

Provider tests that read Hermes configuration must isolate `HERMES_HOME` and
`LOCALAPPDATA` alongside the temporary home directory, and restore their previous
values afterward. Hermes uses `LOCALAPPDATA` on Windows, so changing `HOME` alone
can leave providers from earlier tests visible to later tests.

Shared Agent projection tests run portable client assertions on every platform.
Claude Desktop fixtures use `Library/Application Support` on macOS and separate
`AppData/Roaming/Claude` input files from `AppData/Local/Claude-3p` output files on
Windows, all under the temporary test home. Linux must assert that Desktop
projection is rejected while retaining the portable client coverage.

Gemini and T3 Code publish readable ASCII model aliases. Refreshes retain an
existing readable alias when its display label changes, and keep historical
identities addressable so existing model selections and sessions can continue.

The Qoder TLS listener owns its accepted connection tasks and cancels and joins
them during shutdown. `stop()` waits for the listener task to finish; a timeout
must not detach it and report completion while it still owns sockets. A portable
regression delays listener cleanup past the former three-second deadline and checks
an immediate same-address rebind. It leaves the listener unconnected so Windows
connection reuse restrictions do not obscure the shutdown check. The macOS
certificate test also keeps an immediate rebind assertion and checks that pending
TLS handshakes close. It uses an isolated certificate and never installs system
trust.

## Toolchain dependency review

The Windows-to-WSL2 CI contract keeps the distribution running with a detached Linux process while Windows accesses its UNC filesystem. Preparation, contract execution, and cleanup have separate timeouts. Preparation logs distinguish Linux directory setup from Windows UNC access so a stalled runner can be diagnosed without waiting for the whole job timeout. Both the PR workflow and the manual WSL2 workflow use the same lifecycle.

Run `pnpm audit --prod` for application dependencies and `pnpm audit` for the complete development tree. The checked-in overrides set security floors for build and test dependencies; review compatibility when removing or changing them.

Zod is pinned to 4.6.7 to address GHSA-mrhx-c7xc-m3w5. Its pnpm patch rewords two explanatory comments that Rollup mistakes for misplaced pure annotations; it does not change executable code or actual optimization annotations. Recheck whether the patch is needed when upgrading Zod. Validation code has a separate renderer chunk to retain the existing bundle size budget without suppressing build warnings.

The remaining known development dependency advisory is [GHSA-vfj7-8cjw-p6xm](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm) in `braces` 3.0.3, used by the Tailwind 3 globbing toolchain. As of October 22, 2026, no patched version is published. This package is absent from the production dependency tree. The documented build consumes repository-controlled patterns rather than a public glob-processing API; this limits exposure but does not fix the dependency. Maintainers must revisit the exception when a patch or a compatible toolchain replacement becomes available, or when a workflow starts accepting untrusted patterns. Do not suppress the advisory as though it were fixed.

## Google Analytics configuration

Desktop statistics use gtag.js directly with Measurement ID `G-2R2MZ76G1E`. Set `VARYAI_GA4_MEASUREMENT_ID` at compile time to override the public ID; an empty or invalid value disables statistics availability. `VARYAI_ANALYTICS_BASE_URL` is no longer used. No Measurement Protocol API secret belongs in the desktop build, and no application server forwards these events.

The `analytics.html` entry runs in an incognito, hidden WebView with no plugin capabilities. Its native command guard permits only token-scoped dispatch acknowledgements. Packaged analytics responses receive a separate Google-only CSP and navigation is restricted to the local analytics asset. The main application CSP stays unchanged. The analytics bundle must not import application state or UI chunks; the production build regression checks its complete static dependency graph. Window-state persistence excludes this WebView, and disabling statistics destroys it.

The general Settings page provides an enabled-by-default switch under Privacy. Existing disabled choices remain disabled. Schema 3 preserves valid legacy IDs but resets earlier backend dispatch markers so that Google receives an initial observation. Invalid state and unavailable local storage disable reporting. See [data and network behavior](data-and-network.md) for the events, Google metadata and dispatch limitations.

## Development boundaries

- Follow [AGENTS.md](../AGENTS.md) for proxy, protocol, database and secret-handling invariants.
- Keep tests isolated from real client configuration, credentials, running apps and the default proxy port.
- Use synthetic provider endpoints, keys and sample content. Sanitize logs and screenshots before sharing.
- Keep the four runtime locale files aligned; the application remains multilingual.
- Provider editor operation failures use the shared Sonner toast with an action-specific message (save, model discovery, model test, account import, enable/disable or delete). Do not render a generic catalog-update error inside the form. Preserve the draft after a failed save; show only recognized, localized error codes and never raw backend error details. Account availability, sign-in guidance and empty/loading states remain contextual.
- Add provider uses four entry cards: subscription accounts, model vendors, import and custom providers. Import lives inside this page and returns focus to its card when closed. The popular-provider table shows four subscription shortcuts and four official API vendors by default; it omits the API relay group. Subscription and vendor cards open their full directories in centered dialogs with independent search. Closing a picker cancels its unfinished authorization and restores focus to its card while preserving the page search and four popular shortcuts per row. Escape clears the picker search before closing; deferred picker focus cleanup must not steal focus from a newly opened provider editor. Preserve inline sign-in, cancel authorization before opening import or an API editor, and keep narrow layouts, dark mode and all four runtime languages supported.
- Provider forms use a centered modal capped at 560 px, compact 12–13 px field text, 28 px inputs, and a scrolling body. Provider sizing must override shared dialog styles regardless of stylesheet load order; preview with provider CSS before the global stylesheet, matching the renderer entry point. Preserve the existing fields, validation, protocol detection, key management and save behavior when adjusting this presentation. Narrow windows stack labels above controls and retain outer margins; keep dark mode and all four runtime languages supported.
- Subscription confirmation must keep the selected credential source visible and automatically load its missing model catalog. Codex native/managed account deduplication uses only backend-proven aliases; project the selected ID into the display row and use that credential's own readiness, never another alias's readiness. Reconfirming a proven alias replaces that identity's existing binding while preserving other accounts and model preferences. Subscription model toolbars share the API form's tri-state select-all checkbox and inline refresh action. Bulk selection applies to visible chat models and preserves hidden/image selections, ordering and saved metadata.
- Provider field labels align right on desktop. Saved API keys use a four-column table for credentials, model scope, limits and actions, with editors and model pickers expanded below the relevant row. Use square checkboxes in the credential column to enable or disable keys; do not duplicate these controls in the actions column. Keep copy beside the masked key, and put edit and red delete icons in the actions column with localized accessible labels. Omit the single-key "In use" label; retain the first-key indicator when multiple keys are enabled. Keep the table within its own horizontal scroll area on narrow windows and preserve all existing key callbacks and draft updates.
- A successful key copy shows a green animated check for two seconds after the latest successful copy, then restores the copy icon and accessible label. Keep confirmation scoped to the copied row, clear its timer when replaced or unmounted, respect reduced motion, and show success only after the clipboard write completes. Never include copied credentials in feedback text or diagnostics.
- Provider duplication follows Magpie's `duplicateProvider`: open the Add form directly from the saved provider, show its copy title, and keep stored credentials private until saving. Ignore deferred focus cleanup from an editor that has been replaced by that draft; only the matching closed editor may clear the active draft. Regression tests must let the old dialog's deferred cleanup run before checking the copy.
- Single-choice native selects share `src/components/ui/native-select.css` for a flat surface, theme-aware chevron, keyboard focus and disabled states. Keep page-specific heights, and avoid resetting the chevron or its reserved end padding with equally specific background/padding shorthands. The compact login proxy retains its own SVG indicator; multiple and sized list boxes retain native rendering. Forced-color mode restores the system select indicator.
- Read the [storage design](architecture/storage.md) before changing migrations or backup formats.
- Use the [platform build guides](README.md) for local packaging. Production release infrastructure is separate.

Contribution and reporting expectations are in [CONTRIBUTING.md](../CONTRIBUTING.md) and [SECURITY.md](../SECURITY.md).
