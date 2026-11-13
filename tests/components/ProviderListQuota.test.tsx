import { act, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import i18n from "i18next";
import { ProviderListQuota } from "@/components/providers/ProviderListQuota";
import { SubscriptionAccountQuota } from "@/components/providers/SubscriptionAccountQuota";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: { codexQuota: vi.fn(), editor: vi.fn() },
}));
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: { quota: vi.fn() },
}));
const record: RoutingUpstream = {
  schemaVersion: 1,
  id: "fixture",
  revision: 1,
  name: "Synthetic subscription",
  hasApiKey: false,
  reauthRequired: false,
  source: { kind: "codex" },
  codexSettings: {
    accounts: ["selected"],
    mode: "vendor",
    models: [],
    groupsOnly: false,
  },
};
const quota = {
  credentialStatus: "valid",
  success: true,
  tiers: [
    { name: "five_hour", utilization: 27, resetsAt: null },
    { name: "weekly", utilization: 60, resetsAt: null },
  ],
};
function mount(value = record, client = new QueryClient()) {
  return render(
    <QueryClientProvider client={client}>
      <ProviderListQuota record={value} />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  i18n.addResourceBundle(
    "zh",
    "translation",
    {
      codexProvider: {
        remaining: "{{n}}% left",
        creditBalance: "Credits: {{balance}}",
      },
      subscription: { fiveHour: "5h" },
    },
    true,
    true,
  );
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...record,
    codex: {
      settings: record.codexSettings!,
      models: [],
      accounts: [
        { id: "selected", login: "Fixture", native: false, ready: true },
      ],
    },
  });
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue(quota as never);
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue(quota as never);
});
it("reads only selected Codex accounts and displays remaining window percentages", async () => {
  mount();
  await screen.findByText(/73%/);
  const status = screen.getByRole("status");
  expect(status).toHaveTextContent("5h");
  expect(status).toHaveTextContent("40%");
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledExactlyOnceWith(
    "selected",
  );
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});
it("keeps a skeleton through both Codex account and quota loading", async () => {
  let resolveEditor!: (value: RoutingUpstream) => void;
  let resolveQuota!: (value: typeof quota) => void;
  vi.mocked(routingUpstreamsApi.editor).mockReturnValue(
    new Promise((resolve) => {
      resolveEditor = resolve;
    }),
  );
  vi.mocked(routingUpstreamsApi.codexQuota).mockReturnValue(
    new Promise((resolve) => {
      resolveQuota = resolve;
    }) as never,
  );
  const view = mount();
  expect(view.container.querySelector(".pv-quota-skeleton")).not.toBeNull();
  await act(async () =>
    resolveEditor({
      ...record,
      codex: {
        settings: record.codexSettings!,
        models: [],
        accounts: [
          { id: "selected", login: "Fixture", native: false, ready: true },
        ],
      },
    }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith("selected"),
  );
  expect(view.container.querySelector(".pv-quota-skeleton")).not.toBeNull();
  await act(async () => resolveQuota(quota));
  expect(await screen.findByText(/73%/)).toBeVisible();
  expect(view.container.querySelector(".pv-quota-skeleton")).toBeNull();
});
it("uses a skeleton while a subscription account quota is pending", async () => {
  vi.mocked(nativeSubscriptionApi.quota).mockReturnValue(new Promise(() => {}));
  const view = mount({
    ...record,
    source: {
      kind: "subscription",
      provider: "claude",
      accountId: "selected",
      mode: "vendor",
      models: [],
      catalog: [],
      groupsOnly: false,
    },
  });
  expect(screen.getByRole("status")).toHaveAttribute("aria-busy", "true");
  expect(view.container.querySelector(".pv-quota-skeleton")).not.toBeNull();
});
it("keeps multiple account allowances separate", async () => {
  mount({
    ...record,
    codex: {
      settings: { ...record.codexSettings!, accounts: ["one", "two"] },
      models: [],
      accounts: ["one", "two"].map((id) => ({
        id,
        login: id,
        native: false,
        ready: true,
      })),
    },
  });
  expect(await screen.findAllByText(/73%/)).toHaveLength(2);
});
it("shares fresh Codex quota with the form cache for the same account identity", async () => {
  const client = new QueryClient();
  client.setQueryData(["codex-provider-quota", "selected", "Fixture"], {
    quota,
    failed: false,
  });
  mount(record, client);
  expect(await screen.findByText(/73%/)).toBeVisible();
  expect(routingUpstreamsApi.codexQuota).not.toHaveBeenCalled();
});
it("shows a safe failure without displaying vendor errors", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockRejectedValue(
    new Error("synthetic-private-error"),
  );
  mount();
  expect(await screen.findByText("codexProvider.quotaFailed")).toBeVisible();
  expect(screen.queryByText(/synthetic-private-error/)).not.toBeInTheDocument();
});
it("does not fetch a signed-out Codex account", async () => {
  mount({ ...record, reauthRequired: true });
  await act(async () => {});
  expect(routingUpstreamsApi.codexQuota).not.toHaveBeenCalled();
  expect(screen.getByText("codexProvider.quotaLoginRequired")).toBeVisible();
});
it("keeps Codex restrictions and credit balance visible", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    ...quota,
    tiers: [],
    codex: { allowed: false, credits: { balance: "12.50" } },
  } as never);
  mount();
  expect(await screen.findByText("codexProvider.quotaHeld")).toBeVisible();
  expect(screen.getByText(/12.50/)).toBeVisible();
});
it("reuses subscription account quota in compact list and full form displays", async () => {
  const source = {
    kind: "subscription",
    provider: "claude",
    accountId: "selected",
    mode: "vendor",
    models: [],
    catalog: [],
    groupsOnly: false,
  } as const;
  render(
    <QueryClientProvider client={new QueryClient()}>
      <button aria-label="Edit fixture">
        <ProviderListQuota
          record={{ ...record, source: { ...source, models: [], catalog: [] } }}
        />
      </button>
      <SubscriptionAccountQuota
        provider="claude"
        id="selected"
        login="Fixture"
        ready
        disabled={false}
      />
    </QueryClientProvider>,
  );
  const button = screen.getByRole("button", { name: "Edit fixture" });
  expect(await within(button).findByText(/73%/)).toBeVisible();
  expect(within(button).queryByRole("button")).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "codexProvider.refreshQuota Fixture" }),
  ).toBeVisible();
  expect(nativeSubscriptionApi.quota).toHaveBeenCalledExactlyOnceWith(
    "claude",
    "selected",
  );
});
it("does not invent ChatGPT API allowances", () => {
  mount({
    ...record,
    source: {
      kind: "subscription",
      provider: "chatgpt_api",
      accountId: "selected",
      models: [],
      catalog: [],
      mode: "vendor",
      groupsOnly: false,
    },
  });
  expect(
    screen.getByText("subscriptionProvider.quotaUnavailable"),
  ).toBeVisible();
  expect(nativeSubscriptionApi.quota).not.toHaveBeenCalled();
});
