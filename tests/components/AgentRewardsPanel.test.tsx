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
    screen.getByRole("button", { name: "agentRewards.claim" }),
  ).toBeDisabled();
});
it.each([
  "inactive",
  "own_app_required",
  "ineligible",
  "connection_required",
] as const)("does not offer a claim for %s", async (outcome) => {
  vi.mocked(agentRewardsApi.status).mockResolvedValue({ ...initial, outcome });
  mount("zcode");
  await screen.findByText(`agentRewards.outcomes.${outcome}`);
  expect(
    screen.getByRole("button", { name: "agentRewards.claim" }),
  ).toBeDisabled();
  expect(agentRewardsApi.claim).not.toHaveBeenCalled();
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
