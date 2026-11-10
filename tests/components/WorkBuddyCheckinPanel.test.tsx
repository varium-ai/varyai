import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorkBuddyCheckinPanel } from "@/components/WorkBuddyCheckinPanel";
import { settingsApi } from "@/lib/api/settings";
import {
  workbuddyCheckinApi,
  type WorkBuddyCheckinStatus,
} from "@/lib/api/workbuddy-checkin";

vi.mock("@/lib/api/workbuddy-checkin", () => ({
  workbuddyCheckinApi: {
    status: vi.fn(),
    checkIn: vi.fn(),
    setAutoEnabled: vi.fn(),
    accounts: vi.fn(),
    startLogin: vi.fn(),
    pollLogin: vi.fn(),
    cancelLogin: vi.fn(),
    removeAccount: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));

const initial: WorkBuddyCheckinStatus = {
  autoEnabled: false,
  day: "2026-11-02",
  outcome: "not_checked",
  checkedAt: null,
  credit: null,
  streak: null,
  creditDelayed: false,
};
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <WorkBuddyCheckinPanel />
    </QueryClientProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "workbuddy.checkin.title" }),
  );
}

beforeEach(() => {
  vi.mocked(workbuddyCheckinApi.status).mockResolvedValue(initial);
  vi.mocked(workbuddyCheckinApi.accounts).mockImplementation(async () => {
    const status = await workbuddyCheckinApi.status();
    return {
      accounts: [
        {
          id: "native-account",
          label: "Native account",
          source: "native",
          status,
        },
      ],
      nativeStatus: status,
    };
  });
  vi.mocked(workbuddyCheckinApi.checkIn).mockResolvedValue({
    ...initial,
    outcome: "claimed",
    credit: 30,
    streak: 3,
    creditDelayed: true,
  });
  vi.mocked(workbuddyCheckinApi.setAutoEnabled).mockImplementation(
    async (enabled) => ({ ...initial, autoEnabled: enabled }),
  );
});

describe("WorkBuddy daily check-in", () => {
  it("adds an independently authorized account without claiming or enabling automation", async () => {
    const native = {
      id: "native-account",
      label: "Native account",
      source: "native" as const,
      status: initial,
    };
    const browser = {
      id: "browser-account",
      label: "Browser account",
      source: "browser" as const,
      status: initial,
    };
    vi.mocked(workbuddyCheckinApi.accounts)
      .mockResolvedValueOnce({ accounts: [native], nativeStatus: initial })
      .mockResolvedValue({
        accounts: [native, browser],
        nativeStatus: initial,
      });
    vi.mocked(workbuddyCheckinApi.startLogin).mockResolvedValue({
      loginId: "synthetic-login",
      url: "https://www.workbuddy.cn/login",
    });
    vi.mocked(workbuddyCheckinApi.pollLogin).mockResolvedValue({
      complete: true,
      accountId: "browser-account",
    });
    vi.mocked(settingsApi.openExternal).mockResolvedValue();
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.addAccount" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox")).toHaveValue("browser-account"),
    );
    expect(settingsApi.openExternal).toHaveBeenCalledWith(
      "https://www.workbuddy.cn/login",
    );
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
    expect(workbuddyCheckinApi.setAutoEnabled).not.toHaveBeenCalled();
  });

  it("cancels pending authorization without saving a check-in preference", async () => {
    vi.mocked(workbuddyCheckinApi.startLogin).mockResolvedValue({
      loginId: "pending-login",
      url: "https://www.workbuddy.cn/login",
    });
    vi.mocked(workbuddyCheckinApi.pollLogin).mockResolvedValue({
      complete: false,
      accountId: null,
    });
    vi.mocked(workbuddyCheckinApi.cancelLogin).mockResolvedValue();
    vi.mocked(settingsApi.openExternal).mockResolvedValue();
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.addAccount" }),
    );
    await screen.findByText("workbuddy.checkin.loginWaiting");
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() =>
      expect(
        screen.queryByText("workbuddy.checkin.loginWaiting"),
      ).not.toBeInTheDocument(),
    );
    expect(workbuddyCheckinApi.cancelLogin).toHaveBeenCalledWith(
      "pending-login",
    );
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
    expect(workbuddyCheckinApi.setAutoEnabled).not.toHaveBeenCalled();
  });
  it("claims and enables automation only for the selected account", async () => {
    vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValue({
      nativeStatus: initial,
      accounts: [
        {
          id: "account-a",
          label: "Account A",
          source: "browser",
          status: initial,
        },
        {
          id: "account-b",
          label: "Account B",
          source: "browser",
          status: initial,
        },
      ],
    });
    mount();
    const select = await screen.findByRole("combobox", {
      name: "workbuddy.checkin.account",
    });
    await waitFor(() => expect(select).toBeEnabled());
    fireEvent.change(select, { target: { value: "account-b" } });
    const disclosure = screen.getByRole("button", {
      name: "workbuddy.checkin.title",
    });
    fireEvent.click(disclosure);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
    fireEvent.click(disclosure);
    expect(screen.getByRole("combobox")).toHaveValue("account-b");
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(workbuddyCheckinApi.setAutoEnabled).toHaveBeenCalledWith(
        true,
        "account-b",
      ),
    );
    await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    );
    await waitFor(() =>
      expect(workbuddyCheckinApi.checkIn).toHaveBeenCalledWith("account-b"),
    );
  });
  it("refreshes a changed desktop account without claiming, and claims only on a press", async () => {
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValueOnce({
      accounts: [
        {
          id: "switched-native-account",
          label: "Switched native account",
          source: "native",
          status: initial,
        },
      ],
      nativeStatus: initial,
    });
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.reload" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("combobox")).toHaveValue(
        "switched-native-account",
      ),
    );
    const button = await screen.findByRole("button", {
      name: "workbuddy.checkin.checkNow",
    });
    expect(button).toBeEnabled();
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
    expect(
      screen.getByRole("switch", { name: "workbuddy.checkin.autoLabel" }),
    ).not.toBeChecked();
    fireEvent.click(button);
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.claimed"),
    ).toBeInTheDocument();
    expect(workbuddyCheckinApi.checkIn).toHaveBeenCalledOnce();
    expect(workbuddyCheckinApi.checkIn).toHaveBeenCalledWith(
      "switched-native-account",
    );
    expect(screen.getByText("workbuddy.checkin.reward")).toBeInTheDocument();
    expect(screen.getByText("workbuddy.checkin.delayed")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "agentRewards.checkedIn" }),
    ).toBeDisabled();
  });

  it("saves automatic check-in independently from the manual action", async () => {
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    const toggle = await screen.findByRole("switch", {
      name: "workbuddy.checkin.autoLabel",
    });
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(workbuddyCheckinApi.setAutoEnabled).toHaveBeenCalledWith(
      true,
      "native-account",
    );
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
  });

  it("disables both actions during a claim and does not echo raw errors", async () => {
    let reject!: (reason: Error) => void;
    vi.mocked(workbuddyCheckinApi.checkIn).mockReturnValue(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    mount();
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    fireEvent.click(
      await screen.findByRole("button", { name: "workbuddy.checkin.checkNow" }),
    );
    await waitFor(() => expect(screen.getByRole("switch")).toBeDisabled());
    reject(new Error("private-token-from-upstream"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "workbuddy.checkin.operationFailed",
    );
    expect(screen.queryByText(/private-token/)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeEnabled();
  });

  it("guides an unreadable login back to the official client", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "credentials_unavailable",
    });
    mount();
    expect(
      await screen.findByText(
        "workbuddy.checkin.outcomes.credentials_unavailable",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeDisabled();
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
  });

  it("allows a retry after the user renews a rejected login in WorkBuddy", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "sign_in_required",
    });
    mount();
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.sign_in_required"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    );
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.claimed"),
    ).toBeInTheDocument();
  });

  it("explains protected credentials and keeps check-in disabled", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "credentials_protected",
    });
    mount();
    expect(
      await screen.findByText(
        "workbuddy.checkin.outcomes.credentials_protected",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeDisabled();
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
  });

  it("shows the own-app requirement without claiming a reward", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome: "own_app_required",
    });
    mount();
    expect(
      await screen.findByText("workbuddy.checkin.outcomes.own_app_required"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("workbuddy.checkin.reward"),
    ).not.toBeInTheDocument();
  });

  it("offers reload on a local status error without enabling actions", async () => {
    vi.mocked(workbuddyCheckinApi.status).mockRejectedValueOnce(
      new Error("private-state"),
    );
    mount();
    const alert = await screen.findByRole("alert");
    const reload = screen.getByRole("button", {
      name: "workbuddy.checkin.reload",
    });
    expect(alert).toHaveTextContent("workbuddy.checkin.loadFailed");
    expect(screen.queryByText(/private-state/)).not.toBeInTheDocument();
    fireEvent.click(reload);
    await screen.findByText("workbuddy.checkin.outcomes.not_checked");
    expect(
      await screen.findByRole("button", { name: "workbuddy.checkin.checkNow" }),
    ).toBeEnabled();
  });
});

it.each(["claimed", "already_checked"] as const)(
  "disables WorkBuddy claims for %s and enables the next day after refresh",
  async (outcome) => {
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      outcome,
    });
    mount();
    const completed = await screen.findByRole("button", {
      name: "agentRewards.checkedIn",
    });
    expect(completed).toBeDisabled();
    fireEvent.click(completed);
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
    vi.mocked(workbuddyCheckinApi.status).mockResolvedValue({
      ...initial,
      day: "2026-11-03",
    });
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.checkin.reload" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "workbuddy.checkin.checkNow" }),
      ).toBeEnabled(),
    );
    expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
  },
);
