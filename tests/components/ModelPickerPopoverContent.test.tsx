import { expect, it } from "vitest";
import { modelPickerPlacement } from "@/components/models/modelPickerPlacement";

it("opens below a high trigger and leaves eight pixels to the window edge", () => {
  expect(
    modelPickerPlacement({
      trigger: { left: 700, top: 247, bottom: 277, width: 196 },
      viewport: { width: 980, height: 740 },
      width: 380,
      height: 560,
    }),
  ).toEqual({ side: "bottom", maxHeight: 447, originX: 138 });
});

it("opens above a low trigger instead of shifting over it", () => {
  expect(
    modelPickerPlacement({
      trigger: { left: 700, top: 620, bottom: 650, width: 196 },
      viewport: { width: 980, height: 740 },
      width: 380,
      height: 560,
    }),
  ).toEqual({ side: "top", maxHeight: 560, originX: 138 });
});

it("leaves the title bar clear and points the arrow at the trigger's leading side", () => {
  expect(
    modelPickerPlacement({
      trigger: { left: 8, top: 200, bottom: 230, width: 196 },
      viewport: { width: 390, height: 260 },
      width: 374,
      height: 560,
      topInset: 58,
    }),
  ).toEqual({ side: "top", maxHeight: 134, originX: 30 });
});

it("keeps a short list below when it fits even if there is more room above", () => {
  const placed = modelPickerPlacement({
    trigger: { left: 300, top: 450, bottom: 480, width: 50 },
    viewport: { width: 980, height: 740 },
    width: 380,
    height: 180,
  });
  expect(placed.side).toBe("bottom");
  expect(placed.originX).toBe(35);
});

it("uses the Agent menu's height limit when choosing the side", () => {
  expect(
    modelPickerPlacement({
      trigger: { left: 300, top: 550, bottom: 580, width: 196 },
      viewport: { width: 1120, height: 1060 },
      width: 490,
      height: 900,
      maxHeight: 420,
    }),
  ).toMatchObject({ side: "bottom", maxHeight: 420 });
});
