# Workspace UI Reference

The workspace follows the supplied 1050 x 724 Figma export. This is a
presentation-only adaptation, not a change to the reference parity worklist or
native client support.

- Match the supplied `151_16756` export's light surfaces: a 94% opaque
  `#f3f6fa` canvas, 40% white sidebar and 65% white list cards. List borders use
  65% `#d7e1ec`, with 70% dividers and `#637184` secondary text. Keep
  `#3278ff` active controls, semantic warning/error colors and opaque dark
  surfaces with readable borders.
- Use the shared `--mp-hover` token for neutral button hover surfaces: opaque
  white in light mode and blue-gray in dark mode. Apply it to workspace toolbar
  actions, provider choices, row actions and shared ghost, secondary and outline
  buttons. Keep the sidebar's existing hover treatment and keyboard focus
  indicators.
- Keep the desktop sidebar at 184 px with 12 px horizontal insets, 28 px
  navigation rows and 8 px between rows. Use the same width token for the native
  drag-region background; narrow windows retain a 52 px icon rail. Use the
  existing 1024 px brand source rendered at 32 px in the sidebar, with a 12 px gap before the
  17 px / 22 px semibold name. Keep 12 px before navigation. Navigation labels
  use 14 px / 20 px type, regular weight for inactive items and medium weight
  for the active item, 7 px corners and 10 px horizontal padding. Use 16 px
  Lucide icons with a 1.75 px stroke and a 10 px gap before each label.
  Preserve the existing menu entries, icons, labels, focus behavior and
  selection animation.
  Do not use the export's 32 px raster for the brand: Retina displays require
  a source larger than its CSS display size.
- The Tools / Agent page uses an 18 px title without an introductory subtitle.
  Keep its opaque header at the top of the main scroll area while the update
  notice and tool lists scroll beneath it. Place page content 24 px below the
  window top. On macOS, use a 24 px drag region with no extra page inset and a
  24 px brand inset beneath it, preserving the brand's vertical position.
  Preserve narrow-screen spacing; the compact tray view has no page header.
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
- Keep tool and provider icons at 32 px without colored background tiles; compact
  tray icons remain 22 px. Use 14 px / 22 px medium-weight row titles and
  12 px / 18 px secondary lines separated by 2 px. Standard desktop rows are
  60 px high before wrapping or expansion. Use inset list dividers, 10 px card
  corners and 36 x 20 px switches. Keep switch tracks pill-shaped in dialogs by excluding
  `role="switch"` controls from ordinary button rounding. Preserve recognizable
  vendor logos.
- The Routing account-visibility control masks email addresses and known account
  logins with blurred stand-in text in labels, menus and request history. Keep
  vendor names, model names and brand icons recognizable; do not rename providers
  to numbered accounts. Tooltips use masked text too. Toggling visibility never
  changes the saved provider or routing configuration.
- Keep the workspace routing stage as an inline flow with small outer insets: a compact Agent
  node with its icon above its name, thin connecting wires with 56–112 px gaps
  between nodes (20 px in narrow stages), a 48 px circular Vary
  icon and a candidate list that fills the remaining width. Candidate rows show
  the configured supplier icon, account name and model chip above a persistent status line.
  Resolve icons by the recorded supplier ID using the same icon rules as the provider list;
  missing or removed suppliers use a neutral box. Use a
  neutral dark border and solid dot for the active seat, an outlined dot for
  standby seats, and a slim quota bar when available. Keep rows at least 60 px
  tall with 7 px gaps, bounded scrolling and readable narrow layouts. Use a simple robot outline for the generic
  Agent and retain vendor logos for identified tools. Avoid large source tiles
  and a full-width empty candidate panel. Keep
  the requested model, listener and strategy available to assistive technology
  and in existing hover descriptions. Preserve all candidate branches, status
  indicators, quota observations and packet animation. Narrow stages retain the
  horizontal flow with a 40 px hub and truncate long labels. Scope these styles
  to the workspace stage so the compact tray keeps its own dimensions.
  While recorded requests are routing or streaming, show two subtle rings expanding
  from the gateway border on 2.4-second cycles staggered by 1.2 seconds. Stop when
  all displayed requests finish; selected static requests and replay remain still.
  Reduced motion omits both rings.
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
- Keep the main Tools list flat: omit Details buttons and row expansion in
  disconnected, connected and recovery states. Models, visibility, connection,
  installation, update and recovery controls stay directly in each row.
  Long-conversation settings remain available in Settings regardless of tool
  installation status. The compact tray retains its accessible expansion for
  model and connection controls, versions and individual setup. Turning off the
  connection switch requests disconnection and
  restoration of the original configuration through the existing confirmation
  and recovery guards. Omit the duplicate disconnect-and-restore button in both
  workspace rows and compact tray details. Omit the official website button,
  connection-instructions block and duplicate model ID. Tray connection, model,
  installation and update controls do not toggle its disclosure. Preserve the
  restore confirmation and recovery guards.
- Put a six-dot drag handle before each tool icon in the main list. Only the
  handle starts sorting. Reveal it when its row is hovered or keyboard focus
  enters the row. Position it within the card's leading inset instead of
  reserving a separate layout column, so hovering never shifts the content. Touch
  devices keep the handle visible. Support pointer dragging and Space / arrow keys /
  Space for keyboard sorting, with Escape to cancel. Save the tool order in
  local storage and retain undetected tools' positions across rescans. Sorting
  keeps controls attached to their tool and never changes a connection.
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
  Omit the selection page title. Keep Back at the left of a single toolbar and
  align vendor search, Import and Custom provider to the right. Keep Custom
  provider last and show its API compatibility hint on hover instead of in a
  footer. Allow the toolbar to wrap on narrow layouts so its actions stay visible.
- Keep WorkBuddy and Doubao Work daily check-in and account controls directly
  below their main rows and inside compact tray details, available while
  disconnected. Loading the controls only reads account status; claiming rewards,
  adding an account and enabling automatic check-in retain their explicit actions.

Shared tokens and Agent styles live in
`src/components/layout/workspace.css`. Provider and routing styles consume the
same palette. The font stack prefers Inter and Noto Sans SC when available and
uses installed platform fonts otherwise, without remote font requests.

Verify with synthetic catalogs and client states in an isolated renderer.
Check desktop and 390 px layouts, menu focus, the installation disclosure,
provider forms and routing selection. Do not launch a native client or write
its configuration for visual acceptance.
