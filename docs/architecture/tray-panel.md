# Tray Panel

The tray uses the independent `index.html?panel=tray` renderer, with Agent,
Allowances, Usage and Security tabs. It must not render the main workspace sidebar.

Usage follows Magpie's compact usage view. The toolbar offers Today, 7 days,
30 days and All, a provider picker, refresh and Open Usage. The last action
opens workspace statistics. A saved Routing selection opens Usage. The four
summary cells show cache-inclusive tokens, requests, effective cost and
prompt cache hits. The stacked trend and ranked list switch together between
tokens, cost and requests. Selecting a provider displays its model breakdown.
Long model names are truncated with an ellipsis so the metric values and bars
stay inside the card. Hover over a truncated name to read it in full.
Hover or focus a chart column to see its date or hour, colored breakdown,
total and request count.

The picker, ranking and chart resolve supplier names from local provider
metadata, following Magpie's
[`providerFaces`](https://github.com/yetone/magpie/blob/14b6b955563cbca042c6c5d4e6e5efd7d122be48/internal/usageapi/usageapi.go#L140)
display-name lookup. The global routing catalog resolves historical details
and rollups by execution identity, including disabled and renamed providers.
The default native `ChatGPT / Codex` entry displays as `Codex`. Custom names,
Agent-specific supplier names and local session labels retain their identities.
Unrecognized suppliers retain their recorded IDs.

Select a day on a daily trend to narrow the totals and ranking. The heatmap
follows Magpie's full Usage calendar. It shows the last 53 week columns from
Monday through the current local day, with month and weekday labels. Its
scrolling area starts at the latest week and keeps weekday labels visible.
The heatmap keeps its own token, request and cost preference, uses the
selected provider and remains visible when the selected period is empty
but the calendar contains activity. The five shades use quartiles when at
least eight days have positive values and a proportional scale otherwise.
Hover or focus a day for its date and totals. The arrow keys move between days
and weeks.

The tab refreshes its aggregate ledger view every 10 seconds while visible.
The toolbar refresh reads both the selected period and the activity heatmap.
It confirms completion even if totals are unchanged, or reports a read failure
so the user can retry. Background refreshes do not show these notifications.
It combines existing gateway and imported session usage, applies ledger
deduplication and includes older daily rollups. Local policy blocks with the
recorded block marker are excluded from detail aggregates. Archived rollups
carry totals only, so historical policy blocks and individual timing or
price observations cannot be separated. The tray query leaves storage and
forwarding unchanged. Only the period, metric and grouping preferences are
saved in browser storage.

Allowances follows Magpie's compact provider/account cards: provider icon and plan,
account labels, one ring per supported quota window, named reset times, and a
per-card refresh. The provider header and rings switch between used and remaining
percentages, defaulting to remaining when no preference has been saved. Preserve
explicit used/remaining choices and decimal percentages (96.8% remaining is 3.2%
used). Keep independent model windows visible even when their remainder is zero;
they must not be dropped or replaced by the family's shared allowance. Only that
display preference is saved in local storage. The panel uses the existing
provider/account query keys and their last successful snapshots,
without storing account or quota responses in browser local storage. Read only
the accounts selected for the saved provider. Failed refreshes retain the last
reading with a stale status; login-required, unsupported and failed observations
must not become zero allowance. Relay balances use the existing API allowance
reader; direct API vendors without a supported reader are omitted. Load this
content only while its tab is selected and the panel is active. Refresh reads all
quota query families; card refresh reads only its account/provider. Main-window
navigation from this tab opens Providers. Keep the four-tab indicator and keyboard
wrapping based on the tab count, and preserve the fixed native panel height.

Quota card styling follows Magpie's `panelQuotaCard` and `quotaSpark` at
[`58f24cc`](https://github.com/yetone/magpie/tree/58f24cc23527776b5f1958cfd4a8092a659bdfb5/internal/gui/assets).
Use 34-point rings with 3-point strokes, compact two-line pool labels, and amber
for nearly exhausted windows. Keep the complete pool name and reset timestamp
in each ring's tooltip. Show short local reset times in the account subtitle;
unused windows say so instead of filling the subtitle with future deadlines.
Up to four rings sit beside the account's two text rows. More rings use the
card's full width below the account information and wrap from the left on narrow
screens, keeping every independent model window visible.

Subscription cards' 30-point sparkline shows the first window's current cycle,
as Magpie's tray does. Its solid indigo line contains only observed readings;
the faint dashed diagonal is an even-consumption reference. Shade the observed interval green
above that diagonal and amber below it. A single reading is a small round stroke
without a filled band or invented history. Named pools resolve their own window
suffixes, including `Gemini · five_hour`. Small deadline drift must not erase
history: use Magpie's five-minute tolerance and rolling-window handling, breaking
the line at a genuine reset or allowance grant. History failures remain auxiliary
and must not remove the current allowance reading. These components and styles
are shared by the macOS and Windows renderers.

Relay API cards show their balance, supported window rings and refresh control
without a quota sparkline. Do not mount history readers or record renderer
samples for these cards, even when a window has a reset timestamp and earlier
samples exist. This keeps balance cards compact and avoids an unexplained
consumption reference line under an unused daily allowance.

Relay API keys use the read-only `/v1/usage`, then `/api/usage/token` probe.
The Providers list omits API allowances entirely and does not mount their query
observers, including for a newly saved copy. Subscription rows retain their
existing allowance summaries. API allowance cards remain available in the tray.

Interface discovery follows Magpie's origin-and-key cache rather than provider
IDs: copies using the same origin and credential share discovery, including
concurrent requests. Cache a found interface for 24 hours, a missing interface
for six hours, and a temporary discovery failure for five minutes. Found and
missing interfaces survive restart in the device-local
`~/.varyai/relay-balance-probes.json`; temporary discovery failures do not. Store
only a digest of the origin and credential, a fixed interface identifier, and
expiry. Do not persist balances, credentials or endpoint URLs. Cache read/write
failures remain auxiliary. This cache and private atomic replacement run on both
macOS and Windows without database schema changes.

An unsuccessful first discovery hides the card and heading. Explicit credential
rejection hides the tray card. An already discovered interface still reports query
failures; a failed or unrecognized refresh after a successful reading keeps that
reading with a stale status. An explicit key rejection clears the previous
balance. Each successful observation reads a fresh balance at the discovered
interface; it does not reuse another provider's balance snapshot.

Renderer observations remain keyed by provider ID and revision. Hidden relay
observers stay mounted while the active quota tab is open, polling again at the
backend's remaining discovery interval. Card and header refresh explicitly
bypass discovery expiry, so a missing interface can recover immediately. When
every candidate is unsupported, show the normal empty state. Closing the active
quota tab stops its polling. These changes align discovery and list presentation;
they do not add Magpie's configurable Balance URL or Auto-detect editor controls.

The native context menu contains Open quick panel, Open main window, Official
website and Quit, with one separator before Quit. It does not expose a
lightweight-mode toggle in any supported language.

Treat this surface as a quick panel: use a compact segmented tab control and
uniform Agent rows with an icon, name, current model and disclosure button.
Place the Agent list in one opaque white card with an 11-point radius, a subtle
border and full-width row dividers. Use a matching dark card in dark mode.
Keep 10-point side insets, 38-point collapsed rows, 22-point icons and 14-point
names. Align muted, single-line model labels and disclosure controls on the
right; truncate long names and model labels without widening the panel.
Keep the unconfigured section inside the same card with an inset label.
Agent rows and their disclosure buttons do not change color on hover.
The expanded More tools list uses the same card, row height, icon size and name
alignment. Keep installation and website actions on the right, with the rescan
action below the card. Its summary stays transparent on hover; only its muted
text and chevron change to the primary text color.
Scope compact layout overrides under `.tray-panel .tray-agents` so workspace
mobile rules cannot change row padding or move controls. Keep version numbers
and update actions in expanded Agent details; leave setup instructions and
long-conversation settings in the main workspace. On opening, focus the panel
container; keyboard navigation must still expose a visible focus indicator on
tabs and controls.
An expanded daily check-in card spans the compact Agent row and uses the
card's narrow layout at the available width.

Use the shared `--mp-hover` surface for neutral buttons and clickable rows,
including header and footer
actions. It is opaque white in light mode and dark blue-gray in dark mode.
Keep selected and expanded surfaces, menu highlights and switch tracks distinct.
The selected tab uses an opaque white indicator in light mode and a brighter
translucent surface in dark mode. Tabs do not change their background or text
color on hover. Preserve the selected indicator and visible keyboard focus.
The security protection card and recent security events use opaque white backgrounds in
light mode. Keep the surrounding panel translucent and preserve dark-mode
surfaces, selection states and hover feedback.

Show the header's update entry only when an available version has been detected.
Keep it absent while checking with no known update and when already up to date.
Clicking the entry reopens the available-update dialog.

Read recent security events from page 1 with a page size of 8; the backend rejects
page 0. Distinguish loading, an empty history and event-query failures. A failed
history read must not imply that protection is disabled. The header's refresh
action retries the event query as well as the protection and gateway status.
Render event timestamps from Unix milliseconds in the user's local time and
the current application language.

Keep the panel at a fixed 520-point height, reduced only to fit the screen's
available work area. Header, tabs and footer stay in place while `.tray-body`
scrolls. Do not resize the native window after query updates, tab switches or
expanded Agent details. The legacy resize IPC entry point still enforces this
fixed geometry.

Place the allowance or security details action in the footer immediately
before Quit. Keep the profile switcher on the left and actions on the right
for Agent, Allowances and Security. Usage provides its details action in the
toolbar and shows Quit in the footer.

The footer's compact profile menu is a 232-point frosted popover with a 34-point
search field and 30-point list items. It must remain inside the webview and scroll
its list independently when many profiles exist. Empty lists have no redundant
section separator. Scope these styles to `.tray-profile-menu`, since the shared
profile switcher also appears in the main workspace.

Keep panel switches at 30 by 18 points, including the Agent and Security tabs.
Use the shared portalled Radix Select for the Usage provider menu. The menu
fits inside the webview and consumes Escape before the quick panel dismisses.

Agent, Allowances and Security follow the application's frosted theme with a
62% tint, 40-pixel backdrop blur and 115% saturation. Usage uses neutral solid
surfaces for the chart and ranking. Portalled menus inherit the matching theme
tokens. Theme changes from other windows must update an already-open panel
through the shared theme storage key. Keep the shared tabs and footer free of outline
borders; the Agent list has one subtle outer border, with row separators and
fills distinguishing its contents.
The renderer's backdrop filter also provides a browser preview.
Reduced-transparency preferences use solid tints.

The macOS panel has a clear, non-opaque native background and a transparent
WebView. A native `NSVisualEffectView` with the Popover material and BehindWindow
blending blurs the desktop and windows beneath it. Do not force a light or dark
appearance on the material; it inherits the panel's appearance from the Tauri host.
Its active state is independent of application activation. The glass and retained
WebView are siblings inside a common container. Clip the common container to the
same 12-point radius as `.tray-panel`. Both child views must follow panel size changes.
Tauri's `macos-private-api` feature enables WebKit background transparency on macOS;
CSS blur alone cannot sample other application windows. Keep `app.macOSPrivateApi`
enabled in the Tauri configuration so its CLI preserves the required Cargo feature
during build preparation. Other platforms use an opaque native-window fallback
that follows theme changes to keep the foreground readable.

On macOS, native status-item access requires an AppKit `MainThreadMarker`.
Acquire it inside the main-thread callback passed to `with_inner_tray_icon`,
then pass it to `NSStatusItem::button`. Do not send a main-thread marker
through a callback that requires `Send`.

Screen placement uses native AppKit points. The status-item window determines
the screen and its visible work area; screen changes preserve the fixed height
when space allows.
The non-activating panel takes keyboard focus without activating the unrelated
main workspace. Pure geometry tests cover negative secondary-screen origins,
fixed height, bounded small-screen geometry and tray-edge clamping. They do not establish
live multi-monitor or full-screen Space acceptance.

Renderer events from `useTauriEvent` contain the payload directly, not an event
envelope. Resolve enabled connection IDs against `APP_IDS` before using them
as typed profile scopes; unknown backend IDs must not enter profile actions.

Use `cargo check` and isolated tests for verification. Do not start another
native application instance or restart the user's clients as a routine check.

### First-party API allowances

The tray and provider editor query the first enabled API key on a saved provider. The backend selects the reader from its effective HTTPS origin. Fixed first-party billing hosts serve ModelScope, Mistral key identification and OpenCode Go. Requests reject redirects and bound the response size. Each reported currency or credit unit stays separate, including zero. The tray hides a provider until it has an amount or quota window to display.

| Provider | Reading |
| --- | --- |
| DeepSeek | Each currency in `GET /user/balance` |
| Moonshot and StepFun | Available account balance in the configured region's currency |
| OpenRouter | Account credits minus usage, subject to the key's permissions |
| AiHubMix | A limited key's remaining balance |
| ModelScope | Available Magicubes, excluding frozen units |
| Zhipu and Z.ai | Available account balance or GLM Coding Plan windows |
| Kimi Code, MiniMax Coding Plan, OpenCode Go | Reported coding allowance windows |
| Ollama Cloud | Purchased credits and included allowance or legacy session and weekly limits |
| Command Code | Remaining credits and reported five-hour and weekly limits |
| Mistral | The monthly API or Vibe budget identified for this key |
| Novita AI | Available balance in USD, converted from the API's billing units |

The API-key inventory follows Magpie's [balance readers](https://github.com/yetone/magpie/blob/593ea437d8dbd0d6bbc7b9f0acd722f23bfe81e6/internal/provider/balance.go) and [plan readers](https://github.com/yetone/magpie/blob/593ea437d8dbd0d6bbc7b9f0acd722f23bfe81e6/internal/provider/planquota.go). Novita comes from Vary AI's existing balance integration. SiliconFlow retired its balance endpoint. OpenCode Zen exposes workspace credit through its console. AiHubMix's account-wide reader requires a separate console token. These configurations have no automatic API-key balance in the tray. The test fixtures exercise provider response contracts independently of live account access.
