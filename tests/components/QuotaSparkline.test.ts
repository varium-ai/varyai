import { expect, it } from "vitest";
import {
  quotaCurve,
  windowSeconds,
} from "@/components/providers/QuotaSparkline";
it("recognizes real window spans and rejects unknown or unbounded windows", () => {
  expect(windowSeconds("five_hour")).toBe(18000);
  expect(windowSeconds("Gemini Pro · 7d")).toBe(604800);
  expect(windowSeconds("30_day")).toBe(2592000);
  expect(windowSeconds("999999999h")).toBeNull();
  expect(windowSeconds("unknown")).toBeNull();
});
it("draws observed points from the current cycle without inventing missing history", () => {
  const point = { at: 150, remaining: 60, resetsAt: 300, span: 300 };
  expect(quotaCurve([], point)).toBe("M100.00,12.00");
  expect(
    quotaCurve([{ ...point, at: 30, remaining: 90, resetsAt: 200 }], point),
  ).toBe("M100.00,12.00");
  expect(quotaCurve([{ ...point, at: 30, remaining: 90 }], point)).toBe(
    "M20.00,3.00 L100.00,12.00",
  );
});
it("starts a new line after a reset or allowance grant", () => {
  const point = { at: 150, remaining: 90, resetsAt: 300, span: 300 };
  expect(quotaCurve([{ ...point, at: 30, remaining: 40 }], point)).toBe(
    "M100.00,3.00",
  );
});
