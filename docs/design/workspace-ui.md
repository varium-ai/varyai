# Workspace UI Reference

The workspace follows the supplied 1050 x 724 Figma export. This is a
presentation-only adaptation, not a change to the reference parity worklist or
native client support.

- Use a `#f3f6fa` canvas, translucent light sidebar, white cards, `#dbe3eb`
  borders and `#3278ff` active controls. Keep semantic warning and error colors.
- Keep the desktop sidebar at 184 px, brand icon at 32 px, navigation at 14 px,
  page headings at 22 px and Agent names at 15 px.
- Use 28 px pale-blue icon tiles, inset list dividers, 12 px card corners and
  36 x 20 px switches. Preserve recognizable vendor logos.
- Place the installation disclosure in its own card with its existing hint.
  Do not change installation, update, connection, model selection or recovery
  handlers to reproduce static example content from the design.
- Preserve localized labels, including the concise navigation destinations.
  Keep native window controls; do not draw fake controls from export bitmaps.
- Adapt to icon-only navigation on narrow windows and retain a blue-gray dark
  theme. Respect reduced motion and visible keyboard focus.
- Shared modal dialogs scale from 95% to 100% with a 220 ms fade-in, then
  shrink to 95% with a 160 ms fade-out. Fade the backdrop on the same schedule.
  Keep centered and fullscreen geometry unchanged. Radix Presence owns removal
  after the exit animation; keep the dialog root and its draft mounted until
  `onCloseAutoFocus` runs. Do not replace controlled closure with conditional
  unmounting. Disable both animations for `prefers-reduced-motion: reduce`.
- Use one decorative sidebar selection layer. Move it vertically over 260 ms
  only when the controlled destination changes, including Settings. Resize it
  with the active row and switch instantly when reduced motion is requested.
- Open tool details in place from the row or its keyboard-accessible Details
  control, including disconnected and recovery states. Keep one row open at a
  time. Show instructions, the official website, individual settings and
  disconnect-and-restore actions inside the details instead of an extra
  ellipsis menu. Connection, model, installation and update controls do not
  toggle the disclosure. Preserve the restore confirmation and recovery guards.
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
