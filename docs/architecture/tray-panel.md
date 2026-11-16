# Tray Panel

The tray uses the independent `index.html?panel=tray` renderer, with Agent,
Allowances, Routing and Security tabs. It must not render the main workspace sidebar.

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
rejection remains visible. An already discovered interface still reports query
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

Use the shared `--mp-hover` surface for neutral buttons and clickable rows,
including header actions, routing controls and footer
actions. It is opaque white in light mode and dark blue-gray in dark mode.
Keep selected and expanded surfaces, menu highlights and switch tracks distinct.
The selected tab uses an opaque white indicator in light mode and a brighter
translucent surface in dark mode. Tabs do not change their background or text
color on hover. Preserve the selected indicator and visible keyboard focus.
Routing request cards, routing group lists, strategy controls, the security
protection card and recent security events use opaque white backgrounds in
light mode. Keep the surrounding panel translucent and preserve dark-mode
surfaces, selection states and hover feedback.

Show the header's update entry only when an available version has been detected.
Keep it absent while checking with no known update and when already up to date.
Clicking the entry reopens the available-update dialog.

Read recent security events from page 1 with a page size of 8; the backend rejects
page 0. Distinguish loading, an empty history and event-query failures. A failed
history read must not imply that protection is disabled. The header's refresh
action retries the event query as well as the protection and gateway status.

Keep the panel at a fixed 520-point height, reduced only to fit the screen's
available work area. Header, tabs and footer stay in place while `.tray-body`
scrolls. Do not resize the native window after query updates, tab switches or
expanded Agent details. The legacy resize IPC entry point still enforces this
fixed geometry.

Place the current tab's routing or security details action in the footer,
immediately before Quit. Keep the profile switcher on the left and the actions
on the right. The Agent tab has no details action.

The footer's compact profile menu is a 232-point frosted popover with a 34-point
search field and 30-point list items. It must remain inside the webview and scroll
its list independently when many profiles exist. Empty lists have no redundant
section separator. Scope these styles to `.tray-profile-menu`, since the shared
profile switcher also appears in the main workspace.

Routing uses the stage's compact layout: keep the Agent, gateway and upstreams
in explicit left-to-right columns, with a 32-point gateway and a bounded stage
height. Leave 20–32 points between nodes and cap the upstream label at 148
points instead of stretching it across the remaining space. Truncate long
provider and model labels while retaining their full hover descriptions.
Do not apply the workspace stage's narrow layout in the tray. An empty
candidate message is a request status, not an upstream; never draw a wire to it.
Request indicators distinguish live traffic, successful replies, errors and
cancellations. HTTP error statuses must not appear as successful replies.

Routing groups use flat 44-point rows with a single-line, ellipsized name and a
muted model count. Keep strategy triggers at 24 points tall and panel switches
at 30 by 18 points, including the Agent and Security tabs. Use the shared Radix
Select with a compact, portalled menu instead of a native HTML select. The menu
must fit inside the webview, support keyboard selection and consume Escape before
the quick panel is dismissed. Generated groups retain their protected switches.

The panel follows the application theme: white frosted glass with dark text in
light mode and charcoal glass with bright text in dark mode. Both use a 62% tint,
40-pixel backdrop blur and 115% saturation. Keep fills translucent so background
colors still diffuse through the panel. Strategy menus use a 76% tint and
32-pixel blur, and portalled menus inherit the matching theme tokens. Theme
changes from other windows must update an already-open panel through the shared
theme storage key. Keep its tabs, footer and strategy controls free of outline
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
