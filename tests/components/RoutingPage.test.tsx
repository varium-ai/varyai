import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingPage } from "@/components/routing/RoutingPage";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { usageApi } from "@/lib/api/usage";
import { agentConnectionsApi } from "@/lib/api/agentConnections";

vi.mock("@/components/routing/IntentRoutingPanel", () => ({
  IntentRoutingPanel: () => <div>Group editor</div>,
}));
vi.mock("@/lib/api/routingTrace", () => ({
  routingTraceApi: { snapshot: vi.fn(), history: vi.fn() },
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: { models: vi.fn() },
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
  vi.mocked(routingTraceApi.snapshot).mockResolvedValue({
    requests: [route],
    accounts: [],
    capacity: 60,
    running: true,
    days: ["2026-11-02"],
  } as Awaited<ReturnType<typeof routingTraceApi.snapshot>>);
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
  expect(screen.queryByText(/Synthetic vendor/)).not.toBeInTheDocument();
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

it("masks account names throughout the trace and persists the request grouping preference", async () => {
  const view = show();
  await screen.findAllByText("Synthetic fallback");
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.hideAccounts" }),
  );
  expect(screen.queryByText("Synthetic fallback")).not.toBeInTheDocument();
  expect(screen.queryByText("Synthetic primary")).not.toBeInTheDocument();
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
