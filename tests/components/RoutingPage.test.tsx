import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingPage } from "@/components/routing/RoutingPage";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { usageApi } from "@/lib/api/usage";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

vi.mock("@/components/routing/IntentRoutingPanel", () => ({
  IntentRoutingPanel: ({ createRequest }: { createRequest: number }) => (
    <div data-create-request={createRequest}>Group editor</div>
  ),
}));
vi.mock("@/lib/api/routingTrace", () => ({
  routingTraceApi: { snapshot: vi.fn(), history: vi.fn() },
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: { models: vi.fn(), list: vi.fn() },
}));
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: { list: vi.fn() },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getProxyStatus: vi.fn(async () => ({
      running: true,
      address: "127.0.0.1",
      port: 15722,
    })),
  },
}));
vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getRequestLogs: vi.fn(async () => ({
      data: [],
      total: 0,
      page: 0,
      pageSize: 80,
    })),
  },
}));

const route = {
  id: "request-a",
  startedAt: Date.now(),
  agent: "codex",
  session: "opaque-session-a",
  model: "group/research",
  groupName: "Research",
  strategy: "ordered",
  state: "completed",
  status: 200,
  durationMs: 1240,
  firstTokenMs: 200,
  inputTokens: 100,
  outputTokens: 20,
  cacheReadTokens: 40,
  estimatedTokens: 200,
  candidates: [
    {
      providerId: "provider-a",
      providerName: "Synthetic primary",
      model: "model-a",
      contextWindow: 128000,
    },
    {
      providerId: "provider-b",
      providerName: "Synthetic fallback",
      model: "model-b",
      contextWindow: 1000000,
    },
  ],
  steps: [
    { kind: "strategy", atMs: 2 },
    { kind: "attempt", atMs: 3, candidate: 0 },
    { kind: "rejected", atMs: 100, candidate: 0, status: 429 },
    { kind: "attempt", atMs: 101, candidate: 1 },
    { kind: "completed", atMs: 1240, candidate: 1, status: 200 },
  ],
};

function show(
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
      <RoutingPage />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  vi.mocked(routingTraceApi.history).mockResolvedValue({
    requests: [],
    cut: false,
  });
  vi.mocked(usageApi.getRequestLogs).mockClear();
  vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([]);
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
});

it("keeps the live overview and group editor without request details, history or accounts", async () => {
  show();
  const stage = screen.getByRole("region", { name: "routingPage.stage" });
  await within(stage).findAllByText("Synthetic fallback");
  expect(
    within(stage).getByText("routingPage.state.completed"),
  ).toBeInTheDocument();
  expect(screen.getByText("Group editor")).toBeInTheDocument();
  expect(stage.querySelector(".rt-log")).toBeNull();
  expect(document.querySelector(".rt-cols")).toBeNull();
  expect(
    screen.queryByRole("region", { name: "routingPage.accounts" }),
  ).toBeNull();
  for (const name of [
    "routingPage.replay",
    "routingPage.purpose",
    "routingPage.bySession",
  ]) {
    expect(screen.queryByRole("button", { name })).toBeNull();
  }
  expect(screen.queryByText("routingPage.metrics")).toBeNull();
  expect(routingTraceApi.history).not.toHaveBeenCalled();
});

it("requests a new group from the toolbar without scrolling the page", async () => {
  const originalScroll = Element.prototype.scrollIntoView;
  const scrollIntoView = vi.fn();
  Element.prototype.scrollIntoView = scrollIntoView;
  try {
    show();
    await userEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    expect(screen.getByText("Group editor")).toHaveAttribute(
      "data-create-request",
      "1",
    );
    expect(scrollIntoView).not.toHaveBeenCalled();
  } finally {
    Element.prototype.scrollIntoView = originalScroll;
  }
});

it("follows refreshed live requests and keeps summary counts without legacy list preferences", async () => {
  localStorage.setItem("vary.routingBySession", "1");
  localStorage.setItem("vary.routingMetrics", "[]");
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  show(client);
  await screen.findAllByText("Synthetic fallback");
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      { ...route, id: "new-request", model: "new-model", state: "failed" },
      route,
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  await client.invalidateQueries({ queryKey: ["routing-trace"] });
  await waitFor(() =>
    expect(document.querySelector(".rt-what")).toHaveTextContent("new-model"),
  );
  expect(screen.getByText("routingPage.state.failed")).toBeInTheDocument();
  const stats = document.querySelector(".rt-stats") as HTMLElement;
  expect(within(stats).getByText("routingPage.requests")).toHaveTextContent(
    "2routingPage.requests",
  );
  expect(within(stats).getByText("routingPage.rerouted")).toHaveTextContent(
    "2routingPage.rerouted",
  );
  expect(within(stats).getByText("routingPage.errors")).toHaveTextContent(
    "1routingPage.errors",
  );
  expect(routingTraceApi.history).not.toHaveBeenCalled();
});

function configuredCatalog() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const providers = ["primary", "secondary", "unrelated"].map((id) => ({
    schemaVersion: 1 as const,
    id,
    name: `Synthetic ${id}`,
    enabled: true,
    revision: 1,
    hasApiKey: false,
    source: { kind: "codex" as const },
  }));
  const connections = ["codex", "gemini"].map((app) => ({
    app,
    enabled: true,
    configured: true,
    recoveryPending: false,
    model: null,
    restartRequired: false,
  }));
  vi.mocked(agentConnectionsApi.list).mockResolvedValue(connections);
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue(providers);
  client.setQueryData(["agent-connections"], connections);
  client.setQueryData(["routing-upstreams"], providers);
  return client;
}

it("loads supplier icons by provider ID for recorded candidates", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "provider-a",
      name: "Configured supplier",
      revision: 1,
      hasApiKey: true,
      icon: "deepseek",
      source: { kind: "codex" },
    },
  ]);
  show();
  await screen.findByTitle("Configured supplier");
  expect(
    document.querySelector('.rt-seat-icon [title="Configured supplier"] svg'),
  ).not.toBeNull();
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(2);
});

it("keeps idle topology empty even when connected clients and providers are configured", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  });
  show(configuredCatalog());
  await screen.findByText("routingPage.emptyCandidates");
  expect(document.querySelectorAll(".rt-src")).toHaveLength(1);
  expect(document.querySelector(".rt-src")).toHaveTextContent(
    "routingPage.agent",
  );
  expect(document.querySelectorAll(".rt-accts li:not(.idle)")).toHaveLength(0);
  expect(routingUpstreamsApi.list).not.toHaveBeenCalled();
  expect(agentConnectionsApi.list).not.toHaveBeenCalled();
  const stage = screen.getByRole("region", { name: "routingPage.stage" });
  expect(within(stage).getByText("routingPage.waiting").tagName).toBe("B");
  expect(
    within(stage).queryByText("routingPage.stageHint"),
  ).not.toBeInTheDocument();
  expect(within(stage).getByText("routingPage.emptyStage")).toHaveClass(
    "rt-mode",
  );
  expect(stage.querySelector(".rt-cap")).not.toBeInTheDocument();
  expect(stage.querySelector(".rt-foot")).toHaveClass("idle");
});

it("shows only the recorded destination for a direct request", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        candidates: [route.candidates[0]],
        steps: [
          { kind: "attempt", candidate: 0, atMs: 1 },
          { kind: "completed", candidate: 0, atMs: 20 },
        ],
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show(configuredCatalog());
  await screen.findByText("routingPage.singleSeat");
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(1);
  expect(document.querySelector(".rt-accts")).toHaveTextContent(
    "Synthetic primary",
  );
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent(
    "Synthetic secondary",
  );
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent(
    "Synthetic unrelated",
  );
  expect(document.querySelectorAll(".rt-src")).toHaveLength(1);
});

it("keeps cancellation neutral and hides HTTP 499 while retaining actual upstream errors", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        state: "cancelled",
        status: 499,
        steps: [
          ...route.steps.slice(0, -1),
          { kind: "cancelled", candidate: 1, atMs: 1240, status: 499 },
        ],
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findByText("routingPage.state.cancelled");
  const stage = screen.getByRole("region", { name: "routingPage.stage" });
  expect(stage).not.toHaveTextContent("499");
  expect(stage).toHaveTextContent("429");
});

it("does not replace a candidate-free failure with configured providers or earlier destinations", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        id: "early-failure",
        state: "failed",
        status: 409,
        candidates: [],
        steps: [{ kind: "failed", atMs: 0, status: 409 }],
      },
      route,
    ],
    accounts: [
      {
        providerId: "older",
        providerName: "Synthetic earlier destination",
        model: "model-a",
        restUntil: 0,
        windows: [],
      },
    ],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show(configuredCatalog());
  await screen.findByText("routingPage.noCandidatesFailed");
  expect(document.querySelectorAll(".rt-accts li:not(.idle)")).toHaveLength(0);
  expect(document.querySelector(".rt-accts")).toHaveTextContent("409");
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent(
    "Synthetic",
  );
});

it("shows the catalog model name throughout a direct request, retaining the internal alias only in tooltips", async () => {
  const internal = `vary-auto/model-${"a".repeat(64)}`;
  const row = {
    ...route,
    model: internal,
    groupName: "Direct model",
    candidates: [
      {
        providerId: "fixture",
        providerName: "Synthetic vendor",
        model: "vendor/flash-wire",
      },
    ],
    steps: [
      { kind: "attempt", atMs: 1, candidate: 0 },
      { kind: "completed", atMs: 20, candidate: 0, status: 200 },
    ],
  };
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [row],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  vi.mocked(agentConnectionsApi.models).mockResolvedValue([
    {
      id: internal.replace("vary-auto/", "vary-provider/"),
      name: "Friendly Flash · Synthetic vendor",
      group: "Synthetic vendor",
      contextWindow: 1000000,
    },
  ]);
  show();
  expect(await screen.findAllByText("Friendly Flash")).toHaveLength(2);
  for (const label of document.querySelectorAll(".rt-model-label")) {
    expect(label).toHaveTextContent("Friendly Flash");
    expect(label).toHaveAttribute("title", internal);
  }
  expect(screen.queryByText(internal)).not.toBeInTheDocument();
  expect(row.model).toBe(internal);
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.hideAccounts" }),
  );
  expect(screen.getAllByText(/Synthetic vendor/).length).toBeGreaterThan(0);
  expect(screen.getAllByText("Friendly Flash")).toHaveLength(2);
});

it("uses the recorded group name rather than a hash or the last fallback model", async () => {
  const internal = `vary-auto/auto-${"b".repeat(64)}`;
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [{ ...route, model: internal, groupName: "Research pool" }],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findAllByText("Research pool");
  const labels = document.querySelectorAll(".rt-model-label");
  expect(labels).toHaveLength(1);
  for (const label of labels) {
    expect(label).toHaveTextContent("Research pool");
    expect(label).toHaveAttribute("title", internal);
  }
  expect(screen.queryByText(internal)).not.toBeInTheDocument();
});

it("does not expose an opaque model alias when no readable model metadata remains", async () => {
  const internal = `vary-auto/model-${"d".repeat(64)}`;
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        model: internal,
        groupName: "Direct model",
        candidates: [],
        steps: [],
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  expect(
    await screen.findAllByText("routingPage.modelUnavailable"),
  ).toHaveLength(2);
  expect(screen.queryByText(internal)).not.toBeInTheDocument();
});

it("shows the latest request and its recorded fallback destinations", async () => {
  show();
  await screen.findAllByText("Synthetic fallback");
  const stage = screen.getByRole("region", { name: "routingPage.stage" });
  expect(
    within(stage).getAllByText("Synthetic primary").length,
  ).toBeGreaterThan(0);
  expect(
    within(stage).getAllByText("Synthetic fallback").length,
  ).toBeGreaterThan(0);
  expect(within(stage).getAllByText(/429/).length).toBeGreaterThan(0);
  expect(
    within(stage).getByText("routingPage.state.completed"),
  ).toBeInTheDocument();
  expect(stage.querySelector(".live")).toBeNull();
  expect(screen.getByText("Group editor")).toBeInTheDocument();
});

it("masks recorded key labels in the stage and its tooltip", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        candidates: [
          {
            ...route.candidates[0],
            account: {
              id: "opaque-key",
              kind: "key",
              label: "sk-4…766b",
              providerId: "provider-a",
              providerName: "Synthetic primary",
              model: "model-a",
              restUntil: 0,
              windows: [],
            },
          },
        ],
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findAllByText("sk-4…766b");
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.hideAccounts" }),
  );
  expect(document.querySelector(".rt-accts")?.innerHTML).not.toContain(
    "sk-4…766b",
  );
  expect(document.querySelector(".rt-accts .account-mask")).not.toBeNull();
});

it("preserves vendor names while masking account addresses", async () => {
  const name = "Codex · fixture.account@example.test";
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        candidates: route.candidates.map((candidate, index) =>
          index ? candidate : { ...candidate, providerName: name },
        ),
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  const view = show();
  await screen.findAllByText("Synthetic fallback");
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.hideAccounts" }),
  );
  expect(screen.getAllByText("Synthetic fallback").length).toBeGreaterThan(0);
  expect(document.querySelector(".account-mask")).not.toBeNull();
  expect(view.container.innerHTML).not.toContain(
    "fixture.account@example.test",
  );
  expect(
    document.querySelector(".rt-accts li")?.getAttribute("title"),
  ).toContain("Codex ·");
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.showAccounts" }),
  );
  expect(screen.getAllByText(name).length).toBeGreaterThan(0);
});

it("shows a trace failure explicitly without inventing requests or moving packets", async () => {
  vi.mocked(routingTraceApi.snapshot).mockRejectedValue(
    new Error("unavailable"),
  );
  show();
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "routingPage.failed",
  );
  expect(screen.queryByText("Synthetic fallback")).not.toBeInTheDocument();
  expect(document.querySelector(".rt-wires .live")).toBeNull();
});

it("never reads imported session usage or renders it as gateway traffic", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  });
  show();
  await screen.findByText("routingPage.emptyCandidates");
  expect(usageApi.getRequestLogs).not.toHaveBeenCalled();
  expect(document.querySelectorAll(".rt-req")).toHaveLength(0);
  expect(
    screen.queryByRole("region", { name: "routingPage.accounts" }),
  ).toBeNull();
  expect(screen.queryByText("routingPage.state.completed")).toBeNull();
});

it("clears live traffic and counters while the gateway is stopped", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route],
    accounts: [],
    capacity: 60,
    running: false,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  expect(
    await screen.findAllByText("routingPage.gatewayStopped"),
  ).not.toHaveLength(0);
  expect(document.querySelectorAll(".rt-req")).toHaveLength(0);
  expect(document.querySelector(".rt-livedot")).toHaveClass("off");
  expect(screen.queryByText("routingPage.state.completed")).toBeNull();
  expect(document.querySelector(".rt-stats")).toHaveTextContent(
    "0routingPage.requests",
  );
  expect(routingTraceApi.history).not.toHaveBeenCalled();
});

it.each([
  { model: "auto/other-group" },
  { session: "another-session" },
  { agent: "gemini" },
])("isolates the focused request from another route with %o", async (scope) => {
  const other = {
    ...route,
    id: "other-request",
    ...scope,
    state: "routing",
    status: 503,
    candidates: [
      {
        providerId: "unrelated",
        providerName: "Unrelated supplier",
        model: "other-model",
      },
    ],
    steps: [
      { kind: "attempt", candidate: 0, atMs: 1 },
      { kind: "failed", candidate: 0, atMs: 2, status: 503 },
    ],
  };
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route, other],
    accounts: [],
    running: true,
    capacity: 60,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findAllByText("Synthetic fallback");
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent(
    "Unrelated supplier",
  );
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent("503");
});
it("masks configured API supplier names even when there are no login accounts", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "provider-a",
      name: "Synthetic primary",
      revision: 1,
      hasApiKey: true,
      source: {
        kind: "api",
        baseUrl: "https://fixture.invalid/v1",
        apiFormat: "openai_chat",
        mode: "picked",
        models: [],
        catalog: [],
        groupsOnly: false,
      },
    },
  ]);
  const view = show();
  await screen.findAllByText("Synthetic primary");
  await waitFor(() => expect(routingUpstreamsApi.list).toHaveBeenCalled());
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.hideAccounts" }),
  );
  await waitFor(() =>
    expect(document.querySelector(".rt-accts .account-mask")).not.toBeNull(),
  );
  expect(view.container.innerHTML).not.toContain("Synthetic primary");
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.showAccounts" }),
  );
  expect(screen.getAllByText("Synthetic primary").length).toBeGreaterThan(0);
});

it("masks duplicate supplier labels and email-shaped IDs in the stage and tooltips", async () => {
  const providers = ["supplier-a", "fixture.account@example.test"].map(
    (id) => ({
      schemaVersion: 1 as const,
      id,
      name: "Synthetic relay",
      revision: 1,
      hasApiKey: false,
      source: { kind: "codex" as const },
    }),
  );
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue(providers);
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        candidates: providers.map((provider) => ({
          providerId: provider.id,
          providerName: provider.name,
          model: "shared-model",
        })),
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findByText("Synthetic relay · supplier-a");
  await waitFor(() => expect(routingUpstreamsApi.list).toHaveBeenCalled());
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.hideAccounts" }),
  );
  const seats = document.querySelector(".rt-accts")!;
  await waitFor(() =>
    expect(seats.innerHTML).not.toContain("Synthetic relay"),
  );
  expect(seats.querySelector(".account-mask")).not.toBeNull();
  expect(seats.innerHTML).not.toContain("fixture.account@example.test");
  expect(seats.textContent).toContain("supplier-a");
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.showAccounts" }),
  );
  expect(screen.getByText("Synthetic relay · supplier-a")).toBeInTheDocument();
  expect(
    screen.getByText("Synthetic relay · fixture.account@example.test"),
  ).toBeInTheDocument();
});
