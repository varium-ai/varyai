import { selectAppOption } from "../utils/appSelect";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { DoubaoWorkCheckinPanel } from "@/components/DoubaoWorkCheckinPanel";
import {
  doubaoCheckinApi,
  type DoubaoCheckinStatus,
} from "@/lib/api/doubao-checkin";

vi.mock("@/lib/api/doubao-checkin", () => ({
  doubaoCheckinApi: {
    accounts: vi.fn(),
    checkIn: vi.fn(),
    setAutoEnabled: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { day?: string }) =>
      options?.day ? `${key}: ${options.day}` : key,
  }),
}));
const initial: DoubaoCheckinStatus = {
  connected: true,
  day: "2026-11-03",
  outcome: "not_checked",
  accounts: [
    {
      id: "account-a",
      label: "Account A",
      current: true,
      autoEnabled: false,
      outcome: "not_checked",
      checkedAt: null,
      credit: null,
      streak: null,
      totalDays: null,
    },
    {
      id: "account-b",
      label: "Account B",
      current: false,
      autoEnabled: false,
      outcome: "not_checked",
      checkedAt: null,
      credit: null,
      streak: null,
      totalDays: null,
    },
  ],
};
function mount() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <DoubaoWorkCheckinPanel />
    </QueryClientProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "doubaowork.checkin.title" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "dailyCheckin.manage" }));
}
beforeEach(() => {
  vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue(initial);
});

it("shows confirmed daily records without a current native account and disables mutations", async () => {
  vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
    ...initial,
    connected: false,
    outcome: "connection_required",
    accounts: [{ ...initial.accounts[0], current: false, outcome: "claimed" }],
  });
  mount();
  await screen.findByText("doubaowork.checkin.outcomes.claimed");
  expect(
    screen.getByText(/doubaowork.checkin.outcomes.connection_required/),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", {
      name: /^dailyCheckin\.(todayReward|todayChecked|claimedReward|claimed)$/,
    }),
  ).toBeDisabled();
  expect(screen.getByRole("switch")).toBeDisabled();
  expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
});

it("lists accounts without claiming and offers an explicit switch action for the selected account", async () => {
  mount();
  const select = await screen.findByRole("combobox", {
    name: "doubaowork.checkin.account",
  });
  await waitFor(() => expect(select).toBeEnabled());
  expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
  expect(
    screen.getByRole("button", { name: "doubaowork.checkin.checkNow" }),
  ).toBeEnabled();
  selectAppOption(select, "account-b");
  const disclosure = screen.getByRole("button", {
    name: "doubaowork.checkin.title",
  });
  fireEvent.click(disclosure);
  expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
  fireEvent.click(disclosure);
  expect(screen.getByRole("combobox")).toHaveValue("account-b");
  expect(screen.getByText("doubaowork.checkin.switchHint")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "doubaowork.checkin.switchAndCheckin" }),
  ).toBeEnabled();
  vi.mocked(doubaoCheckinApi.setAutoEnabled).mockResolvedValue({
    ...initial,
    accounts: initial.accounts.map((a) =>
      a.id === "account-b" ? { ...a, autoEnabled: true } : a,
    ),
  });
  fireEvent.click(screen.getByRole("switch"));
  await waitFor(() =>
    expect(doubaoCheckinApi.setAutoEnabled).toHaveBeenCalledWith(
      "account-b",
      true,
    ),
  );
  vi.mocked(doubaoCheckinApi.checkIn).mockResolvedValue({
    ...initial,
    accounts: initial.accounts.map((account) => ({
      ...account,
      current: account.id === "account-b",
      outcome: account.id === "account-b" ? "claimed" : "not_checked",
    })),
  });
  const switchButton = screen.getByRole("button", {
    name: "doubaowork.checkin.switchAndCheckin",
  });
  await waitFor(() => expect(switchButton).toBeEnabled());
  fireEvent.click(switchButton);
  await screen.findByText("doubaowork.checkin.outcomes.claimed");
  expect(doubaoCheckinApi.checkIn).toHaveBeenCalledWith("account-b");
});

it("claims only the selected account and displays a confirmed result", async () => {
  vi.mocked(doubaoCheckinApi.checkIn).mockResolvedValue({
    ...initial,
    accounts: initial.accounts.map((a) =>
      a.current
        ? { ...a, outcome: "claimed", credit: 200, streak: 4, totalDays: 9 }
        : a,
    ),
  });
  mount();
  const button = await screen.findByRole("button", {
    name: "doubaowork.checkin.checkNow",
  });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  await screen.findByText("doubaowork.checkin.outcomes.claimed");
  expect(doubaoCheckinApi.checkIn).toHaveBeenCalledWith("account-a");
  expect(button).toBeDisabled();
  expect(screen.getByText("dailyCheckin.todayReward")).toBeVisible();
  expect(screen.getByText("dailyCheckin.days")).toBeVisible();
  expect(screen.getByText("doubaowork.checkin.totalDays")).toBeVisible();
  expect(
    screen
      .getAllByRole("listitem")
      .filter((day) => day.dataset.state === "complete"),
  ).toHaveLength(4);
});

it("refreshes a previously completed reward without claiming or switching accounts", async () => {
  mount();
  await screen.findByText("doubaowork.checkin.outcomes.not_checked");
  vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
    ...initial,
    accounts: initial.accounts.map((account) =>
      account.current
        ? {
            ...account,
            outcome: "already_checked",
            credit: 200,
            streak: 1,
            totalDays: 1,
          }
        : account,
    ),
  });
  fireEvent.click(
    screen.getByRole("button", { name: "doubaowork.checkin.reload" }),
  );
  await screen.findByText("doubaowork.checkin.outcomes.already_checked");
  expect(screen.getByText("dailyCheckin.todayReward")).toBeVisible();
  expect(screen.getByText("dailyCheckin.days")).toBeVisible();
  selectAppOption(screen.getByRole("combobox"), "account-b");
  expect(
    screen.queryByText("dailyCheckin.todayReward"),
  ).not.toBeInTheDocument();
  expect(screen.queryByText("dailyCheckin.days")).not.toBeInTheDocument();
  expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
});

it.each(["claimed", "already_checked"] as const)(
  "disables Doubao claims for %s and enables the next day after refresh",
  async (outcome) => {
    vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
      ...initial,
      accounts: initial.accounts.map((account) => ({ ...account, outcome })),
    });
    mount();
    const completed = await screen.findByRole("button", {
      name: /^dailyCheckin\.(todayReward|todayChecked|claimedReward|claimed)$/,
    });
    expect(completed).toBeDisabled();
    fireEvent.click(completed);
    expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
    vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
      ...initial,
      day: "2026-11-04",
    });
    fireEvent.click(
      screen.getByRole("button", { name: "doubaowork.checkin.reload" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "doubaowork.checkin.checkNow" }),
      ).toBeEnabled(),
    );
    expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
  },
);

it("displays the computer's local date instead of the Beijing service day", async () => {
  const localDate = vi
    .spyOn(Date.prototype, "toLocaleDateString")
    .mockReturnValue("local-calendar-date");
  try {
    mount();
    expect(await screen.findByText("local-calendar-date")).toBeInTheDocument();
    expect(localDate).toHaveBeenCalledWith();
    expect(screen.queryByText(/2026-11-03/)).not.toBeInTheDocument();
  } finally {
    localDate.mockRestore();
  }
});

it("keeps the observed reward and updates only the account whose preference was saved", async () => {
  vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
    ...initial,
    accounts: [
      { ...initial.accounts[0], outcome: "already_checked", credit: 200 },
      initial.accounts[1],
    ],
  });
  vi.mocked(doubaoCheckinApi.setAutoEnabled).mockResolvedValueOnce({
    ...initial,
    autoConfigured: true,
    accounts: [
      { ...initial.accounts[0], autoEnabled: true },
      initial.accounts[1],
    ],
  });
  mount();
  await screen.findByText("doubaowork.checkin.outcomes.already_checked");
  const toggle = screen.getByRole("switch", {
    name: "doubaowork.checkin.autoLabel",
  });
  fireEvent.click(toggle);
  await waitFor(() => {
    expect(toggle).toBeChecked();
    expect(toggle).toBeEnabled();
  });
  expect(doubaoCheckinApi.setAutoEnabled).toHaveBeenCalledWith(
    "account-a",
    true,
  );
  expect(
    screen.getByText("doubaowork.checkin.outcomes.already_checked"),
  ).toBeVisible();
  selectAppOption(screen.getByRole("combobox"), "account-b");
  expect(toggle).not.toBeChecked();
});

it("shows progress while saving the automatic check-in preference and restores it on failure", async () => {
  let fail!: (reason: Error) => void;
  vi.mocked(doubaoCheckinApi.setAutoEnabled).mockImplementationOnce(
    () =>
      new Promise((_, reject) => {
        fail = reject;
      }),
  );
  mount();
  const toggle = await screen.findByRole("switch", {
    name: "doubaowork.checkin.autoLabel",
  });
  await waitFor(() => expect(toggle).toBeEnabled());
  fireEvent.click(toggle);
  await screen.findByText("common.saving");
  expect(toggle).toBeChecked();
  expect(toggle).toBeDisabled();
  fail(new Error("synthetic-private-detail"));
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).not.toBeChecked();
  expect(screen.queryByText("common.saving")).toBeNull();
});
