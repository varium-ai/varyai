# Tray Panel

The tray uses the independent `index.html?panel=tray` renderer, with Agent,
Routing and Security tabs. It must not render the main workspace sidebar.

Treat this surface as a quick panel: use a compact segmented tab control and
uniform Agent rows with an icon, name, current model and disclosure button.
Scope compact layout overrides under `.tray-panel .tray-agents` so workspace
mobile rules cannot change row padding or move controls. Keep version numbers
and update actions in expanded Agent details; leave setup instructions and
long-conversation settings in the main workspace. On opening, focus the panel
container; keyboard navigation must still expose a visible focus indicator on
tabs and controls.

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
theme storage key. Keep its cards, tabs, footer and strategy controls free of
outline borders; use subtle row separators and fills to distinguish controls.
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
