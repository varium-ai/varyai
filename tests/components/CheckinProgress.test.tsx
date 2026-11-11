import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { CheckinProgress } from "@/components/CheckinProgress";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number }) =>
      options?.count == null ? key : `${key} ${options.count}`,
  }),
}));

it("shows only the consecutive days reported by the service as complete", () => {
  render(<CheckinProgress streak={3} />);
  const days = screen.getAllByRole("listitem");
  expect(days).toHaveLength(7);
  expect(days.filter((day) => day.dataset.state === "complete")).toHaveLength(
    3,
  );
  expect(days.filter((day) => day.dataset.state === "pending")).toHaveLength(4);
  expect(screen.getByText("dailyCheckin.days 3")).toBeVisible();
});

it("does not invent a streak or history when the service supplies none", () => {
  render(<CheckinProgress streak={null} />);
  expect(
    screen
      .getAllByRole("listitem")
      .every((day) => day.dataset.state === "unknown"),
  ).toBe(true);
  expect(screen.getByText("dailyCheckin.noData")).toBeVisible();
});

it("keeps long streaks intact while limiting the progress display to seven steps", () => {
  render(<CheckinProgress streak={12} />);
  expect(screen.getByText("dailyCheckin.days 12")).toBeVisible();
  expect(screen.getAllByRole("listitem")).toHaveLength(7);
  expect(
    screen
      .getAllByRole("listitem")
      .every((day) => day.dataset.state === "complete"),
  ).toBe(true);
});
