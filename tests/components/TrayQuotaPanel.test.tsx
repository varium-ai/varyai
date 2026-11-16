import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import i18n from "i18next";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";
import { quotaPanelApi } from "@/lib/api/quotaPanel";
import { TrayQuotaPanel } from "@/components/tray/TrayQuotaPanel";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    editor: vi.fn(),
    codexQuota: vi.fn(),
    quota: vi.fn(),
  },
}));
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: { quota: vi.fn() },
}));
vi.mock("@/lib/api/quotaPanel", () => ({
  quotaPanelApi: {
    codex: vi.fn(),
    history: vi.fn(async () => []),
    note: vi.fn(async () => []),
  },
}));
const provider: RoutingUpstream = {
  schemaVersion: 1,
  id: "synthetic-codex",
  revision: 1,
  name: "ChatGPT",
  hasApiKey: false,
  source: { kind: "codex" },
  subscriptionPlans: ["Pro"],
  codex: {
    settings: {
      accounts: ["first", "second"],
      mode: "vendor",
      models: [],
      groupsOnly: false,
    },
    models: [],
    accounts: [
      { id: "first", login: "First account", native: false, ready: true },
      { id: "second", login: "Second account", native: false, ready: true },
      {
        id: "unselected",
        login: "Unselected account",
        native: false,
        ready: true,
      },
    ],
  },
};
const quota = {
  tool: "codex",
  credentialStatus: "valid" as const,
  credentialMessage: null,
  success: true,
  tiers: [
    { name: "five_hour", utilization: 27, resetsAt: "2030-01-01T12:00:00Z" },
    { name: "weekly", utilization: 60, resetsAt: null },
  ],
  extraUsage: null,
  error: null,
  queriedAt: null,
};
const relay: RoutingUpstream = {
  schemaVersion: 1,
  id: "synthetic-relay",
  revision: 7,
  name: "Relay",
  hasApiKey: true,
  source: {
    kind: "api",
    baseUrl: "https://relay.example/v1",
    apiFormat: "openai_chat",
    models: [],
    catalog: [],
    mode: "vendor",
    groupsOnly: false,
  },
};
const unsupported = {
  status: "unsupported" as const,
  balance: null,
  tiers: [],
};
function mount(
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
      <TrayQuotaPanel />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  localStorage.setItem("vary:tray-quota-left", "false");
  vi.mocked(quotaPanelApi.codex).mockResolvedValue({
    identity: "synthetic",
    policy: { week: false, expiring: false, credits: "on" },
    resets: [],
    pending: false,
  });
  i18n.addResourceBundle(
    "zh",
    "translation",
    { subscriptionProvider: { balance: "Balance: {{amount}}" } },
    true,
    true,
  );
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([provider]);
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue(quota);
});
afterEach(() => vi.useRealTimers());
it("shows relay balances and window rings using the saved provider revision", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([relay]);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "ready",
    balance: 5.25,
    tiers: quota.tiers,
  });
  mount();
  const card = await screen.findByRole("article", { name: "Relay" });
  expect(await within(card).findByText(/Balance:.*5.25/)).toBeVisible();
  expect(within(card).getByText("27%")).toBeVisible();
  expect(routingUpstreamsApi.quota).toHaveBeenCalledExactlyOnceWith(
    "synthetic-relay",
    7,
  );
});
it("removes unsupported relay cards and headings while retaining a reported zero balance", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    relay,
    { ...relay, id: "supported", name: "Supported" },
  ]);
  vi.mocked(routingUpstreamsApi.quota).mockImplementation(async (id) =>
    id === relay.id ? unsupported : { status: "ready", balance: 0, tiers: [] },
  );
  mount();
  expect(await screen.findByText(/Balance:.*0.00/)).toBeVisible();
  await waitFor(() =>
    expect(
      screen.queryByRole("region", { name: "Relay" }),
    ).not.toBeInTheDocument(),
  );
  expect(screen.getByRole("region", { name: "Supported" })).toBeVisible();
  expect(screen.queryByText("Relay")).not.toBeInTheDocument();
});
it("shows an empty state when every relay is unsupported and can retry from the header", async () => {
  const client = new QueryClient();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([relay]);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue(unsupported);
  mount(client);
  expect(await screen.findByText("trayPanel.quotaEmpty")).toBeVisible();
  expect(screen.queryByRole("region")).not.toBeInTheDocument();
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "ready",
    balance: 5.25,
    tiers: [],
  });
  await act(async () => {
    await client.invalidateQueries({ queryKey: ["routing-provider-quota"] });
  });
  expect(await screen.findByText(/Balance:.*5.25/)).toBeVisible();
  expect(screen.queryByText("trayPanel.quotaEmpty")).not.toBeInTheDocument();
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(2);
});
it.each(["failed", "unauthorized"])(
  "keeps %s relay queries visible for retry",
  async (state) => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([relay]);
    if (state === "failed") {
      vi.mocked(routingUpstreamsApi.quota).mockRejectedValue(
        new Error("Synthetic private failure"),
      );
    } else {
      vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
        ...unsupported,
        status: "unauthorized",
      });
    }
    mount();
    const message =
      state === "failed"
        ? "codexProvider.quotaFailed"
        : "subscriptionProvider.quotaKeyRejected";
    expect(await screen.findByText(message)).toBeVisible();
    expect(screen.getByRole("region", { name: "Relay" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "codexProvider.refreshQuota Relay" }),
    ).toBeEnabled();
    expect(
      screen.queryByText("Synthetic private failure"),
    ).not.toBeInTheDocument();
  },
);
it("keeps the last successful tray balance when a later reply is unrecognized", async () => {
  const client = new QueryClient();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([relay]);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "ready",
    balance: 5.25,
    tiers: [],
  });
  mount(client);
  await screen.findByText(/Balance:.*5.25/);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue(unsupported);
  await act(async () => {
    await client.invalidateQueries({ queryKey: ["routing-provider-quota"] });
  });
  expect(await screen.findByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.getByText(/Balance:.*5.25/)).toBeVisible();
});
it("reuses a missing-interface result when reopening and probes an edited provider revision", async () => {
  const client = new QueryClient();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([relay]);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue(unsupported);
  const view = mount(client);
  await waitFor(() =>
    expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(1),
  );
  await screen.findByText("trayPanel.quotaEmpty");
  view.unmount();
  mount(client);
  expect(screen.getByText("trayPanel.quotaEmpty")).toBeVisible();
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(1);
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    { ...relay, revision: 8 },
  ]);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "ready",
    balance: 5.25,
    tiers: [],
  });
  await act(async () => {
    await client.invalidateQueries({ queryKey: ["routing-upstreams"] });
  });
  expect(await screen.findByText(/Balance:.*5.25/)).toBeVisible();
  expect(routingUpstreamsApi.quota).toHaveBeenLastCalledWith(relay.id, 8);
});
it("probes a hidden relay six hours after the observation even if the tab reopens", async () => {
  vi.useFakeTimers();
  const client = new QueryClient();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([relay]);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue(unsupported);
  let view = mount(client);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10);
  });
  expect(
    screen.queryByRole("region", { name: "Relay" }),
  ).not.toBeInTheDocument();
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(1);
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5 * 60 * 60_000);
  });
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(1);
  view = mount(client);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10);
  });
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60 * 60_000 - 1000);
  });
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(1);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "ready",
    balance: 5.25,
    tiers: [],
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10);
  });
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(2);
  expect(screen.getByText(/Balance:.*5.25/)).toBeVisible();
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(6 * 60 * 60_000);
  });
  expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(2);
  client.clear();
});
it("names unavailable observations instead of inventing a zero balance", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      ...provider,
      id: "synthetic-claude",
      name: "Claude",
      codex: undefined,
      source: {
        kind: "subscription",
        provider: "claude",
        accountId: "synthetic",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    },
  ]);
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    ...quota,
    credentialStatus: "expired",
    success: false,
    tiers: [],
  });
  mount();
  const card = await screen.findByRole("article", { name: "Claude" });
  expect(
    await within(card).findByText("codexProvider.quotaLoginRequired"),
  ).toBeVisible();
  expect(within(card).queryByRole("meter")).not.toBeInTheDocument();
  expect(screen.queryByText("0%")).not.toBeInTheDocument();
});
it("keeps model pools and their window spans distinct", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      ...provider,
      id: "pool",
      name: "Antigravity",
      codex: undefined,
      source: {
        kind: "subscription",
        provider: "antigravity",
        accountId: "synthetic",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    },
  ]);
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    ...quota,
    tiers: [
      { name: "Claude and GPT models · 5h", utilization: 12, resetsAt: null },
      { name: "Claude and GPT models · 7d", utilization: 34, resetsAt: null },
    ],
  });
  mount();
  const card = await screen.findByRole("article", { name: "Antigravity" });
  expect(await within(card).findByText("5h")).toBeVisible();
  expect(within(card).getByText("7d")).toBeVisible();
  expect(within(card).getAllByText("Claude & GPT")).toHaveLength(2);
});
it("defaults to remaining percentages and retains an exhausted independent model pool", async () => {
  localStorage.removeItem("vary:tray-quota-left");
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      ...provider,
      id: "pool",
      name: "Antigravity",
      codex: undefined,
      source: {
        kind: "subscription",
        provider: "antigravity",
        accountId: "synthetic",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    },
  ]);
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    ...quota,
    tiers: [
      { name: "Gemini · five_hour", utilization: 0, resetsAt: null },
      { name: "Gemini · seven_day", utilization: 3.2, resetsAt: null },
      { name: "Claude & GPT · five_hour", utilization: 0, resetsAt: null },
      { name: "Claude & GPT · seven_day", utilization: 0, resetsAt: null },
      { name: "Gemini 4 Argon · seven_day", utilization: 100, resetsAt: null },
    ],
  });
  mount();
  const card = await screen.findByRole("article", { name: "Antigravity" });
  await within(card).findByRole("meter", { name: "Gemini 7d" });
  for (const [label, percent] of [
    ["Gemini 5h", "100"],
    ["Gemini 7d", "96.8"],
    ["Claude & GPT 5h", "100"],
    ["Claude & GPT 7d", "100"],
    ["Gemini 4 Argon 7d", "0"],
  ]) {
    expect(within(card).getByRole("meter", { name: label })).toHaveAttribute(
      "aria-valuenow",
      percent,
    );
  }
  expect(within(card).getAllByRole("meter")).toHaveLength(5);
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.showUsed" }));
  expect(
    within(card).getByRole("meter", { name: "Gemini 7d" }),
  ).toHaveAttribute("aria-valuenow", "3.2");
  expect(
    within(card).getByRole("meter", { name: "Gemini 4 Argon 7d" }),
  ).toHaveAttribute("aria-valuenow", "100");
  expect(localStorage.getItem("vary:tray-quota-left")).toBe("false");
});
it("shows selected accounts, quota rings, plan and named reset times; switches used and remaining", async () => {
  mount();
  const card = await screen.findByRole("article", { name: "First account" });
  expect(await within(card).findByText("27%")).toBeVisible();
  expect(within(card).getByText("60%")).toBeVisible();
  expect(within(card).getAllByRole("meter")[0]).toHaveAttribute(
    "aria-valuenow",
    "27",
  );
  expect(within(card).getByText(/codexProvider.resets/)).toBeVisible();
  expect(screen.getByText("Pro")).toBeVisible();
  expect(screen.queryByText("Unselected account")).not.toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "trayPanel.showRemaining" }),
  );
  expect(within(card).getByText("73%")).toBeVisible();
  expect(within(card).getByText("40%")).toBeVisible();
  expect(localStorage.getItem("vary:tray-quota-left")).toBe("true");
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(2);
});
it("refreshes only the clicked account, spins through the request and keeps the reading on failure", async () => {
  mount();
  const card = await screen.findByRole("article", { name: "First account" });
  await within(card).findByText("27%");
  const refresh = within(card).getByRole("button", {
    name: "codexProvider.refreshQuota First account",
  });
  let rejectRead!: () => void;
  vi.mocked(routingUpstreamsApi.codexQuota).mockReturnValueOnce(
    new Promise((_resolve, reject) => {
      rejectRead = () => reject(new Error("Synthetic quota failure"));
    }),
  );
  fireEvent.click(refresh);
  await waitFor(() => expect(refresh).toBeDisabled());
  expect(refresh.querySelector("svg")).toHaveClass("animate-spin");
  expect(within(card).getByText("27%")).toBeVisible();
  fireEvent.click(refresh);
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(3);
  expect(routingUpstreamsApi.codexQuota).toHaveBeenLastCalledWith("first");
  await act(async () => rejectRead());
  await waitFor(() => expect(refresh).toBeEnabled());
  expect(within(card).getByText("27%")).toBeVisible();
  expect(within(card).getByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.queryByText("Synthetic quota failure")).not.toBeInTheDocument();
});
it("reuses cached account readings when the tab reopens", async () => {
  const client = new QueryClient();
  const view = mount(client);
  await screen.findAllByText("27%");
  view.unmount();
  mount(client);
  expect(screen.getAllByText("27%")).toHaveLength(2);
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(2);
});
it("distinguishes a failed provider read from an empty allowance list", async () => {
  vi.mocked(routingUpstreamsApi.list).mockRejectedValue(
    new Error("Synthetic catalog failure"),
  );
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "trayPanel.quotaFailed",
  );
  expect(screen.queryByText("trayPanel.quotaEmpty")).not.toBeInTheDocument();
});
