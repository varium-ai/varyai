import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, beforeEach, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { TrayPanel } from "@/components/tray/TrayPanel";
import { trayPanelApi } from "@/lib/trayPanel";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { securityEventsApi } from "@/lib/api/securityEvents";
import type { RoutingTrace } from "@/lib/api/routingTrace";
import {
  intentRoutingApi,
  type RoutingCatalog,
  type RoutingGroup,
} from "@/lib/api/intentRouting";

const updateState = vi.hoisted(() => ({
  isChecking: false,
  hasUpdate: false,
  checkUpdate: vi.fn(async () => false),
  resetDismiss: vi.fn(),
}));

vi.mock("@/components/agents/AgentConnectionsPage", () => ({
  AgentConnectionsPage: ({ compact }: { compact: boolean }) => (
    <div data-testid="compact-agents">{String(compact)}</div>
  ),
}));
vi.mock("@/components/profiles/ProfileSwitcher", () => ({
  ProfileSwitcher: ({ side }: { side: string }) => <div>{side}</div>,
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => updateState,
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    list: vi.fn(async () => []),
    models: vi.fn(async () => []),
  },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { getProxyStatus: vi.fn(async () => ({ running: false })) },
}));
vi.mock("@/lib/api/routingTrace", () => ({
  routingTraceApi: {
    snapshot: vi.fn(async () => ({
      requests: [],
      accounts: [],
      running: false,
      days: [],
      capacity: 0,
    })),
  },
}));
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    list: vi.fn<() => Promise<RoutingCatalog>>(),
    save: vi.fn(async (group: RoutingGroup) => group),
  },
}));
vi.mock("@/lib/api/security", () => ({
  securityApi: { getSettings: vi.fn(async () => ({ enabled: true })) },
}));
vi.mock("@/lib/api/securityEvents", () => ({
  securityEventsApi: {
    getEvents: vi.fn(async () => ({ items: [], total: 0 })),
  },
}));
vi.mock("@/lib/trayPanel", async (original) => ({
  ...(await original<typeof import("@/lib/trayPanel")>()),
  trayPanelApi: {
    hide: vi.fn(async () => {}),
    resize: vi.fn(async () => {}),
    open: vi.fn(async () => {}),
    quit: vi.fn(async () => {}),
  },
}));
const scrollIntoView = Object.getOwnPropertyDescriptor(
  Element.prototype,
  "scrollIntoView",
);
beforeAll(() => {
  Object.defineProperty(Element.prototype, "scrollIntoView", {
    value: vi.fn(),
    configurable: true,
  });
});
afterAll(() => {
  if (scrollIntoView)
    Object.defineProperty(Element.prototype, "scrollIntoView", scrollIntoView);
  else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});
beforeEach(() => {
  localStorage.clear();
  updateState.isChecking = false;
  updateState.hasUpdate = false;
  vi.mocked(intentRoutingApi.list).mockResolvedValue({
    groups: [],
    autoCreate: false,
    omittedGroups: 0,
  });
  vi.mocked(securityEventsApi.getEvents).mockResolvedValue({
    items: [],
    total: 0,
    page: 1,
    pageSize: 8,
  });
});
const group: RoutingGroup = {
  id: "synthetic-group",
  name: "Synthetic routing group",
  revision: 3,
  client: "global",
  enabled: true,
  routing: "smart",
  members: [
    { id: "first", providerId: "synthetic", modelId: "model-a" },
    { id: "second", providerId: "synthetic", modelId: "model-b" },
  ],
  defaultMemberId: "first",
  fallbackMemberIds: ["second"],
  intentRecognitionEnabled: false,
  classifierTarget: null,
  classificationBudgetMs: 2000,
  rules: [],
};
function mockGroups(groups: RoutingGroup[]) {
  vi.mocked(intentRoutingApi.list).mockResolvedValue({
    groups,
    autoCreate: false,
    omittedGroups: 0,
  });
}
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <TrayPanel />
    </QueryClientProvider>,
  );
}
it.each([false, true])(
  "hides the update entry without an available update while checking is %s",
  (isChecking) => {
    updateState.isChecking = isChecking;
    const { container } = mount();
    expect(container.querySelector(".tray-update")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "trayPanel.checkUpdate" }),
    ).not.toBeInTheDocument();
  },
);
it("shows the update entry only for an available version and reopens its dialog", () => {
  updateState.hasUpdate = true;
  mount();
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.update" }));
  expect(updateState.resetDismiss).toHaveBeenCalledOnce();
  expect(updateState.checkUpdate).not.toHaveBeenCalled();
});
it("renders exactly three tabs and compact agents, never the main sidebar", async () => {
  mount();
  expect(screen.getAllByRole("tab")).toHaveLength(3);
  expect(screen.getByTestId("compact-agents")).toHaveTextContent("true");
  expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  expect(screen.getByText("top")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  expect(
    await screen.findByText("routingPage.gatewayStopped"),
  ).toBeInTheDocument();
  expect(localStorage.getItem("vary:tray-tab")).toBe("routing");
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.openMain" }));
  expect(trayPanelApi.open).toHaveBeenCalledWith("routing");
});
it("opens with neutral focus and preserves keyboard tab navigation", () => {
  mount();
  expect(document.activeElement).toHaveClass("tray-panel");
  const agents = screen.getByRole("tab", { name: "trayPanel.agents" });
  agents.focus();
  fireEvent.keyDown(agents, { key: "ArrowRight" });
  const routing = screen.getByRole("tab", { name: "trayPanel.routing" });
  expect(routing).toHaveFocus();
  expect(routing).toHaveAttribute("aria-selected", "true");
});
it("keeps the native panel height fixed when content and tabs change", async () => {
  let resize = () => {};
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  try {
    const { container } = mount();
    const panel = container.querySelector<HTMLElement>(".tray-panel")!;
    const body = container.querySelector<HTMLElement>(".tray-body")!;
    const content = container.querySelector<HTMLElement>(".tray-content")!;
    Object.defineProperty(panel, "offsetHeight", { value: 560 });
    Object.defineProperty(body, "clientHeight", { value: 430 });
    Object.defineProperty(body, "scrollHeight", { value: 430 });
    Object.defineProperty(content, "scrollHeight", { value: 240 });
    body.style.padding = "0 8px 8px";
    resize();
    await new Promise((resolve) => setTimeout(resolve, 60));
    fireEvent.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
    await screen.findByText("routingPage.gatewayStopped");
    resize();
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(trayPanelApi.resize).not.toHaveBeenCalled();
  } finally {
    vi.unstubAllGlobals();
  }
});
it("shows configured protection separately from the stopped gateway", async () => {
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.security" }));
  expect(
    await screen.findByText("trayPanel.securityStopped"),
  ).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(trayPanelApi.hide).toHaveBeenCalled());
});
it("reads the first security-event page using the backend's one-based pagination", async () => {
  vi.mocked(securityEventsApi.getEvents).mockImplementation(
    async (_filter, page, pageSize) => {
      if (page < 1) throw new Error("SECURITY_EVENTS_INVALID_PARAMETERS");
      return { items: [], total: 0, page, pageSize };
    },
  );
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.security" }));
  expect(await screen.findByText("trayPanel.noEvents")).toBeVisible();
  await waitFor(() =>
    expect(securityEventsApi.getEvents).toHaveBeenCalledWith(
      { view: "security_events" },
      1,
      8,
    ),
  );
  expect(screen.queryByText("trayPanel.eventsFailed")).not.toBeInTheDocument();
});
it("identifies event loading failures and retries them without changing protection", async () => {
  vi.mocked(securityEventsApi.getEvents).mockRejectedValueOnce(
    new Error("SECURITY_EVENTS_DATABASE_ERROR"),
  );
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.security" }));
  expect(await screen.findByText("trayPanel.eventsFailed")).toBeVisible();
  expect(screen.getByText("trayPanel.securityStopped")).toBeVisible();
  expect(screen.queryByText("trayPanel.noEvents")).not.toBeInTheDocument();
  expect(screen.queryByText("trayPanel.failed")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.refresh" }));
  expect(await screen.findByText("trayPanel.noEvents")).toBeVisible();
  expect(screen.queryByText("trayPanel.eventsFailed")).not.toBeInTheDocument();
});
it("waits for the event query before showing an empty history", async () => {
  let finish!: (
    value: Awaited<ReturnType<typeof securityEventsApi.getEvents>>,
  ) => void;
  vi.mocked(securityEventsApi.getEvents).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.security" }));
  expect(screen.getByRole("status")).toHaveTextContent("common.loading");
  expect(screen.queryByText("trayPanel.noEvents")).not.toBeInTheDocument();
  finish({ items: [], total: 0, page: 1, pageSize: 8 });
  expect(await screen.findByText("trayPanel.noEvents")).toBeVisible();
});
it("lets inner dialogs consume Escape before dismissing the panel", async () => {
  mount();
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("data-state", "open");
  document.body.append(dialog);
  fireEvent.keyDown(window, { key: "Escape" });
  await new Promise((resolve) => setTimeout(resolve, 10));
  expect(trayPanelApi.hide).not.toHaveBeenCalled();
  dialog.remove();
});

it("keeps errors and cancellations distinct from successful requests", async () => {
  const request: RoutingTrace = {
    id: "early-failure",
    agent: "trae",
    model: "synthetic-model",
    groupName: "",
    strategy: "ordered",
    startedAt: Date.now(),
    durationMs: 10,
    candidates: [],
    steps: [{ kind: "failed", atMs: 10, status: 409 }],
    state: "failed",
    status: 409,
  };
  vi.mocked(routingTraceApi.snapshot).mockResolvedValueOnce({
    requests: [
      request,
      { ...request, id: "cancelled", state: "cancelled", status: 499 },
      { ...request, id: "http-error", state: "completed", status: 502 },
      { ...request, id: "completed", state: "completed", status: 200 },
    ],
    accounts: [],
    running: true,
    days: [],
    capacity: 8,
  });
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  const failed = await screen.findByRole("button", { name: /409/ });
  expect(failed).toHaveAttribute("aria-pressed", "true");
  expect(failed.querySelector(".tray-dot")).toHaveClass("bad");
  expect(
    screen.getByRole("button", { name: /499/ }).querySelector(".tray-dot"),
  ).toHaveAttribute("data-state", "cancelled");
  expect(
    screen.getByRole("button", { name: /502/ }).querySelector(".tray-dot"),
  ).toHaveClass("bad");
  const completed = screen.getByRole("button", { name: /200/ });
  expect(completed.querySelector(".tray-dot")).not.toHaveClass("bad");
  fireEvent.click(completed);
  expect(completed).toHaveAttribute("aria-pressed", "true");
  expect(failed).toHaveAttribute("aria-pressed", "false");
});

it("changes routing strategy through the application selector without losing group settings", async () => {
  mockGroups([group]);
  mount();
  const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  const strategy = await screen.findByRole("combobox", {
    name: `intentRouting.routingStrategy ${group.name}`,
  });
  expect(strategy.tagName).toBe("BUTTON");
  await user.click(strategy);
  expect(screen.getAllByRole("option")).toHaveLength(9);
  await user.click(
    screen.getByRole("option", { name: "routingPage.strategy.ordered" }),
  );
  await waitFor(() =>
    expect(intentRoutingApi.save).toHaveBeenCalledWith({
      ...group,
      routing: "ordered",
    }),
  );
});

it("dismisses the strategy menu with Escape before hiding the panel", async () => {
  mockGroups([group]);
  mount();
  const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  const strategy = await screen.findByRole("combobox", {
    name: `intentRouting.routingStrategy ${group.name}`,
  });
  await user.click(strategy);
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(trayPanelApi.hide).not.toHaveBeenCalled();
  expect(strategy).toHaveFocus();
  await user.keyboard("{Escape}");
  expect(trayPanelApi.hide).toHaveBeenCalledOnce();
});

it("preserves generated-group protection and disables changes while saving", async () => {
  mockGroups([
    group,
    { ...group, id: "generated", name: "Generated group", autoGenerated: true },
  ]);
  let finish!: (group: RoutingGroup) => void;
  vi.mocked(intentRoutingApi.save).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  mount();
  const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  const toggle = await screen.findByRole("switch", { name: group.name });
  expect(
    screen.getByRole("switch", { name: "Generated group" }),
  ).toBeDisabled();
  await user.click(toggle);
  expect(intentRoutingApi.save).toHaveBeenCalledWith({
    ...group,
    enabled: false,
  });
  expect(toggle).toBeDisabled();
  expect(
    screen.getByRole("combobox", {
      name: `intentRouting.routingStrategy ${group.name}`,
    }),
  ).toBeDisabled();
  finish({ ...group, enabled: false });
  await waitFor(() => expect(toggle).toBeEnabled());
});
