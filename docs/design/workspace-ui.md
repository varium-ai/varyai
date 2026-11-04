# Workspace UI Reference

The workspace follows the supplied 1050 x 724 Figma export. This is a
presentation-only adaptation, not a change to the reference parity worklist or
native client support.

- Use a `#f3f6fa` canvas, translucent light sidebar, white cards, `#dbe3eb`
  borders and `#3278ff` active controls. Keep semantic warning and error colors.
- Keep the desktop sidebar at 184 px with 10 px horizontal insets, 28 px
  navigation rows and 6 px between rows. Use the same width token for the native
  drag-region background; narrow windows retain a 52 px icon rail. Keep the brand
  icon at 32 px, navigation at 14 px, page headings at 22 px and Agent names at
  15 px. Use 15 px Lucide command, shuffle, component, column chart, scan eye and
  settings icons with a consistent 1.75 px stroke and an 8 px gap before
  each label. Keep icons unfilled; they inherit the label color and have no
  background tile.
- The Tools / Agent page uses an 18 px title without an introductory subtitle.
  Keep its opaque header at the top of the main scroll area while the update
  notice and tool lists scroll beneath it. Preserve the platform title-bar inset
  and narrow-screen spacing; the compact tray view has no page header.
- Keep installed versions and update controls beside the Agent name, with the
  connection status on the next line. Starting-model menus use the Magpie
  reference's full-width search row, narrow icon rail, compact group headings,
  source labels and trailing selection check. Omit reasoning-range badges and
  image capability icons. Keep recovery notes below the scrollable list and
  preserve the disconnect confirmation. Desktop visibility retains its own
  show/hide, ordering and future-model controls in the shared picker component.
- TRAE has one starting-model picker in its main row. Keep the read-only shared
  catalog in expanded details beside the bridge explanation and current status,
  including disconnected and pending states in both workspace and tray views.
- Keep tool icons centered in transparent 28 px containers without colored
  background tiles. Use inset list dividers, 12 px card corners and
  36 x 20 px switches. Keep switch tracks pill-shaped in dialogs by excluding
  `role="switch"` controls from ordinary button rounding. Preserve recognizable
  vendor logos.
- The Routing account-visibility control masks email addresses and known account
  logins with blurred stand-in text in labels, menus and request history. Keep
  vendor names, model names and brand icons recognizable; do not rename providers
  to numbered accounts. Tooltips use masked text too. Toggling visibility never
  changes the saved provider or routing configuration.
- Keep the workspace routing stage as a centered inline flow: a compact Agent
  node with its icon above its name, thin connecting wires, a 48 px circular Vary
  icon and small candidate labels. Use a simple robot outline for the generic
  Agent and retain vendor logos for identified tools. Avoid large source tiles
  and a full-width empty candidate panel. Keep
  the requested model, listener and strategy available to assistive technology
  and in existing hover descriptions. Preserve all candidate branches, status
  indicators, quota observations and packet animation. Narrow stages retain the
  horizontal flow with a 40 px hub and truncate long labels. Scope these styles
  to the workspace stage so the compact tray keeps its own dimensions.
- Place the installation disclosure in its own card with its existing hint.
  Do not change installation, update, connection, model selection or recovery
  handlers to reproduce static example content from the design.
- Use paired navigation labels for Tools/Agent, Plans/Routing and Models/Providers.
  Localize both resource terms while retaining Agent as the shared product term.
  Keep Statistics and Security Center as separate destinations.
  Keep native window controls; do not draw fake controls from export bitmaps.
- Adapt to icon-only navigation on narrow windows and retain a blue-gray dark
  theme. Respect reduced motion and visible keyboard focus.
- Shared modal dialogs scale from 95% to 100% with a 220 ms fade-in, then
  shrink to 95% with a 160 ms fade-out. Fade the backdrop on the same schedule.
  Keep centered and fullscreen geometry unchanged. Radix Presence owns removal
  after the exit animation. Center ordinary dialogs with fixed zero insets,
  automatic margins and a content-sized height; animate only scale and opacity.
  Do not use percentage translation to position the surface. Form autofocus
  must use `preventScroll: true` both on opening and after asynchronous loading.
  Subscription provider editors load their account fields before opening, so
  a short loading form cannot turn into a taller dialog during the entrance.
  Failed loads leave the list visible with an error. Fullscreen dialogs use
  their own geometry. Keep the dialog root and its draft mounted until
  `onCloseAutoFocus` runs. Do not replace controlled closure with conditional
  unmounting. Disable both animations for `prefers-reduced-motion: reduce`.
- Use `AppDialog` for standard confirmations, notices and forms. It composes the
  shared Radix primitives with a title, optional description, scrollable body and
  action footer; compact, medium and large widths are 420, 520 and 680 px. Keep
  20 px horizontal insets, 12 px corners, a 15 px title, 13 px body text and 32 px
  action buttons. Use one continuous surface without tinted header/footer bands
  or heavy dividers. Backdrops use 24% blue-gray dimming and 2 px blur; dark mode
  increases dimming to 48%. Destructive actions keep their red confirmation.
  The wrapper focuses the surface without scrolling, restores the opener after
  exit, retains nullable content through the exit animation, and blocks Escape
  and outside dismissal while pending. Form dialogs can request an initial field
  through `initialFocusRef`. Specialized editors retain their custom bodies and
  consume the same surface, backdrop, title and footer primitives.
- Use one decorative sidebar selection layer. Move it vertically over 260 ms
  only when the controlled destination changes, including Settings. Resize it
  with the active row and switch instantly when reduced motion is requested.
- Open tool details in place from the row or its keyboard-accessible Details
  control, including disconnected and recovery states. Keep one row open at a
  time. Keep individual settings and applicable long-conversation controls in
  the details. Turning off the connection switch requests disconnection and
  restoration of the original configuration through the existing confirmation
  and recovery guards. Omit the duplicate disconnect-and-restore button in both
  workspace and compact tray details. Omit the official website button,
  connection-instructions block and duplicate model ID. When no actions are
  available, start directly with the settings without an empty action row or
  extra divider; hide empty detail panels. Connection, model,
  installation and update controls do not toggle the disclosure. Preserve the
  restore confirmation and recovery guards.
- Put a six-dot drag handle before each tool icon in the main list. Only the
  handle starts sorting. Reveal it when its row is hovered or keyboard focus
  enters the row. Position it within the card's leading inset instead of
  reserving a separate layout column, so hovering never shifts the content. Touch
  devices keep the handle visible. Support pointer dragging and Space / arrow keys /
  Space for keyboard sorting, with Escape to cancel. Save the tool order in
  local storage and retain undetected tools' positions across rescans. Sorting
  keeps the open details attached to their tool and never changes a connection.
  The compact tray view keeps its existing connected-first grouping and omits
  drag handles.
- The Models / Providers list uses the same handles and drag surface. Keep the
  handle inside the row's leading padding, leaving only a narrow gap before the
  icon. Dragging uses an inert visual copy in a portal with the same width and
  content geometry, including expanded details. Strip duplicate element IDs and
  accessibility references from the copy; do not mount another live component.
  Keep the preview opaque and rounded with a quiet inset edge and shadow. The
  source row becomes a subdued placeholder while siblings make room. Suppress
  the source divider and constrain movement to the list. Hovering and starting
  a drag must never shift the icon or controls horizontally.
  Store provider order locally without rewriting provider records or connection
  settings. Enabled and switched-off providers sort within their own sections.
  Reordering filtered results replaces only their slots in the saved order;
  hidden providers keep their positions, and new providers follow saved entries.
- Place Add provider at the upper right of the Models / Providers header,
  including narrow layouts. Open vendor and relay selection as a separate page
  within the same workspace destination. Back restores list search, fold state,
  scroll position and button focus. Canceling a provider editor returns to
  selection; successful saves and imports return to the provider list.
- Keep WorkBuddy and Doubao Work daily check-in and account controls inside
  their Agent details, available while disconnected. Opening details only reads
  account status; claiming rewards, adding an account and enabling automatic
  check-in retain their explicit actions.

Shared tokens and Agent styles live in
`src/components/layout/workspace.css`. Provider and routing styles consume the
same palette. The font stack prefers Inter and Noto Sans SC when available and
uses installed platform fonts otherwise, without remote font requests.

Verify with synthetic catalogs and client states in an isolated renderer.
Check desktop and 390 px layouts, menu focus, the installation disclosure,
provider forms and routing selection. Do not launch a native client or write
its configuration for visual acceptance.
