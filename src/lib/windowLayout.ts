const HEADER_HEIGHT = 64;

export function windowLayout(
  isMac: boolean,
  useAppWindowControls: boolean,
  showSecondaryHeader: boolean,
) {
  const dragBarHeight = useAppWindowControls ? 32 : 0;
  return {
    dragBarHeight,
    contentTop: dragBarHeight + HEADER_HEIGHT * (showSecondaryHeader ? 2 : 1),
    macTitlebar: isMac,
  };
}
