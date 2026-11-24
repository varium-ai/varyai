# Agent feature coverage

## Shared API provider catalog discovery and selection (macOS and Windows)

Assessment against `APP_IDS`, `AppType`, the shared connection inventory and
the provider forms. Both the draft and saved shared-provider commands use the
same model-catalog service. Agent editors that call that service with either
Gemini format use the same fallback. Discovery supplies model metadata to the
existing picker. Runtime routing, selection, and status use the saved protocol.

For initial selection, the shared Add Provider editor is the entry point for
every row that can consume shared supplier models. Fetching a catalog in that
form sets the same explicit model selection already consumed by the Agent
menus and routing groups. Agent-specific editors, protocol adapters, connection
status, and route execution keep their existing contracts. Doubao Work's
official audit-only connection has no shared supplier picker. Renderer
verification covers default selection, manual removal, saving an empty
selection, and the existing catalog refresh path on the common UI. Native
Agent and Windows acceptance remain pending for this editor change.

Manual catalog discovery errors are handled in the same shared provider editor
for all applicable Agents below. The renderer regression verifies dismissal in
a new draft and retry in a saved provider. This is the form's error state.

| Agent ID           | Discovery entry point in scope                           |
| ------------------ | -------------------------------------------------------- |
| `claude`           | Shared supplier or Gemini-compatible provider editor     |
| `claude-desktop`   | Shared supplier or Gemini-compatible desktop editor      |
| `codex`            | Shared supplier or Gemini-compatible provider editor     |
| `gemini`           | Shared supplier or native Gemini provider editor         |
| `grokbuild`        | Shared supplier model menu                               |
| `opencode`         | Shared supplier or OpenCode provider form                |
| `openclaw`         | Shared supplier or OpenClaw provider form                |
| `hermes`           | Shared supplier or Hermes provider form                  |
| `pi`               | Shared supplier or Gemini-compatible Pi provider form    |
| `workbuddy`        | Shared supplier or China custom-provider form            |
| `workbuddy-ai`     | Shared supplier or international custom-provider form    |
| `qoder`            | Shared supplier or CN custom-provider form               |
| `doubaowork`       | Official audit only                                      |
| `trae`             | CN shared supplier model menu                           |
| `trae-intl`        | International shared supplier model menu                |
| `kimi`             | Shared supplier or managed provider form                 |
| `qwen`             | Shared supplier or managed provider form                 |
| `zcode`            | Shared supplier or managed provider form                 |
| `deepseek-harness` | Shared supplier or managed provider form                 |
| `t3code`           | Shared supplier model menu                               |

Loopback regressions cover both Gemini format names, native success and
pagination, and fallback to an authenticated OpenAI catalog. The saved-provider
editor was verified in the running macOS development build. Its Gemini mode
attempted both catalog paths at the same origin and displayed the models.
Individual installed Agent editors and Windows verification remain pending.

On the working tree over `eb416e75`, 38 model-catalog tests, `cargo check`,
Rust formatting, Clippy, and the focused renderer checks passed. The full
Cargo run failed to build its archive when local storage filled up.

## Responses reasoning stream completion (macOS and Windows)

Pre-implementation assessment against `APP_IDS`, `AppType`, the connection
registry and the gateway protocol endpoints. The shared Responses stream
decoder runs when a routed request uses a Responses upstream and the gateway
converts or collects its stream. The Agent model picker, connection status and
policy enforcement retain their existing behavior. Native passthrough retains
its original response frames. The same Rust decoder runs on both platforms.

| Agent ID           | Applicable adapter and entry point                           | Verification plan                     |
| ------------------ | ------------------------------------------------------------ | ------------------------------------- |
| `claude`           | Managed Messages and external Messages bridge                | Synthetic Anthropic output and errors |
| `claude-desktop`   | Managed Desktop Messages bridge                              | Synthetic Anthropic output and errors |
| `codex`            | Native Responses and routed Responses or Chat                | Synthetic Responses and Chat output   |
| `gemini`           | Managed Gemini and external Gemini streaming bridge          | Live reproduction and Gemini fixture  |
| `grokbuild`        | Managed native Responses, including converted routes         | Synthetic Responses output and errors |
| `opencode`         | Managed configured protocol                                  | Shared protocol fixture               |
| `openclaw`         | Managed configured protocol                                  | Shared protocol fixture               |
| `hermes`           | Managed configured protocol                                  | Shared protocol fixture               |
| `pi`               | Provider-scoped Responses, Messages, Chat and Gemini         | Shared four-protocol fixture           |
| `workbuddy`        | China managed Chat bridge                                    | Synthetic Chat output and errors      |
| `workbuddy-ai`     | International managed Chat bridge                            | Synthetic Chat output and errors      |
| `qoder`            | CN managed custom model route                                | Shared protocol fixture               |
| `doubaowork`       | Existing native route and separate audit path                | Shared decoder where conversion runs  |
| `trae`             | CN managed custom-provider bridge                            | Synthetic Chat output and errors      |
| `trae-intl`        | International managed custom-provider bridge                 | Synthetic Chat output and errors      |
| `kimi`             | Managed custom-provider route                                | Shared protocol fixture               |
| `qwen`             | Managed custom-provider route                                | Shared protocol fixture               |
| `zcode`            | Managed custom-provider route                                | Shared protocol fixture               |
| `deepseek-harness` | Managed Chat bridge                                          | Synthetic Chat output and errors      |
| `t3code`           | Managed model menu and native route                          | Shared protocol fixture               |

The shared stream suite passed 15 tests. The Responses fixture covers an empty
summary and a distinct summary with matching typed reasoning content. All four
output protocols retain the answer, successful terminal and usage.

Native verification used Gemini CLI 0.67.0 on macOS 26.6.2 arm64. A live request
through the bound GLM-5.3-Flash route reproduced the missing terminal in the old
development process. The same routed request completed in the rebuilt process
from a temporary CLI home. Other installed clients and Windows remain pending.

## Gateway key model picker (macOS and Windows)

Assessment against `APP_IDS`, `AppType`, the shared connection registry and
the gateway binding registry. The Keys tab uses the same shared provider and
routing-group catalog on both platforms. The Agents tab keeps each entry's
binding eligibility and readiness. New picker selections save canonical route
IDs. Existing saved IDs and binding enforcement continue through their existing
paths.

| Agent ID           | Existing policy entry point and adapter                  | Verification plan                         |
| ------------------ | -------------------------------------------------------- | ----------------------------------------- |
| `claude`           | Managed or legacy Messages routing                       | Shared catalog and existing policy tests  |
| `claude-desktop`   | Managed or legacy Desktop Messages and model mapping     | Shared catalog and existing policy tests  |
| `codex`            | Managed or legacy Chat/Responses and temporary sessions  | Shared catalog and existing policy tests  |
| `gemini`           | Managed or legacy Gemini model from the URL              | Shared catalog and existing policy tests  |
| `grokbuild`        | Managed or legacy Responses and compact                  | Shared catalog and existing policy tests  |
| `opencode`         | Managed provider-scoped routing                          | Shared catalog and binding inventory      |
| `openclaw`         | Managed routing with no automatic gateway-key binding    | Shared catalog and unavailable binding    |
| `hermes`           | Managed provider-scoped routing                          | Shared catalog and binding inventory      |
| `pi`               | Managed/provider-scoped native protocol routing          | Shared catalog and existing policy tests  |
| `workbuddy`        | China managed or legacy custom/official Chat             | Shared catalog and binding inventory      |
| `workbuddy-ai`     | International managed or legacy custom/official Chat     | Shared catalog and binding inventory      |
| `qoder`            | CN managed or legacy custom/official model routing       | Shared catalog and binding inventory      |
| `doubaowork`       | Existing native custom-provider and separate audit path   | Shared catalog and binding inventory      |
| `trae`             | China native bridge and custom-provider routing          | Shared catalog and binding inventory      |
| `trae-intl`        | International native bridge and custom-provider routing  | Shared catalog and binding inventory      |
| `kimi`             | Managed or legacy native custom-provider routing         | Shared catalog and binding inventory      |
| `qwen`             | Managed or legacy native custom-provider routing         | Shared catalog and binding inventory      |
| `zcode`            | Managed or legacy native custom-provider routing         | Shared catalog and binding inventory      |
| `deepseek-harness` | Managed provider-scoped Chat routing                     | Shared catalog and binding inventory      |
| `t3code`           | Managed model-menu and native route                      | Shared catalog and binding inventory      |

Selected-model list follow-up coverage uses the same entry points in the table
on macOS and Windows. For every listed Agent, the shared Keys form displays
catalog names, removes entries by exact route ID, and preserves saved IDs absent
from the catalog. Existing native adapters and policy enforcement consume the
same policy format. Renderer checks cover creation, editing, removal and the
existing binding inventory. Installed-client verification remains pending.

Synthetic regressions passed for catalog identity, exact policy saves,
historical IDs absent from the picker, external connection examples and the
full Agent binding inventory. The Gemini HTTP regression verified that the
legacy upstream ID is denied on a managed connection and that the route ID
returned by the picker permits the same request.

The working tree over `ae21978f` passed TypeScript, Prettier, the renderer
build, 4,868 renderer tests, the helper checks and the production dependency
audit on macOS 26.6.2 arm64 with Node 20.20.2 and pnpm 10.12.3. Rust 1.98.1
passed `cargo check`, `cargo fmt --check`, Clippy with `-D warnings` and
the full Cargo suite (7,846 library and 165 integration tests, 21 ignored).
The installed Gemini CLI bridge smoke test passed separately with a temporary
home and loopback fixtures. Installed-app picker acceptance with real keys and
Windows verification remain pending.

## Routed security errors (macOS and Windows)

| Agent entries                                                                                                                             | Runtime and UI                                                                                                                    | Verification                                                      |
| ----------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| DeepSeek Harness, Pi, Grok Build, OpenCode, OpenClaw, Hermes, Kimi, Qwen, ZCode, Qoder, T3 Code, TRAE, TRAE Intl, WorkBuddy, WorkBuddy AI | Authenticated routing-group requests only; error envelope follows the wire protocol. Routing history records the returned status. | Synthetic protocol projections; individual native clients pending |
| Codex, Claude, Claude Desktop, Gemini                                                                                                     | Same contract on routing-group requests; direct routes unchanged                                                                  | Synthetic protocol projections; native acceptance pending         |
| Doubao Work                                                                                                                               | Audit-only, no model error projection                                                                                             | Not applicable                                                    |

## Fragmented native Chat restoration (macOS and Windows)

| Agent entry points                                                                                                         | Applicable runtime path                                                        | Verification                                                           |
| -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------- |
| DeepSeek Harness, Pi                                                                                                       | Shared native Chat restoration when selected                                   | Pre-fix failures observed on macOS; post-fix native acceptance pending |
| Codex, Grok Build, OpenCode, OpenClaw, Hermes, Kimi, Qwen, ZCode, Qoder, TRAE, TRAE Intl, WorkBuddy, WorkBuddy AI, T3 Code | Only requests admitted to the native Chat finalizer; no change to other routes | Shared synthetic regression, not individual native acceptance          |
| Claude, Claude Desktop, Gemini                                                                                             | Other native protocols retain existing limits                                  | No new route or UI behavior                                            |
| Doubao Work                                                                                                                | Audit-only                                                                     | Not a model restoration path                                           |

Native Chat has no cumulative fragment-count cutoff. Shared HTTP fixtures cover an answer with 2,049 choice records and interleaved reasoning and content exceeding the old text-site cutoff. The rule UI, disabled policy, and platform adapters are unchanged. No Agent gains restoration authority from this change.

## UI consistency and text contrast

Pre-implementation assessment against `APP_IDS`, `AppType`, and the Agent
connections registry (including both TRAE bridge variants and Doubao Work's
separate audit entry). The same application-mode renderer runs on macOS and
Windows. For each row, the Tools heading and existing text use shared
presentation. VA-12 applies to the existing profile capture picker when offered.
The gateway protocol menu only selects a connection example. The existing
availability metadata, native adapters, and runtime commands own behavior.

| Agent ID           | Existing profile picker state      | Verification plan                                       |
| ------------------ | ---------------------------------- | ------------------------------------------------------- |
| `claude`           | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `claude-desktop`   | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `codex`            | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `gemini`           | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `grokbuild`        | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `opencode`         | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `openclaw`         | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `hermes`           | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `pi`               | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `workbuddy`        | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `workbuddy-ai`     | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `qoder`            | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `doubaowork`       | Unavailable with existing reason   | Synthetic picker passed; native macOS / Windows pending |
| `trae`             | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `trae-intl`        | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `kimi`             | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `qwen`             | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `zcode`            | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `deepseek-harness` | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |
| `t3code`           | Selectable when reported available | Synthetic picker passed; native macOS / Windows pending |

The application picker fixture verified all 20 displayed entries. Renderer
regressions exercised capture for each of the 19 available entries and checked
that Doubao Work was disabled with its existing availability metadata. The
gateway menu fixture exercised its four protocol examples. See the
[UI visual consistency verification](ui-visual-consistency.md) for computed
styles, color measurements and browser coverage.

## Tray Usage (macOS and Windows)

For every Agent and ledger adapter in the table below, the tray header refresh
re-reads the selected period and the activity heatmap through the same shared
renderer. The header shows a busy state and prevents duplicate clicks while
the refresh runs. The synthetic renderer regression checks both queries and
the busy state; native tray verification on macOS and Windows is pending.

For the GPT-6.2 pricing correction, each Agent row below retains its existing
entry point and adapters. The shared model catalog supplies the standard
list-price estimate to the gateway, existing session importers, and startup
backfill on both platforms. All rows display the result through the same
statistics and tray projections. The focused synthetic regression covers the
affected proxy and Codex session ledger sources, plus an unknown model and a
user-defined override. Native client and Windows verification remain pending.

For each Agent and native adapter in the matrix below, the tray heatmap uses
the same recorded ledger details and archived daily rollups. The calendar,
metric control, provider scope and keyboard navigation use the shared macOS
and Windows renderer. The synthetic fixture covers every Agent ID, the
first Monday of the 53-week window and exclusion of the preceding day.
The renderer fixture covers an empty selected period with recorded calendar
history, metric switching and keyboard navigation.

The heatmap patch over `bda7541b` passed 32 Chromium and WebKit cases across
320- and 440-point trays, both themes and all four runtime languages on
macOS 15.3.1 arm64. Type checking, formatting, the renderer build, Cargo
check and Clippy passed, using Node 24.11.0, pnpm 10.12.3 and Rust 1.98.1.
The full renderer suite passed all 4,701 tests with four Vitest workers.
The Rust suites passed 7,786 library and 165 integration tests, with 21
library tests ignored. Windows and native tray acceptance remain pending
for every Agent.

This read-only tab displays existing usage ledger records from every Agent ID.
The tray reads aggregated records without accessing client configuration.
All rows use the same entry point (tray → Usage), provider/model controls and
empty state when nothing has been recorded. Native self-tests are pending for every row.
The macOS synthetic SQLite regression covers the 20 Agent IDs below,
session deduplication, cache accounting, date boundaries and archived usage.
Chromium and WebKit renderer fixtures passed for 320- and 440-point panels in
all four runtime languages and both themes. Windows and native webview
verification remain pending.

The trend tooltip shares the same ledger projection and renderer across every
Agent row below. Hover and keyboard focus expose the selected bucket's provider
or model values and request count. On the same macOS host and base revision, the
tooltip follow-up passed 73 focused tests, all 4,697 renderer tests, TypeScript,
Prettier and the renderer build. Chromium and WebKit passed 32 synthetic tooltip
and drill-down cases across widths, themes and languages. Native webview
verification remains pending. The Rust build and test suites were not repeated
for this renderer-only follow-up.

For each Agent entry below, including the listed desktop, CLI and regional
variants, the provider label is resolved after its existing ledger adapter
records usage. The same macOS and Windows tray command feeds the picker,
ranking and trend labels. The label regression uses one routed ledger detail
for each Agent ID and also checks archived usage, saved and custom names,
disabled upstreams, malformed catalog metadata and unknown IDs. Native
self-tests remain pending for each Agent and platform.

The provider label patch over `bda7541b` passed all three tray ledger
regressions on macOS 15.3.1 arm64 with Rust 1.98.1. `cargo check`,
`cargo fmt --check` and `cargo clippy -- -D warnings` passed. The library
and full Cargo suites passed 7,786 library tests with 21 ignored and
165 integration tests. With Node 24.11.0 and pnpm 10.12.3, `pnpm typecheck`,
`pnpm format:check`, `pnpm build:renderer`, `pnpm audit --prod` and the
development helper checks passed. `pnpm test:unit` passed all 4,700 tests.
Windows and native Agent acceptance remain pending.

Verification used the local Usage patch over `bda7541b` on macOS 15.3.1 arm64
with Node 24.11.0, pnpm 10.12.3 and Rust 1.98.1. TypeScript, the renderer
build, Prettier, the production dependency audit, the helper tests, Cargo
check, Rust formatting and Clippy passed. The two focused Vitest files passed
70 tests. The full Vitest run passed 4,693 tests and failed the DeepSeek
multi-currency balance test in concurrent quota changes. The failure also
reproduced in the quota test file by itself. Cargo library and full test runs
passed 7,782 library tests with 21 ignored and 165 integration tests. Browser
checks used synthetic data. Platform CI and native Agent checks remain pending.

| Agent / entry variants     | Ledger adapter / limitation                             |
| -------------------------- | ------------------------------------------------------- |
| Claude Code                | Gateway and Claude session importer                     |
| Claude Desktop             | Recorded gateway calls; not a full desktop chat history |
| Codex CLI / Desktop        | Gateway and Codex session importer; shared identity     |
| Gemini CLI                 | Gateway and Gemini session importer                     |
| Grok Build                 | Gateway and Grok session importer                       |
| OpenCode                   | OpenCode session importer                               |
| Pi CLI                     | Pi session importer and recorded gateway calls          |
| OpenClaw                   | Ledger records only; no new native reader               |
| Hermes                     | Ledger records only; no new native reader               |
| WorkBuddy CN desktop / CLI | Recorded gateway calls                                  |
| WorkBuddy International    | Recorded gateway calls                                  |
| Qoder desktop              | Recorded gateway calls                                  |
| Doubao Work                | Recorded gateway calls                                  |
| TRAE CN                    | Recorded bridge calls                                   |
| TRAE International         | Recorded bridge calls                                   |
| Kimi CLI / desktop         | Recorded gateway calls                                  |
| Qwen CLI                   | Recorded gateway calls                                  |
| ZCode desktop              | Recorded gateway calls                                  |
| DeepSeek Harness           | Recorded gateway calls                                  |
| T3 Code                    | Ledger records only; no new native reader               |

This is a required implementation and acceptance ledger, not a declaration that every listed feature is supported. Follow the mandatory Agent coverage rules in [AGENTS.md](../../AGENTS.md). Derive the inventory from `src/config/appConfig.tsx`, `AppType` and the connection registries, including native integrations outside the shared `APPS` list.

For every Agent-facing change, assess all entries below against its entry point, protocol/native adapter, runtime behavior, UI state and supported platforms. Record implementation, synthetic regression and native acceptance separately. New Agents must be added here and assessed against existing cross-Agent features. Missing or unverified entries prevent an all-Agent completion claim unless the user explicitly accepts a narrower scope.

## Windows 0.4.0 bug-fix sweep (macOS and Windows)

This ledger covers the eight Windows tracker items 17–24. The installer,
taskbar icon and tray-panel language changes are application-wide; native
connection handlers, CLI installation and installation discovery were assessed
against the full `APP_IDS` / `AppType` inventory. macOS keeps the existing
status-bar icon family and Dock artwork; the new ICO only changes Windows
installer, taskbar and window icon density. The Windows CLI installation
subprocess now receives the same reconstructed PATH that detection already
uses. Native connection panics are contained at the Win32 callback boundary and
at the diagnostics command boundary, and DeepSeek profile writes validate model
entries before selecting one.

| Agent ID           | Entry points and applicable fixes                                                                 | Verification status                                                                 |
| ------------------ | ------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `claude`           | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `claude-desktop`   | Shared connection switch and detection; installer, taskbar and tray panel                          | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `codex`            | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `gemini`           | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `grokbuild`        | Grok CLI install path stays PowerShell-first; shared connection switch; installer and tray panel    | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `opencode`         | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `openclaw`         | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `hermes`           | PowerShell installer path unchanged; shared connection switch; installer, taskbar and tray panel    | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `pi`               | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `workbuddy`        | Windows CN registration aliases, LOCALAPPDATA root, CLI discovery, model control and connection     | Synthetic Windows fixture passed; native installed-client acceptance pending        |
| `workbuddy-ai`     | Windows international registration aliases, LOCALAPPDATA root, model control and connection        | Synthetic Windows fixture passed; native installed-client acceptance pending        |
| `qoder`            | Model-control connection switch; installer, taskbar and tray panel                                  | Synthetic regression passed; native installed-client acceptance pending             |
| `doubaowork`       | Audit-only connection switch; installer, taskbar and tray panel                                     | Synthetic regression passed; native installed-client acceptance pending             |
| `trae`             | TRAE China bridge connection switch; installer, taskbar and tray panel                              | Synthetic regression passed; native installed-client acceptance pending             |
| `trae-intl`        | TRAE International bridge connection switch; installer, taskbar and tray panel                     | Synthetic regression passed; native installed-client acceptance pending             |
| `kimi`             | CLI discovery; shared connection switch; installer, taskbar and tray panel                          | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `qwen`             | CLI install/update PATH; shared connection switch; installer, taskbar and tray panel               | Synthetic regression passed; native installer and live Agent acceptance pending     |
| `zcode`            | Desktop detection; shared connection switch; installer, taskbar and tray panel                      | Synthetic regression passed; native installed-client acceptance pending             |
| `deepseek-harness` | Validated profile writes; shared connection switch; installer, taskbar and tray panel              | Synthetic regression passed; native installed-client acceptance pending             |
| `t3code`           | Desktop detection; shared connection switch; installer, taskbar and tray panel                      | Synthetic regression passed; native installed-client acceptance pending             |

## WorkBuddy subscription upstreams

Both regional providers enter through Providers → Add → Subscription. The
shared editor binds a browser-authorized account and publishes its selected
models to the normal routing group catalog on macOS and Windows. Each row below
uses its existing setup and model selection UI. Chat completions and China
Claude Messages are the upstream protocols. The gateway adapts each consumer's
protocol and retains the existing routing and security decisions.

| Agent ID           | Agent                         | Entry and native adapter                                              | Native acceptance (macOS / Windows) |
| ------------------ | ----------------------------- | --------------------------------------------------------------------- | ----------------------------------- |
| `claude`           | Claude Code                   | Routing connection and Messages                                       | Pending / Pending                   |
| `claude-desktop`   | Claude Desktop                | Built-in gateway and Messages                                         | Pending / Pending                   |
| `codex`            | Codex CLI / Desktop           | Shared Codex catalog and Responses                                    | Pending / Pending                   |
| `gemini`           | Gemini CLI                    | Routing connection and Gemini native                                  | Pending / Pending                   |
| `grokbuild`        | Grok Build                    | Routing connection and Responses                                      | Pending / Pending                   |
| `opencode`         | OpenCode                      | Published gateway provider and Chat or native protocol                | Pending / Pending                   |
| `openclaw`         | OpenClaw                      | Published gateway provider and Chat or native protocol                | Pending / Pending                   |
| `hermes`           | Hermes                        | Published gateway provider and Chat                                   | Pending / Pending                   |
| `pi`               | Pi                            | Published routing provider and Pi wire adapter                        | Pending / Pending                   |
| `workbuddy`        | WorkBuddy China desktop / CLI | Native custom model catalog and Chat                                  | Pending / Pending                   |
| `workbuddy-ai`     | WorkBuddy AI desktop / CLI    | Native custom model catalog and Chat                                  | Pending / Pending                   |
| `qoder`            | Qoder desktop                 | Native connection and Qoder upstream projection                       | Pending / Pending                   |
| `doubaowork`       | Doubao Work                   | Audit-only native integration. No selectable routing group connection | Missing / Missing                   |
| `trae`             | TRAE China                    | Native custom model and Chat bridge                                   | Pending / Pending                   |
| `trae-intl`        | TRAE International            | Native custom model and Chat bridge                                   | Pending / Pending                   |
| `kimi`             | Kimi CLI / desktop            | Native connection and Chat                                            | Pending / Pending                   |
| `qwen`             | Qwen CLI                      | Native connection and Chat                                            | Pending / Pending                   |
| `zcode`            | ZCode desktop                 | Native connection and gateway protocol projection                     | Pending / Pending                   |
| `deepseek-harness` | DeepSeek Harness              | Native connection and Responses or Chat                               | Pending / Pending                   |
| `t3code`           | T3 Code                       | Published gateway provider and Responses                              | Pending / Pending                   |

The renderer fixture covers account binding and catalog discovery for each
edition. The HTTP fixture captures each edition's upstream path and checks
that structured tool calls retain their name and ID when rendered in Chat,
Messages, Responses, and Gemini. Native client verification and authenticated
vendor inference remain pending. Doubao Work lacks a native route to a global
supplier in the existing connection registry.

Local verification used the working tree based on `763ee981` on macOS 26.6.2
arm64 with Rust 1.98.1, Node 22.22.0, and pnpm 10.12.3. Type checking,
renderer build, renderer formatting, the 4,741-test renderer run, the
development helper tests, and the production dependency audit passed.
`cargo check` and `cargo clippy -- -D warnings` passed. The full Cargo run
passed 7,803 library and 165 integration tests, with 21 library tests ignored.
Rust formatting passed for the WorkBuddy patch files. The repository-wide
Rust format check reported diffs in concurrent routing-trace and protocol-probe
changes. Platform CI and native acceptance remain pending.

## Routing group creation templates

Pre-implementation assessment against `APP_IDS`, `AppType`, the shared Agent
registry and the separate native integrations. Creation, editing and tray state
are shared on macOS and Windows. The template builds a normal global group.
The native catalogs, wire protocols and continuation adapters feed the same
candidate planner where a global group is selectable. Routing must preserve
request parameters, replies, exact continuation ownership and per-Agent visibility.
The allowance observation is tied to the upstream credential and applies only
when a new destination can be selected.

The subscription allowance follow-up applies to each shared gateway entry
listed below, independently of the calling Agent. Codex official targets use
the frozen account's quota observations. Claude subscription targets use the
explicit managed account and model scope. Messages, Chat, Responses and Gemini
adapters must retain request and response semantics, authorization, and exact
continuation ownership. Doubao Work retains its separate audit entry. The
template help is shared by the macOS and Windows renderer.

The model order layout follow-up uses the same shared renderer for every entry
below. It changes the creation editor on macOS and Windows. Native catalogs,
Agent connections, candidate planning and protocol adapters retain their
existing behavior.

The automatic review model filter applies to the shared template editor for
every entry below on macOS and Windows. It affects default task and classifier
suggestions in new drafts. Agent protocols, manual selections and saved groups
keep their existing behavior.

The classifier timeout message uses the shared group editor for every entry
below on macOS and Windows. It displays the draft's deadline and recovery
steps. No native adapter or runtime classification behavior changes.

The gap between the classifier controls and trial uses the same shared
editor for every entry below on macOS and Windows. This spacing change
affects only the layout of those controls.

Recommended model ordering uses the shared editor for every entry below on
macOS and Windows. The capability template action reorders explicitly selected
members before save. It writes the existing default and fallback fields, so
the protocol adapters continue to consume the ordinary ordered plan.

The capability recommendation now uses model tiers and within-family versions
for both initial suggestions and reordered members. The same shared editor
applies to every entry below on macOS and Windows, and the saved plan uses
the existing member IDs, default and fallback fields.

The capability draft reapplies the recommendation after member selection
until the user moves a member manually. The same editor behavior applies
to each entry below on macOS and Windows, before the ordinary group is saved.

The shared template chooser prefers distinct model identities across suppliers
for every Agent in the matrix below on macOS and Windows. The smart template
editor wraps each rule description and moves the model picker underneath it
when the form is narrow. Every Agent continues to consume the saved provider
and model IDs through the listed entry point and protocol adapter. Doubao Work
uses its separate audit connection. Native verification is tracked separately.

This follow-up was checked on the working tree based on `ff49fdd8` using
macOS 26.6.2 arm64, Node 20.20.2, pnpm 10.12.3, and Rust 1.98.1. Chromium
reproduced clipping before the CSS change. Chromium and macOS WebKit passed
the final layout checks across six widths. Renderer regressions cover
all four templates, configured equivalents, reassignment for distinct models
and fallback with a sparse catalog. Type checking, the renderer build,
Prettier, 4,760 renderer tests, the development helpers and the production
dependency audit passed. Rust formatting and Cargo check passed. The full
Cargo run passed 7,801 library tests and 165 integration tests, with 21
library tests ignored. Strict Clippy reported a variant-naming warning on a
separate working-tree edit to the Rust `RoutingTemplate` enum. Windows
WebView and installed Agent verification remain pending.

The November 21 narrow-form follow-up places the template hint in the same
grid column as the rule editor. At a 520 px viewport the original layout
collapsed the rules column and wrapped the descriptions almost character by
character. On the working tree based on `d32c8a68`, Chromium and macOS WebKit
verified the full hard- and easy-task descriptions and usable model selectors
when creating or editing a smart group at viewport widths from 380 to 900 px.
The shared renderer applies this layout to every Agent listed below.
Windows WebView2 verification remains pending.

The classifier information button and expiry template ordering action use the
same group editor for each Agent in this matrix. The information button opens
the existing disclosure and returns keyboard focus when dismissed with Escape.
The expiry action reorders selected member IDs by the catalog-based template
ranking and saves the existing default and fallback fields. Each listed Agent
retains its regular request entry point, and live quota still ranks eligible
accounts. Renderer regressions verify opening and dismissing the explanation,
restoring a reordered draft, and saving its member order and quota policy.
Chromium and macOS WebKit verified the explanation in narrow dialog fixtures.
macOS WebKit also verified moving an expiry template row and restoring its
original position with the recommended-order button.
Windows WebView2 verification remains pending.

The three renderer fixes were checked on the working tree based on
`d32c8a68` using macOS 26.6.2 arm64, Node 20.20.2, pnpm 10.12.3, and
Rust 1.98.1. Type checking, the renderer build, Prettier, all 4,766 renderer
tests, development helpers and the production dependency audit passed.
Cargo formatting, check, strict Clippy, and the library and full test runs
passed. The full run passed 7,817 library tests and 165 integration tests,
with 21 library tests ignored.

Each new template chooser draws fresh model suggestions in the shared editor
on macOS and Windows. For each Agent in the matrix below, the editor previews
and saves the same draw through the ordinary routing-group entry point. The
existing adapters and runtime policies consume the saved models in their
template order. Doubao Work uses its separate audit connection. The renderer
regressions inject different draws to verify the four templates' constraints
and that opening a second draft saves the models previewed in its chooser.
The draw regressions ran on the working tree based on `ff49fdd8` with
macOS 26.6.2 arm64, Node 20.20.2 and pnpm 10.12.3. Type checking, the renderer
build, formatting, all 4,762 renderer tests, development helpers and the
production dependency audit passed. Rust 1.98.1 formatting and Cargo check
passed. `cargo test` passed 7,803 library tests and 165 integration tests,
with 21 library tests ignored. Strict Clippy reported the existing
`RoutingTemplate` variant-naming lint. Windows WebView and installed Agent
checks remain pending.

The shared editor hides Advanced settings while creating from the capability,
speed and expiry templates, for every entry below on macOS and Windows. The
task-classification template keeps its rules, classifier and trial visible;
Custom and saved group editors keep the remaining settings. No Agent entry
point, protocol adapter, or runtime behavior changes. Renderer checks cover
the template and regular editors; installed Agent tests do not exercise this
Vary AI editor change.

| Agent ID           | Entry point and adapter                                      | Runtime and UI                                                               | Verification                                     |
| ------------------ | ------------------------------------------------------------ | ---------------------------------------------------------------------------- | ------------------------------------------------ |
| `claude`           | Claude Code / Messages                                       | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `claude-desktop`   | Claude Desktop / Messages                                    | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `codex`            | Codex CLI and Desktop / Responses                            | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `gemini`           | Gemini CLI / Gemini                                          | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `grokbuild`        | Grok Build CLI / shared gateway                              | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `opencode`         | OpenCode / shared catalog and gateway                        | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `openclaw`         | OpenClaw / shared catalog and gateway                        | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `hermes`           | Hermes / shared catalog and gateway                          | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `pi`               | Pi custom provider / shared gateway                          | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `workbuddy`        | WorkBuddy China desktop / native bridge                      | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `workbuddy-ai`     | WorkBuddy international desktop / native bridge              | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `qoder`            | Qoder China desktop / native TLS and gateway                 | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `doubaowork`       | Doubao Work / separate native audit connection               | Audit connection only. No global model group projection in the native client | Native audit path outside group execution        |
| `trae`             | TRAE China / native Chat bridge                              | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `trae-intl`        | TRAE International / native Chat bridge                      | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `kimi`             | Kimi Code CLI and desktop / shared gateway                   | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `qwen`             | Qwen Code CLI / shared gateway                               | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `zcode`            | ZCode desktop / native adapter and shared gateway            | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `deepseek-harness` | DeepSeek Harness desktop / native adapter and shared gateway | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |
| `t3code`           | T3 Code desktop / managed Codex Responses                    | Shared group chooser and candidate planner                                   | Synthetic gateway passed; native testing pending |

The synthetic gateway matrix exercises Messages, Chat, Responses and Gemini
requests, including native continuations, exact provider and model selection,
request field preservation, revoked connections and malformed input. The
renderer covers the four templates, the Custom draft, model order, cancel/save,
expiry import/export and legacy groups. Validation uses fixture upstreams and
does not imply acceptance on any installed Agent or actual relay account.

The smart-template member fix applies to every row above through the shared
Vary AI editor on macOS and Windows. When a rule target changes, the ordinary
group save submits only the selected rule targets as task members. The same
save repairs oversized explicit smart groups. Each listed adapter continues
to consume the resulting group via its existing planner. Doubao Work remains
audit-only. Synthetic coverage exercises creation and saved-group edits at the
shared save boundary. Native Agent and Windows WebView checks are pending.

The November 21 follow-up covers the same Agent entries and adapters. A smart
draft with rules publishes only distinct rule targets, including when several
rules share a model. The save path applies this to explicit template groups
and legacy groups identified by their smart rules. Two renderer regressions
resave and reopen these groups with a hidden fallback and verify that the
editor shows the selected rule targets. The six focused renderer suites
passed all 199 tests on the working tree based on `d32c8a68` using macOS
26.6.2 arm64, Node 20.20.2 and pnpm 10.12.3. Type checking and targeted
formatting passed.

The routing stage uses the same renderer for all Agents in the matrix above.
It can explain an existing continuation or session affinity from the recorded
gateway step. A single candidate gives the number of destinations for the
request. The request entry points, protocol adapters and candidate planner
supply the existing trace fields. The six focused routing renderer suites
passed 300 tests, and TypeScript checking passed on macOS arm64 with Node
20.20.2 and pnpm 10.12.3 on the working tree based on `d32c8a68`. Native
WebView checks are pending.

- `workbuddy` and `workbuddy-ai` use separate authenticated Chat entries. The
  gateway keeps a surviving owner and replans a visible-text turn when the
  owner was removed. The existing native chat displays the response. Synthetic
  HTTP coverage includes both editions, retained and removed owners, streamed
  and buffered replies, and opaque history. All eight WorkBuddy history tests
  passed on macOS arm64 with Rust 1.98.1 on the same working tree. Native
  verification is pending on macOS and Windows.
- Each other Agent in the table uses its existing entry, adapter, continuation
  policy and chat UI. All ten shared continuation tests passed across Chat,
  Messages, Responses and Gemini policy paths.

The intent classifier uses the same model picker layout as the rule targets
for every row above on macOS and Windows. The shared editor still filters to
classifier-eligible providers and allows clearing the classifier. Agent
catalogs, protocol adapters and classifier execution are unchanged. The
renderer checks picker variant, search focus, scrolling, dismissal and
selection. Native WebView checks are pending.

Both changes were checked on the working tree based on `ff49fdd8` on
macOS 26.6.2 arm64 with Node 20.20.2, pnpm 10.12.3 and Rust 1.98.1.
`pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer` and
`pnpm test:unit` passed, including 223 focused tests and 4,763 tests in the
full renderer suite. The production audit and 150 helper tests passed.
Cargo check passed before concurrent WorkBuddy edits. Rust formatting
reported WorkBuddy formatting differences. The library suite reported a
Pi Gemini tool-event failure. Strict Clippy and the full Cargo suite were
blocked by WorkBuddy compile errors. Platform CI and native verification
are pending.

Model picker scrolling applies to every row in this matrix through the Vary AI
Create group editor on macOS and Windows. It covers smart rule targets, the smart
default target and classifier, and adding or replacing members in the other
three templates. Each row retains its listed entry point, catalog eligibility,
adapter and runtime behavior. Doubao Work retains its audit-only projection.
The synthetic renderer checks wheel delivery inside the picker, blocking
background scrolling, selecting a model, and Escape restoring focus without
closing the editor. Without the picker scroll-lock fix, the wheel regressions
fail for all four templates and the separate smart default-target case.
Native Vary AI WebView verification on macOS and Windows is pending for every row.

The model-picker follow-up was checked on the working tree based on
`763ee981` on macOS 26.6.2 arm64 with Node 20.20.2, pnpm 10.12.3, and
Rust 1.98.1. The focused renderer run passed 123 tests and the full renderer
run passed 4,756 tests. TypeScript, renderer build, Prettier, the production
dependency audit, and the CI helper checks passed. Cargo formatting, check,
and Clippy passed. Both the library-only and full Cargo suites passed with
7,805 library tests and 21 ignored; the full suite also passed 165
integration tests. A fresh Chromium run could not start its renderer under
the local sandbox, so this check did not provide new browser wheel results.
No platform CI ran for this working tree.

Local validation used `baf1c29e` plus the routing template working-tree changes
on macOS 26.6.2 arm64, Node 20.20.2, pnpm 10.12.3 and Rust 1.98.1.
`pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` and
`pnpm test:unit` passed (4,376 tests). The helper checks listed in Development
and `pnpm audit --prod` passed. `cargo fmt --check`, `cargo check`,
`cargo clippy -- -D warnings` and `cargo test --lib` passed against
`src-tauri/Cargo.toml` (7,749 passed and 21 ignored in the library suite).
The full `cargo test --manifest-path src-tauri/Cargo.toml` also passed, with
165 integration tests in addition to the library suite.

The subscription allowance follow-up was checked on `3226a979` with the UI
layout and allowance changes applied, using the same macOS host and toolchain.
The expanded gateway matrix passed for all four protocols, including
exhausted-account exclusion, relay expiry preference and durable continuation
after restart. Tests cover account-scoped pace, 5-hour-only confirmation,
missing or stale data, and Claude's model-specific weekly limits.
`pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`,
`pnpm test:unit` (4,376 passed), the Development helper checks and
`pnpm audit --prod` passed. Rust formatting, `cargo check`,
`cargo clippy -- -D warnings`, and the full Cargo test suite
(7,921 passed, 21 ignored) passed. Native subscription accounts and Windows
were not exercised.

The automatic review follow-up was checked on `3226a979` with the routing
template working-tree changes on the same macOS host, Node and pnpm versions.
The fixture reproduced the incorrect classifier suggestion before the fix.
`pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, and
`pnpm test:unit` (4,377 passed) passed with the fix. Live classifier requests
and native clients were not exercised for this follow-up.

The timeout guidance was checked on the same revision and host with the
routing template changes. `pnpm typecheck`, `pnpm build:renderer`,
`pnpm format:check` and `pnpm test:unit` (4,378 passed) passed. Seven classifier
tests from the local Rust test binary passed, including a rule match and a
timeout fallback against synthetic HTTP upstreams. Live classifier requests
and native clients were not exercised for this follow-up.

The spacing follow-up was checked on `3226a979` with the routing template
working-tree changes on the same macOS host, Node and pnpm versions. A
Chromium layout fixture using the built renderer CSS measured both gaps at
10 px with the panels collapsed or expanded, at 490 px and 820 px dialog
widths. `pnpm build:renderer`, `pnpm format:check` and 121 focused renderer
tests passed. Native window layout was not exercised.

Recommended ordering was checked on `3226a979` with the routing template
working-tree changes on the same macOS host, Node and pnpm versions. The
renderer regression reorders 50 selected members, excludes an unselected
model, and verifies the saved default and fallback order. A unit fixture
checks equal ranks and missing catalog entries. `pnpm typecheck`,
`pnpm build:renderer`, `pnpm format:check` and `pnpm test:unit` (4,380 passed)
passed. Native clients were not exercised for this editor change.

Capability ranking was checked on `3226a979` with the routing template
working-tree changes on the same macOS host, Node and pnpm versions. Tests
cover tier precedence, numeric version segments, and independent family
ordering. `pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check` and
`pnpm test:unit` (4,382 passed) passed. The macOS dev editor also reordered
13 existing catalog selections in an unsaved draft. Live routing and the
Windows native editor were not exercised for this change.

Selection ordering was verified on the merge of `origin/main` at `815b2286`
into `3226a979` with all routing template follow-ups applied. The 50-member
fixture covers removing and reselecting a model, preserving a manual order,
resuming recommendations and saving. On macOS 26.6.2 arm64 with Node 20.20.2,
pnpm 10.12.3 and Rust 1.98.1, `pnpm typecheck`, `pnpm build:renderer`,
`pnpm format:check`, and `pnpm test:unit` (4,578 passed) passed. The helper
tests listed in Development and `pnpm audit --prod` passed. `cargo fmt
--check`, `cargo check --locked`, `cargo clippy --locked -- -D warnings`,
and `cargo test --locked --no-fail-fast` passed against `src-tauri/Cargo.toml`.
The Rust build and test checks ran with debug information and incremental
compilation disabled. The locked desktop prevented a native editor recheck.

## Live routing source cards

The Routing workspace derives its source cards
from the existing gateway trace snapshot on macOS and Windows. `Trace` below
means one card per recorded Agent and active route, independent request
animations, and only destinations referenced by a valid gateway `attempt` step
in either layout. Candidates that were planned or skipped without an attempt have no
card or branch. Attempted fallbacks and concurrent routes retain their original
trace indices for packet motion. Each row uses its existing native or protocol
adapter. Concurrent requests to the same route share a source card. The
workspace uses the route alias to attribute each attempted upstream, including
when the same Agent has concurrent requests on different routes. Flows
outside the gateway trace recorder have no source card. The compact tray has
its own selection flow and already supports multiple live sources.

For every Agent and variant below, the workspace and compact tray share the
candidate card without a leading status dot. The exact seat pulses its fixed
border between a muted accent and a saturated violet-tinted accent while it
shows the responding state, without waiting for first content. The shared
stylesheet changes neither layout geometry nor packet motion. Reduced-motion
mode keeps a static accent border. Workspace
source names wrap rather than truncate. A parked packet is stationary at the
provider edge with only its outer half visible. The existing return and
failover motion, entry points, native adapters and trace semantics are unchanged
on macOS and Windows. Renderer checks cover both stage layouts;
native client and WebView acceptance remain pending.

On the macOS 15.3.1 arm64 working tree based on `8df4a222`, with Node
24.11.0 and Rust 1.98.1, Chromium verified the half-visible steady dot, the
border-only pulse, the return after completion, untruncated Agent names and
reduced motion. `tsc --noEmit`, `vite build`, Prettier and `vitest run` passed
(4,673 tests, including 204 focused checks). Cargo formatting, `cargo check`
and Clippy passed. The full Cargo suites were not rerun because recent runs
exhausted the disk during linking and this follow-up changes only the renderer.

The outward-outline follow-up on the same macOS working tree passed the
205 focused Vitest checks, TypeScript, the renderer build and targeted
Prettier. Chromium fixtures verified that the outline expands without
clipping or shifting the card in either layout, and that completed and
reduced-motion states are static. Full suites and native clients were not
rerun for the stylesheet-only follow-up.

The fixed-border follow-up over `ad551d25` on macOS arm64 passed 205
focused tests, TypeScript, the renderer build and targeted Prettier. It
does not change route selection, candidate retention or native adapters.
Native acceptance and full-suite verification were not repeated.

For the attempted-destination filter over `ad551d25`, the shared renderer
regression exercises each registered Agent ID in both the workspace and tray
layouts. It covers pre-dispatch hiding, a non-first attempt, response state,
and completed replay. Additional synthetic cases exercise skipped and invalid
attempt indices, fallback outcomes, concurrent paths, and packet return.
Native client and WebView acceptance on macOS and Windows remain pending.
On the `ad551d25` working tree with this renderer patch, macOS 15.3.1 arm64,
Node 24.11.0 and Rust 1.98.1, `pnpm typecheck`, `pnpm format:check`,
`pnpm build:renderer` and `pnpm test:unit` passed (4,678 tests, including 209
focused checks). Cargo formatting, `cargo check` and Clippy passed. The Rust
test suites were not rerun because prior linking exhausted available disk space
and this patch does not change Rust code.

The color-pulse stylesheet follow-up over the same `ad551d25` working tree
on macOS 15.3.1 arm64 with Node 24.11.0 passed 209 focused tests, TypeScript,
the renderer build, and Prettier. Headless Chromium fixtures verified distinct
border colors at the two animation stops, fixed card geometry, and still
completed and reduced-motion states in both layouts and themes. The full
suites and native client acceptance were not repeated for this follow-up.

All entries below share per-request rate measurements and both stage layouts.
The macOS and Windows paths use the same session readers and protocol observer.
Native title lookup requires an explicit local identity and an exact match.
Gemini derives its session list title from message content. It uses the same
short opaque reference as clients that lack a verified native title reader.
The reference appears when a route has a session digest. Rate availability
depends on each provider's usage events and stream timing.

| Agent ID           | Existing entry point and adapter   | Conversation display                                | Verification                    |
| ------------------ | ---------------------------------- | --------------------------------------------------- | ------------------------------- |
| `claude`           | Claude Code CLI                    | Custom title from exact session match               | Renderer passed, native pending |
| `claude-desktop`   | Claude Desktop connection          | Short reference                                     | Renderer passed, native pending |
| `codex`            | Codex CLI and desktop connection   | Thread title from the state DB or index             | Renderer passed, native pending |
| `gemini`           | Gemini CLI                         | Short reference                                     | Renderer passed, native pending |
| `grokbuild`        | Grok Build CLI                     | Generated title from exact session match            | Renderer passed, native pending |
| `opencode`         | OpenCode connection                | Stored title from exact session match               | Renderer passed, native pending |
| `openclaw`         | OpenClaw connection                | Display name when explicit session metadata matches | Renderer passed, native pending |
| `hermes`           | Hermes CLI                         | Stored title from exact session match               | Renderer passed, native pending |
| `pi`               | Pi CLI provider connection         | Explicit session name from exact session match      | Renderer passed, native pending |
| `workbuddy`        | WorkBuddy China desktop and CLI    | Short reference                                     | Renderer passed, native pending |
| `workbuddy-ai`     | WorkBuddy AI international desktop | Short reference                                     | Renderer passed, native pending |
| `qoder`            | Qoder CN desktop                   | Short reference                                     | Renderer passed, native pending |
| `doubaowork`       | Dedicated Doubao Work connection   | Short reference                                     | Renderer passed, native pending |
| `trae`             | TRAE China connection              | Short reference                                     | Renderer passed, native pending |
| `trae-intl`        | TRAE International connection      | Short reference                                     | Renderer passed, native pending |
| `kimi`             | Kimi Code CLI and desktop          | Short reference                                     | Renderer passed, native pending |
| `qwen`             | Qwen Code CLI                      | Short reference                                     | Renderer passed, native pending |
| `zcode`            | ZCode desktop                      | Short reference                                     | Renderer passed, native pending |
| `deepseek-harness` | DeepSeek Harness desktop           | Short reference                                     | Renderer passed, native pending |
| `t3code`           | T3 Code desktop                    | Short reference                                     | Renderer passed, native pending |

The conversation and rate changes were checked on the working tree over
`fc0fe3c3`, using macOS 15.3.1 arm64, Node 24.11.0, pnpm 10.12.3 and Rust
1.98.1. TypeScript, the renderer build, Prettier and all 4,689 frontend unit
tests passed. The 88 routing card tests passed again after strengthening the
concurrent response assertion. Cargo formatting, `cargo check`, `cargo check
--tests` and Clippy passed. The Rust test executables could not be built because
the filesystem ran out of space. Native client checks on macOS and Windows
remain pending.

## Routing conversion parity (2026-11-18)

The [conversion coverage matrix](2026-11-18-routing-conversion-parity.md)
tracks Responses allowed-tool restrictions, classified conversion errors and
the unchanged native adapters for every supported Agent. Shared protocol
regressions and native acceptance are recorded separately. Native acceptance
on macOS and Windows remains pending.

## Local connection recovery (2026-11-18)

| Agents                                                                        | Credential lifecycle on macOS and Windows                                                                                                     | Verification                                          |
| ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| Claude Desktop                                                                | Reapply the shared profile with its current credential, including supported sealed values. An ownership conflict requires a fresh connection. | Local HTTP regression, native acceptance pending      |
| Claude Code, Pi, Gemini, Grok Build, OpenCode, OpenClaw, Hermes, Qwen         | Reapply the journaled CLI configuration through the ordinary catalog refresh.                                                                 | Shared adapter regressions, native acceptance pending |
| WorkBuddy CN and International, Qoder, Kimi, ZCode, DeepSeek Harness, T3 Code | The same refresh uses each native adapter and its existing credential store.                                                                  | Shared adapter regressions, native acceptance pending |
| TRAE CN and International, Codex CLI and Desktop, Doubao Work                 | Dedicated bridge retry, Codex reconnection and audit launch use their own commands.                                                           | Outside the shared receipt rebuild path               |

The per-Agent inventory below also covers the main and tray connection
controls on macOS and Windows. After a successful connection write, Claude
Desktop, WorkBuddy CN/International, Qoder, Kimi, ZCode, Harness, T3 Code,
TRAE CN/International and Codex Desktop use native freshness to offer a
confirmed restart. Claude Code, Gemini, Grok Build, OpenCode, OpenClaw,
Hermes, Pi, Qwen and CLI variants retain session guidance. Doubao Work uses
its dedicated audit launcher. Shared Agent ingress paths include local
recovery instructions on credential rejection. Codex and Doubao have
dedicated ingress paths. Native verification of the new UI is pending.

On the macOS arm64 working tree based on `8df4a222`, the Desktop HTTP
regression accepted the pre-reapply bearer token on the model catalog endpoint.
The shared adapter matrix retained each existing credential and detected a
changed route before reuse. The fresh connection after disconnect had a
different token. Typecheck, renderer build, 4,617 frontend tests, Prettier,
Rust formatting, Cargo check, Clippy and the full Cargo test command passed.
The Rust library suite passed 7,771 tests with 21 ignored. Windows native
acceptance and remote CI remain pending.

## Figma workspace design preview (2026-11-18)

The workspace, Agent rows and existing shared controls receive the semantic
light and dark palette. Provider list recovery applies to the shared provider
registry. The matrix uses `Shared` for the existing Agent row, including its
current availability, connection, error and disabled states. UI styling is
shared by macOS and Windows. The connection, projection and protocol adapters
use their existing entry points.

The desktop Agent directory follow-up moves each installed or recoverable Agent
into a responsive card with its existing model and connection controls. Each
row of this matrix also covers the Installed and Not installed tabs, name and
ID search, and grid ordering on macOS and Windows. Both tabs use the existing
presence results. The Installed tab keeps model and connection controls. The
Not installed tab exposes the existing install and official website actions.
The tray retains its compact entry point. Verification covers synthetic card
states, tab selection by keyboard, search in either tab, detection refresh,
horizontal and keyboard sorting, and persisted order. Native Agent and
Windows WebView2 acceptance are pending.

The List and Cards selector applies to every Agent in this matrix on macOS
and Windows. Both views use the same installed controls and installation
actions. The preview checks both tabs, the saved view, ordering and search,
and keeps the tray on its existing layout.

The List view polish applies to every installed Agent below at the shared
desktop entry point on macOS and Windows. It removes the icon backplate and
repeated control captions, and reveals the reorder handle on hover or focus,
with a visible handle on devices without hover. The model control or official
model note, connection switch, and platform adapters use their existing paths.
The reorder handle uses the leading gutter before the icon for every
sortable desktop Agent row on macOS and Windows.

| Agent ID           | Entry point and adapter                               | Runtime and UI scope                            |
| ------------------ | ----------------------------------------------------- | ----------------------------------------------- |
| `claude`           | Shared connection, Claude Code CLI                    | Shared                                          |
| `claude-desktop`   | Shared connection, Claude Desktop                     | Shared                                          |
| `codex`            | Shared connection, Codex CLI and desktop              | Shared                                          |
| `gemini`           | Shared connection, Gemini CLI                         | Shared                                          |
| `grokbuild`        | Shared connection, Grok Build CLI                     | Shared                                          |
| `opencode`         | Shared connection, OpenCode                           | Shared                                          |
| `openclaw`         | Shared connection, OpenClaw                           | Shared                                          |
| `hermes`           | Shared connection, Hermes CLI                         | Shared                                          |
| `pi`               | Shared connection, Pi CLI                             | Shared                                          |
| `workbuddy`        | Shared connection, WorkBuddy China desktop and CLI    | Shared                                          |
| `workbuddy-ai`     | Shared connection, WorkBuddy AI international desktop | Shared                                          |
| `qoder`            | Shared connection, Qoder CN desktop                   | Shared                                          |
| `doubaowork`       | Dedicated Doubao Work connection and launch           | Shared styling with existing dedicated controls |
| `trae`             | Dedicated TRAE CN connection                          | Shared styling with existing dedicated controls |
| `trae-intl`        | Dedicated TRAE International connection               | Shared styling with existing dedicated controls |
| `kimi`             | Shared connection, Kimi Code CLI and desktop          | Shared                                          |
| `qwen`             | Shared connection, Qwen Code CLI                      | Shared                                          |
| `zcode`            | Shared connection, ZCode desktop                      | Shared                                          |
| `deepseek-harness` | Shared connection, DeepSeek Harness desktop           | Shared                                          |
| `t3code`           | Shared connection, T3 Code desktop                    | Shared                                          |

All 20 rows rendered with synthetic connection fixtures in Chromium on macOS.
The preview exercised the light theme at 1160 × 720 and the dark theme at
900 × 650 with Windows renderer platform detection. Both runs verified the
shared provider search, editor and keyboard focus return. Native Agent and
Windows WebView2 verification remains pending for every row.

Local verification covered the `codex/figma-design-preview` working tree based
on `8df4a222` on macOS 15.3.1 arm64 with Node 24.11.0, pnpm 10.12.3 and
Rust 1.98.1.

- `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`,
  `pnpm audit --prod` and `pnpm test:unit` passed. The unit suite passed
  4,573 tests.
- The CI merge-policy test and the development, DMG, TRAE, model-default, Pi
  session, WorkBuddy, Doubao Work, Agent rewards and Qoder helper suites passed.
- `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test --lib` and `cargo test` passed against `src-tauri/Cargo.toml`.
  The full run passed 7,934 tests with 21 ignored by the standard command.
- Remote CI and native cross-platform verification remain pending.

### Agent directory preview verification

The desktop directory uses Figma's raised selected tab on a shared muted
track in both themes, with equal-width tabs inset from the track edges. Each
tab shows its installation count. The Not installed
tab uses the same presence checks and install consent flow as the existing
catalog. A tool detected after a refresh or a successful CLI install appears
in the Installed tab.

Chromium on macOS verified all 20 tools across the two tabs using synthetic
responses in both themes. It exercised one-, two-, and three-column layouts,
tab selection by keyboard, search and focus recovery, installation refresh,
and pointer and keyboard card ordering. Native Agent and Windows WebView2
verification are pending.

Verification used the `codex/figma-design-preview` working tree over
`8df4a222` on macOS 15.3.1 arm64 with Node 24.11.0, pnpm 10.12.3, and
Rust 1.98.1. `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`,
`pnpm audit --prod`, `pnpm test:unit` (4,576 tests), the CI policy and helper
suites, and diff whitespace checks passed. With the CI debug profile
settings, Rust formatting, check, Clippy, library tests and the full suite
passed (7,934 total passed and 21 ignored). The final CSS-only spacing
refinement passed renderer formatting, build and both browser previews.

### List view polish verification

The layout regression exercises the model and connection controls and the
reorder handles for all 20 Agent IDs. Chromium on macOS verified the List
and Cards views in both themes with synthetic data. It checked transparent
icon backplates, the absence of repeated control captions in List, and
reorder handle visibility at rest, on hover, on keyboard focus, during the
drag preview and with touch emulation. Native Agent and Windows WebView2
verification remain pending.

The List and Cards changes were checked in the `codex/figma-design-preview`
working tree over `8df4a222` on macOS 15.3.1 arm64 with Node 24.11.0, pnpm
10.12.3, and Rust 1.98.1. After the List polish, `pnpm typecheck`,
`pnpm format:check`, `pnpm build:renderer`, and `pnpm test:unit` passed
(4,579 tests). The Rust checks for the layout changes passed with the
CI debug profile settings. The library test run passed 7,769 tests and the
full run passed 7,934, with 21 ignored in each run.

### Leading reorder handle

Chromium on macOS verified the leading handle for all 20 installed Agent rows
with synthetic responses in both themes. The checks covered hover, keyboard
reordering, drag preview positioning, narrow windows and emulated touch.
Native Agent and Windows WebView2 verification remain pending.

On macOS 15.3.1 arm64, the `codex/figma-design-preview` working tree over
`8df4a222` passed `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`
and 411 focused Agent tests with Node 24.11.0 and pnpm 10.12.3. The frontend
suite passed 4,612 tests and reported three failures in routing and tray
tests. A focused rerun passed the routing tests and reproduced the tray
failure. Rust 1.98.1 formatting, check and Clippy passed. Disk space blocked
compilation of the Rust library tests and the subsequent full Rust test run.

### Agent directory scrolling

Every Agent in the matrix uses the same desktop directory on macOS and Windows.
The workspace keeps the title, installation check, update notices, tabs,
search, and footer outside the scrollable area. The Installed tab scrolls its
rows and the Not installed tab scrolls its installation actions. Each panel
also contains its own empty states. The tray uses its compact entry point.
Runtime detection, connection state, and protocol and native adapters follow
the matrix above.

Chromium on macOS exercised both installation states for all 20 Agent IDs
with synthetic responses across a light 1160 × 720 viewport and a dark
900 × 600 viewport with Windows renderer detection. Wheel scrolling,
PageDown, tab navigation, empty search results, and reorder auto-scroll kept
the header controls and footer in place. Native WebKit and Windows WebView2
verification are pending.

The `codex/figma-design-preview` working tree based on `8df4a222` passed
`pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, and 511 focused
frontend tests on macOS 15.3.1 arm64 with Node 24.11.0 and pnpm 10.12.3.
The full frontend run passed 4,613 tests and reported eight failures. On a
focused rerun, the tray, provider, and Windows build-parser tests passed.
The coverage check still reported duplicate Agent IDs in the live routing
lanes section.

Rust 1.98.1 formatting, check, and Clippy passed on macOS. The library and
full Cargo test commands were stopped when available disk space fell below
400 MiB.

### Workspace surface and hover refinement

The shared palette and control hover styles apply to all 20 Agent IDs in
the matrix through the existing desktop UI on macOS and Windows. The shared
styles cover the Agent tabs, model controls and sort handles in both views.
The tray uses the same palette. Connection state, installation discovery,
and native and protocol adapters continue through the existing paths.

Chromium on macOS rendered the 20 Agent rows and checked navigation hover,
stable routing table headers, matching sticky and non-sticky cell colors,
keyboard focus, editor entry from a summary, and reduced motion in both
themes. Native Agent and Windows WebView2 verification remain pending.

On macOS 15.3.1 arm64, the `codex/figma-design-preview` working tree over
`8df4a222` passed TypeScript checking, renderer build, formatting, and
the full frontend suite with Node 24.11.0 and pnpm 10.12.3. Vitest ran
4,593 tests using the runner config loader, thread workers, and caching
disabled after the standard invocation ran out of disk space.
Rust 1.98.1 formatting, check, and Clippy passed. Disk space blocked a
fresh compilation of the Rust test binaries. The Rust source tree matched
`8df4a222`, and running the existing library and integration test binaries
in this checkout yielded 7,934 passed and 21 ignored.

### Card alignment

Each of the 20 Agents in the matrix above uses the shared Cards layout at
the desktop directory entry point on macOS and Windows. Installed cards
separate the name, version metadata, connection status, and secondary
actions. Model and connection controls align at the bottom of each grid
row. Installation cards anchor the install and website actions together.
The directory retains the existing installation, update, restart, rewards,
connection, and native adapter paths.

Component regressions exercise all 20 installed Agents, both connection
toggle states, restart and rewards actions, and desktop version order
with macOS and Windows renderer detection. The existing directory tests
exercise installation cards for all 20 Agents. Chromium layout fixtures
verified alignment for installed and installation cards in both themes
at 290, 600, 900, and 1200 CSS pixels. Native Agent and Windows WebView2
verification are pending.

Verification used the working tree over `8df4a222` on macOS 15.3.1
with Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1. The 12 focused
component tests, TypeScript, targeted Prettier, renderer build,
repository-wide renderer formatting, Rust check and Rust formatting
passed. `pnpm test:unit` passed all 325 test files. Disk space blocked
Rust Clippy and both Rust test commands.

The card outline and density follow-up applies to every Agent in the same
matrix. In the macOS and Windows desktop directory, installed and
installation cards use transparent surfaces with complete borders.
Installed cards also remove the icon backplate and internal divider.
The shared renderer continues to use the existing version, status,
rewards, restart, model and connection controls for each Agent.

The working tree over `8df4a222` was checked on macOS 15.3.1
with Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1. The 17 focused
directory, border and coverage tests passed. Chromium fixtures verified
the complete outlines, transparent backgrounds, aligned controls and
list separator at 280, 560, 900 and 1200 CSS pixels in both themes.
TypeScript, renderer build, renderer Prettier and Rust formatting passed.
Disk space interrupted the full unit suite and blocked Rust check,
Clippy, and both Rust test runs. Native WebView and Agent acceptance
remain pending.

The Cards and List transition uses the same desktop directory entry
point for every Agent in the matrix on macOS and Windows. Both installation
tabs animate explicit layout changes and respect reduced-motion preferences.
Cards use the themed surface with a white light theme. Installed cards
remove the repeated model and connection captions. The accessible controls,
runtime state, and native adapters follow their existing paths.

The Cards and List switch was verified in Chromium with synthetic native
data on macOS 15.3.1. Both layout directions, reduced motion, caption
removal and themed card surfaces passed. The workspace frontend suite
passed 4,596 tests on the checked snapshot over `8df4a222`. After the
responsive status edits, 20 focused tests, TypeScript, renderer build,
and targeted formatting passed. Rust check, formatting and Clippy
passed. Disk space blocked the Rust test builds.
Native WebView and Agent acceptance remain pending.

### Restart placement in the installed list

The 20 Agent rows in the directory matrix use the shared renderer on macOS
and Windows. Claude Desktop, Codex, WorkBuddy, WorkBuddy AI, Qoder, TRAE CN,
TRAE International, Kimi, ZCode, DeepSeek Harness, and T3 Code show Restart
immediately after the tool name when their runtime state calls for it.
Doubao Work uses its dedicated launch action. The other eight entries
do not offer desktop restart. The compact tray renders Restart in its
inline actions. Restart requires confirmation and dispatches through the
native adapter for the selected Agent.

The component regression covers placement and visibility for all 20 rows
with synthetic states under both platform detections. Chromium at a
720-pixel viewport verified Claude Desktop title alignment and confirmation
cancellation in both themes. Native restart and WebView acceptance are
pending.

The working tree over `8df4a222` was checked on macOS 15.3.1 arm64 with
Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1. TypeScript, renderer build,
Prettier, production dependency audit, and frontend helper checks passed.
The installed Qoder codec helper case was skipped. The final renderer
suite passed 4,590 tests using Vitest thread workers with caching disabled.
Rust formatting, check, Clippy, and the library tests passed. The library
suite passed 7,769 tests with 21 ignored.

### List-only desktop directory

All 20 entries in the Agent matrix above use a single-column directory on
macOS and Windows. The Installed and Not installed tabs use the shared layout.
Previously saved layout preferences have no effect. Installed rows expose the
existing status, model, restart, rewards, and connection controls. Installation
rows retain the actions supplied by each Agent's existing installation adapter.
The tray uses its compact directory.

Regression coverage uses both platform detections and exercises all 20 matrix
entries in both tabs, including existing saved order, keyboard and pointer
sorting, search, and existing layout preferences. The standalone macOS
WKWebView fixture verified both lists and their row separators at 350, 820,
and 1180 CSS pixels. Windows WebView2 and live Agent verification are pending.

Verification used the working tree over `8df4a222` on macOS 15.3.1 arm64 with
Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1. TypeScript, renderer build,
renderer formatting, production dependency audit, and CI helper suites passed.
The frontend unit run passed 4,590 tests, and the focused Agent and tray
suites passed 555 tests after the final test edits. Rust formatting, check,
and Clippy passed.

### Directory search empty state

Every Agent row in the matrix above uses the shared desktop directory search
on macOS and Windows. In either installation tab and either layout, a query
with no matches in a populated tab shows an illustration, localized guidance,
and a clear action that returns focus to search. The coverage uses the existing
presence, connection, installation, and protocol adapters.

Synthetic regressions cover all 20 `APP_IDS` entries in each tab and layout
under macOS and Windows renderer detection. Chromium on macOS verified the
illustration, localized guidance, layout and focus recovery in both themes
at 1160 × 720 and 900 × 650. Native WebView and Agent verification remain
pending for every row.

The new asset is
[`agent-search-empty.png`](../../src/assets/illustrations/agent-search-empty.png).
The built-in ImageGen prompt called for a small, transparent desktop search
illustration with a pale slate-blue directory tray, two stepped index tabs,
and a cobalt magnifying glass. Rounded shapes and restrained shading keep it
legible on both themes at 152 CSS pixels.

Verification used the working tree over `8df4a222` on macOS 15.3.1 arm64
with Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1. `pnpm typecheck`,
`pnpm build:renderer`, `pnpm audit --prod`, the frontend CI helper suites,
targeted Prettier, and `git diff --check` passed. The Agent directory
component suite passed all 397 tests.
Repository-wide `pnpm format:check` flagged routing and design-token files.
The full `pnpm test:unit` rerun reported failures in the routing suites and
exited before producing totals. `cargo fmt --check`, `cargo check`,
`cargo clippy -- -D warnings`, and `cargo test --lib` passed against
`src-tauri/Cargo.toml`, with 7,769 library tests passed and 21 ignored.
Disk space interrupted `cargo test` and prevented the integration-only retry
from starting. Remote CI was not run.

The button-removal follow-up applies to every Agent row above on macOS and
Windows, in both installation tabs and layouts. The illustration and guidance
appear without an action in the empty state. The search input still clears
the filter and restores the list. Entry points, native adapters, and runtime
behavior use the existing paths.

On macOS 15.3.1 arm64 with Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1,
the working tree over `8df4a222` passed all 16 focused tests. Chromium
verified both themes, both tabs and layouts, the search input's clear
control, and the provider list's clear action with synthetic data.
Type checking, renderer build, formatting, Rust formatting, and
`cargo check` passed. The full frontend run passed 4,588 tests and failed
five outside search recovery. Low disk space interrupted Clippy and left
Rust library and integration tests pending. Native WebView verification
and remote CI remain pending.

### Provider list search empty state

The provider list shares the illustration and layout with every Agent
directory row in the matrix. When a populated list has no search results,
the empty state suggests trying a different provider name, endpoint host,
or model ID. Clearing the search restores the list and returns focus to
the search field. Both Agent tabs and layouts use the shared component.

On the same host and toolchain, the Agent and provider component suites
passed all 490 tests against the follow-up working tree. Chromium verified
both themes and focus recovery using synthetic provider records.
`pnpm typecheck`, `pnpm build:renderer`, `pnpm format:check`, `pnpm audit --prod`,
the frontend CI helper suites, and `git diff --check` passed. The full suite
passed 4,593 tests with `pnpm test:unit --maxWorkers=4 --pool=threads`.
`cargo fmt --check`, `cargo check`, and `cargo clippy -- -D warnings`
passed. Disk space prevented the Rust library and integration test builds.
Native WebView verification and remote CI remain pending.

The shared search empty state shows the illustration and guidance without a
button on the provider list and on both Agent tabs and layouts. The search
field clears the filter and restores results for every Agent in the matrix
on macOS and Windows. This presentation uses the existing shared directory
entry points and adapters.

On macOS 15.3.1 arm64 with Node 24.11.0, pnpm 10.12.3, and Rust 1.98.1,
the working tree over `8df4a222` passed 16 focused tests and 4,594 tests
in the full frontend suite. Chromium verified the provider search input's
clear control in both themes using synthetic data. Type checking, renderer
build, formatting, production dependency audit, Rust formatting, `cargo check`,
and Clippy passed. Insufficient disk space blocked the Rust library and
integration test builds. Native WebView verification and remote CI remain
pending.

## Windows installation discovery (2026-11-18)

Assessment started before implementation against `APP_IDS`, `AppType` and the
backend connection registry. Entry points are `get_agent_installations`,
`get_agent_versions`, Tools / Agent and Rescan installations. Discovery must
remain read-only and must never start a client. Protocol, native configuration,
security policy and forwarding behavior are unaffected. macOS bundle discovery
and Linux CLI discovery must retain their existing behavior.

| Agent ID           | Entry point and native discovery adapter                   | Runtime / UI assessment                                              | Verification                                                                       |
| ------------------ | ---------------------------------------------------------- | -------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `claude`           | Shared presence; Windows CLI candidates and effective PATH | Retain CLI presence independently of version execution               | Existing CLI regressions; native unverified                                        |
| `claude-desktop`   | Shared presence/version; Windows desktop adapter           | Missing Windows discovery; must show installed desktop               | Synthetic adapter and native read-only discovery passed; client execution untested |
| `codex`            | Shared presence/version; CLI and Windows desktop adapters  | Desktop-only installs must not depend on a CLI                       | Synthetic adapter and native read-only discovery passed; client execution untested |
| `gemini`           | Shared presence; Windows CLI candidates and effective PATH | Retain CLI presence                                                  | Existing CLI regressions; native unverified                                        |
| `grokbuild`        | Shared presence; `grok` CLI alias                          | Retain alias and native installer paths                              | Existing CLI regressions; native unverified                                        |
| `opencode`         | Shared presence; CLI and installer paths                   | Currently detected on this Windows host                              | Existing CLI presence evidence; native execution untested                          |
| `openclaw`         | Shared presence; Windows CLI candidates and effective PATH | Retain CLI presence                                                  | Existing CLI regressions; native unverified                                        |
| `hermes`           | Shared presence; Windows CLI/Python candidates             | Retain CLI presence                                                  | Existing CLI regressions; native unverified                                        |
| `pi`               | Shared presence; Windows CLI candidates and effective PATH | Retain CLI presence                                                  | Existing CLI regressions; native unverified                                        |
| `workbuddy`        | Shared presence/version; China desktop and CLI             | Missing Windows desktop discovery; keep editions separate            | Synthetic adapter and native read-only discovery passed; client execution untested |
| `workbuddy-ai`     | Shared presence/version; international desktop             | Missing Windows desktop discovery; keep editions separate            | Synthetic adapter passed on Windows; native unverified                             |
| `qoder`            | Shared presence/version; China desktop                     | Missing Windows desktop discovery; reject international product      | Synthetic adapter passed on Windows; native unverified                             |
| `doubaowork`       | Shared presence/version; desktop                           | Missing Windows desktop discovery                                    | Synthetic adapter passed on Windows; native unverified                             |
| `trae`             | Shared presence/version; China desktop                     | Installed on D:; fixed default roots miss custom installation        | Synthetic adapter passed; native read-only discovery passed                        |
| `trae-intl`        | Shared presence/version; international desktop             | Fixed default roots miss custom installation; keep editions separate | Synthetic adapter passed on Windows; native unverified                             |
| `kimi`             | Shared presence/version; desktop and CLI                   | Missing Windows desktop discovery; retain CLI fallback               | Synthetic adapter passed on Windows; native unverified                             |
| `qwen`             | Shared presence; Windows CLI candidates and effective PATH | Retain CLI presence                                                  | Existing CLI regressions; native unverified                                        |
| `zcode`            | Shared presence/version; desktop                           | Missing Windows desktop discovery                                    | Synthetic adapter passed on Windows; native unverified                             |
| `deepseek-harness` | Shared presence/version; desktop                           | Missing Windows desktop discovery                                    | Synthetic adapter passed on Windows; native unverified                             |
| `t3code`           | Shared presence/version; stable/alpha/beta desktop         | Fixed default roots miss custom installation                         | Synthetic adapter passed on Windows; native unverified                             |

Implementation, comparison sources and check results are recorded in
[the Windows discovery report](2026-11-18-windows-agent-detection.md).

## Qoder reward-date refresh (2026-11-17)

Pre-implementation assessment: Qoder's persisted claim expires at 09:59 UTC+8,
but the date adapter requires an exact 10:00 end. Keep the exact 10:00 start
and accept an end in the following boundary's last minute. Do not derive a
date from a claim timestamp or an arbitrary campaign. The shared helper covers
native and browser Qoder accounts on every existing supported platform; this
change does not add native credential support. Verification is recorded in
[the follow-up report](2026-11-17-qoder-checkin-dot.md).

| Agent ID           | Entry point / adapter                               | Runtime and UI impact                                                                                     | Verification / native acceptance                                                                                          |
| ------------------ | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `claude-desktop`   | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `codex`            | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `gemini`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `grokbuild`        | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `opencode`         | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `openclaw`         | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `hermes`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `pi`               | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `workbuddy`        | Separate check-in API and official date history     | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `workbuddy-ai`     | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `qoder`            | AgentRewardsPanel; native/browser qoderCampaigns    | Manual/status/automatic claims retain the confirmed reward date; current dot updates from returned status | Helper and renderer regressions passed; macOS native account currently protected; Windows/Linux native acceptance pending |
| `doubaowork`       | Separate native check-in adapter                    | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `trae`             | Shared reward panel; separate native bridge adapter | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `trae-intl`        | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `kimi`             | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `qwen`             | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `zcode`            | Shared reward panel; separate plan adapter          | Unaffected                                                                                                | Existing regressions; no native retest                                                                                    |
| `deepseek-harness` | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |
| `t3code`           | No reward panel or Qoder adapter                    | Unaffected                                                                                                | Inventory assessed; no native retest                                                                                      |

## Agent status-line restart feedback and update verification recovery

Updated before implementation on 2026-11-18 after the user identified the exact
Agent status line in a screenshot. Place restart feedback below its Agent name,
replacing that row's current status for eight seconds. Use a neutral capsule
and retain the phase-specific wording. Remove the footer host. Keep feedback
independent per Agent and preserve it during navigation within the same query
session. Distinguish a refused quit, a still-running app, and failure to reopen
after exit. Preserve identity and configuration conflict
checks and native process handling. Present update results awaiting verification
without failure colouring, retry stale version observations, and allow read-only
rechecking without rerunning an installer. Main/tray entry points and all four
runtime languages are included. Native acceptance is separate from synthetic tests.

The directory spacing follow-up applies to every Agent and variant below on
macOS and Windows. In the full workspace list, status text sizes to its content
so an available reconnect or restart action sits beside it. Long text can wrap
within the space available beside the action. All rows keep their existing
state, action availability, protocol and native adapter. The compact tray keeps
its existing layout. The CSS regression covers the shared full-list layout.

Spacing verification used the working tree based on `8df4a222` on macOS
15.3.1 arm64 with Node 24.11.0 and Rust 1.98.1. Chromium measured a 6px
gap in wide and narrow fixtures and verified that long text wraps. TypeScript,
the Vite build, Prettier, and all 4,608 Vitest tests passed. `cargo fmt --check`,
`cargo check`, and Clippy with `-D warnings` passed. `cargo test --lib`
encountered sandbox socket restrictions. Its retry and the full `cargo test`
run stopped with insufficient disk space. Native WebView acceptance remains
pending on macOS and Windows.

| Agent ID           | Entry point and adapter                                                                                                                         | Runtime and UI                                                                      | Verification                                                                                    |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `claude`           | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `claude-desktop`   | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `codex`            | macOS/Windows native restart phase mapping; CLI version observer                                                                                | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `gemini`           | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `grokbuild`        | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `opencode`         | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `openclaw`         | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `hermes`           | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `pi`               | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `workbuddy`        | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `workbuddy-ai`     | macOS/Windows native restart phase mapping; desktop version observation only (no update feed)                                                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `qoder`            | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `doubaowork`       | Dedicated audit launcher on macOS/Windows; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                    | The launcher retains its own guidance. The row has no generic restart button.       | Launcher and active connection UI covered by synthetic tests; native unverified for this change |
| `trae`             | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `trae-intl`        | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `kimi`             | macOS/Windows native restart phase mapping; CLI version-only, macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `qwen`             | Existing session/service adapter; no desktop restart; CLI version observer                                                                      | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `zcode`            | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `deepseek-harness` | macOS/Windows native restart phase mapping; macOS desktop updater; Windows/Linux automatic desktop updates remain unsupported                   | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |
| `t3code`           | Existing adapter; no desktop restart; desktop version observation only (no update feed)                                                         | Per-Agent status-line capsule and update-verification controls; protocols unchanged | Status-line synthetic passed; native unverified                                                 |

See [status-line feedback verification](2026-11-18-agent-status-line-feedback.md) for the latest placement checks; [feedback and update verification](2026-11-18-agent-feedback-and-updates.md) records the unchanged native and update paths.

## Desktop restart guidance presentation

Historical presentation, superseded by the footer capsules above at the user's request.

Planned before implementation on 2026-11-18. Only the presentation of the
restart results (`restartBlocked`, `restartFailed`, `restartUnsupported`) changes: show the Agent
name, suggest checking its window for a confirmation, and explain how to quit
and reopen it. Use a neutral information icon, polite status and information
toast in the main and compact/tray views. Failures of other operations remain error alerts.
All four runtime languages are updated. Native process handling, configuration,
protocol adapters, requests and responses are unchanged on every platform.

| Agent ID           | Entry point and adapter                                                | Runtime and UI                                                                                                                           | Verification                                                                                                       |
| ------------------ | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `claude`           | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `claude-desktop`   | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `codex`            | Existing Codex desktop/CLI and app-server adapters                     | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained; CLI/app-server behavior retained | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `gemini`           | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `grokbuild`        | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `opencode`         | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `openclaw`         | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `hermes`           | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `pi`               | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `workbuddy`        | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `workbuddy-ai`     | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `qoder`            | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `doubaowork`       | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `trae`             | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `trae-intl`        | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `kimi`             | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `qwen`             | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |
| `zcode`            | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `deepseek-harness` | Existing desktop restart/native adapter                                | Neutral manual-restart guidance; existing macOS/Windows runtime and Linux unsupported adapter retained                                   | Synthetic passed: macOS/Windows, main/compact, connected/restored, failed/unsupported/malformed; native unverified |
| `t3code`           | Existing CLI/session or service adapter; desktop restart not supported | Existing entry points, session guidance and error styling retained                                                                       | Inventory and existing component suite passed; native unverified                                                   |

See [restart guidance verification](2026-11-18-restart-guidance.md) for results
and native acceptance limits.

## Application log export and operation diagnostics

On 2026-11-18 the user explicitly narrowed diagnostic export to the current
`varyai.log` only. This replaces the earlier all-Agent ZIP plan. Settings exports
a complete sanitized snapshot of this one file; native Agent logs, configurations,
crash files, rotations and databases are outside the accepted scope.

Before implementation, the following matrix records shared operation
diagnostics and verification scope. Each row uses Settings for export and the
existing connection controls for operation logs. New fixed operation events record
start, success/failure, cancellation, elapsed time and an allowlisted reason;
model values, native error text, credentials and request content are excluded.
No protocol conversion or native configuration behavior is changed. Shared Rust
logic applies on macOS, Windows and Linux where the existing adapter is supported.

| Agent ID           | Entry point and adapter                                 | Runtime and UI                                 | Verification                                                        |
| ------------------ | ------------------------------------------------------- | ---------------------------------------------- | ------------------------------------------------------------------- |
| `claude`           | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `claude-desktop`   | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `codex`            | Codex CLI/desktop connection adapter; existing controls | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `gemini`           | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `grokbuild`        | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `opencode`         | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `openclaw`         | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `hermes`           | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `pi`               | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `workbuddy`        | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `workbuddy-ai`     | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `qoder`            | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `doubaowork`       | Separate Doubao takeover adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `trae`             | Separate TRAE edition bridge; existing controls         | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `trae-intl`        | Separate TRAE edition bridge; existing controls         | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `kimi`             | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `qwen`             | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `zcode`            | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `deepseek-harness` | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |
| `t3code`           | Shared connection/native adapter; existing controls     | Operation events; current application log only | Shared lifecycle event regression passed (macOS); native unverified |

Application export uses the actual device log directory, a blocking worker and a
private same-directory temporary destination. Publishing happens only after a
complete sanitized snapshot is written. Missing logs, unsafe destinations and
unavailable privacy processing produce fixed errors. The command name changes to
`export_application_log` so a hot-reloaded UI cannot accidentally invoke the old
all-Agent ZIP handler. Native save-dialog acceptance and Windows/Linux execution
remain unverified until separately tested.

Synthetic lifecycle tests cover all 20 frontend/backend Agent IDs, connection on/off
actions, malformed Agent IDs, fixed error categories and preservation of returned
success/error values. They verify the shared event boundary rather than actual
native client execution. Export fixtures use temporary files only. See the
[application log verification report](2026-11-18-application-log-export.md) for
checked source scope, host checks and remaining acceptance gaps.

## Responses history after model switching

Assessment started on 2026-11-17 before implementation against frontend `APP_IDS`,
backend `AppType`, shared `agent_connections::APPS`, and separate TRAE/Doubao
integrations. Same-conversation model changes must preserve visible messages and
tool pairs while preventing nonportable reasoning IDs from breaking Responses
replay. Official Codex normalization and named reasoning-item refusal recovery
are shared across macOS, Windows and Linux. Native applications, accounts,
configuration and UI controls are not changed. Security admission, final history
validation, restoration, account binding and response semantics remain mandatory.
The matrix distinguishes applicable Responses transports from protocols without
Responses item references; no native acceptance is implied by shared routing.

| Agent ID           | Entry point and adapter                                      | Runtime and UI state                                                            | Verification                                                                                     |
| ------------------ | ------------------------------------------------------------ | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `claude`           | Messages CLI                                                 | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `claude-desktop`   | Messages desktop                                             | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `codex`            | Native and managed Responses; CLI and desktop                | Official filtering and named-item recovery; existing picker                     | Native/managed HTTP and reversible restoration passed (macOS); CLI/desktop self-tests unverified |
| `gemini`           | Gemini CLI                                                   | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `grokbuild`        | Provider-selected CLI protocol                               | Responses refusal recovery where this adapter uses Responses; existing picker   | Shared Responses HTTP passed (macOS); native unverified                                          |
| `opencode`         | Provider-selected Responses/Chat/Messages                    | Responses refusal recovery where this adapter uses Responses; existing picker   | Shared Responses HTTP passed (macOS); native unverified                                          |
| `openclaw`         | Provider-selected Responses/Chat/Messages                    | Responses refusal recovery where this adapter uses Responses; existing picker   | Shared Responses HTTP passed (macOS); native unverified                                          |
| `hermes`           | Chat CLI                                                     | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `pi`               | Provider-selected protocol; CLI                              | Responses refusal recovery where this adapter uses Responses; existing picker   | Shared Responses HTTP passed (macOS); native unverified                                          |
| `workbuddy`        | Chat/native; China desktop                                   | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `workbuddy-ai`     | Chat/native; international desktop                           | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `qoder`            | Native custom Responses and shared connection; desktop       | Shared named-item recovery; saved-key authentication unchanged; existing picker | Shared/native-adapter HTTP passed (macOS); native unverified                                     |
| `doubaowork`       | Separate native desktop integration                          | No Responses routing adapter; native protocol/UI unchanged                      | Not applicable to named Responses replay; native unverified                                      |
| `trae`             | Native Chat/Responses; China edition                         | Native/shared Responses recovery; existing picker                               | Shared/native-adapter HTTP passed (macOS); native unverified                                     |
| `trae-intl`        | Native Chat; international edition                           | No Responses item-reference replay on native wire; preserve existing adapter/UI | Shared Responses HTTP passed (macOS); native unverified                                          |
| `kimi`             | Native provider Responses and shared connection; CLI/desktop | Shared named-item recovery; native projection/UI unchanged                      | Shared/native-adapter HTTP passed (macOS); native unverified                                     |
| `qwen`             | Native provider Responses/Chat and shared connection; CLI    | Shared named-item recovery; native `/v1` suffix and UI preserved                | Shared/native-adapter HTTP passed (macOS); native unverified                                     |
| `zcode`            | Native provider Responses and shared connection; desktop     | Shared named-item recovery; native projection/UI unchanged                      | Shared/native-adapter HTTP passed (macOS); native unverified                                     |
| `deepseek-harness` | Native/shared provider-selected Responses/Chat/Messages      | Native/shared Responses recovery; existing picker                               | Shared/native-adapter HTTP passed (macOS); native unverified                                     |
| `t3code`           | Responses through managed Codex; desktop                     | Responses refusal recovery where this adapter uses Responses; existing picker   | Shared Responses HTTP passed (macOS); native unverified                                          |

The independent native HTTP send adapter is also covered: `kimi`, `qwen`,
`zcode`, `deepseek-harness`, TRAE China and Qoder custom Responses routes use the
same bounded recovery. Native Chat/Messages/Gemini requests do not acquire
Responses deletion behavior. Existing limitations on reversible restoration in
transparent native adapters are unchanged; native on/off checks use their
supported inspection/redaction policy rather than claiming restoration support.
Codex native and shared managed Responses separately test reversible restoration.

Synthetic regression verification passed on macOS for the final owned proxy
source recorded in `/tmp/varyai-responses-replay-fix-20261117/owned-source-final.json`.
All supported native wire protocols retain their existing UI and policy behavior;
the full shared inventory additionally exercises named/unnamed and JSON/SSE
Responses contracts. Native self-tests and Windows/Linux/remote CI remain
unverified. The exact commands, final check results, initial failures and
concurrent-checkout limitations are recorded in the
[replay verification report](2026-11-17-codex-reasoning-replay.md). This does not
claim all-Agent native acceptance.

## Tray security event localization

Assessment started on 2026-11-17 before implementation against frontend
`APP_IDS`, backend `AppType`, shared `agent_connections::APPS` and the separate
TRAE/Doubao Work integrations. The entry point is the tray Security tab's
existing event query. Reuse the security center's result and risk labels;
show warning/uncertain events in amber, blocked events in red, known successful
outcomes in green, and unknown/unchecked outcomes in neutral gray. The shared
renderer and CSS apply to macOS, Windows and Linux. Native/protocol adapters,
forwarding, collection, persistence, session correlation and notifications are
unchanged; this display fix does not establish native tool-call acceptance.

Each Agent below receives synthetic event-display checks with protection enabled
and disabled. Four real language bundles cover DLP actions (including reversible
redaction), tool verdicts, exact risk levels, and unsupported historical values.
Queries and returned event records must remain unchanged. Native client self-tests
and Windows/Linux runtime acceptance remain unverified.

| Agent ID           | Entry point                 | Protocol/native adapters and runtime                 | UI state                                                 | Verification                              |
| ------------------ | --------------------------- | ---------------------------------------------------- | -------------------------------------------------------- | ----------------------------------------- |
| `claude`           | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `claude-desktop`   | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `codex`            | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `gemini`           | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `grokbuild`        | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `opencode`         | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `openclaw`         | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `hermes`           | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `pi`               | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `workbuddy`        | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `workbuddy-ai`     | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `qoder`            | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `doubaowork`       | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `trae`             | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `trae-intl`        | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `kimi`             | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `qwen`             | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `zcode`            | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `deepseek-harness` | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |
| `t3code`           | Tray Security event history | Existing adapters unchanged; render returned records | Localized result/risk; outcome colors; protection on/off | Synthetic passed (macOS); native untested |

Verification on 2026-11-17 uses the working-tree changes to `TrayPanel.tsx`,
`tray-panel.css`, `tests/components/TrayPanel.test.tsx` and this ledger over
revision `eb24a0e3b947ba89bd9aa668148e23b0e9bbe59c`. Host: macOS 15.3.1 arm64;
Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1. Rust checks use
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
`CARGO_INCREMENTAL=0`; no application was launched or installed.

- Test-first regression: the unchanged renderer failed all 49 new display
  cases; the fix passed the focused tray, coverage-ledger and locale suites.
- Passed: `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`,
  `pnpm test:unit` (322 files, 4,388 tests), `pnpm audit --prod`,
  `cargo fmt --check --manifest-path src-tauri/Cargo.toml`,
  `cargo check --manifest-path src-tauri/Cargo.toml`,
  `cargo test --lib --manifest-path src-tauri/Cargo.toml` (7,712 passed,
  21 ignored), and
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
- Passed helper checks: `pnpm test:dev`,
  `node --test scripts/tests/ci-policy.node.mjs`, `pnpm test:macos-dmg`,
  `pnpm test:trae-bridge`, `pnpm test:model-default`, `pnpm test:pi-session`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`, and
  `python3 -B scripts/tests/recover-shared-v23.test.py`.
- Initial `cargo test --manifest-path src-tauri/Cargo.toml` failed one existing
  test: `commands::tool_risk::tests::event_query_omitted_and_null_verdict_preserve_all_event_pagination`.
  At `commands/tool_risk.rs:462`, repeated reads returned different sanitized
  command summaries (`public summary` versus `<redacted>`). Its standalone
  exact-filter rerun passed. The assertion and Rust implementation were unchanged.
- Full rerun passed: `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`
  (7,712 library tests, all integration and doc tests; 21 library tests ignored).
  The initial intermittent failure remains recorded rather than counted as a pass.
- Verification boundary: after the Rust test executable was built, concurrent
  work added `proxy/intent/native_replay_tests.rs` and its module registration.
  Those unrelated changes are preserved and are not covered by the Rust results
  above. The coverage-ledger inventory check passed again after the documentation
  update; the complete concurrently changing working tree is not accepted here.
- Unverified: native client self-tests, native tray visual acceptance,
  Windows/Linux runtime execution and remote CI. This renderer fix does not
  establish all-Agent native security acceptance.

## Statistics maintenance panel removal

The statistics dashboard removes the cost-pricing accordion and Codex usage
maintenance accordion, including the rebuild confirmation dialog. This is a
shared renderer change on macOS, Windows and Linux. Automatic session scanning,
manual synchronization when scanning is disabled, recorded costs and backend
pricing/rebuild APIs retain their existing behavior. Inventory was checked against
`src/config/appConfig.tsx` and `services/agent_connections.rs` before implementation.

Each row uses the statistics dashboard entry point. Existing protocol/native
adapters are unchanged; this UI removal does not invoke an adapter or require
native client execution. Synthetic verification passed on macOS for each Agent with
automatic scanning enabled and disabled, absent maintenance controls, visible
request logs, manual-sync visibility and the scan toggle callback. Native client
self-tests and Windows/Linux execution are not exercised by this change and are
not claimed as accepted.

| Agent ID           | Entry point                       | Protocol/native adapters and runtime                      | UI state                                                    | Verification                              |
| ------------------ | --------------------------------- | --------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------- |
| `claude`           | Statistics; CLI                   | Unchanged Messages adapter and accounting                 | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `claude-desktop`   | Statistics; desktop               | Unchanged desktop Messages adapter and accounting         | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `codex`            | Statistics; CLI/desktop           | Unchanged Responses, rollout accounting and rebuild API   | Remove both panels and rebuild dialog; retain scan controls | Synthetic passed (macOS); native untested |
| `gemini`           | Statistics; CLI                   | Unchanged Gemini adapter and accounting                   | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `grokbuild`        | Statistics; CLI                   | Unchanged provider-selected adapter and accounting        | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `opencode`         | Statistics; CLI                   | Unchanged provider-selected adapter and accounting        | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `openclaw`         | Statistics                        | Unchanged provider-selected adapter and accounting        | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `hermes`           | Statistics; CLI                   | Unchanged Chat adapter and accounting                     | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `pi`               | Statistics; CLI                   | Unchanged provider-selected adapter and accounting        | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `workbuddy`        | Statistics; China desktop         | Unchanged Chat/native adapter and accounting              | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `workbuddy-ai`     | Statistics; international desktop | Unchanged Chat/native adapter and accounting              | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `qoder`            | Statistics; China desktop         | Unchanged Chat/native adapter and accounting              | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `doubaowork`       | Statistics; desktop               | Unchanged separate native audit/launch integration        | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `trae`             | Statistics; China edition         | Unchanged native Chat bridge and accounting               | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `trae-intl`        | Statistics; international edition | Unchanged native Chat bridge and accounting               | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `kimi`             | Statistics; CLI/desktop           | Unchanged native provider projection and accounting       | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `qwen`             | Statistics; CLI                   | Unchanged Chat adapter and accounting                     | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `zcode`            | Statistics; desktop               | Unchanged native protocol projection and accounting       | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `deepseek-harness` | Statistics                        | Unchanged Chat/Responses/Messages adapters and accounting | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |
| `t3code`           | Statistics; desktop               | Unchanged Responses/managed Codex adapter and accounting  | Remove both panels; retain scan controls                    | Synthetic passed (macOS); native untested |

Verification on 2026-11-17 used the working-tree changes to `UsageDashboard.tsx`,
`UsageDashboard.test.tsx`, this ledger and `docs/usage.md` over revision
`8207f3efd7dc5eaf28ec191f7d9b2eafde6f704c`. Host: macOS 15.3.1 arm64;
Node 24.11.0, pnpm 10.12.3, Rust/Cargo 1.98.1.

- Passed: `pnpm typecheck`, `pnpm format:check`, `pnpm build:renderer`,
  `pnpm test:unit` (322 files, 4,339 tests), `pnpm audit --prod`,
  `cargo check --manifest-path src-tauri/Cargo.toml` and
  `cargo fmt --check --manifest-path src-tauri/Cargo.toml`.
- Passed helper checks: `pnpm test:dev`, `pnpm test:macos-dmg`,
  `pnpm test:trae-bridge`, `pnpm test:model-default`, `pnpm test:pi-session`,
  `pnpm test:workbuddy-checkin-auth`, `pnpm test:doubao-checkin`,
  `pnpm test:agent-rewards`, `pnpm test:qoder-codec`,
  `node --test scripts/tests/ci-policy.node.mjs` and
  `python3 -B scripts/tests/recover-shared-v23.test.py`.
- Incomplete: `cargo test --lib --manifest-path src-tauri/Cargo.toml`,
  `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and
  `cargo test --manifest-path src-tauri/Cargo.toml` failed during compilation or
  linking with disk-full errors. Failed compilation objects were cleaned up.
  No native client self-tests or remote CI runs were performed; Windows/Linux
  verification remains untested. No application was launched or installed.

## Catalog refresh and continuation ownership

Assessment started on 2026-11-17 before implementation. The implemented fix
excludes only `modelCatalog` from a forwarding destination's continuation identity. Authentication,
actual destination changes, ambiguous owners and opaque-state checks remain in
force. Existing model pickers and connection controls remain the entry points;
no new UI action or native configuration format is required. Shared Rust behavior
applies to macOS, Windows and Linux wherever the existing Agent adapter is
supported. Native application acceptance is separate from synthetic HTTP tests.

| Agent ID           | Entry point and protocol/native adapter                        | Pre-fix runtime risk and current UI                                                  | Verification                                                 |
| ------------------ | -------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| `claude`           | Shared connection; Messages; CLI                               | Catalog refresh can invalidate strict ownership; existing picker                     | HTTP refresh/restart passed; native unverified               |
| `claude-desktop`   | Shared connection; Messages; desktop aliases                   | Observed local 409 after catalog sync; existing picker                               | HTTP refresh/restart passed; native macOS/Windows unverified |
| `codex`            | Managed and native Responses; CLI/desktop                      | Replay avoids some 409s, but catalog changes can lose affinity                       | HTTP refresh/restart passed; native unverified               |
| `gemini`           | Shared connection; Gemini CLI                                  | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `pi`               | Shared connection; provider-selected protocols; CLI            | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `workbuddy`        | Shared connection; Chat; China desktop                         | Visible text recovery exists; tool/opaque history remains exposed                    | HTTP refresh/restart passed; native macOS/Windows unverified |
| `workbuddy-ai`     | Shared connection; Chat; international desktop                 | Same strict tool/opaque history as China edition                                     | HTTP refresh/restart passed; native macOS/Windows unverified |
| `qoder`            | Shared connection; Chat; China desktop                         | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native macOS/Windows unverified |
| `kimi`             | Shared connection; native provider projection; CLI/desktop     | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `qwen`             | Shared connection; Chat; CLI                                   | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `opencode`         | Shared connection; provider-selected protocols; CLI            | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `grokbuild`        | Shared connection; provider-selected protocols; CLI            | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `openclaw`         | Shared connection; provider-selected protocols                 | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `hermes`           | Shared connection; Chat; CLI                                   | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `zcode`            | Shared connection; native protocol projection; desktop         | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `deepseek-harness` | Shared connection; Chat/Responses/Messages; native credentials | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `t3code`           | Shared connection; Responses; managed Codex instance           | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native unverified               |
| `trae`             | Native bridge to shared Chat; China edition                    | Observed call/payload ownership still depends on target identity                     | HTTP refresh/restart passed; native macOS/Windows unverified |
| `trae-intl`        | Native bridge to shared Chat; international edition            | Strict continuation ownership uses shared target identity                            | HTTP refresh/restart passed; native macOS/Windows unverified |
| `doubaowork`       | Separate native audit/launch adapter                           | No shared model-catalog forwarding destination; not affected by this identity change | Source assessment; native unverified                         |

Implementation and verification details are recorded in
[the continuation refresh report](2026-11-17-catalog-continuation.md).

## Desktop update exit feedback

Assessment started on 2026-11-17 before implementation. An update that waits for
its target application to quit is deferred, with a neutral hint on that Agent's
version row. It does not produce a global failure banner or count as a successful
update. Existing updater eligibility, native replacement/rollback and shared
update guards remain in effect. macOS exit checks must observe kernel process
exit rather than cached AppKit termination metadata. Windows automatic desktop
replacement remains unsupported; shared feedback applies wherever an updater
returns the recognized deferred outcome.

| Agent ID           | Entry point and protocol/native adapter                                 | Runtime and UI assessment                                                                                         | Verification                                                                               |
| ------------------ | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `claude`           | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `claude-desktop`   | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `codex`            | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `gemini`           | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `pi`               | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `workbuddy`        | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `workbuddy-ai`     | Existing version row and native/regional adapter                        | No existing automatic desktop update path; retain unsupported/absent behavior on every platform                   | Source assessment and shared update regressions passed; native not rechecked               |
| `qoder`            | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `kimi`             | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `qwen`             | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `opencode`         | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `grokbuild`        | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `openclaw`         | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `hermes`           | Tools version row; existing CLI/native adapter                          | Preserve CLI update eligibility and genuine error handling on supported macOS/Windows/Linux paths                 | Source assessment and shared update regressions passed; native not rechecked               |
| `zcode`            | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `deepseek-harness` | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `t3code`           | Existing version row and native/regional adapter                        | No existing automatic desktop update path; retain unsupported/absent behavior on every platform                   | Source assessment and shared update regressions passed; native not rechecked               |
| `trae`             | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `trae-intl`        | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |
| `doubaowork`       | Tools version row; existing macOS desktop updater and publisher adapter | Implemented deferred outcome and local neutral feedback; kernel exit detection; Windows/Linux updater unsupported | 88 frontend and 13 backend scoped tests passed on macOS; native install/quit not exercised |

See [the update feedback report](2026-11-17-desktop-update-feedback.md).

## Desktop startup probe compatibility

Assessment started on 2026-11-17 before implementation. The exact authenticated
Claude Desktop startup probe must be isolated from saved conversation ownership
and may retry once after an explicit upstream requirement to enable thinking. Actual upstream success remains required. Ordinary requests,
credential failures and security decisions retain their existing behavior. The
adapter runs in shared Rust on macOS and Windows; native acceptance is separate.

| Agent ID           | Entry point and protocol/native adapter                                                  | Runtime and UI assessment                                                                                                             | Verification                                                                                                                                                                                                                                            |
| ------------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `claude-desktop`   | Authenticated native startup probe; Messages to Messages/Chat/Responses/Gemini           | Implemented isolated request-local probe state and bounded compatibility retry; native gateway banner clears only on upstream success | 22 shared-ingress/recovery regressions passed on macOS, including stale ownership, four upstream protocols and both policy states; installed old backend returned 409; updated macOS startup check accepted with banner removed; native Windows pending |
| `codex`            | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `gemini`           | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `pi`               | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `workbuddy`        | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `workbuddy-ai`     | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `qoder`            | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `kimi`             | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `qwen`             | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `opencode`         | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `grokbuild`        | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `openclaw`         | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `hermes`           | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `zcode`            | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `deepseek-harness` | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `t3code`           | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `trae`             | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `trae-intl`        | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |
| `doubaowork`       | Existing shared/native connection; CLI/desktop and regional variant as inventoried above | No Desktop startup probe marker; preserve request/response and UI behavior on supported platforms                                     | Source assessment and shared-ingress isolation regressions passed on macOS; native paths not rechecked                                                                                                                                                  |

See [the startup probe report](2026-11-17-desktop-startup-probe.md).

## Tool-call risk warnings: current gaps

The full feature remains incomplete. Gateway connectivity and DLP text inspection do not establish tool-call observation. Registered native shell semantics require structured tool calls and a matching enabled risk policy. Warnings and records observe proposed calls; they do not block local execution.

| Agent ID           | Agent                   | Implementation status                                                                                                                          | Native acceptance evidence                                                                                                                                                                                                                                        |
| ------------------ | ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | Claude Code             | Registered `Bash` schema and shell semantics                                                                                                   | Not rechecked in this follow-up                                                                                                                                                                                                                                   |
| `claude-desktop`   | Claude Desktop          | Registered `Bash` schema and shell semantics                                                                                                   | Not rechecked in this follow-up                                                                                                                                                                                                                                   |
| `codex`            | Codex                   | Registered `exec_command` and `shell_command`                                                                                                  | Not rechecked in this follow-up; keep the user's route off                                                                                                                                                                                                        |
| `gemini`           | Gemini CLI              | Registered `run_shell_command`                                                                                                                 | Not rechecked in this follow-up                                                                                                                                                                                                                                   |
| `pi`               | Pi                      | Registered `bash`; native session helper delivered                                                                                             | User confirmed warning/session ID and `pi_` display acceptance                                                                                                                                                                                                    |
| `workbuddy`        | WorkBuddy China         | Registered `Bash`                                                                                                                              | A native risk event with a conversation ID was observed                                                                                                                                                                                                           |
| `workbuddy-ai`     | WorkBuddy International | Registered `Bash`                                                                                                                              | A native risk event with a conversation ID was observed                                                                                                                                                                                                           |
| `qoder`            | Qoder CN                | Registered `Bash`                                                                                                                              | Native retest remains pending                                                                                                                                                                                                                                     |
| `kimi`             | Kimi Code               | Registered native `Shell`; schema, policy and shared-routing regressions passed                                                                | User reported no warning; native retest pending                                                                                                                                                                                                                   |
| `qwen`             | Qwen Code               | Registered native `run_shell_command`; schema, policy and shared-routing regressions passed                                                    | User reported no warning; native retest pending                                                                                                                                                                                                                   |
| `opencode`         | OpenCode                | Registered native `shell` and legacy `bash` string-command schemas on shared routes                                                            | User confirmed native tool-warning and session-ID acceptance passed on 2026-11-17; nested code-mode calls and other platforms remain unverified                                                                                                                   |
| `grokbuild`        | Grok Build              | Registered `run_terminal_command`, internal compatibility alias `run_terminal_cmd` and Cursor preset `Shell`                                   | User confirmed native tool-risk acceptance passed on 2026-11-17; other platforms and presets remain unverified                                                                                                                                                    |
| `openclaw`         | OpenClaw                | Registered native `exec` string-command schema on shared routes                                                                                | User requested skipping OpenClaw on 2026-11-17; native acceptance remains unverified; nested code-mode calls remain unverified                                                                                                                                    |
| `hermes`           | Hermes                  | Registered native `terminal` string-command schema on shared routes                                                                            | User confirmed native tool-warning and session-ID acceptance passed on 2026-11-17; scheduled/delegated scope and other platforms remain unverified                                                                                                                |
| `zcode`            | ZCode                   | Registered native `Bash` string-command schema on shared routes                                                                                | User confirmed the instructed native tool-risk acceptance passed on 2026-11-17; automated regressions remain unexecuted                                                                                                                                           |
| `deepseek-harness` | DeepSeek Harness        | Registered native `bash`; added a native request-header extension and validated session-header extraction                                      | User confirmed native warning and session-ID acceptance passed on 2026-11-17; PowerShell/interactive variants remain unverified                                                                                                                                   |
| `t3code`           | T3 Code                 | Registered `exec_command`/`cmd` and `shell_command`/`command` for the managed Codex driver, including Responses routing                        | User confirmed completion of native tool-warning testing on 2026-11-17; the session-header and prefix repairs are implemented, but final session-ID appearance was not separately confirmed; regressions remain unexecuted and other T3 drivers remain unverified |
| `trae`             | TRAE CN                 | Native `PreToolUse` / `RunCommand` observer with edition-bound authentication, supplied session identity and asynchronous risk persistence     | User confirmed native warning delivery after enabling Configured Hooks on 2026-11-17; `trae_cn_` session-prefix correction is implemented, awaiting new-record retest                                                                                             |
| `trae-intl`        | TRAE International      | Separate native `PreToolUse` / `RunCommand` observer; existing user hooks and permissions preserved                                            | User reported routing_unknown_continuation / 409 after a tool call on 2026-11-17; edition-parity continuation repair is implemented, awaiting new-build retest; native warning delivery remains unverified                                                        |
| `doubaowork`       | Doubao Work             | Legacy `Bash` lifecycle retained; current-runtime shell report collector added with `doubaowork_` session identity and truthful reported stage | **Implemented; new-build native notification acceptance pending.** Previous native failure reproduced; actual report schema confirmed on macOS. Windows native acceptance pending.                                                                                |

Shared Rust implementation applies across platforms, but native Windows/macOS/Linux acceptance must be recorded for each applicable path. A passed macOS fixture is not Windows acceptance. Do not infer session availability where a client supplies no recognized stable identity, and never substitute request IDs or generated per-request IDs.

Required verification for each missing adapter includes a structured native schema, JSON/SSE transport as applicable, enabled and disabled policy, malformed/duplicate/namespaced definitions, bounded collection, preserved forwarding, client identity, session correlation when supplied, persisted event and notification delivery. Native collectors must distinguish a proposed call from executed output and preserve existing security decisions and user configuration.

Kimi's native `Shell` command input is defined by its [official shell implementation](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/tools/shell/__init__.py). Qwen's `run_shell_command` identity and string command schema are defined in its [official tool names](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-names.ts) and [shell implementation](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/shell.ts). These contracts establish the bounded adapters; they do not prove native acceptance on the user's installed versions.

## Current follow-up evidence and limits

The seven new shared-route adapters use native string-command definitions rather than matching tool-name text in chat messages. Their schema, catalog-boundary, client-policy and shared-routing regressions were expanded, including the managed T3 Responses path. The installed ZCode `Bash` definition, DeepSeek Harness `dsh-tool-bash` packages, T3 Codex projector, OpenClaw `execSchema`, Hermes terminal schema, OpenCode shell declaration and Grok Build embedded tool/preset documentation establish these contracts. [OpenCode tools](https://opencode.ai/docs/tools/) and the [Hermes terminal implementation](https://github.com/NousResearch/hermes-agent/blob/main/tools/terminal_tool.py) provide primary references. This does not establish coverage for arbitrary JavaScript code-mode tools, PowerShell-specific syntax, interactive terminal input or other unregistered tools.

TRAE uses its [official pre-execution hook contract](https://docs.trae.cn/ide_hook-configuration-reference). With existing connection consent and live risk collection enabled, VaryAI adds an owned global `RunCommand` hook for each edition, using separately authenticated loopback endpoints. Disabled collection/consent removes the owned entry; user-edited and unrelated hooks remain untouched. The helper emits no permission decision, never rewrites tool arguments and bounds observation to 750 ms. Native Hook settings and sandbox restrictions are not overridden. Installation does not prove that the client has enabled or loaded the Hook.

Doubao Work's background bundle retains a legacy ToolCallManager lifecycle; its presence does not establish that the current native execution runtime uses it. Read-only inspection on 2026-11-17 confirmed its nine lifecycle observers and the `onBeforeExecution`/`onExecutionComplete` contract without installing an observer or executing a command. The new collector discovers that contract, observes only native-instance `Bash` calls, extracts the supported command field and supplied conversation ID, and removes its observer on shutdown/reconnect. It does not read chat histories, modify signed calls, change permissions, pause the debugger or buffer network responses. Windows uses the same observer when its existing loopback debugging connection is available; configuring or launching that connection on Windows remains outside the existing macOS-only launch helper.

The user requested no test execution in this follow-up. Added regression cases are unexecuted, and native warning delivery and applicable Windows/Linux acceptance remain pending. Do not reuse the earlier committed tree's passing results for this working tree.

Initial verification on 2026-11-17, before the subsequent Harness session-ID repair, covers the uncommitted tool-risk follow-up over `1c53f58f`, on macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3:

- `cargo check --lib --manifest-path src-tauri/Cargo.toml`: passed during implementation.
- `cargo check --tests --manifest-path src-tauri/Cargo.toml`: passed against that initial source snapshot, including compilation of its added regressions; no tests were executed. The existing unused `quota_panel::Manager::expiring` warning remains outside this change.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`, Prettier checks for the changed helper scripts/documents, `node --check` for all four new JavaScript files, and `git diff --check`: passed.
- Full unit/integration test execution, renderer checks, Clippy, production dependency audit, platform-native warning delivery, Windows/Linux jobs and CI acceptance were not performed in this follow-up. No application was built, installed or restarted for acceptance; Codex routing was left untouched.

## Harness session-ID acceptance follow-up

The user confirmed that the native risk warning appears, but the tested build recorded no session ID. The installed agent loop passes its native session ID into pi-ai. Its configurable compatibility gate withholds automatic session-affinity switches for Chat/Anthropic, so the repair uses the supported synchronous `llm-pi-ai/request-headers` extension instead of unsupported provider configuration. The extension forwards only the actual session ID to local Harness connection URLs through `x-vary-harness-session-id`; it never reads histories or credentials, derives identity from cache/request IDs, or exports that private header upstream. Existing native session headers are also recognized, and stored audit/tool IDs retain the `deepseek-harness_` prefix.

The owned Cordis extension entry is journaled separately from foreign plugins, preserves pre-extension receipts, and is removed when the connection is restored. Its isolated module is installed without replacing unexpected content and repaired if missing for an active connection. After a new build, reconnect Harness and restart it once to load the extension, then generate a new event; historical missing IDs are not backfilled. Regression cases for the header waterfall, native schema round trips, restoration, supplied-session correlation, invalid/duplicate headers and upstream privacy were added but not executed. JavaScript syntax and diff-whitespace checks passed. `cargo check --tests --manifest-path src-tauri/Cargo.toml` passed for this follow-up on the same macOS/Rust host; tests were compiled but not executed, and the existing unrelated `quota_panel::Manager::expiring` warning remains. The user subsequently confirmed native warning and new-record session-ID acceptance passed on 2026-11-17. Windows/Linux verification remains pending.

## T3 Code session-ID follow-up

Managed T3 Code uses the native Codex Responses driver. Session correlation now recognizes its `thread-id`, `session-id`, `session_id` and `x-session-id` headers, with supported metadata as a fallback, and retains the `t3code_` audit prefix. Extraction validates bounded identities and rejects repeated header values; request IDs, response IDs and cache keys are never substituted. Shared-routing regressions now supply the native thread header instead of synthetic metadata. Existing missing IDs are not backfilled. Implementation is shared across platforms; native acceptance requires a new event after installing the updated VaryAI build.

## WorkBuddy native catalog additions (2026-11-18)

Both WorkBuddy editions previously compared the complete model array with the
connection receipt. Adding unrelated native models could therefore report a lost
connection even though every managed model remained unchanged. Receipt loading
now accepts additive native rows and row reordering while preserving these rows
in the disconnect/suspend restoration baseline. Existing row changes, removals,
duplicate IDs, newly added managed rows and unrelated object-field changes still
require recovery. Status reads do not write client configuration or advance its
application timestamp.

The inventory below was checked against `src/config/appConfig.tsx` and the
backend connection registries. Entry points are existing connection status,
reload, suspension and disconnect. The new adapter is limited to both WorkBuddy
editions and handles array/object catalogs using shared macOS/Windows code;
existing platform support is preserved. There are no changes to tool security,
forwarding protocols, model parameters, credentials, session correlation or
notifications. Other Agent adapters and their UI states are unchanged.

| Agent              | Variant                         | Adapter, runtime and UI impact                                                 | Verification / native acceptance                                                       |
| ------------------ | ------------------------------- | ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- |
| `claude`           | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `claude-desktop`   | Desktop                         | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `codex`            | CLI/desktop; existing WSL paths | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `gemini`           | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `grokbuild`        | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `opencode`         | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `openclaw`         | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `hermes`           | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `pi`               | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `workbuddy`        | China desktop                   | Adopt native catalog additions; preserve connection, timestamp and restoration | Array/object status, reload, conflict and disconnect; array suspend; native unverified |
| `workbuddy-ai`     | International desktop           | Adopt native catalog additions; preserve connection, timestamp and restoration | Array/object status, reload, conflict and disconnect; array suspend; native unverified |
| `qoder`            | China desktop                   | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `doubaowork`       | Desktop; separate native audit  | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `trae`             | China native bridge             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `trae-intl`        | International native bridge     | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `kimi`             | CLI/desktop                     | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `qwen`             | CLI                             | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `zcode`            | Desktop                         | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `deepseek-harness` | Native harness                  | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |
| `t3code`           | Desktop; managed Codex          | Existing adapter, runtime and UI behavior retained                             | Existing suite coverage; native not rechecked                                          |

The new regression first failed against the old implementation. Earlier focused
validation over `2adbcd63` passed 149 connection tests with one ignored test.
Final verification uses the working tree over `81763452`, after fast-forwarding
`origin/main` and aligning `package.json`, the Rust package/lock entry and Tauri
configuration to version `0.4.0`. Host: macOS 26.6.2 arm64, Node 22.22.0,
pnpm 10.12.3, Rust/Cargo 1.98.1. Passed: `pnpm typecheck`,
`pnpm format:check`, `pnpm build:renderer`, `pnpm test:unit` (322 files,
4,376 tests), `pnpm audit --prod`, all documented helper suites (`test:dev`,
`test:macos-dmg`, `test:trae-bridge`, `test:model-default`, `test:pi-session`,
`test:workbuddy-checkin-auth`, `test:doubao-checkin`, `test:agent-rewards`,
`test:qoder-codec`, CI policy and shared-v23 recovery), Rust formatting,
`cargo check` and `cargo clippy -- -D warnings` with the application manifest.
`cargo test --lib --manifest-path src-tauri/Cargo.toml` passed 7,740 tests
with 21 ignored. `cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml`
passed 7,905 tests across 16 groups with 21 ignored and no failures.
The test build reported the existing unused-method and macOS unwind-table linker
warnings. Version consistency checks, formatting of the changed JSON manifests
and `git diff --check` also passed. This section was formatted
separately; inherited document-wide table formatting was preserved.

Native rebuilt-app acceptance remains unverified. Initial cross-platform CI
results for PR #68 are recorded in the shutdown-fixture section below. Local
checks do not establish native integration, installer or release acceptance.

## Qoder TLS shutdown regression fixture (2026-11-18)

[PR #68 CI run 44226715687](https://github.com/varium-ai/varyai/actions/runs/44226715687)
checked `51e83af94462a3cd403473556ebf16c6e5e264bd`. Frontend, macOS, Linux and
Windows-to-WSL2 jobs passed. The ordinary Windows job failed only
`proxy::qoder_tls::tests::stop_waits_for_socket_cleanup_beyond_the_old_deadline`
with Winsock 10048 / `AddrInUse`; its WorkBuddy catalog regression passed.

The shutdown fixture released an OS-assigned ephemeral port and then tried to
bind that address again, leaving a race with other parallel socket fixtures.
It now holds the original listener in an `Arc` and observes its lifetime through
a `Weak` reference. Shutdown must still wait for the 3,100 ms delayed task,
complete the original cleanup marker and release the original listener.
The resource assertions no longer depend on the address remaining unclaimed.
This only changes the test fixture; production shutdown, native adapters,
forwarding and UI behavior for all Agents in the inventory above are unchanged.

The updated fixture passed locally. A negative control temporarily restored the
historical three-second `timeout(task)` implementation and failed the retained
`cleaned` assertion as expected; the correct production implementation was then
restored byte-for-byte. The first negative-control build was interrupted by
missing shared-target artifacts, so the completed negative control and subsequent
backend checks use an independent `CARGO_TARGET_DIR`, dev/test debug information
set to zero (as in CI), and incremental compilation disabled. Host: macOS 26.6.2
arm64, Rust/Cargo 1.98.1, Node 22.22.0, pnpm 10.12.3.

Frontend typecheck, renderer build, formatting, production dependency audit and
all documented helper checks passed. The first full frontend run encountered two
10-second subprocess timeouts in the unchanged mocked notarization fixture;
its 21 tests passed independently, and the final full rerun passed all 4,376 tests
across 322 files. No notarization test or timeout was changed.

Backend formatting, compile and Clippy checks passed. `cargo test --lib` passed
7,740 tests with 21 ignored; `cargo test --no-fail-fast` passed 7,905 tests across
16 groups with 21 ignored. All Cargo commands used `--manifest-path
src-tauri/Cargo.toml`. The final production prefix of `qoder_tls.rs` matches
`51e83af9` exactly, and diff whitespace checks passed. Native Agent self-tests
remain unverified. Follow-up cross-platform acceptance must be checked on the
new head of [PR #68](https://github.com/varium-ai/varyai/pull/68), rather than
inferred from these local results.

## Restart freshness after a gateway restart

The suspended shared-connection resume path previously assigned a new application timestamp even when restoring the identical projection. This made a gateway restart look like a new configuration change and invalidated WorkBuddy and WorkBuddy AI persisted default-model confirmations. The resume path now retains the previous timestamp when startup-relevant configuration is unchanged and no client loaded the temporarily restored native configuration. Native macOS launch times and Windows process creation times detect clients opened during suspension; actual configuration changes and reopened clients still require loading the current projection. This changes automatic freshness reporting, not the unconditional explicit restart action. Previously invalidated confirmation markers require one client restart to establish current readback. Added regressions cover both WorkBuddy editions, all shared Agent configuration comparisons, mixed process ages and unknown launch times; test execution and native acceptance remain pending.

Verification for the subsequent T3 session and restart-freshness working-tree changes on the same macOS host: `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, Prettier for the two updated documents and `git diff --check` passed. `cargo check --tests --manifest-path src-tauri/Cargo.toml` was delayed by another Cargo process holding the build-directory lock and then stopped during dependency compilation; this snapshot has no completed compile acceptance. No tests were executed, and Windows/Linux verification and native retests remain pending. No commit, push, application installation or Codex route change was performed.

## Explicit configuration and reconnection

Source assessment covers every entry point. Native acceptance below is pending for this follow-up. Shared reconnection archives the old receipt, retains external edits as its new baseline and writes the current managed projection. Existing native consent, credential access and platform support remain applicable. TRAE's explicit replacement intent is distinct from ordinary model selection and background refresh.

| Agent ID           | Implementation path                                                                                   | Native acceptance in this follow-up                                       |
| ------------------ | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| `claude`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `claude-desktop`   | Desktop-aware shared release, visibility validation and install                                       | Not rechecked                                                             |
| `codex`            | Explicit native Codex reconnect and durable selection                                                 | Not rechecked; keep the user's route off                                  |
| `gemini`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `pi`               | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `workbuddy`        | Desktop-aware shared release, rebuild and install                                                     | Not rechecked                                                             |
| `workbuddy-ai`     | Desktop-aware shared release, rebuild and install                                                     | Not rechecked                                                             |
| `qoder`            | Desktop-aware shared release, rebuild and install                                                     | Not rechecked                                                             |
| `opencode`         | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `grokbuild`        | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `openclaw`         | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `hermes`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `kimi`             | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `qwen`             | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `zcode`            | Shared release with native rule-envelope repair, rebuild and install                                  | Not rechecked                                                             |
| `deepseek-harness` | Native credential-aware release, rebuild and install                                                  | Not rechecked                                                             |
| `t3code`           | Shared receipt release, rebuild and install                                                           | Not rechecked                                                             |
| `trae`             | Explicit configuration/retry requests native managed-model replacement; failures retain the intention | Read-only saved/native catalog match passed; new replacement path pending |
| `trae-intl`        | Explicit configuration/retry requests native managed-model replacement; failures retain the intention | Read-only saved/native catalog match passed; new replacement path pending |
| `doubaowork`       | Native audit connection/launch path; the product exposes no editable provider/model projection        | Not rechecked                                                             |

## Default-model restart controls (2026-11-17)

Pre-implementation review found T3 Code absent from the desktop restart registry despite having a desktop configuration adapter. The shared UI also discarded the backend `restartRequired` flag and hid pending restart actions during configuration recovery. Harness has a macOS bundle adapter; process discovery, native reload and stopped clients are separate states. No running Harness desktop instance was observed during the read-only inspection, so that observation cannot establish the state at the user's model switch. This change must not claim that every model switch requires restarting or that existing conversations automatically change their model.

| Agent              | macOS entry point                          | Windows entry point                          | Model-change UI assessment                                                                  |
| ------------------ | ------------------------------------------ | -------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Claude Code        | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| Claude Desktop     | Verified bundle                            | Native executable/package                    | Shared restart status; native retest pending                                                |
| Codex              | Verified bundle and reload state           | Native package and reload state              | Shared restart status; route untouched                                                      |
| Gemini CLI         | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| Grok Build         | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| OpenCode           | CLI integration                            | CLI integration                              | Desktop lifecycle absent; not claimed supported                                             |
| OpenClaw           | CLI integration                            | CLI integration                              | Reload/reopen CLI; no desktop process restart                                               |
| Hermes             | CLI integration                            | CLI integration                              | Reload/reopen CLI; no desktop process restart                                               |
| Pi                 | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| WorkBuddy          | Verified bundle and default-model readback | Native executable and default-model readback | Shared restart status; previous user acceptance predates this follow-up                     |
| WorkBuddy AI       | Separate verified bundle and readback      | Separate native executable and readback      | Shared restart status; previous user acceptance predates this follow-up                     |
| Qoder CN           | Verified bundle and readback               | Native executable and readback               | Shared restart status; native retest pending                                                |
| Doubao Work        | Audit launch/restart                       | Native lifecycle adapter absent              | Audit-only; no managed default-model dropdown                                               |
| TRAE CN            | Edition-bound bridge and bundle            | Edition-bound bridge and executable          | Shared restart status and native selection readback                                         |
| TRAE International | Separate bridge and bundle                 | Separate bridge and executable               | Shared restart status and native selection readback                                         |
| Kimi Code          | Verified desktop bundle; CLI also exists   | Native desktop lifecycle absent              | Desktop/CLI runtime distinction remains necessary                                           |
| Qwen Code          | CLI                                        | CLI                                          | Reload/reopen CLI; no desktop process restart                                               |
| ZCode              | Verified bundle                            | Native lifecycle absent                      | Shared restart status on macOS; Windows gap explicit                                        |
| DeepSeek Harness   | Verified bundle                            | Native lifecycle absent                      | Shared restart status; stopped/native-reloaded states have no stale-running-instance prompt |
| T3 Code            | Missing before this change                 | Missing before this change                   | Add verified bundle/native executable lifecycle and honor backend restart status            |

Session-label assessment: existing Agent identities remain unchanged. Pi's existing display normalization is extended to T3 Code so both historical raw IDs and new prefixed IDs display exactly one `t3code_` prefix in the event list and details. Missing IDs stay missing; filtering and persisted identity are not rewritten.

T3 Code now registers `com.t3tools.t3code` on macOS and standard Windows main/Alpha/Beta executable identities for freshness detection and explicit quit/reopen. Shared restart controls honor the backend flag independently of recovery state; configuration repair remains a separate action. No client was launched, restarted or reconfigured during this inspection. CLI-only agents still have no desktop restart adapter. Harness's stopped/unknown/native-reloaded cases are not established as fixed by these changes; its reported switch requires native retest while the desktop instance is running.

Verification for the final uncommitted restart-control and T3 session-label follow-up over `1c53f58f`, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, targeted Prettier checks, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. This supersedes the interrupted compilation recorded above for the earlier snapshot, including compilation of the prior WorkBuddy freshness and T3 session repairs. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Added regressions cover all UI Agent identifiers' backend restart requirement, T3 Windows installation identity boundaries and historical/already-prefixed T3 event labels in both list and details. Tests were compiled but not executed, as requested; Windows/Linux native builds, native restart/model-switch acceptance and full CI acceptance remain pending. No commit or push was made; Codex routing remains untouched.

## OpenCode session-ID follow-up (2026-11-17)

Pre-implementation assessment: the installed Homebrew OpenCode 1.19.5 binary sends `x-opencode-session-id` on inference requests, plus `x-session-affinity` and `X-Session-Id` for custom providers. Its built-in provider path also sends `x-opencode-session`. The audit extractor previously accepted only generic body metadata for OpenCode. Read-only inspection of the bundled request builder confirms these fields contain the native session ID; request, parent-session and prompt-cache identities have different scopes and must not replace it. The fix is shared across macOS, Windows and Linux, requires no client extension or account/configuration modification, and must preserve the original request.

The existing all-Agent inventory remains applicable: OpenCode alone gains recognition of its two native private headers; other Agent extractors and native collectors keep their existing contracts. Regressions must check cross-Agent isolation against `AppType::all()`, invalid/duplicate IDs, metadata compatibility, native headers on the managed shared Chat route and the `opencode_` event prefix. Event-list and detail formatting must handle existing raw OpenCode IDs without double-prefixing new records. Missing historical identities are not backfilled. Native retest remains pending. Verification results for this snapshot follow below.

Verification for the final OpenCode follow-up over the uncommitted `1c53f58f` working tree on the same macOS/Rust/Node/pnpm host: `pnpm typecheck` and `cargo check --tests --manifest-path src-tauri/Cargo.toml` passed. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Rust formatting and diff-whitespace checks passed; the two documentation formatting findings were corrected and targeted Prettier checks rerun. Added regressions were compiled but not executed, following the user's request. Native new-record acceptance, Windows/Linux jobs and full CI acceptance remain pending. No build artifact was installed, no client or VaryAI process was restarted, no account/configuration was changed, Codex routing was left untouched, and no commit or push was performed.

The user subsequently confirmed OpenCode native new-record acceptance passed on 2026-11-17, covering the tool warning and session-ID follow-up. This does not establish other-platform or nested code-mode acceptance. OpenClaw was subsequently skipped at the user’s request.

## Hermes session-correlation follow-up (2026-11-17)

Pre-implementation assessment: Hermes custom providers intentionally omit session identity unless the native `session_affinity_header` option is enabled. Installed source `agent/opencode_affinity.py`, `agent/chat_completion_helpers.py` and `hermes_cli/config_providers.py` establish that this option adds a client-declared conversation affinity header on Chat, Responses and Anthropic requests without a plugin. VaryAI should enable a dedicated header on its two managed provider entries and recognize that header only for Hermes. The existing all-Agent matrix remains authoritative; other Agent projections and extractors retain their contracts, with explicit cross-Agent isolation regressions.

This is native conversation affinity, which can survive compression and follow the conversation root. It is not guaranteed to equal the current physical transcript ID; native cron-job scope can group scheduled runs. VaryAI must not compute it from prompt cache keys, request IDs, histories or generated per-request identifiers. Empty native identity stays absent. Managed configuration changes must preserve foreign providers and restore the preceding header option on disconnect. New events should carry `hermes_`; existing raw IDs should display consistently, while missing historical IDs remain missing. This shared configuration/extraction covers macOS, Windows and Linux, with native acceptance recorded separately.

Hermes implementation uses the supported native option on both `varyai` and `varyai-anthropic` entries; it does not add a plugin or patch the installed Hermes source. Recognized native physical-session headers take precedence when supplied; the local affinity header is next, followed by existing supported metadata. Invalid/repeated identities stay absent and private Hermes headers do not identify other Agents. Added regressions cover both managed provider declarations, foreign-provider/header preservation and restoration, all-Agent header isolation, request/cache/token exclusion, shared-route correlation/privacy, and raw/already-prefixed labels in list and details. Rebuild VaryAI, refresh/reconnect Hermes and reopen Hermes once if its running runtime has cached provider configuration; generate a new tool event. Native new-record acceptance remains pending; compile/type/format verification results follow below.

Verification for the final Hermes follow-up over the uncommitted `1c53f58f` working tree, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, targeted Prettier checks, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Installed Hermes checkout source at `3b42157858` and both managed workspace sources expose the native custom-provider affinity option and merge implementation. Tests were compiled but not executed as requested. Native identity/warning acceptance, scheduled/delegated identity scope, Windows/Linux native jobs and full CI acceptance remain unverified. No native application/configuration was changed during inspection, no app was installed or restarted, Codex routing was untouched, and no commit or push was made. Grok Build was user-confirmed passed; TRAE CN is the current native test, while OpenClaw remains skipped.

## TRAE edition parity and native Hook acceptance (2026-11-17)

Pre-implementation review: tool-call continuation normalization, durable ownership discovery, response observation and slow-header handling matched only domestic `trae`, excluding `trae-intl`. Both native editions rewrite Chat tool-call IDs; the international adapter must use the same observed-payload fingerprints while preserving edition isolation, known owners, ambiguous/unknown-call rejection and the original forwarded request. Other Agents retain their existing continuation rules; this is an edition-parity fix on shared Rust code for all supported operating systems, not a relaxation of ownership checks.

Both owned global Hook files exist with the expected RunCommand matcher. The installed native UI separately controls `global_hooks_enabled` and requires native confirmation to enable Hook commands. Merely writing hooks.json does not prove collection; enabled/disabled native Hooks and sandbox access must be distinguished from schema/rule matching. Native global and project Hook decisions remain unchanged. The user was asked to check the TRAE CN global Hook switch while the international continuation fix proceeds. Current evidence does not establish that the switch is off or that warning delivery works.

The user subsequently confirmed Hermes acceptance passed on 2026-11-17. The provided TRAE settings screenshot confirms Configured Hooks is disabled; enabling that native global switch and retesting in the existing sandbox mode is the next native step. This does not establish successful Hook delivery yet.

The continuation repair now includes both edition identities in rewritten-call normalization, persisted ownership discovery, response-payload observation and slow-header waiting. Regressions cover complete observed payloads, changed/unknown calls, authoritative known owners, original-request preservation, persisted ownership across restart and exact target selection for both JSON/SSE transports and both edition credentials. The original forwarded tool calls remain unchanged, and unknown/ambiguous ownership still fails closed. Native global Hook activation was confirmed disabled by the screenshot and the user has now enabled it; warning delivery remains pending native retest.

The user confirmed TRAE CN native warning delivery after enabling Configured Hooks and requested the domestic session label `trae_cn_` instead of `trae_`. Pre-implementation scope: retain the internal Agent ID `trae`, native schema, policy bindings and edition authentication; change only domestic event correlation prefix and historical display formatting. The subsequent international naming request is included below; other Agent prefix contracts remain unchanged. Add native collector and list/detail regressions for raw, old-prefixed and already-correct domestic IDs, preserving stored/filter identity and not filling absent IDs.

The user clarified the prefix contract: TRAE CN uses `trae_cn_`, and TRAE International uses `trae_`. Both native collector persistence and old-record list/detail formatting must follow that mapping without changing internal Agent IDs (`trae` and `trae-intl`), routing scope or policy identity. Domestic legacy audit inspection must use the same new prefix; no database migration or historical identity rewrite is required.

The final prefix implementation normalizes new native tool and audit IDs by edition (`trae_cn_` domestic, `trae_` international), including the domestic legacy audit path. List/detail display accepts raw, previous-prefix and already-correct IDs while preserving the underlying stored ID and internal client category. Added synthetic collector/audit/UI cases cover both editions and idempotent prefixing. TRAE CN warning delivery is user-confirmed passed; both new prefixes and international continuation require retest with the final build.

Final verification for the uncommitted TRAE continuation/prefix changes over `1c53f58f`, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, targeted Prettier, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. An initial compilation error in the new Observer fixture's struct-update initialization was corrected before the final successful check. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Added regressions compiled but were not executed, following the user's request. TRAE CN native warning delivery is user-confirmed passed; domestic/international prefix changes and the international 409 repair await final-build native retest. Windows/Linux native acceptance and full CI checks remain pending. Native global Hook activation was performed by the user; no application was installed/launched/restarted, no native permission or account configuration was changed by the agent, Codex routing remained untouched, and no commit or push was performed.

## TRAE reconnect latency follow-up (2026-11-17)

Pre-implementation assessment: read-only inspection found the international helper healthy and its outbox delivered, while explicit replacement deletes and recreates every owned model and forces a native list refresh after each addition. For a large catalog this causes prolonged UI waiting despite a live connection. Both editions and supported desktop platforms use the same renderer transaction. Compatible owned models can be overwritten in place without changing wire identity or skipping the user's explicit write intent; incompatible protocols/identities and prior ownership still require the existing replacement path. Failures must restore prior editable fields and ownership without persisting credentials. Other Agent mutation paths remain unchanged. Native latency and all-platform acceptance remain unverified.

Doubao Work restart is also pending: the audit launch adds loopback-only remote-debugging arguments, while generic desktop restart currently uses a plain launch. Both explicit UI actions must launch in audit mode, with restart quitting an existing instance first. The existing macOS-only audit-launch boundary and other-Agent restart arguments must remain explicit in the inventory; this is not permission to install or restart a native app during verification.

TRAE reconnect now overwrites compatible owned rows in place while still writing every requested model's current fields. New rows are created in settled batches of at most four, followed by one shared publication refresh and strict native identity readback. Successful peers of a failed batch are discovered by exact new-row identity and rolled back only after all outstanding creates have settled; existing foreign models are excluded. Incompatible prior ownership/protocols retain the existing replacement path. Prior editable fields and ownership restore after a failed compatible overwrite. Helper revision 19 loads the new renderer instead of continuing with a resident revision-18 helper. Regression cases cover a 58-model overwrite, the four-create concurrency ceiling, batched refresh count, wait-before-rollback, original-row preservation and failed-overwrite restoration. These cases are added but not executed under the user's no-test-execution preference; native performance remains pending.

Doubao's two explicit buttons now delegate to the same consent/takeover/audit-launch command. The audit launcher uses verified desktop lifecycle handling instead of rejecting an already-running client. A running verified instance quits and reopens; a stopped client opens directly. Both use loopback-only, dynamic-port Chromium audit arguments. macOS LaunchServices and the Windows native launch branches share the argument selector, and all other Agent arguments remain unchanged. The existing automatic audit launcher is still macOS-only; Windows lifecycle/launcher acceptance is not claimed. No native application was restarted or installed during implementation.

Final verification for the uncommitted TRAE reconnect/batching and Doubao audit-restart changes over `1c53f58f`, on the same macOS 26.6.2 / arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3 host: `pnpm typecheck`, `cargo check --tests --manifest-path src-tauri/Cargo.toml`, `node --check` for the four changed TRAE helper/test scripts, targeted Prettier, `cargo fmt --check --manifest-path src-tauri/Cargo.toml` and `git diff --check` passed. The existing unrelated `quota_panel::Manager::expiring` dead-code warning remains. Rust tests were compiled, JavaScript syntax was checked, and no tests were executed under the user's preference. Native concurrency/rollback performance, Doubao restart-to-audit acceptance, Windows/Linux native jobs and full CI acceptance remain pending. Read-only local helper health/list requests confirmed the international bridge was reachable and its outbox delivered; no native configuration or running client was changed during diagnosis. No commit, push, app installation or app restart was performed; Codex routing stayed untouched.

## Provider TLS compatibility and confirmed desktop restart races

Assessment for the provider TLS and pending desktop restart changes on
2026-11-17 covers the full inventory below. Provider discovery/protocol detection
uses the common Reqwest client; routed HTTP requests use its bound client or the
original-header/Hyper transports. Five synthetic TLS regressions cover these
transports, repeated connections, certificate checks and session resumption.
The live provider check establishes one authorized provider's catalog and
inference behavior, not native acceptance for every Agent.

Confirmed restart enters from the Agents row and confirmation dialog. The dialog
closes on confirmation while the row retains progress and localized failure
feedback. Supported macOS bundle and Windows process adapters resample a bounded
pre-quit discovery race, wait for a verified return, and reject unidentified
replacements. Automatic configuration guards are retained. CLI-only sessions and
unsupported native platform adapters gain no process restart action. Existing
macOS-only desktop identities remain explicit; this change does not add Windows
support for them. Synthetic lifecycle tests never inspect or restart installed
applications. Native per-Agent acceptance remains pending on every platform.

| Agent ID           | Provider TLS path and entry                                                              | Confirmed restart adapter and UI                                                     | Verification and remaining native acceptance                       |
| ------------------ | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `claude`           | Shared API routing; provider discovery                                                   | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `claude-desktop`   | Shared API routing; provider discovery                                                   | Agents confirmation; macOS and Windows identity adapters; Linux unsupported          | Synthetic lifecycle/TLS; native macOS/Windows pending              |
| `codex`            | Native Responses/managed API routing; provider discovery                                 | Agents confirmation; macOS and Windows package/bundle identity; Linux unsupported    | Synthetic lifecycle/TLS; native pending; keep the user's route off |
| `gemini`           | Shared Gemini/API routing; provider discovery                                            | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `pi`               | Shared API routing; provider discovery                                                   | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `workbuddy`        | Shared API routing; provider discovery                                                   | Agents confirmation; domestic macOS bundle and Windows image; Linux unsupported      | Synthetic lifecycle/TLS; native macOS/Windows pending              |
| `workbuddy-ai`     | Shared API routing; provider discovery                                                   | Agents confirmation; international macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native macOS/Windows pending              |
| `qoder`            | Shared API routing; provider discovery                                                   | Agents confirmation; Qoder CN macOS bundle and Windows image; Linux unsupported      | Synthetic lifecycle/TLS; native macOS/Windows pending              |
| `kimi`             | Shared API routing; provider discovery                                                   | macOS desktop bundle; CLI session separate; Windows/Linux restart unsupported        | Synthetic macOS orchestration/TLS; native pending                  |
| `qwen`             | Shared API routing; provider discovery                                                   | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `opencode`         | Shared API routing; provider discovery                                                   | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `grokbuild`        | Shared API routing; provider discovery                                                   | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `openclaw`         | Shared API routing; provider discovery                                                   | No native desktop lifecycle adapter                                                  | Shared TLS fixtures; native client not rechecked                   |
| `hermes`           | Shared API routing; provider discovery                                                   | CLI session; no desktop restart action                                               | Shared TLS fixtures; native client not rechecked                   |
| `zcode`            | Shared API routing; provider discovery                                                   | macOS desktop bundle; Windows/Linux restart unsupported                              | Synthetic macOS orchestration/TLS; native pending                  |
| `deepseek-harness` | Shared API routing; provider discovery                                                   | macOS desktop bundle; Windows/Linux restart unsupported                              | Synthetic macOS orchestration/TLS; native pending                  |
| `t3code`           | Shared API routing; provider discovery                                                   | No native desktop lifecycle adapter                                                  | Shared TLS fixtures; native client not rechecked                   |
| `trae`             | Existing native integration and shared provider discovery                                | Agents confirmation; domestic macOS bundle and Windows image; Linux unsupported      | Synthetic lifecycle/TLS; native integration not rechecked          |
| `trae-intl`        | Existing native integration and shared provider discovery                                | Agents confirmation; international macOS bundle and Windows image; Linux unsupported | Synthetic lifecycle/TLS; native integration not rechecked          |
| `doubaowork`       | Native audit integration has no editable provider projection; native collector unchanged | Agents confirmation on macOS; Windows/Linux restart unsupported                      | Synthetic macOS orchestration; native integration not rechecked    |

## Routing list refresh and bounded catalog synchronization

The November 23 renderer follow-up applies to the common Routing workspace
used by every Agent listed below. Cached refresh confirms completion, and the
classification deadline can remain empty while editing. Saving and trialing
require a deadline in the existing supported range. The native adapters and
stored deadline format stay on their existing paths on macOS and Windows.

The same observation path rechecks missing native session titles on its
two-second interval for every Agent with a native title reader. It still
requires an exact Agent-scoped session match and excludes titles from history.
The shared group executor applies model-entitlement fallback to requests from
every Agent that can select an explicitly bound WorkBuddy China or WorkBuddy
AI subscription. The regression covers both subscription types, a non-WorkBuddy
provider, pinned continuations, a missing fallback and unrelated refusals.
Native Agent and paid-account acceptance remain pending.

Implementation assessment before the 2026-11-17 change: the Routing page has one
shared entry for all Agents. A local list refresh must use cached catalogs;
explicit model synchronization may fetch catalogs, with bounded concurrency and
a shared network deadline. Publication continues through the existing global
and per-client queues. No native protocol, credential, configuration-consent,
or request forwarding contract changes. The common Rust and React paths apply
to macOS, Windows and Linux; native integration support remains as recorded above.

| Agent ID           | Entry and runtime path                                                   | Protocol/native adapter impact                                           | UI and planned verification                                                  | Native acceptance            |
| ------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ---------------------------------------------------------------------------- | ---------------------------- |
| `claude`           | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `claude-desktop`   | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `codex`            | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `gemini`           | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `pi`               | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `workbuddy`        | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `workbuddy-ai`     | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `qoder`            | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `opencode`         | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `grokbuild`        | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `openclaw`         | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `hermes`           | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `kimi`             | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `qwen`             | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `zcode`            | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `deepseek-harness` | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `t3code`           | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `trae`             | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `trae-intl`        | Shared cached list and explicit catalog sync; existing publication queue | Existing adapter and platform support retained                           | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |
| `doubaowork`       | Shared cached list and explicit catalog sync; existing publication queue | No editable model projection; existing native audit integration retained | Shared UI, timeout, cached-fallback and queue progression regressions passed | Not exercised in this change |

Shared synthetic regressions passed on macOS. Full host checks are recorded with the verification results; native per-Agent acceptance and remote Windows/Linux/macOS CI remain pending. This assessment does not claim per-Agent native acceptance.

## TRAE native concurrent-write correction (2026-11-17)

Assessment: both `trae` and `trae-intl` use the same renderer model writer on macOS and Windows. The other 18 Agent writers are unaffected. The revision-19 four-create batching described above is superseded: the running international client reproduced `model_apply_failed` during a user-authorized reconnect operation, with an incomplete native catalog. Helper health was ready at revision 19; this was a write/readback failure rather than an unavailable connection.

The revision-20 writer creates missing models sequentially, retaining compatible in-place updates and one post-creation publication refresh. A regression models native read/modify/write snapshots, where overlapping successful calls can lose rows; failure coverage verifies that subsequent creates stop and only new rows are rolled back. Native storage internals were not independently proven; the snapshot regression covers the observed lost-row behavior without claiming their exact implementation.

Native verification used the corrected renderer against the already-running TRAE International instance through its existing local native connection, without installing a build, restarting an application, changing Codex routing or exposing credentials. The original writer returned `model_apply_failed` in 40.84 seconds. The corrected writer completed in 15.789 seconds with all 58 requested models and `matches=true`, including native credential proofs. This confirms this live write/readback operation, not VaryAI's visible button state or full reconnection acceptance. Computer Use was unavailable; no UI click was claimed. The temporary diagnostic connection closed after completion and no background diagnostic process was retained. Domestic-edition and Windows native acceptance remain pending.

Comparison: Magpie main at [`f9492fd8ed2c6fd3c65f1dab00062771b551d7b5`](https://github.com/yetone/magpie/tree/f9492fd8ed2c6fd3c65f1dab00062771b551d7b5) has no TRAE desktop Agent writer. Its [`trae_checkin.go`](https://github.com/yetone/magpie/blob/f9492fd8ed2c6fd3c65f1dab00062771b551d7b5/internal/provider/trae_checkin.go) documents TRAE CN as a community-plugin provider with no built-in mover. Provider access/check-in does not establish desktop model-list configuration support.

Verification scope: working-tree changes over `7919f897`, macOS arm64; Node syntax checks for the four changed helper/test scripts, targeted JavaScript/architecture-document Prettier, Rust formatting and diff whitespace checks passed. The pre-existing provider-TLS table formatting in this ledger was preserved and still fails whole-file Prettier. Regression tests were updated but not executed under the user's earlier preference. Full build, CI and cross-platform acceptance are not claimed. No commit or push.

## TRAE Hook setup guidance (2026-11-17)

Scope before implementation: the `trae` and `trae-intl` connection details and setup dialog share the same Hook instructions on macOS and Windows; the existing TRAE CN audit notice also displays them. The other 18 Agent entries keep their existing instructions because this native switch is specific to TRAE. Explain the pre-execution observation purpose, the Settings > Hooks > Configured Hooks switch, independent per-edition activation, no requirement to import Claude Hooks, and retesting after activation with an enabled matching risk policy. This is guidance only: it does not toggle native permissions, change execution mode or alter tool decisions. Native UI verification remains pending.

Guidance verification: `pnpm typecheck`, targeted Prettier for both changed React components and all four locale files, JSON parsing and `git diff --check` passed. No tests, application build, installation or native UI interaction was performed for this text-only follow-up. No commit or push.

## Doubao single audit-launch action (2026-11-17)

Pre-implementation coverage: only `doubaowork` uses the dedicated audit launcher; the other 19 Agent restart paths retain their existing controls. Both compact and full layouts must suppress the generic restart control for Doubao, expose one audit-launch action while disconnected, label a known running instance as an audit restart, and show audit-waiting status instead of stale model/configuration language. The shared frontend applies on every platform; the existing macOS-only native audit-launch boundary remains unchanged. An active audit connection must hide the launcher even if a stale restart flag arrives.

Implemented the single-action condition and localized running/stopped labels in all four runtime languages. Added compact/full-layout regressions for pending audit with stale restart flags, audit-launch dispatch, and active audit suppressing both launcher and generic restart even with stale flags. `pnpm typecheck`, targeted component/test/locale Prettier and `git diff --check` passed. Regression execution and native UI acceptance remain pending; no commit, push, build or native restart was performed.

## Doubao native tool-risk acceptance failure (2026-11-17)

The user ran the synthetic `kubectl delete pod vary-test-nonexistent --dry-run=client` command in the actual client and confirmed a native tool result. The current background collector installed successfully after audit-mode launch, but a temporary metadata-only lifecycle observer received zero callbacks during this execution; the risk-event database contained no Doubao records. Missing Kubernetes configuration affects the command result, not whether risk observation should occur.

Read-only runtime inspection confirmed dispatcher mode and an uninitialized legacy tool-context source in the background webview. The installed client separately hosts `entry-moa` in its native JSRuntime utility process. This exposes a missing adapter path, not successful tool-risk acceptance. The native `jsRuntime.enableDebuggingPort` API rejected requests from the existing background origin with `INVALID_ORIGIN`; no origin change, security-policy override or signed tool mutation was attempted, and no new inspector listener was established. A supported native observation interface is still required. Do not relabel model prose or post-execution text as a pre-execution tool call.

The temporary lifecycle diagnostic recorded only tool names and field-presence metadata, had an automatic three-minute cleanup, and was explicitly removed after the user's test. An attempted invocation of the Doubao executable as a JavaScript host unexpectedly launched extra native processes; that diagnostic process tree was stopped, and the user subsequently opened the client in audit mode. Later scripts ran under standalone Node and closed their debugging connections. No tool-risk implementation fix or acceptance is claimed from this investigation. All other Agent coverage remains as recorded above; no commit or push.

## Disconnect and restart presentation

Pre-implementation comparison: Magpie `3ec89c52` puts the reopen note in the
Codex and Claude Code disconnect dialog. A Codex route change is handled by the
desktop app-server worker after turns finish. For desktop Agents with a supported
lifecycle adapter, the disconnected row can offer a confirmed restart alongside
its normal status when the running instance predates restoration.

Both the full Agents page and compact panel use the same row and dialog.
Disconnect and restart continue to use their existing commands. Planned
synthetic coverage checks each row after disconnect, the dialog notes, enabled
restart, recovery, and unsupported or stale status. Native verification is
pending on all platforms.

| Agent              | Entry and adapter                                                         | Runtime and UI after disconnect                                                                                                    |
| ------------------ | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | Shared switch or model menu. Claude Code config.                          | Reopen note in confirmation. No process restart action.                                                                            |
| `claude-desktop`   | Shared switch or model menu. Desktop profile.                             | Confirmed status-line restart for a stale instance on macOS and Windows.                                                           |
| `codex`            | Shared switch or model menu. Codex config and desktop app-server monitor. | Reopen note in confirmation. Route changes refresh the idle app-server on macOS and Windows. Failure gives manual reopen guidance. |
| `gemini`           | Shared switch or model menu. Gemini CLI config.                           | Normal disconnected status. No process restart action.                                                                             |
| `grokbuild`        | Shared switch or model menu. Grok Build config.                           | Normal disconnected status. No process restart action.                                                                             |
| `opencode`         | Shared switch or model menu. OpenCode config.                             | Normal disconnected status. No process restart action.                                                                             |
| `openclaw`         | Shared switch or model menu. OpenClaw config.                             | Normal disconnected status. No process restart action.                                                                             |
| `hermes`           | Shared switch or model menu. Hermes profile.                              | Normal disconnected status. No process restart action.                                                                             |
| `pi`               | Shared switch or model menu. Pi config and helper.                        | Normal disconnected status. No process restart action.                                                                             |
| `workbuddy`        | Shared switch or model menu. WorkBuddy CN catalog.                        | Confirmed status-line restart for a stale instance on macOS and Windows.                                                           |
| `workbuddy-ai`     | Shared switch or model menu. International catalog.                       | Confirmed status-line restart on macOS and through Vary's existing verified Windows identity.                                      |
| `qoder`            | Shared switch or model menu. Qoder CN adapter.                            | Confirmed status-line restart on macOS or Windows when native freshness requires it.                                               |
| `doubaowork`       | Shared switch. Official-session audit adapter.                            | Dedicated audit launch action. No generic restart action.                                                                          |
| `trae`             | Shared switch. Domestic native bridge.                                    | Confirmed status-line restart for a stale instance on macOS and Windows. Bridge cleanup status takes precedence.                   |
| `trae-intl`        | Shared switch. International native bridge.                               | Confirmed status-line restart for a stale instance on macOS and Windows. Bridge cleanup status takes precedence.                   |
| `kimi`             | Shared switch or model menu. CLI profile and macOS desktop adapter.       | Confirmed status-line restart for a stale macOS desktop instance.                                                                  |
| `qwen`             | Shared switch or model menu. Qwen Code config.                            | Normal disconnected status. No process restart action.                                                                             |
| `zcode`            | Shared switch or model menu. ZCode config.                                | Confirmed status-line restart for a stale instance on macOS and Windows. Use known Windows install paths for process identity.     |
| `deepseek-harness` | Shared switch or model menu. Harness projection.                          | Confirmed status-line restart for a stale macOS desktop instance.                                                                  |
| `t3code`           | Shared switch or model menu. T3 Code projection.                          | Confirmed status-line restart for a stale instance on macOS and Windows.                                                           |

Linux and WSL have no native restart path in this change. OpenMausBot exists in
Magpie's restart registry and has no Vary Agent entry.

Verification used working-tree changes over `8df4a222` on macOS 15.3.1 arm64
with Rust 1.98.1, Node 24.11.0 and pnpm 10.12.3. Type checking, renderer build,
frontend and Rust formatting, production dependency audit, CI helper suites,
`cargo check`, and Clippy with `-D warnings` passed. The full frontend unit run
passed 4,605 tests across 325 files, including the compact and full Agents
layouts with macOS, Windows and Linux status fixtures. Isolated Rust harnesses
passed 23 Windows lifecycle tests and the Codex idle refresh regression.

The full Rust library and integration runs stopped during compilation with
`ENOSPC`. Native desktop restart and cross-platform CI acceptance remain
pending.

## Doubao current-runtime observation follow-up

Pre-implementation assessment: retain the legacy native lifecycle for older Doubao versions and investigate a passive structured-event feed for the current JSRuntime execution path. Do not bypass the native `INVALID_ORIGIN` decision or infer tool calls from chat prose. Both macOS and Windows collectors must use the same validated adapter where their existing audit connection is available; automatic audit launch remains macOS-only. The other 19 Agent collectors and their recorded acceptance are unaffected. New transport/source/stage claims require actual native schema evidence, bounded parsing, supplied-session correlation and preservation of normal execution.

### Current-runtime implementation

The user repeated the native terminal test while a bounded, metadata-only Network diagnostic was attached. Live completion SSE contained native file-operation cards; read-only inspection of the current rendered card confirmed `block_type=10019`, `file_type=file_content_type=shell`, `operation_type=1`, `scene=1`, and a separate `display_content.operation` exactly matching the synthetic test command. Output remained a different field and was not copied into observations. The actual endpoints were `/chat/completion` and `/chat/completion/action`. Temporary diagnostics detached and exited.

Added a separate bounded CDP Network collector and shell-report parser, retaining the old background hook. Reports use `client_report` / `reported`, a distinct schema, validated field conversion and the actual supplied conversation ID with `doubaowork_`. The collector does not claim a pre-execution warning or block. Ordinary prose, history echoes, unsupported cards, oversized input and invalid session changes are excluded. Split streams, duplicates, replacement/deletion, native-source validation, enabled/disabled queue ingress, sanitized persistence and frontend source/stage labels have regression coverage. Other Agent collectors keep their original behavior and source/stage contracts; no new acceptance is inferred for them. The parser/transport is shared by the macOS and Windows code paths; Windows compilation and native connection/launch acceptance remain pending. No native application was rebuilt, replaced or relaunched for this implementation; Codex routing stayed untouched.

### Commit verification for this follow-up

Checked working-tree changes over `f010e2b3` on macOS arm64, Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3. `pnpm typecheck`, `pnpm build:renderer`, `pnpm test:unit` (322 files, 4,376 tests), `pnpm format:check`, production dependency audit, targeted helper/document formatting and diff whitespace checks passed. TRAE's 72 helper tests passed. The development runner, CI policy, macOS DMG, default-model, Pi session, WorkBuddy authentication, Doubao check-in, Doubao legacy tool hook, Agent rewards, Qoder codec and shared-v23 recovery helper suites also passed. Renderer generation did not build, install or launch the desktop application.

The initial full Rust run found a fixture pinned to TRAE helper revision 19 after the production helper moved to 20. The fixture now reads the bundled revision and retains its assertion that configuration synchronization must not enable auditing. The initial frontend run found an outdated generic Doubao restart-button expectation; the all-Agent recovery test now expects Doubao's dedicated audit launcher, while the other Agents retain generic restart expectations. Full frontend regression passed after that correction. Final `cargo check`, `cargo fmt --check`, `cargo clippy -- -D warnings` and full `cargo test` (all with `--manifest-path src-tauri/Cargo.toml`) passed: 7,904 tests passed and 21 intentionally ignored, including 7,739 library tests passed. The existing test-only unused-method and macOS unwind-table linker warnings remain. Native new-build notifications, Windows/Linux CI and installer acceptance are not inferred from local checks. Earlier no-test notes above describe earlier revisions, not this final verification run.

## November 23 tray and provider regressions

The tray reads persisted Security Events without changing collection, policies,
event identity or gateway framing. It renders DLP and tool events in local time
using the stored Unix milliseconds. The compact Agents tab uses the same
check-in cards and account actions as the workspace and gives each expanded card
the full tray row. The provider editor pages its API chat catalog and allows
scrolling to continue into the surrounding form at the catalog boundary. Only
the editor's presentation changes. Saved selection, discovery, protocol
adapters and Agent catalogs keep their existing contracts on macOS and Windows.
When a saved API provider uses automatic selection and has no saved model rows,
its provider-list count and search use the prepared catalog already read by
the routing editor. This is a read-only fallback before the per-Agent adapters.
The workspace and tray profile dialogs share the capture entry points in the
UI consistency matrix. A capture error belongs to the submitted draft name.
When the name changes, the dialog checks the current value and allows a retry
through the existing per-Agent capture adapter.

| Agent              | Existing event and provider entry point                      | Compact check-in entry point | Native macOS / Windows |
| ------------------ | ------------------------------------------------------------ | ---------------------------- | ---------------------- |
| `claude`           | Claude Code Messages through the gateway                     | Unavailable                  | Pending / pending      |
| `claude-desktop`   | Claude Desktop Messages through the desktop gateway          | Unavailable                  | Pending / pending      |
| `codex`            | CLI and Desktop Responses through the managed gateway        | Unavailable                  | Pending / pending      |
| `gemini`           | Gemini CLI native gateway                                    | Unavailable                  | Pending / pending      |
| `grokbuild`        | Grok Build CLI gateway                                       | Unavailable                  | Pending / pending      |
| `opencode`         | OpenCode native provider and gateway                         | Unavailable                  | Pending / pending      |
| `openclaw`         | OpenClaw provider registry and gateway                       | Unavailable                  | Pending / pending      |
| `hermes`           | Hermes profile and Chat gateway                              | Unavailable                  | Pending / pending      |
| `pi`               | Pi provider and native gateway                               | Unavailable                  | Pending / pending      |
| `workbuddy`        | WorkBuddy China desktop and CLI connection                   | WorkBuddy account card       | Pending / pending      |
| `workbuddy-ai`     | WorkBuddy International connection                           | Unavailable                  | Pending / pending      |
| `qoder`            | Qoder China desktop connection and gateway                   | Qoder rewards card           | Pending / pending      |
| `doubaowork`       | Doubao native audit adapter, outside the API provider editor | Doubao account card          | Pending / pending      |
| `trae`             | TRAE China bridge and native audit collector                 | TRAE rewards card            | Pending / pending      |
| `trae-intl`        | TRAE International bridge and native audit collector         | Unavailable                  | Pending / pending      |
| `kimi`             | Kimi CLI and desktop provider projection                     | Unavailable                  | Pending / pending      |
| `qwen`             | Qwen CLI provider projection                                 | Unavailable                  | Pending / pending      |
| `zcode`            | ZCode desktop provider projection                            | ZCode rewards card           | Pending / pending      |
| `deepseek-harness` | DeepSeek Harness native provider and gateway                 | Unavailable                  | Pending / pending      |
| `t3code`           | Managed T3 Code Responses connection                         | Unavailable                  | Pending / pending      |

Renderer verification exercises event time for all 20 Agent IDs, protection
enabled and disabled, both event types, and all four locales. The compact
check-in fixtures cover the five existing cards. The provider fixture covers
catalogs that exceed a single page, model filtering and selection.

## November 20 bug-list regression coverage

The Agent picker and key binding status use the following paths on macOS and
Windows. Each platform needs a native self-test in addition to fixture tests.

For the November 23 key-status follow-up, each policy-capable row below reads
the same per-Agent status as the Agents page when a key is bound and the
gateway is running. A failed status read is local to that row. TRAE China and
International can have an active managed route while native starting-model
selection is pending. Binding, authentication, and inference admission still
use the existing per-Agent adapters. The external OpenClaw row has no managed
key status. Synthetic coverage includes both TRAE editions, Gemini, Codex, and
the other policy-capable rows. Native acceptance is tracked separately.

The November 23 gateway publication check covers the same Agent inventory
through its existing native routes. The publication flag controls the external
key-authenticated gateway catalog and inference for Chat, Messages, Responses
and Gemini. The per-Agent adapters and their own key bindings continue to use
the local catalog. Doubao Work's audit integration has no local model route.
The routing page's status filter names the gateway publication state on macOS
and Windows. Loopback gateway tests verify publication changes independently
of native-client acceptance.

Supplier grouping in the VaryAI Agent picker uses the shared catalog for the
applicable Agents in this matrix. Numbered supplier labels follow the same
provider identities as the model labels. The renderer uses those labels for
the provider filter, while selection submits the original model route ID.
Doubao Work uses its native audit path. Per-Agent native picker verification
for this grouping change remains pending on both platforms.

Claude Code gateway discovery sends an authenticated
`GET /connections/claude/v1/models?limit=1000`. This read uses the same
catalog and route aliases as its unparameterized model listing. The loopback
fixture checks query admission and catalog payloads across the shared Agent
connections. Live native picker validation for the updated backend is pending.

| Agent              | Picker entry point and adapter                       | Policy status entry point and runtime                    | Verification |
| ------------------ | ---------------------------------------------------- | -------------------------------------------------------- | ------------ |
| `claude`           | `/model`, Claude Code settings, Messages             | Settings key binding, shared or legacy Messages route    | Pending      |
| `claude-desktop`   | Native Desktop catalog and Messages                  | Settings key binding, Desktop connection or legacy route | Pending      |
| `codex`            | CLI and Desktop model pickers, managed Responses     | Settings key binding, managed or legacy Responses route  | Pending      |
| `gemini`           | `/model`, Gemini settings and native Gemini requests | Settings key binding, shared or legacy Gemini route      | Pending      |
| `grokbuild`        | CLI model profiles and provider-selected protocol    | Settings key binding, shared or legacy route             | Pending      |
| `opencode`         | CLI provider catalog and provider-selected protocol  | Settings key binding, shared connection                  | Pending      |
| `openclaw`         | Native provider and model registry                   | Settings displays external key setup                     | Pending      |
| `hermes`           | CLI provider catalog and Chat                        | Settings key binding, shared connection                  | Pending      |
| `pi`               | CLI model registry and provider-selected protocol    | Settings key binding, shared or native Pi routing        | Pending      |
| `workbuddy`        | China Desktop model registry and Chat                | Settings key binding, shared or native route             | Pending      |
| `workbuddy-ai`     | International Desktop model registry and Chat        | Settings key binding, shared connection                  | Pending      |
| `qoder`            | China Desktop provider and Chat                      | Settings key binding, shared or Qoder gateway            | Pending      |
| `doubaowork`       | Native model picker and separate audit adapter       | Settings key binding, native route readiness             | Pending      |
| `trae`             | China bridge catalog and Chat                        | Settings key binding, China bridge readiness             | Pending      |
| `trae-intl`        | International bridge catalog and Chat                | Settings key binding, international bridge readiness     | Pending      |
| `kimi`             | Native Kimi selection and provider projection        | Settings key binding, shared or native route             | Pending      |
| `qwen`             | CLI model settings and Chat                          | Settings key binding, shared or native route             | Pending      |
| `zcode`            | Desktop model registry and native projection         | Settings key binding, shared or native route             | Pending      |
| `deepseek-harness` | Native model registry and Chat/Responses/Messages    | Settings key binding, shared or native route             | Pending      |
| `t3code`           | Managed Codex instance picker and Responses          | Settings key binding, shared connection                  | Pending      |

## WorkBuddy subscription upstream 400 handling

For every Agent that can target the WorkBuddy China or WorkBuddy AI
subscription upstream, the change is inside the shared native subscription
adapter. Routing entry points, per-Agent protocol adapters and UI state are
unchanged; each Agent inherits the same classification and tool-compatibility
behavior through its existing routing entry point. Upstream error carriers are
classified into fixed codes only and surface in the Agent's client error body;
no application UI state changed.

| Agent              | Entry point and adapter                                        | Runtime behavior inherited                                                                                                                  | Verification                                                                   |
| ------------------ | -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `claude`           | Intent/group routing → shared subscription adapter (unchanged) | Bounded `11128`/`11133` classification, tool-name grammar, schema completions, stream name restoration                                      | Shared adapter fixtures; live subscription verification pending                |
| `claude-desktop`   | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |
| `codex`            | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `gemini`           | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |
| `grokbuild`        | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `opencode`         | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `openclaw`         | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |
| `hermes`           | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `pi`               | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |
| `workbuddy`        | Same shared adapter, China edition (unchanged)                 | Same, plus shared Chat fingerprint, Kimi, tool-result, and content-filter repairs; fixed carriers for 11140, 11153, 11158, 11159, and 14018 | China and international transport fixtures cover both editions; native pending |
| `workbuddy-ai`     | Same shared adapter, international edition (unchanged)         | Same shared Chat repairs, edition-specific User-Agent, growth event, activity reports, and the same fixed numeric carriers                  | China and international transport fixtures cover both editions; native pending |
| `qoder`            | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `doubaowork`       | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |
| `trae`             | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `trae-intl`        | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `kimi`             | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `qwen`             | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Chat-source fixtures cover its wire shape; live verification pending           |
| `zcode`            | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |
| `deepseek-harness` | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Anthropic and Chat source fixtures cover both upstream protocols               |
| `t3code`           | Same shared adapter (unchanged)                                | Same                                                                                                                                        | Shared adapter fixtures; live subscription verification pending                |

The prompt-slicing regression uses the entry point, protocol adapter, and
model selection UI listed for each Agent in the WorkBuddy subscription matrix
above. Both regional Chat upstreams share the affected system and assistant
text processing. The regression checks the outgoing text at the shared
adapter and in loopback transport. China Anthropic Messages uses a separate
preparation path. Doubao Work has no selectable subscription route.
On macOS arm64, Claude Code CLI and the Code tab in Claude Desktop both
received streamed replies through WorkBuddy China after the fix. Native
verification for the other Agents, WorkBuddy AI, and Windows remains pending.

A loopback HTTP fixture sends a named tool through China Chat Completions,
China Anthropic Messages and international Chat Completions. It collects the
stream, renders it as Chat, Messages, Responses and Gemini, and checks the
restored client name and call ID. Other fixtures cover named choices, tool
history, schema conversion and safe error classification. Native acceptance
with each Agent and real subscription on macOS and Windows is pending.

On macOS 26.6.2 arm64 with Rust 1.98.1 and pnpm 10.12.3, the local working
tree over `ff49fdd8` passed all 15 WorkBuddy adapter fixtures, `cargo check`,
Rust formatting, 7,803 library tests (21 ignored), and 165 integration tests. Type checking,
Prettier, the renderer build, all 4,760 renderer tests, the development
helper tests and the production dependency audit passed. Strict Clippy
reported a shared `Template` prefix on the `RoutingTemplate` enum in
concurrent routing changes. Remote platform CI and live Agent checks are pending.

## Full names in the Codex model picker

The change publishes the complete VaryAI model or routing-group label as
Codex picker description metadata after any source and collision suffixes.
The Codex picker owns the row width and displays the description on hover.
Names are presentation metadata. Wire IDs, request adapters, default
selection and historical aliases retain their existing roles.

| Agent ID           | Entry point and adapter                                                     | Picker behavior and verification scope                                                                 |
| ------------------ | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `claude`           | Claude Code / Messages model list                                           | Separate native projection                                                                             |
| `claude-desktop`   | Claude Desktop / Messages model list                                        | Separate native projection                                                                             |
| `codex`            | Desktop and CLI, local and native Responses catalogs, temporary CLI catalog | Full description in published fixtures and the macOS CLI. Desktop tooltip and Windows checks pending   |
| `gemini`           | Gemini CLI / Gemini model registry                                          | Separate native projection                                                                             |
| `grokbuild`        | Grok Build / model profiles                                                 | Separate native projection                                                                             |
| `opencode`         | OpenCode / provider catalog                                                 | Separate native projection                                                                             |
| `openclaw`         | OpenClaw / provider and model registry                                      | Separate native projection                                                                             |
| `hermes`           | Hermes / CLI provider catalog                                               | Separate native projection                                                                             |
| `pi`               | Pi / custom provider registry                                               | Separate native projection                                                                             |
| `workbuddy`        | WorkBuddy China / Desktop registry                                          | Separate native projection                                                                             |
| `workbuddy-ai`     | WorkBuddy AI / Desktop registry                                             | Separate native projection                                                                             |
| `qoder`            | Qoder China / Desktop registry                                              | Separate native projection                                                                             |
| `doubaowork`       | Doubao Work / audit adapter                                                 | Native model picker outside this catalog                                                               |
| `trae`             | TRAE China / bridge catalog                                                 | Separate native projection                                                                             |
| `trae-intl`        | TRAE International / bridge catalog                                         | Separate native projection                                                                             |
| `kimi`             | Kimi Code / provider projection                                             | Separate native projection                                                                             |
| `qwen`             | Qwen Code / CLI model settings                                              | Separate native projection                                                                             |
| `zcode`            | ZCode / Desktop registry                                                    | Separate native projection                                                                             |
| `deepseek-harness` | DeepSeek Harness / native registry                                          | Separate native projection                                                                             |
| `t3code`           | T3 Code / independent Codex instance and Responses                          | Full description in the instance catalog and full name in custom model settings. Native checks pending |

On macOS, Codex CLI 0.171.0 ran against a temporary configuration and
synthetic catalog. Its app-server `model/list` returned the complete
`displayName` and `description` for long Chinese and English labels, including
a numbered provider suffix. The native `/model` picker displayed both
distinguishing suffixes in a 160-column terminal. The running desktop app
was left out of the smoke test.

On the working tree based on `d32c8a68`, macOS 26.6.2 arm64 with Rust 1.98.1,
Node 20.20.2 and pnpm 10.12.3 passed Rust formatting, check, strict Clippy
and 7,817 library tests (21 ignored), along with 4,766 renderer tests, the
renderer build, type checking, formatting, helper tests and the production
dependency audit. The full Cargo run first exhausted disk space. During the
retry the local target directory disappeared and 32 subprocess-dependent
tests reported missing executables. Full integration and remote CI acceptance
remain pending.

## Routing group names and intent classifier responses

The shared Agent picker consumes canonical IDs and catalog labels. Gemini's
native footer reads the configured model ID, and Grok Build's native default
refers to a profile key. Intent classification is shared across executable
group routes. Classifier output, classification-disabled fallback, and
continuation ownership are checked separately from client display. A shared
renderer regression covers installed Tools rows for all 19 picker-capable
Agents while the executable catalog lacks the saved group.

The routing group editor also uses the Agent inventory below for saved model
order. The shared editor opens legacy ranked groups, updates their default
and fallback IDs, and reapplies catalog ranking when requested. Each
executable adapter consumes those saved IDs through the existing group
route. Doubao Work retains its audit-only path. Renderer fixtures cover the
editor and saved order on the shared macOS and Windows code path. Native
verification remains separate for the adapters listed below.

The routing group model count uses the shared workspace table for each
Agent listed below on macOS and Windows. It includes a separately
configured classifier once per supplier and model. The existing Agent
adapters continue to receive the same task member order and classifier
configuration. Renderer fixtures cover the table count.

Verified the count change on the working tree based on `21dceb85`, using
macOS 26.6.2 arm64, Node 20.20.2 and pnpm 10.12.3. TypeScript, Prettier
and the renderer build passed. The full renderer suite passed 4,844 tests,
including the new cases for repeated supplier and model pairs, matched
members and separate suppliers. Installed-client verification is pending.

For each Agent in the matrix below, the Custom routing group's rule editor
uses the same renderer on macOS and Windows. The layout groups the match
conditions, preferred member and ordered fallbacks. The editor continues to
save the same rule fields, which feed the existing native adapter and runtime
listed for each Agent. Doubao Work remains audit-only. Editor verification
covers condition editing, pause rules, disabled rules, keyboard ordering and
saved targets with synthetic fixtures. Native status remains as listed below.

On the working tree based on `21dceb85`, Chromium on macOS 26.6.2 arm64
verified the editor at 544 and 900 px in both themes across Simplified Chinese,
Traditional Chinese, English and Japanese. The fixtures exercise long model
IDs, conditions covering the full Agent inventory, mouse and keyboard rule
ordering, and focus on the intent field after adding a rule. The full renderer
suite passed 4,848 tests with Node 20.20.2 and pnpm 10.12.3 after the concurrent
classifier changes and before the alignment follow-up. Windows WebView2 and
installed-client verification remain pending.

The alignment follow-up reproduced the rules starting at the form's left
edge and the live trial starting in a separate column. At 900 and 544 px,
Chromium verified a common left edge for rules, live trial, offline plan
validation and advanced execution settings, with empty and populated rules.
TypeScript, Prettier, the renderer build and all 136 routing panel tests passed
after the alignment change.

The rules title now uses the same label styling and column as the other
advanced settings. Chromium verified the label's position, font and color,
the adjacent **Add rule** button and the help popover at both widths, for
empty and populated rules. The popover opens by mouse or keyboard and returns
focus to the information button on Escape. All 137 routing panel tests,
TypeScript, Prettier and the renderer build passed after this change.

The Custom rule model picker uses the same renderer as the template picker
for every Agent in the matrix below. It selects existing group member IDs,
including effort variants. Primary, fallback and pause selections still feed
the same saved rule fields and the same native adapters.
Chromium on macOS verified wheel scrolling within a 44-member list at 900 and
544 px, along with search, model replacement and saved fallback order.
The renderer regressions cover retained effort variants and changing the
member selected by a pause rule. On the working tree based on `21dceb85`,
macOS 26.6.2 arm64, Node 20.20.2 and pnpm 10.12.3 passed TypeScript, Prettier,
the renderer build and all 4,849 renderer tests. Windows WebView2 was not
exercised for this change.

The Fastest and intent-rule regressions use the same inventory. Each
executable adapter uses the shared exact-upstream measurements, candidate
pool, continuation ownership and live rule selection. The classifier sees
the latest supported user text through each adapter's canonical request
projection. Codex Responses fixtures also exercise replayed reasoning
summaries. Native acceptance remains pending on both platforms.

The classifier protocol and deadline update applies to each executable Agent
below through the listed adapter and shared runtime. The editor uses the same
provider picker, deadline and trial on macOS and Windows. The classifier's
outbound protocol is independent of the incoming Agent protocol. WorkBuddy
China and international have a dedicated incoming Chat regression for deadline
fallback and easy-rule selection. Doubao Work has no executable intent route.

On macOS 26.6.2 arm64, the working tree based on `21dceb85` passed the
classifier protocol and privacy tests with Rust 1.98.1. Loopback tests used the
WorkBuddy China and international Chat entry points with the same short task
and a classifier that returned after 2,300 ms. A 2,000 ms deadline recorded a
timeout and selected the default model. A 5,000 ms deadline recorded a match
and selected the easy model. Renderer tests covered the same outcomes and
verified that changing a saved deadline survives reopening the editor.

TypeScript, Prettier and the renderer build passed with Node 20.20.2 and
pnpm 10.12.3. The renderer suite passed 4,848 tests. Cargo check, formatting
and Clippy with warnings denied passed. The full Rust suite passed 7,993
tests with 21 ignored. Installed clients and Windows remain pending.

The longer opt-in classifier deadline and Gemini 3 classification effort
apply through the same shared runtime and editor for every executable Agent
below on macOS and Windows. Conversation ownership stays with each adapter's
existing continuation path. Doubao Work has no executable intent route.

The follow-up loopback regression verifies `MINIMAL` on the classifier's
Gemini Native request. In both WorkBuddy editions a synthetic 5,300 ms
classifier response caused a fallback with a 5,000 ms deadline and selected
the easy model with a 10,000 ms deadline. Unknown Gemini models in the
protocol fixture use their upstream thinking default. Renderer fixtures
cover timeout guidance below and at the new 15,000 ms limit.
The updated renderer suite passed 4,849 tests on macOS. Cargo check and
Clippy with warnings denied passed for the follow-up.

New hard/easy drafts use a 10,000 ms classification deadline in the shared
editor for every executable Agent below. The editor regression checks the
default in the draft and the saved configuration. Existing groups keep
their saved deadlines.

The routing tracker follow-up covers the shared group editor, publication
switches and live routing stage for each Agent below on macOS and Windows.
The publisher retains each existing adapter's catalog and request paths.
The native Codex HTTP fixtures check the unavailable-group response,
per-turn priority after a provider returns or the order changes, and
ownership for explicit response continuations. Installed-client acceptance
remains pending for these scenarios.

On the working tree based on `53abd075`, macOS 26.6.2 arm64 with Node
20.20.2, pnpm 10.12.3 and Rust 1.98.1 passed the renderer type check,
formatting, build and 4,849 unit tests. Cargo check, formatting, strict Clippy,
and the Rust library and integration suites passed with 7,994 tests passed
and 21 ignored. The development helper checks and production dependency
audit also passed. Windows and installed-client verification remain pending.

The next routing reports use the same Agent inventory below. The live stage
uses the same trace projection on macOS and Windows. The scheduler shares
cooldown and fallback behavior across Responses, Chat, Messages and Gemini.
Codex Responses has a separate read-only projection check for empty reasoning
items. Chat and Gemini terminal-event fixtures check per-turn routing and
error handling. WorkBuddy and WorkBuddy AI loopback fixtures route a hard
turn and an easy turn to different configured members in the same streaming
conversation. The Codex HTTP fixture checks that the configured summary
member compacts plain history and the original answer member resumes from
the local checkpoint. Other adapters retain their existing request formats.
Installed-client verification remains pending for the new scenarios.

The final macOS run on the working tree based on `53abd075` passed 4,851
renderer tests and 7,995 Rust library and integration tests, with 21 ignored.
An earlier run and an isolated retry failed the existing Pi Gemini tool
observation regression. The regression passed in the final full run.

For the Agent entries in the matrix below, both the editor trial and eligible
live group turns use the same selected classifier transport on macOS and
Windows. Chat classification uses the provider's supported reasoning switch.
An explicit output-limit response has its own fallback reason on each of
the four classifier protocols. Verification for this change uses synthetic
classifier responses and renderer tests. Native Agent acceptance is pending.

For every Agent in the following matrix, the hard/easy and custom editors,
saved classifier target, and live classifier dispatch share the same path
on macOS and Windows. WorkBuddy China and WorkBuddy AI are selectable upstream
accounts independently of the calling Agent. Verification covers both regional
catalogs and the native Chat and Messages transports with synthetic fixtures.
Installed-client and real-account acceptance for this change is pending.

The hard/easy rule descriptions use the same routing group editor for every
Agent below on macOS and Windows. The editor renders built-in descriptions
in the selected language and keeps saved rule text available to each existing
classifier path. Renderer verification exercises the four supported locales
with saved rules. On macOS, type checking, formatting, the renderer build,
and all 4,857 renderer tests passed. Installed WebView verification is pending.

| Agent ID           | Entry point and native adapter                                        | Verification scope                                                                                                                        |
| ------------------ | --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `claude`           | Claude Code CLI, Messages catalog and shared runtime                  | Shared classifier tests. Native pending                                                                                                   |
| `claude-desktop`   | Claude Desktop, Messages catalog and shared runtime                   | Shared classifier tests. Native pending                                                                                                   |
| `codex`            | Codex desktop and CLI, Responses catalogs and shared runtime          | Shared classifier tests and native HTTP fixtures for availability and per-turn priority. Installed clients pending                        |
| `gemini`           | Gemini CLI, dotenv, JSON model definitions, Gemini URL routing        | Alias migration and gateway fixtures passed. macOS CLI footer and synthetic inference passed. Native picker rendering and Windows pending |
| `grokbuild`        | Grok Build CLI, TOML profiles, shared Agent status and picker         | Profile adoption, status and refresh passed with fixtures. Installed client pending                                                       |
| `opencode`         | OpenCode, provider model registry and shared runtime                  | Shared classifier tests. Native pending                                                                                                   |
| `openclaw`         | OpenClaw, provider model registry and shared runtime                  | Shared classifier tests. Native pending                                                                                                   |
| `hermes`           | Hermes, provider model registry and shared runtime                    | Shared classifier tests. Native pending                                                                                                   |
| `pi`               | Pi, custom provider registry and shared runtime                       | Shared classifier tests. Native pending                                                                                                   |
| `workbuddy`        | WorkBuddy China desktop, native model list and shared runtime         | Shared classifier tests and Chat deadline regression passed with loopback fixtures. Native pending                                        |
| `workbuddy-ai`     | WorkBuddy international desktop, native model list and shared runtime | Shared classifier tests and Chat deadline regression passed with loopback fixtures. Native pending                                        |
| `qoder`            | Qoder China desktop, provider registry and shared runtime             | Shared classifier tests. Native pending                                                                                                   |
| `doubaowork`       | Doubao Work desktop, audit-only integration                           | No executable model picker or intent route in this integration                                                                            |
| `trae`             | TRAE China desktop, Chat bridge and shared runtime                    | Shared classifier tests. Native pending                                                                                                   |
| `trae-intl`        | TRAE International desktop, Chat bridge and shared runtime            | Shared classifier tests. Native pending                                                                                                   |
| `kimi`             | Kimi Code CLI, provider model registry and shared runtime             | Shared classifier tests. Native pending                                                                                                   |
| `qwen`             | Qwen Code CLI, JSONC model registry and shared runtime                | Shared classifier tests. Native pending                                                                                                   |
| `zcode`            | ZCode desktop, native model registry and shared runtime               | Shared classifier tests. Native pending                                                                                                   |
| `deepseek-harness` | DeepSeek Harness, CLI model and credentials registry                  | Shared classifier tests. Native pending                                                                                                   |
| `t3code`           | T3 Code desktop, independent Codex backend and shared runtime         | Shared classifier tests. Native pending                                                                                                   |

The projection and classifier paths are shared on macOS and Windows.
Gemini CLI 0.67.0 on macOS displayed and sent the ASCII alias for a Chinese
group using temporary configuration and a loopback upstream. The generated
model definitions retain the original label. Passing a literal Chinese
model ID directly breaks this CLI version's HTTP User-Agent construction.
The exact reported Codex upstream rejection, Fastest selection and GLM
classification request were unavailable in the local routing history.
The GLM recording shows Codex set to High reasoning and includes a short
follow-up in an existing conversation. The recording does not expose the
classifier result or continuation decision.

The Gemini model-label follow-up applies to the Gemini row above on macOS and
Windows. It uses managed dotenv and model definitions, the existing alias map
and the Gemini URL ingress. Other Agent entries above keep their existing
model projection and protocol paths. Regression coverage includes qualified
Unicode names, duplicate names, previous aliases, key policy checks, and
restoration of existing environment fields. Native macOS
and Windows acceptance of the VaryAI integration remains pending.

Verification on the working tree over `d32c8a68`, macOS 26.6.2 arm64,
Rust 1.98.1, Node 20.20.2 and pnpm 10.12.3. The renderer build, TypeScript,
Prettier, all 4,771 renderer tests, 137 CI helper tests and the production
pnpm audit passed. Cargo formatting, check, strict Clippy, 7,821 library tests
and 165 integration tests passed. The Rust suite reported 21 ignored library
tests. Remote CI and native Windows acceptance are pending.

Before committing, the branch fast-forwarded to `6c2c0e9f`. The base tree
and the pending patch were identical across the fast-forward. The remaining
development checks, `pnpm test:agent-rewards` and
`python3 -B scripts/tests/recover-shared-v23.test.py`, passed with 13 and 5
tests respectively.

The ranked editor follow-up was verified on the working tree based on
`868cf24b` on macOS 26.6.2 arm64 with Rust 1.98.1, Node 20.20.2 and
pnpm 10.12.3. The legacy capability fixture reopens the saved group, moves
a member that is missing from the current catalog, checks the default and
fallback IDs sent on save, and reopens the edited order. The recommendation
fixture restores the same priority after a manual swap between tied models.
TypeScript, Prettier, the renderer build and all 4,785 renderer tests passed.
The Cargo library tests filtered by `intent` passed 588 tests with two
ignored. The recorded Fastest and custom-rule requests still need live
reproduction. Native Windows verification remains pending.

The next routing follow-up compared the Fastest report with the saved
group shown in its recording. The standalone Gemini measurement and the
selected Gemini member used different upstreams. A synthetic three-member
loopback fixture checks the saved order before measurements, distinct
upstreams that share a model ID, and the fifth eligible request rotating
to a member with first-content timing and no output rate. The original
single-candidate trace does not show why the other members were absent.

The Codex Responses loopback fixture runs translation and arithmetic in
one named conversation and verifies different rule targets. The classifier
receives only the current user text. A request carrying a known prior
response ID retains the upstream that owns the response. Focused fixtures
also reject encrypted or incomplete reasoning history, and reject actual
or malformed tool fields in classifiers' output.

Verified the updated working tree based on `868cf24b` on macOS 26.6.2
arm64 with Rust 1.98.1. Cargo formatting, check and strict Clippy passed.
The full Cargo test command passed 7,825 library and 165 integration tests
with 21 library tests ignored. The Agent coverage inventory test passed.
The reported upstreams and native Windows clients still need live
verification.

## First-run sign-in and automatic check-in settings

The first-run wizard receives the confirmed account ID and uses each
provider's own model catalog. This shared flow also supports ChatGPT API,
GitHub Copilot, and Antigravity. The daily check-in settings validate the
selected account and persist its preference. The renderer retains the last
reward observation until the next refresh. These paths are shared by macOS
and Windows, with Qoder's native account adapter on macOS and browser
authorization available on both platforms. The entries with only the shared
wizard have no automatic check-in adapter.

| Agent ID           | Entry point and adapter                                                                        | Verification scope                                                                       |
| ------------------ | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `claude`           | Claude Code CLI, Claude subscription                                                           | Claude first-run fixture passed. Installed client pending                                |
| `claude-desktop`   | Claude Desktop, shared first-run wizard                                                        | Shared first-run fixtures passed. Installed client pending                               |
| `codex`            | Codex desktop and CLI, Codex browser and device sign-in                                        | Codex browser, device and first-run fixtures passed. Installed client pending            |
| `gemini`           | Gemini CLI, Gemini CLI subscription                                                            | Gemini CLI first-run fixture passed. Installed client pending                            |
| `grokbuild`        | Grok Build CLI, Grok device sign-in                                                            | Grok device and first-run fixtures passed. Installed client pending                      |
| `opencode`         | OpenCode, shared first-run wizard                                                              | Shared first-run fixtures passed. Installed client pending                               |
| `openclaw`         | OpenClaw, shared first-run wizard                                                              | Shared first-run fixtures passed. Installed client pending                               |
| `hermes`           | Hermes, shared first-run wizard                                                                | Shared first-run fixtures passed. Installed client pending                               |
| `pi`               | Pi, shared first-run wizard                                                                    | Shared first-run fixtures passed. Installed client pending                               |
| `workbuddy`        | WorkBuddy China desktop, WorkBuddy subscription and existing account-scoped preference command | First-run and check-in renderer fixtures passed. Installed client pending                |
| `workbuddy-ai`     | WorkBuddy international desktop, WorkBuddy AI subscription                                     | WorkBuddy AI first-run fixture passed. Installed client pending                          |
| `qoder`            | Qoder China desktop, native macOS and browser reward accounts                                  | Native and browser identity fixtures and renderer tests passed. Installed client pending |
| `doubaowork`       | Doubao Work desktop, native account list and account-scoped preference                         | Check-in renderer and account preference fixtures passed. Installed client pending       |
| `trae`             | TRAE China desktop, native account identity through the rewards bridge                         | Identity IPC fixture and check-in renderer tests passed. Installed client pending        |
| `trae-intl`        | TRAE International desktop, shared first-run wizard                                            | Shared first-run fixtures passed. Installed client pending                               |
| `kimi`             | Kimi Code CLI, shared first-run wizard                                                         | Shared first-run fixtures passed. Installed client pending                               |
| `qwen`             | Qwen Code CLI, shared first-run wizard                                                         | Shared first-run fixtures passed. Installed client pending                               |
| `zcode`            | ZCode desktop, native rewards credentials and account identity                                 | Native identity fixtures and check-in renderer tests passed. Installed client pending    |
| `deepseek-harness` | DeepSeek Harness CLI, shared first-run wizard                                                  | Shared first-run fixtures passed. Installed client pending                               |
| `t3code`           | T3 Code desktop, shared first-run wizard                                                       | Shared first-run fixtures passed. Installed client pending                               |

Verified the working tree based on `868cf24b` on macOS 26.6.2 arm64 with
Rust 1.98.1, Node 20.20.2, and pnpm 10.12.3. `pnpm typecheck`,
`pnpm build:renderer`, `pnpm format:check`, and `pnpm test:unit` passed.
The renderer suite passed 4,783 tests. `pnpm test:agent-rewards` and
`pnpm test:trae-bridge` passed 14 and 73 tests respectively. Cargo
formatting, check, strict Clippy, and the full Cargo test command passed
against the `src-tauri` manifest. Cargo reported 7,821 library tests and
165 integration tests passed with 21 library tests ignored. The original
reporter's subscription accounts and the installed Windows clients still
need live verification.

## Codex browser ambient context (`browser_tabs`), Chat tool media, and routing diagnostics

2026-11-21

Codex desktop injects unsolicited `agent.browser_tabs` (`function_call_output`
without `call_id`) notifications alongside `codex_apps_open_page` and
`codex_apps_selected_page_part_<N>_of_<M>`, and browser/screenshot tools return
multimodal `function_call_output` arrays (`input_text` + `input_image`, or
`input_image` / `input_file` alone). Across macOS and Windows,
`is_unsolicited_page_context`, the IR Responses parser, the legacy Codex-to-Chat
and Codex-to-Anthropic converters, routed DLP history classification, and Intent
tool-continuation tracking now recognize `browser_tabs` as user-level context,
while the IR Chat builder extracts `Part::Image` and `Part::File` from
`Part::ToolResult` into a trailing `role: "user"` message after contiguous
`role: "tool"` string replies. Intent conversion/forwarding rejection logs also
emit safe fixed-label diagnostics (`Conversion rejected`, `Upstream rejected
converted call`, and `Upstream rejected forwarded call`) that bypass the log
exporter's `request:` payload redaction regex without logging raw payloads or
secrets. All changes are platform-independent across macOS and Windows.

| Agent ID           | Entry point and adapter                                                                                        | Verification scope                                                                                    |
| ------------------ | -------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `claude`           | Claude Code CLI (Anthropic `/v1/messages` ingress; IR Chat tool-result media extraction and Intent logging)    | Synthetic IR Anthropic-to-Chat media extraction (`tool_result` + mixed text) passed. Native pending   |
| `claude-desktop`   | Claude Desktop (Anthropic `/v1/messages` ingress; shared IR Chat builder and Intent logging)                   | Covered by shared Anthropic IR builder regressions. Installed client pending                          |
| `codex`            | Codex desktop and CLI (`browser_tabs` context, Responses-to-Chat tool media, DLP admission, Intent logging)    | Synthetic IR, HTTP proxy edge, legacy Chat/Anthropic, DLP, and Intent fixtures passed. Native pending |
| `gemini`           | Gemini CLI (Gemini ingress; shared Intent conversion/forwarding diagnostics and IR Chat builder)               | Shared Intent and 12-edge IR routing regressions passed. Installed client pending                     |
| `grokbuild`        | Grok Build CLI (Responses ingress/classic handlers, IR builder, and Intent rejection hints)                    | Shared IR Responses edge and Intent rejection hint fixtures passed. Installed client pending          |
| `opencode`         | OpenCode (Chat/Responses/Anthropic IR routing and Intent rejection logging)                                    | Shared IR routing and diagnostics regressions passed. Installed client pending                        |
| `openclaw`         | OpenClaw (Chat/Anthropic IR routing and Intent rejection logging)                                              | Shared IR routing and diagnostics regressions passed. Installed client pending                        |
| `hermes`           | Hermes (Chat/Anthropic/Responses IR routing and Intent rejection logging)                                      | Shared IR routing and diagnostics regressions passed. Installed client pending                        |
| `pi`               | Pi (Responses/Chat/Anthropic IR routing and Intent rejection logging)                                          | Shared IR routing and diagnostics regressions passed. Installed client pending                        |
| `workbuddy`        | WorkBuddy China desktop (shared IR Chat builder, `completed_tool_history`, and Intent rejection logging)       | Shared IR and Intent unit regressions passed. Installed client pending                                |
| `workbuddy-ai`     | WorkBuddy international desktop (shared IR Chat builder, `completed_tool_history`, and Intent logging)         | Shared IR and Intent unit regressions passed. Installed client pending                                |
| `qoder`            | Qoder China desktop (shared Intent routing and diagnostics export redaction)                                   | Shared diagnostics and IR regressions passed. Installed client pending                                |
| `doubaowork`       | Doubao Work desktop (shared Intent routing and diagnostics export redaction)                                   | Shared diagnostics and IR regressions passed. Installed client pending                                |
| `trae`             | TRAE China desktop (shared Intent routing and diagnostics export redaction)                                    | Shared diagnostics and IR regressions passed. Installed client pending                                |
| `trae-intl`        | TRAE International desktop (shared Intent routing and diagnostics export redaction)                            | Shared diagnostics and IR regressions passed. Installed client pending                                |
| `kimi`             | Kimi Code CLI (shared Chat/Anthropic IR builder and Intent rejection logging)                                  | Shared IR and Intent regressions passed. Installed client pending                                     |
| `qwen`             | Qwen Code CLI (shared Chat/Responses IR builder and Intent rejection logging)                                  | Shared IR and Intent regressions passed. Installed client pending                                     |
| `zcode`            | ZCode desktop (shared Intent routing and diagnostics export redaction)                                         | Shared diagnostics and IR regressions passed. Installed client pending                                |
| `deepseek-harness` | DeepSeek Harness CLI (shared Chat/Responses IR builder and Intent rejection logging)                           | Shared IR and Intent regressions passed. Installed client pending                                     |
| `t3code`           | T3 Code desktop (shared Intent routing and diagnostics export redaction)                                       | Shared diagnostics and IR regressions passed. Installed client pending                                |

Verified the working tree based on `a8d0e03e` on macOS 15.3.1 (`24D70`)
`arm64` with `rustc 1.98.1 (48a229cea 2026-09-01)`, Node `v24.11.0`, and pnpm
`10.12.3`. `cargo fmt --check --manifest-path src-tauri/Cargo.toml`,
`cargo check --manifest-path src-tauri/Cargo.toml`,
`cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`,
`cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml` (with
`NO_PROXY=127.0.0.1,localhost` to bypass an inactive host system proxy on
`127.0.0.1:7897`), and `pnpm format:check` all passed. Cargo reported 7,849
library tests passed with 21 ignored and 165 integration tests passed. Live
verification with native Codex desktop, live Chat upstream gateways, and
Windows clients remains pending.

## Conversation-only DLP scope and password-assignment regex (macOS and Windows)

Request-side DLP scanning, redaction, tokenization, and final-payload validation
inspect conversational `user` and `assistant` text only (plus Responses
`prompt` and `reasoning` summary/content) by default across Chat, Anthropic,
Gemini, and Responses protocols. System/developer instructions
(`system`, `developer`, `instructions`, `systemInstruction` /
`system_instruction`), tool-call arguments (`tool_calls`, `tool_use`,
`functionCall`, `function_call`, `custom_tool_call`), and tool results
(`tool`, `tool_result`, `functionResponse`, `function_call_output`,
`custom_tool_call_output`) are excluded from request DLP inspection while
retaining structural history-pairing checks.

The `catalog:password-assignment` built-in rule excludes `>` as the first
character of a matched secret (`[^\s>,\[\]"'}][^\s,\[\]"'}]*`) so `=>`
assignments (for example `Password => "[REDACTED]"`) do not backtrack to `=`
and fail final validation with `400 dlp_redaction_failed`.

| Agent IDs                                                                                                                                                                                                    | Entry point and adapter                                                | Verification scope                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `claude`, `claude-desktop`, `codex`, `gemini`, `grokbuild`, `opencode`, `openclaw`, `hermes`, `pi`, `workbuddy`, `workbuddy-ai`, `qoder`, `doubaowork`, `trae`, `trae-intl`, `kimi`, `qwen`, `zcode`, `deepseek-harness`, `t3code` | Shared Chat, Anthropic, Gemini, and Responses DLP adapters and `extractor` | Shared macOS synthetic HTTP and adapter regressions; native macOS/Windows pending |
