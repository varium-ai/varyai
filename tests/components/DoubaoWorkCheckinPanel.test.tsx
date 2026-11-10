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
  useTranslation: () => ({ t: (key: string) => key }),
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
}
beforeEach(() => {
  vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue(initial);
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
  fireEvent.change(select, { target: { value: "account-b" } });
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
  expect(screen.getByText("doubaowork.checkin.reward")).toBeVisible();
  expect(screen.getByText("doubaowork.checkin.streak")).toBeVisible();
  expect(screen.getByText("doubaowork.checkin.totalDays")).toBeVisible();
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
  expect(screen.getByText("doubaowork.checkin.reward")).toBeVisible();
  expect(screen.getByText("doubaowork.checkin.streak")).toBeVisible();
  fireEvent.change(screen.getByRole("combobox"), {
    target: { value: "account-b" },
  });
  expect(
    screen.queryByText("doubaowork.checkin.reward"),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByText("doubaowork.checkin.streak"),
  ).not.toBeInTheDocument();
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
      name: "agentRewards.checkedIn",
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
