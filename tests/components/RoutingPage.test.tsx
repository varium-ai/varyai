import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingPage } from "@/components/routing/RoutingPage";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { usageApi } from "@/lib/api/usage";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

vi.mock("@/components/routing/IntentRoutingPanel", () => ({
  IntentRoutingPanel: () => <div>Group editor</div>,
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
  await screen.findByRole("button", { name: /group\/research/ });
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
  const cancelled = await screen.findByText("routingPage.step.cancelled");
  expect(cancelled.closest("li")).not.toHaveClass("bad");
  expect(document.querySelector(".rt-log")).not.toHaveTextContent("499");
  expect(document.querySelector(".rt-log")).toHaveTextContent("429");
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

it("keeps fallback branches tied to the request in live view, selection, replay and history", async () => {
  vi.mocked(routingTraceApi.history).mockResolvedValue({
    requests: [route],
    cut: false,
  } as Awaited<ReturnType<typeof routingTraceApi.history>>);
  show(configuredCatalog());
  await screen.findByRole("button", { name: /group\/research/ });
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(2);
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent(
    "Synthetic unrelated",
  );
  fireEvent.click(screen.getByRole("button", { name: /group\/research/ }));
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "routingPage.replay" }));
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(2);
  fireEvent.click(document.querySelectorAll(".rt-days > button")[1]);
  await screen.findByRole("button", { name: /group\/research/ });
  expect(document.querySelectorAll(".rt-accts li")).toHaveLength(2);
  expect(document.querySelector(".rt-accts")).not.toHaveTextContent(
    "Synthetic unrelated",
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
  expect(await screen.findAllByText("Friendly Flash")).toHaveLength(4);
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
  expect(screen.getAllByText("Friendly Flash")).toHaveLength(4);
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
  await screen.findByRole("button", { name: /Research pool/ });
  const labels = document.querySelectorAll(".rt-model-label");
  expect(labels).toHaveLength(3);
  for (const label of labels) {
    expect(label).toHaveTextContent("Research pool");
    expect(label).toHaveAttribute("title", internal);
  }
  expect(screen.queryByText(internal)).not.toBeInTheDocument();
});

it("resolves persisted direct aliases from their recorded wire model without a current provider", async () => {
  const internal = `vary-provider/model-${"c".repeat(64)}`;
  vi.mocked(agentConnectionsApi.models).mockRejectedValue(
    new Error("synthetic unavailable"),
  );
  vi.mocked(routingTraceApi.history).mockResolvedValue({
    requests: [
      {
        ...route,
        model: internal,
        groupName: "Direct model",
        candidates: [
          {
            providerId: "removed",
            providerName: "Removed provider",
            model: "vendor/old-model",
          },
        ],
        steps: [
          { kind: "attempt", atMs: 1, candidate: 0 },
          { kind: "completed", atMs: 20, candidate: 0, status: 200 },
        ],
      },
    ] as Awaited<ReturnType<typeof routingTraceApi.history>>["requests"],
    cut: false,
  });
  show();
  await screen.findByRole("button", { name: /group\/research/ });
  fireEvent.click(document.querySelectorAll(".rt-days > button")[1]);
  await screen.findAllByText("vendor/old-model");
  for (const label of document.querySelectorAll(".rt-model-label")) {
    expect(label).toHaveTextContent("vendor/old-model");
    expect(label).toHaveAttribute("title", internal);
  }
  expect(screen.queryByText(internal)).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "routingPage.replay" }),
  ).toBeInTheDocument();
  expect(routingTraceApi.history).toHaveBeenCalledWith("2026-11-02");
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
  ).toHaveLength(4);
  expect(screen.queryByText(internal)).not.toBeInTheDocument();
});

it("distinguishes an available account moved to the back from one still cooling down", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        candidates: [
          {
            providerId: "fixture",
            providerName: "Synthetic",
            model: "model-a",
            account: {
              id: "fixture-key",
              kind: "key",
              providerId: "fixture",
              providerName: "Synthetic",
              model: "model-a",
              restUntil: 0,
              sunkAt: 100,
              windows: [],
            },
          },
        ],
      },
    ],
    accounts: [
      {
        providerId: "fixture",
        providerName: "Synthetic",
        model: "model-a",
        restUntil: 0,
        sunkAt: 100,
        windows: [],
      },
    ],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  expect(await screen.findByText("routingPage.sunk")).toBeInTheDocument();
});

it("selects a real request and links the stage to its recorded fallback decisions", async () => {
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: /group\/research/ }),
  );
  const stage = screen.getByRole("region", { name: "routingPage.stage" });
  expect(
    within(stage).getAllByText("Synthetic primary").length,
  ).toBeGreaterThan(0);
  expect(
    within(stage).getAllByText("Synthetic fallback").length,
  ).toBeGreaterThan(0);
  expect(within(stage).getAllByText(/429/).length).toBeGreaterThan(0);
  expect(
    within(stage).getByText("routingPage.step.completed"),
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

it("preserves vendor names while masking account addresses and persists the request grouping preference", async () => {
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
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.bySession" }),
  );
  expect(localStorage.getItem("vary.routingBySession")).toBe("1");
  view.unmount();
  show();
  expect(
    screen.getByRole("button", { name: "routingPage.bySession" }),
  ).toHaveAttribute("aria-pressed", "true");
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

it("hides the purpose filter for unmarked requests regardless of agent, errors or retries", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      route,
      { ...route, id: "failed", agent: "claude", state: "failed" },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findAllByText("Synthetic fallback");
  expect(screen.queryByLabelText("routingPage.purpose")).toBeNull();
  expect(screen.queryByRole("combobox")).toBeNull();
});

it("groups equivalent call kinds into one purpose instead of showing a redundant filter", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      { ...route, kind: "thread_title" },
      { ...route, id: "title-alias", kind: "title_generation" },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findAllByText("Synthetic fallback");
  expect(screen.queryByLabelText("routingPage.purpose")).toBeNull();
});

it("multi-selects recorded purposes without closing the menu and clears back to all requests", async () => {
  const user = userEvent.setup();
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      { ...route, model: "unmarked-model" },
      { ...route, id: "title", model: "title-model", kind: "title_generation" },
      { ...route, id: "memory", model: "memory-model", kind: "memgen" },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await user.click(
    await screen.findByRole("button", { name: "routingPage.purpose" }),
  );
  expect(
    screen.queryByRole("menuitemcheckbox", { name: "routingPage.errors" }),
  ).toBeNull();
  expect(
    screen.queryByRole("menuitemcheckbox", { name: "routingPage.rerouted" }),
  ).toBeNull();
  expect(screen.queryByRole("menuitemcheckbox", { name: "codex" })).toBeNull();
  await user.click(
    screen.getByRole("menuitemcheckbox", {
      name: "routingPage.purposes.thread_title",
    }),
  );
  expect(screen.getByRole("menu")).toBeInTheDocument();
  expect(
    screen.getByRole("menuitemcheckbox", {
      name: "routingPage.purposes.thread_title",
    }),
  ).toHaveAttribute("aria-checked", "true");
  expect(document.querySelectorAll(".rt-req")).toHaveLength(1);
  expect(document.querySelector(".rt-what")).toHaveTextContent("title-model");
  await user.click(
    screen.getByRole("menuitemcheckbox", {
      name: "routingPage.purposes.memory_consolidation",
    }),
  );
  expect(document.querySelectorAll(".rt-req")).toHaveLength(2);
  await user.click(
    screen.getByRole("menuitemcheckbox", { name: "routingPage.allPurposes" }),
  );
  expect(document.querySelectorAll(".rt-req")).toHaveLength(3);
  expect(
    screen.queryByRole("button", { name: "routingPage.clearPurpose" }),
  ).toBeNull();
  await user.click(
    screen.getByRole("menuitemcheckbox", {
      name: "routingPage.purposes.unmarked",
    }),
  );
  await user.keyboard("{Escape}");
  expect(document.querySelector(".rt-what")).toHaveTextContent(
    "unmarked-model",
  );
  await user.click(
    screen.getByRole("button", { name: "routingPage.clearPurpose" }),
  );
  expect(document.querySelectorAll(".rt-req")).toHaveLength(3);
  expect(
    screen.getByRole("button", { name: "routingPage.purpose" }),
  ).toHaveFocus();
});

it("keeps an active purpose filter visible when refreshed records no longer match", async () => {
  const user = userEvent.setup();
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route, { ...route, id: "title", kind: "thread_title" }],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show(client);
  await user.click(
    await screen.findByRole("button", { name: "routingPage.purpose" }),
  );
  await user.click(
    screen.getByRole("menuitemcheckbox", {
      name: "routingPage.purposes.thread_title",
    }),
  );
  await user.keyboard("{Escape}");
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  });
  await client.invalidateQueries({ queryKey: ["routing-trace"] });
  expect(await screen.findByText("routingPage.noRequests")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "routingPage.selectedPurposes" }),
  ).toBeInTheDocument();
  expect(document.querySelector(".rt-log")).toBeNull();
  await user.click(
    screen.getByRole("button", { name: "routingPage.selectedPurposes" }),
  );
  expect(
    screen.getByRole("menuitemcheckbox", {
      name: "routingPage.purposes.thread_title",
    }),
  ).toHaveAttribute("aria-checked", "true");
});

it("closes the purpose menu on outside clicks and preserves metric preferences", async () => {
  const user = userEvent.setup();
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route, { ...route, id: "title", kind: "thread_title" }],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await user.click(
    await screen.findByRole("button", { name: "routingPage.purpose" }),
  );
  await user.click(document.body);
  expect(screen.queryByRole("menu")).toBeNull();
  fireEvent.click(screen.getByText("routingPage.metrics"));
  fireEvent.click(screen.getByLabelText("routingPage.metric.tokens"));
  expect(localStorage.getItem("vary.routingMetrics")).not.toContain('"tokens"');
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
  await screen.findByText("routingPage.noRequests");
  expect(usageApi.getRequestLogs).not.toHaveBeenCalled();
  expect(document.querySelectorAll(".rt-req")).toHaveLength(0);
  expect(
    screen.queryByRole("region", { name: "routingPage.accounts" }),
  ).toBeNull();
  expect(screen.queryByText("routingPage.state.completed")).toBeNull();
});

it("changes the visible metrics without requiring a purpose filter", async () => {
  show();
  await screen.findAllByText("Synthetic fallback");
  expect(screen.queryByLabelText("routingPage.purpose")).toBeNull();
  fireEvent.click(screen.getByText("routingPage.metrics"));
  fireEvent.click(screen.getByLabelText("routingPage.metric.tokens"));
  expect(localStorage.getItem("vary.routingMetrics")).not.toContain('"tokens"');
});

it("renders future purpose names as literal text without interpreting markup or translation keys", async () => {
  const user = userEvent.setup();
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      route,
      {
        ...route,
        id: "future",
        kind: "future<&purpose>",
        model: "future-model",
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await user.click(
    await screen.findByRole("button", { name: "routingPage.purpose" }),
  );
  await user.click(
    screen.getByRole("menuitemcheckbox", { name: "future<&purpose>" }),
  );
  expect(document.querySelectorAll(".rt-req")).toHaveLength(1);
  expect(document.querySelector(".rt-what")).toHaveTextContent("future-model");
});

it("replays recorded decisions rather than exposing all future steps at once", async () => {
  show();
  fireEvent.click(
    await screen.findByRole("button", { name: "routingPage.replay" }),
  );
  expect(screen.getByText("routingPage.replaying")).toBeInTheDocument();
  expect(
    screen.queryByText("routingPage.step.completed"),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("routingPage.playhead"), {
    target: { value: "1100" },
  });
  expect(screen.getByText("routingPage.step.rejected")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "routingPage.stop" }));
  expect(screen.getByText("routingPage.step.completed")).toBeInTheDocument();
});

it("clears live traffic while the gateway is stopped but keeps real historical decisions replayable", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route],
    accounts: [],
    capacity: 60,
    running: false,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  vi.mocked(routingTraceApi.history).mockResolvedValue({
    requests: [route],
    cut: true,
  } as Awaited<ReturnType<typeof routingTraceApi.history>>);
  show();
  expect(
    await screen.findAllByText("routingPage.gatewayStopped"),
  ).not.toHaveLength(0);
  expect(document.querySelectorAll(".rt-req")).toHaveLength(0);
  expect(document.querySelector(".rt-livedot")).toHaveClass("off");
  expect(screen.queryByText("routingPage.state.completed")).toBeNull();
  fireEvent.click(document.querySelectorAll(".rt-days > button")[1]);
  await screen.findByRole("button", { name: /group\/research/ });
  expect(screen.getByText("routingPage.historyCut")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "routingPage.replay" }));
  expect(screen.getByText("routingPage.replaying")).toBeInTheDocument();
});

it("omits unavailable metrics instead of showing zero latency or zero cost", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [
      {
        ...route,
        durationMs: 0,
        firstTokenMs: null,
        inputTokens: null,
        outputTokens: null,
        cacheReadTokens: null,
      },
    ],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findByRole("button", { name: /group\/research/ });
  const request = document.querySelector(".rt-req") as HTMLElement;
  expect(request.querySelectorAll(".meta > span")).toHaveLength(0);
  expect(request).not.toHaveTextContent("0.00s");
  expect(request).not.toHaveTextContent("$0.0000");
  expect(document.querySelector(".rt-log-head")).not.toHaveTextContent("0.00s");
});

it("lists only stored history days and shows a read failure without stale live requests", async () => {
  vi.mocked(routingTraceApi.history).mockRejectedValue(
    new Error("synthetic private path"),
  );
  show();
  await screen.findByRole("button", { name: /group\/research/ });
  expect(document.querySelectorAll(".rt-days > button")).toHaveLength(2);
  fireEvent.click(document.querySelectorAll(".rt-days > button")[1]);
  expect(
    await screen.findByText("routingPage.historyFailed"),
  ).toBeInTheDocument();
  expect(document.querySelectorAll(".rt-req")).toHaveLength(0);
  expect(screen.queryByText("synthetic private path")).toBeNull();
});

it("shows an exactly matched gateway cost without fetching or adding session usage rows", async () => {
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [{ ...route, costUsd: "0.0123" }],
    accounts: [],
    capacity: 60,
    running: true,
    days: [],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
  show();
  await screen.findByRole("button", { name: /group\/research/ });
  expect(document.querySelector(".rt-req .meta")).toHaveTextContent("$0.0123");
  expect(usageApi.getRequestLogs).not.toHaveBeenCalled();
  expect(document.querySelectorAll(".rt-req")).toHaveLength(1);
});
