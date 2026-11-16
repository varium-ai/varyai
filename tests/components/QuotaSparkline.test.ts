import { expect, it } from "vitest";
import {
  quotaCurve,
  quotaGap,
  windowSeconds,
} from "@/components/providers/QuotaSparkline";
it("recognizes real window spans and rejects unknown or unbounded windows", () => {
  expect(windowSeconds("five_hour")).toBe(18000);
  expect(windowSeconds("Gemini Pro · 7d")).toBe(604800);
  expect(windowSeconds("Gemini · five_hour")).toBe(18000);
  expect(windowSeconds("Claude & GPT · seven_day")).toBe(604800);
  expect(windowSeconds("30_day")).toBe(2592000);
  expect(windowSeconds("999999999h")).toBeNull();
  expect(windowSeconds("unknown")).toBeNull();
});
it("draws observed points from the current cycle without inventing missing history", () => {
  const point = { at: 150, remaining: 60, resetsAt: 300, span: 300 };
  expect(quotaCurve([], point)).toBe("M100.00,12.00");
  expect(
    quotaCurve([{ ...point, at: -120, remaining: 90, resetsAt: 0 }], point),
  ).toBe("M100.00,12.00");
  expect(quotaCurve([{ ...point, at: 30, remaining: 90 }], point)).toBe(
    "M20.00,3.00 L100.00,12.00",
  );
});
it("starts a new line after a reset or allowance grant", () => {
  const point = { at: 150, remaining: 90, resetsAt: 300, span: 300 };
  expect(quotaCurve([{ ...point, at: 30, remaining: 40 }], point)).toBe(
    "M0.00,3.00",
  );
});
it("keeps the observed line when a reset deadline drifts by a few seconds", () => {
  const point = { at: 9000, remaining: 60, resetsAt: 18000, span: 18000 };
  expect(
    quotaCurve([{ ...point, at: 1800, remaining: 90, resetsAt: 17995 }], point),
  ).toBe("M20.00,3.00 L100.00,12.00");
});
it("starts at an early refill without connecting the preceding cycle", () => {
  const point = { at: 10800, remaining: 80, resetsAt: 18000, span: 18000 };
  const points = [
    { ...point, at: 1800, remaining: 40 },
    { ...point, at: 9000, remaining: 100 },
  ];
  expect(quotaCurve(points, point)).toBe("M0.00,0.00 L40.00,6.00");
});
it("shades only the observed interval against the even burn", () => {
  const point = { at: 9000, remaining: 60, resetsAt: 18000, span: 18000 };
  expect(quotaGap([], point)).toBeNull();
  expect(quotaGap([{ ...point, at: 1800, remaining: 90 }], point)).toBe(
    "20.00,3.00 100.00,12.00 100.00,15.00 20.00,3.00",
  );
  expect(quotaGap([{ ...point, at: 1800, remaining: 20 }], point)).toBeNull();
});
it("does not join expired cycles or include future and invalid observations", () => {
  const point = { at: 9000, remaining: 60, resetsAt: 18000, span: 18000 };
  expect(
    quotaCurve(
      [
        { ...point, at: -1800, resetsAt: 0, remaining: 80 },
        { ...point, at: 1800, remaining: 90 },
        { ...point, at: 5000, remaining: NaN },
        { ...point, at: 12000, remaining: 40 },
      ],
      point,
    ),
  ).toBe("M20.00,3.00 L100.00,12.00");
});
