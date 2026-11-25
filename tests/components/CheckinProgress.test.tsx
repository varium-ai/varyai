import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import {
  CheckinProgress,
  WorkBuddyCheckinProgress,
  QoderCheckinProgress,
} from "@/components/CheckinProgress";

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

it("shows campaign totals and actual dates with gaps and today on the right", () => {
  render(
    <WorkBuddyCheckinProgress
      totalDays={7}
      day="2026-11-16"
      checkinDates={[
        "2026-11-16",
        "2026-11-13",
        "2026-11-12",
        "2026-11-11",
        "2026-11-05",
        "2026-11-04",
        "2026-11-03",
      ]}
    />,
  );
  expect(screen.getByText("dailyCheckin.campaignTotal")).toBeVisible();
  expect(screen.queryByText("dailyCheckin.streak")).not.toBeInTheDocument();
  expect(screen.getByText("dailyCheckin.days 7")).toBeVisible();
  const days = screen.getAllByRole("listitem");
  expect(days.map((day) => day.dataset.date)).toEqual([
    "2026-11-10",
    "2026-11-11",
    "2026-11-12",
    "2026-11-13",
    "2026-11-14",
    "2026-11-15",
    "2026-11-16",
  ]);
  expect(days.map((day) => day.dataset.state)).toEqual([
    "pending",
    "complete",
    "complete",
    "complete",
    "pending",
    "pending",
    "complete",
  ]);
  expect(days[6]).toHaveAttribute("aria-current", "date");
  expect(days[6]).toHaveTextContent("dailyCheckin.today");
});

it("does not infer dates from a saved total and crosses month and year boundaries", () => {
  render(
    <WorkBuddyCheckinProgress
      totalDays={3}
      day="2027-01-02"
      checkinDates={null}
    />,
  );
  const days = screen.getAllByRole("listitem");
  expect(days[0]).toHaveAttribute("data-date", "2026-12-27");
  expect(days[6]).toHaveAttribute("data-date", "2027-01-02");
  expect(days.every((day) => day.dataset.state === "unknown")).toBe(true);
});

it("distinguishes an empty official history from unavailable history", () => {
  render(
    <WorkBuddyCheckinProgress
      totalDays={0}
      day="2026-03-02"
      checkinDates={[]}
    />,
  );
  const days = screen.getAllByRole("listitem");
  expect(days[0]).toHaveAttribute("data-date", "2026-02-24");
  expect(days.every((day) => day.dataset.state === "pending")).toBe(true);
});

it("summarizes only Qoder confirmations within the displayed reward windows", () => {
  render(
    <QoderCheckinProgress
      day="2026-11-16"
      confirmedDates={["2026-11-01", "2026-11-16", "2026-11-16"]}
    />,
  );
  expect(screen.getByText("agentRewards.qoderConfirmed")).toBeVisible();
  expect(screen.getByText("dailyCheckin.days 1")).toBeVisible();
  expect(
    screen.getByRole("list", { name: "dailyCheckin.calendar" }),
  ).toBeVisible();
  const info = screen.getByLabelText("agentRewards.qoderCalendarHint");
  expect(info.closest("span")).toHaveAttribute(
    "title",
    "agentRewards.qoderCalendarHint",
  );
  expect(
    screen.queryByText("agentRewards.qoderCalendarHint"),
  ).not.toBeInTheDocument();
  const days = screen.getAllByRole("listitem");
  expect(days.map((day) => day.dataset.state)).toEqual([
    "unknown",
    "unknown",
    "unknown",
    "unknown",
    "unknown",
    "unknown",
    "complete",
  ]);
  expect(days[6]).toHaveTextContent("agentRewards.currentWindow");
});
