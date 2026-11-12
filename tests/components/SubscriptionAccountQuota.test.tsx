import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { SubscriptionAccountQuota } from "@/components/providers/SubscriptionAccountQuota";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";
import { copilotGetUsageForAccount } from "@/lib/api/copilot";
import { subscriptionApi } from "@/lib/api/subscription";
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: { quota: vi.fn() },
}));
vi.mock("@/lib/api/copilot", () => ({ copilotGetUsageForAccount: vi.fn() }));
vi.mock("@/lib/api/subscription", () => ({
  subscriptionApi: { getXaiOauthQuota: vi.fn() },
}));
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [{ name: "five_hour", utilization: 25, resetsAt: null }],
  } as never);
});
function mount(
  provider:
    | "claude"
    | "gemini_cli"
    | "antigravity"
    | "github_copilot"
    | "xai_oauth"
    | "chatgpt_api",
  ready = true,
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <SubscriptionAccountQuota
        provider={provider}
        id="fixture-id"
        login="Fixture"
        ready={ready}
        disabled={false}
      />
    </QueryClientProvider>,
  );
  return client;
}
it.each(["claude", "gemini_cli", "antigravity"] as const)(
  "reads %s allowance for the exact managed account",
  async (provider) => {
    mount(provider);
    expect(await screen.findByText("codexProvider.remaining")).toBeVisible();
    expect(nativeSubscriptionApi.quota).toHaveBeenCalledWith(
      provider,
      "fixture-id",
    );
  },
);
it("reads Copilot quota for its explicit binding and excludes unlimited windows", async () => {
  vi.mocked(copilotGetUsageForAccount).mockResolvedValue({
    quota_reset_date: "2026-11-06",
    quota_snapshots: {
      premium_interactions: { percent_remaining: 35, unlimited: false },
      chat: { percent_remaining: 100, unlimited: true },
    },
  } as never);
  mount("github_copilot");
  expect(await screen.findAllByText("codexProvider.remaining")).toHaveLength(1);
  expect(copilotGetUsageForAccount).toHaveBeenCalledWith("fixture-id");
  expect(nativeSubscriptionApi.quota).not.toHaveBeenCalled();
});
it("reads Grok allowance from the bound OAuth account", async () => {
  vi.mocked(subscriptionApi.getXaiOauthQuota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [],
  } as never);
  mount("xai_oauth");
  await screen.findByText("codexProvider.quotaEmpty");
  expect(subscriptionApi.getXaiOauthQuota).toHaveBeenCalledWith("fixture-id");
});
it("does not query rejected credentials or invent ChatGPT API quota", () => {
  mount("chatgpt_api");
  expect(
    screen.getByText("subscriptionProvider.quotaUnavailable"),
  ).toBeVisible();
  expect(nativeSubscriptionApi.quota).not.toHaveBeenCalled();
});
it("retains good meters on transient failure and clears them on explicit rejection", async () => {
  const client = mount("claude");
  await screen.findByText("codexProvider.remaining");
  vi.mocked(nativeSubscriptionApi.quota).mockRejectedValue(
    "synthetic-private-vendor-error",
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.refreshQuota Fixture" }),
  );
  expect(await screen.findByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.getByText("codexProvider.remaining")).toBeVisible();
  expect(
    screen.queryByText("synthetic-private-vendor-error"),
  ).not.toBeInTheDocument();
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    credentialStatus: "expired",
    success: false,
    tiers: [],
  } as never);
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await waitFor(() =>
    expect(
      screen.getByRole("button", {
        name: "codexProvider.refreshQuota Fixture",
      }),
    ).toBeEnabled(),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.refreshQuota Fixture" }),
  );
  expect(
    await screen.findByText("codexProvider.quotaLoginRequired"),
  ).toBeVisible();
  expect(screen.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ["native-subscription-accounts", "claude"],
  });
});
it("does not fetch unavailable accounts", async () => {
  mount("claude", false);
  await act(async () => {});
  expect(nativeSubscriptionApi.quota).not.toHaveBeenCalled();
  expect(
    screen.getByRole("button", { name: "codexProvider.refreshQuota Fixture" }),
  ).toBeDisabled();
});

it("refreshes subscription plan labels after a Google quota observation", async () => {
  const client = mount("antigravity");
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await screen.findByText("codexProvider.remaining");
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ["native-subscription-accounts", "antigravity"],
  });
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ["routing-upstreams"] });
});

it("keeps an exhausted enterprise shared pool visible without inventing unknown meters", async () => {
  vi.mocked(copilotGetUsageForAccount).mockResolvedValue({
    quota_reset_date: "2026-12-01T00:00:00Z",
    quota_snapshots: {
      premium_interactions: { unlimited: true, has_quota: false },
      chat: { unlimited: true, has_quota: true },
      completions: { percent_remaining: null },
    },
  } as never);
  mount("github_copilot");
  expect(await screen.findAllByText("codexProvider.remaining")).toHaveLength(1);
});
