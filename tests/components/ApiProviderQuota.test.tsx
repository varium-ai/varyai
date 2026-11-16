import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import i18n from "i18next";
import { ApiProviderQuota } from "@/components/providers/ApiProviderQuota";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: { quota: vi.fn() },
}));
const record: RoutingUpstream = {
  schemaVersion: 1,
  id: "relay",
  revision: 4,
  name: "Synthetic relay",
  hasApiKey: true,
  source: {
    kind: "api",
    apiFormat: "openai_chat",
    baseUrl: "https://relay.example/v1",
    models: [],
    catalog: [],
    mode: "vendor",
    groupsOnly: false,
  },
};
const reading = {
  status: "ready",
  balance: 993.86,
  tiers: [{ name: "daily", utilization: 1, resetsAt: "2026-11-11T00:00:00Z" }],
};
function mount(compact = true, value = record) {
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <ApiProviderQuota record={value} compact={compact} disabled={false} />
    </QueryClientProvider>,
  );
  return client;
}
beforeEach(() => {
  vi.clearAllMocks();
  i18n.addResourceBundle(
    "zh",
    "translation",
    {
      codexProvider: { remaining: "{{n}}% left" },
      subscriptionProvider: { balance: "Balance {{amount}}" },
    },
    true,
    true,
  );
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue(reading as never);
});
it("shows a relay's balance and remaining windows without revealing saved keys", async () => {
  mount();
  expect(await screen.findByText(/993.86/)).toBeVisible();
  expect(screen.getByText(/99%/)).toBeVisible();
  expect(routingUpstreamsApi.quota).toHaveBeenCalledExactlyOnceWith("relay", 4);
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});
it("replaces the initial quota skeleton with the allowance", async () => {
  let resolve!: (value: typeof reading) => void;
  vi.mocked(routingUpstreamsApi.quota).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }) as never,
  );
  mount();
  const status = screen.getByRole("status");
  expect(status).toHaveAttribute("aria-busy", "true");
  expect(status.querySelector(".pv-quota-skeleton")).not.toBeNull();
  resolve(reading);
  await screen.findByText(/993.86/);
  expect(status.querySelector(".pv-quota-skeleton")).toBeNull();
  expect(status).toHaveAttribute("aria-busy", "false");
});
it("keeps cached amounts visible during background refresh", async () => {
  const client = mount();
  await screen.findByText(/993.86/);
  vi.mocked(routingUpstreamsApi.quota).mockReturnValue(new Promise(() => {}));
  void client.invalidateQueries({ queryKey: ["routing-provider-quota"] });
  await waitFor(() =>
    expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true"),
  );
  expect(screen.getByText(/993.86/)).toBeVisible();
  expect(document.querySelector(".pv-quota-skeleton")).toBeNull();
});
it("retains full allowance and manual refresh in the provider form", async () => {
  mount(false);
  expect(await screen.findByText(/993.86/)).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexProvider.refreshQuota Synthetic relay",
    }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.quota).toHaveBeenCalledTimes(2),
  );
});
it("retains the last good allowance on a temporary error, with a safe stale message", async () => {
  const client = mount();
  await screen.findByText(/993.86/);
  vi.mocked(routingUpstreamsApi.quota).mockRejectedValue(
    new Error("synthetic-private-error"),
  );
  await client.invalidateQueries({ queryKey: ["routing-provider-quota"] });
  expect(await screen.findByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.getByText(/993.86/)).toBeVisible();
  expect(screen.queryByText(/synthetic-private-error/)).not.toBeInTheDocument();
});
it("retains a known allowance when a later response is unrecognized", async () => {
  const client = mount();
  await screen.findByText(/993.86/);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "unsupported",
    balance: null,
    tiers: [],
  });
  await client.invalidateQueries({ queryKey: ["routing-provider-quota"] });
  expect(await screen.findByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.getByText(/993.86/)).toBeVisible();
});
it("clears cached amounts after explicit credential rejection", async () => {
  const client = mount();
  await screen.findByText(/993.86/);
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "unauthorized",
    balance: null,
    tiers: [],
  });
  await client.invalidateQueries({ queryKey: ["routing-provider-quota"] });
  expect(
    await screen.findByText("subscriptionProvider.quotaKeyRejected"),
  ).toBeVisible();
  expect(screen.queryByText(/993.86/)).not.toBeInTheDocument();
});
it("does not invent balances for unsupported providers", async () => {
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "unsupported",
    balance: null,
    tiers: [],
  });
  mount();
  await waitFor(() => expect(routingUpstreamsApi.quota).toHaveBeenCalled());
  await waitFor(() =>
    expect(screen.queryByRole("status")).not.toBeInTheDocument(),
  );
});
it("does not render an unsupported relay as a tray quota card", async () => {
  vi.mocked(routingUpstreamsApi.quota).mockResolvedValue({
    status: "unsupported",
    balance: null,
    tiers: [],
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ApiProviderQuota
        record={record}
        card={{ remaining: true, toggle: vi.fn() }}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(screen.queryByRole("article")).not.toBeInTheDocument(),
  );
  expect(routingUpstreamsApi.quota).toHaveBeenCalledExactlyOnceWith("relay", 4);
  expect(
    screen.queryByText("subscriptionProvider.quotaUnavailable"),
  ).not.toBeInTheDocument();
});
it.each([
  { ...record, revision: 0 },
  { ...record, hasApiKey: false },
])("does not query unsaved providers or missing keys", async (value) => {
  mount(true, value);
  expect(routingUpstreamsApi.quota).not.toHaveBeenCalled();
});
