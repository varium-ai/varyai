import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { AgentRewardsPanel } from "@/components/AgentRewardsPanel";
import {
  agentRewardsApi,
  type AgentRewardStatus,
} from "@/lib/api/agent-rewards";
import { settingsApi } from "@/lib/api/settings";

vi.mock("@/lib/api/agent-rewards", () => ({
  agentRewardsApi: {
    status: vi.fn(),
    claim: vi.fn(),
    setAuto: vi.fn(),
    startLogin: vi.fn(),
    pollLogin: vi.fn(),
    cancelLogin: vi.fn(),
    forget: vi.fn(),
    nativeAccount: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));
const initial: AgentRewardStatus = {
  app: "qoder",
  accountId: "a".repeat(64),
  label: "Synthetic account",
  autoEnabled: false,
  outcome: "not_checked",
  credit: 100,
  checkedAt: null,
  nextCheckAt: null,
};
function mount(app: "qoder" | "trae" | "zcode" = "qoder") {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <AgentRewardsPanel app={app} />
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "agentRewards.title" }));
  fireEvent.click(screen.getByRole("button", { name: "dailyCheckin.manage" }));
  return client;
}
beforeEach(() => {
  vi.mocked(agentRewardsApi.status).mockResolvedValue(initial);
  vi.mocked(agentRewardsApi.claim).mockResolvedValue({
    ...initial,
    outcome: "claimed",
  });
  vi.mocked(agentRewardsApi.setAuto).mockResolvedValue({
    ...initial,
    autoEnabled: true,
  });
});
it("keeps the saved completion visible alongside a disconnected client warning", async () => {
  vi.mocked(agentRewardsApi.status).mockResolvedValue({
    ...initial,
    app: "trae",
    accountId: null,
    label: null,
    outcome: "connection_required",
    savedOutcome: "claimed",
    checkedAt: Date.UTC(2026, 10, 10),
  });
  mount("trae");
  await screen.findByText("agentRewards.outcomes.claimed");
  expect(
    screen.getByText(/agentRewards.outcomes.connection_required/),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", {
      name: /^dailyCheckin\.(todayReward|todayChecked|claimedReward|claimed)$/,
    }),
  ).toBeDisabled();
  expect(screen.getByRole("switch")).toBeDisabled();
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
});

it("refreshes without claiming and passes the current account identity to an explicit claim", async () => {
  mount();
  await screen.findByText("Synthetic account");
  fireEvent.click(screen.getByRole("button", { name: "agentRewards.refresh" }));
  await waitFor(() => expect(agentRewardsApi.status).toHaveBeenCalledTimes(2));
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "agentRewards.claim" }));
  await screen.findByText("agentRewards.outcomes.claimed");
  expect(agentRewardsApi.claim).toHaveBeenCalledExactlyOnceWith(
    "qoder",
    initial.accountId,
  );
  expect(
    screen.getByRole("button", {
      name: /^dailyCheckin\.(todayReward|todayChecked|claimedReward|claimed)$/,
    }),
  ).toBeDisabled();
});
it.each(["inactive", "ineligible", "connection_required"] as const)(
  "does not offer a claim for %s",
  async (outcome) => {
    vi.mocked(agentRewardsApi.status).mockResolvedValue({
      ...initial,
      outcome,
    });
    mount("zcode");
    await screen.findByText(`agentRewards.outcomes.${outcome}`);
    expect(
      screen.getByRole("button", { name: "agentRewards.claim" }),
    ).toBeDisabled();
    expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  },
);
it("requires verification in ZCode's official client before claiming", async () => {
  vi.mocked(agentRewardsApi.status).mockResolvedValue({
    ...initial,
    app: "zcode",
    outcome: "own_app_required",
  });
  mount("zcode");
  await screen.findByText("agentRewards.zcodeVerificationHint");
  const claim = screen.getByRole("button", {
    name: "agentRewards.verificationRequired",
  });
  expect(claim).toBeDisabled();
  fireEvent.click(claim);
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
});
it("enables automation only for the currently displayed account", async () => {
  mount("trae");
  await screen.findByText("Synthetic account");
  fireEvent.click(
    screen.getByRole("switch", { name: "agentRewards.autoLabel" }),
  );
  await waitFor(() =>
    expect(agentRewardsApi.setAuto).toHaveBeenCalledExactlyOnceWith(
      "trae",
      initial.accountId,
      true,
    ),
  );
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
});
it("authorizes Qoder independently without claiming or enabling automation", async () => {
  vi.mocked(agentRewardsApi.startLogin).mockResolvedValue({
    loginId: "synthetic-login",
    url: "https://qoder.cn/device/selectAccounts",
  });
  vi.mocked(agentRewardsApi.pollLogin).mockResolvedValue({ complete: true });
  mount();
  await screen.findByText("Synthetic account");
  fireEvent.click(
    screen.getByRole("button", { name: "agentRewards.authorize" }),
  );
  await waitFor(() =>
    expect(settingsApi.openExternal).toHaveBeenCalledWith(
      "https://qoder.cn/device/selectAccounts",
    ),
  );
  await waitFor(() => expect(agentRewardsApi.status).toHaveBeenCalledTimes(2));
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
});
it("cancels the pending grant when opening the browser fails and hides private errors", async () => {
  vi.mocked(agentRewardsApi.startLogin).mockResolvedValue({
    loginId: "synthetic-login",
    url: "https://qoder.cn/device/selectAccounts",
  });
  vi.mocked(settingsApi.openExternal).mockRejectedValueOnce(
    new Error("synthetic-private-detail"),
  );
  mount();
  await screen.findByText("Synthetic account");
  fireEvent.click(
    screen.getByRole("button", { name: "agentRewards.authorize" }),
  );
  await screen.findByText("agentRewards.operationFailed");
  expect(agentRewardsApi.cancelLogin).toHaveBeenCalledWith(
    "qoder",
    "synthetic-login",
  );
  expect(
    screen.queryByText(/synthetic-private-detail/),
  ).not.toBeInTheDocument();
});

it("reads the native Qoder login without opening a browser or enabling automatic claims", async () => {
  vi.mocked(agentRewardsApi.status).mockResolvedValue({
    ...initial,
    source: "browser",
    nativeAvailable: true,
  });
  vi.mocked(agentRewardsApi.nativeAccount).mockResolvedValue({
    ...initial,
    source: "native",
    label: "Native Qoder account",
  });
  mount();
  await screen.findByText("Synthetic account");
  fireEvent.click(
    screen.getByRole("button", { name: "agentRewards.readNative" }),
  );
  await screen.findByText("Native Qoder account");
  expect(agentRewardsApi.nativeAccount).toHaveBeenCalledOnce();
  expect(agentRewardsApi.startLogin).not.toHaveBeenCalled();
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
  expect(screen.getByText("agentRewards.sources.native")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "agentRewards.forget" }),
  ).not.toBeInTheDocument();
});

it("removes the backup grant independently of the current native account", async () => {
  const backup = "b".repeat(64);
  vi.mocked(agentRewardsApi.status).mockResolvedValue({
    ...initial,
    source: "native",
    nativeAvailable: true,
    browserAccountId: backup,
  });
  mount();
  await screen.findByText("Synthetic account");
  fireEvent.click(screen.getByRole("button", { name: "agentRewards.forget" }));
  await waitFor(() =>
    expect(agentRewardsApi.forget).toHaveBeenCalledWith("qoder", backup),
  );
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
});

it("offers native access permission when the detected client has no readable account yet", async () => {
  vi.mocked(agentRewardsApi.status).mockResolvedValue({
    ...initial,
    source: "native",
    nativeAvailable: true,
    accountId: null,
    label: null,
    outcome: "credentials_protected",
  });
  vi.mocked(agentRewardsApi.nativeAccount).mockResolvedValue({
    ...initial,
    source: "native",
  });
  mount();
  const button = await screen.findByRole("button", {
    name: "agentRewards.readNative",
  });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  await screen.findByText("Synthetic account");
  expect(agentRewardsApi.nativeAccount).toHaveBeenCalledOnce();
  expect(agentRewardsApi.startLogin).not.toHaveBeenCalled();
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
});

it.each(
  (["qoder", "trae", "zcode"] as const).flatMap((app) =>
    (["claimed", "already_checked"] as const).map((outcome) => ({
      app,
      outcome,
    })),
  ),
)(
  "disables completed $app claims for $outcome until the next reward window",
  async ({ app, outcome }) => {
    vi.mocked(agentRewardsApi.status).mockResolvedValue({
      ...initial,
      app,
      outcome,
    });
    mount(app);
    const completed = await screen.findByRole("button", {
      name: /^dailyCheckin\.(todayReward|todayChecked|claimedReward|claimed)$/,
    });
    expect(completed).toBeDisabled();
    expect(completed).toHaveTextContent(
      app === "zcode"
        ? "dailyCheckin.claimedReward"
        : "dailyCheckin.todayReward",
    );
    expect(screen.getByText("dailyCheckin.noData")).toBeVisible();
    expect(
      screen
        .getAllByRole("listitem")
        .every((day) => day.dataset.state === "unknown"),
    ).toBe(true);
    fireEvent.click(completed);
    expect(agentRewardsApi.claim).not.toHaveBeenCalled();
    vi.mocked(agentRewardsApi.status).mockResolvedValue({ ...initial, app });
    fireEvent.click(
      screen.getByRole("button", { name: "agentRewards.refresh" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "agentRewards.claim" }),
      ).toBeEnabled(),
    );
    expect(agentRewardsApi.claim).not.toHaveBeenCalled();
  },
);

it.each(["zcode", "trae"] as const)(
  "shows the current %s account picker and explains native account management",
  async (app) => {
    vi.mocked(agentRewardsApi.status).mockResolvedValue({ ...initial, app });
    mount(app);
    await screen.findByText("Synthetic account");
    expect(
      screen.getByRole("combobox", { name: "agentRewards.account" }),
    ).toBeEnabled();
    expect(
      screen.getByText("agentRewards.nativeAccountManagement"),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "agentRewards.authorize" }),
    ).not.toBeInTheDocument();
  },
);
