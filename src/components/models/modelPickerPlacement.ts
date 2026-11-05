// Model-list placement from the reference implementation (MIT).
// See third_party/routing-reference/LICENSE.
export function modelPickerPlacement({
  trigger,
  viewport,
  width,
  height,
  topInset = 8,
  maxHeight = 560,
}: {
  trigger: Pick<DOMRect, "left" | "top" | "bottom" | "width">;
  viewport: { width: number; height: number };
  width: number;
  height: number;
  topInset?: number;
  maxHeight?: number;
}) {
  const below = Math.max(0, viewport.height - trigger.bottom - 16);
  const above = Math.max(0, trigger.top - 8 - topInset);
  const side =
    Math.min(maxHeight, height) > below && above > below ? "top" : "bottom";
  const left = Math.max(
    8,
    Math.min(trigger.left - 10, viewport.width - width - 8),
  );
  return {
    side,
    maxHeight: Math.min(maxHeight, side === "top" ? above : below),
    originX: Math.max(
      18,
      Math.min(
        width - 18,
        trigger.left + Math.min(trigger.width, 60) / 2 - left,
      ),
    ),
  } as const;
}
