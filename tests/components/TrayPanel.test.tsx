import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, beforeEach, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { APP_IDS } from "@/config/appConfig";
import { securityApi } from "@/lib/api/security";
import type {
  SecurityEvent,
  SecurityEventResult,
} from "@/types/securityEvents";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { TrayPanel } from "@/components/tray/TrayPanel";
import { trayPanelApi } from "@/lib/trayPanel";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
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
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: { list: vi.fn(async () => []) },
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
  vi.mocked(securityApi.getSettings).mockResolvedValue({ enabled: true });
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
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
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
function mount(
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
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
it("renders exactly four tabs and compact agents, never the main sidebar", async () => {
  mount();
  expect(screen.getAllByRole("tab")).toHaveLength(4);
  expect(screen.getByTestId("compact-agents")).toHaveTextContent("true");
  expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  expect(screen.getByText("top")).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "trayPanel.manageRouting" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  expect(
    await screen.findByText("routingPage.gatewayStopped"),
  ).toBeInTheDocument();
  expect(localStorage.getItem("vary:tray-tab")).toBe("routing");
  const details = screen.getByRole("button", {
    name: "trayPanel.manageRouting",
  });
  expect(details.closest("footer")).toBeInTheDocument();
  expect(details.nextElementSibling).toBe(
    screen.getByRole("button", { name: "trayPanel.quit" }),
  );
  fireEvent.click(details);
  expect(trayPanelApi.open).toHaveBeenCalledWith("routing");
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.openMain" }));
  expect(trayPanelApi.open).toHaveBeenCalledWith("routing");
});
it("opens with neutral focus and preserves keyboard tab navigation", () => {
  mount();
  expect(document.activeElement).toHaveClass("tray-panel");
  const agents = screen.getByRole("tab", { name: "trayPanel.agents" });
  agents.focus();
  fireEvent.keyDown(agents, { key: "ArrowRight" });
  const quota = screen.getByRole("tab", { name: "trayPanel.quota" });
  expect(quota).toHaveFocus();
  expect(quota).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(quota, { key: "ArrowRight" });
  const routing = screen.getByRole("tab", { name: "trayPanel.routing" });
  expect(routing).toHaveFocus();
  expect(routing).toHaveAttribute("aria-selected", "true");
});
it("opens and restores allowances, with provider navigation and four-tab keyboard wrapping", async () => {
  localStorage.setItem("vary:tray-tab", "quota");
  mount();
  const quota = screen.getByRole("tab", { name: "trayPanel.quota" });
  expect(quota).toHaveAttribute("aria-selected", "true");
  expect(await screen.findByText("trayPanel.quotaEmpty")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.openMain" }));
  expect(trayPanelApi.open).toHaveBeenCalledWith("providers");
  fireEvent.click(
    screen.getByRole("button", { name: "trayPanel.manageQuota" }),
  );
  expect(trayPanelApi.open).toHaveBeenCalledWith("providers");
  fireEvent.keyDown(quota, { key: "End" });
  const security = screen.getByRole("tab", { name: "trayPanel.security" });
  expect(security).toHaveFocus();
  fireEvent.keyDown(security, { key: "ArrowRight" });
  const agents = screen.getByRole("tab", { name: "trayPanel.agents" });
  expect(agents).toHaveFocus();
  fireEvent.keyDown(agents, { key: "ArrowLeft" });
  expect(security).toHaveFocus();
  fireEvent.keyDown(security, { key: "Home" });
  expect(agents).toHaveFocus();
});
it("loads allowances only when selected and refreshes every allowance query family", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  mount(client);
  expect(routingUpstreamsApi.list).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.quota" }));
  await screen.findByText("trayPanel.quotaEmpty");
  const refresh = screen.getByRole("button", { name: "trayPanel.refresh" });
  await waitFor(() => expect(refresh).toBeEnabled());
  fireEvent.click(refresh);
  for (const root of [
    "routing-upstreams",
    "provider-list-codex-accounts",
    "codex-provider-quota",
    "subscription-account-quota",
    "routing-provider-quota",
  ]) {
    expect(invalidate).toHaveBeenCalledWith({ queryKey: [root] });
  }
  await waitFor(() =>
    expect(routingUpstreamsApi.list).toHaveBeenCalledTimes(2),
  );
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
  const details = screen.getByRole("button", {
    name: "trayPanel.manageSecurity",
  });
  expect(details.closest("footer")).toBeInTheDocument();
  expect(details.nextElementSibling).toBe(
    screen.getByRole("button", { name: "trayPanel.quit" }),
  );
  fireEvent.click(details);
  expect(trayPanelApi.open).toHaveBeenCalledWith("security");
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
  const cancelled = document.querySelector('.tray-dot[data-state="cancelled"]');
  expect(cancelled).not.toHaveClass("bad");
  expect(cancelled?.closest("button")).not.toHaveTextContent("499");
  expect(
    screen.getByRole("button", { name: /502/ }).querySelector(".tray-dot"),
  ).toHaveClass("bad");
  const completed = screen.getByRole("button", { name: /200/ });
  expect(completed.querySelector(".tray-dot")).not.toHaveClass("bad");
  fireEvent.click(completed);
  expect(completed).toHaveAttribute("aria-pressed", "true");
  expect(failed).toHaveAttribute("aria-pressed", "false");
});

it("shows groups in the tray without a strategy control and preserves their saved settings", async () => {
  mockGroups([group]);
  mount();
  const user = userEvent.setup();
  await user.click(screen.getByRole("tab", { name: "trayPanel.routing" }));
  const toggle = await screen.findByRole("switch", { name: group.name });
  expect(
    screen.queryByRole("combobox", {
      name: `intentRouting.routingStrategy ${group.name}`,
    }),
  ).not.toBeInTheDocument();
  await user.click(toggle);
  await waitFor(() =>
    expect(intentRoutingApi.save).toHaveBeenCalledWith({
      ...group,
      enabled: false,
    }),
  );
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
  finish({ ...group, enabled: false });
  await waitFor(() => expect(toggle).toBeEnabled());
});

const eventCases = [
  ["redact", "redacted", "security.resultRedacted", "MEDIUM"],
  ["redact_restore", "redacted", "security.resultRedactRestore", "CRITICAL"],
  ["block", "blocked", "security.resultBlocked", "HIGH"],
  ["audit", "warning", "security.resultAlert", "LOW"],
  ["allow", "allowed", "security.actionAllow", null],
  ["unsupported-action", "unknown", "securityEvents.unknown", null],
  ["risky", "warning", "securityEvents.resultWarning", "MEDIUM"],
  ["unchecked", "unchecked", "toolRisk.verdict.unchecked", null],
  ["uncertain", "uncertain", "toolRisk.verdict.uncertain", "MEDIUM"],
  ["low_risk", "low_risk", "toolRisk.verdict.low_risk", "LOW"],
  ["unsupported-verdict", "unknown", "securityEvents.unknown", null],
  [null, "unknown", "securityEvents.notRecorded", null],
] as const;
function syntheticEvent(index: number, appType: string | null): SecurityEvent {
  const [action, result, , riskLevel] = eventCases[index];
  const common = {
    key: `synthetic:${index}`,
    timestamp: 1000 + index,
    lastSeenAt: 1000 + index,
    triggerCount: 1,
    appType,
    sessionId: "synthetic-session",
    riskLevel,
    result,
  };
  return index < 6
    ? {
        ...common,
        eventType: "dlp",
        record: {
          id: index,
          timestamp: common.timestamp,
          appType: appType ?? "",
          providerId: "synthetic-provider",
          ruleId: "synthetic-rule",
          ruleName: "Synthetic rule",
          ruleCategory: "custom",
          riskLevel: riskLevel ?? "unsupported-level",
          actionTaken: action ?? "unsupported-action",
          matchedSnippetMasked: "***",
        },
      }
    : {
        ...common,
        eventType: "tool_risk",
        record: {
          id: index,
          timestamp: common.timestamp,
          source: "proxy",
          stage: "model_response",
          detectionScope: "in_scope",
          integrityStatus: "complete",
          verdict: action,
          policyVersion: 1,
          ruleIds: [],
          suppressedCount: 0,
        },
      };
}
async function mountLocalizedEvents(
  language: string,
  tree: typeof zh | typeof en | typeof ja | typeof zhTW,
  items: SecurityEvent[],
) {
  const instance = createInstance();
  await instance.init({
    lng: language,
    fallbackLng: false,
    resources: { [language]: { translation: tree } },
  });
  vi.mocked(securityEventsApi.getEvents).mockResolvedValue({
    items,
    total: items.length,
    page: 1,
    pageSize: 8,
  });
  localStorage.setItem("vary:tray-tab", "security");
  const view = render(
    <I18nextProvider i18n={instance}>
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <TrayPanel />
      </QueryClientProvider>
    </I18nextProvider>,
  );
  await waitFor(() =>
    expect(view.container.querySelectorAll(".tray-events b")).toHaveLength(
      items.length,
    ),
  );
  return { ...view, instance };
}
it.each(
  APP_IDS.flatMap((app) =>
    [true, false].map((enabled) => [app, enabled] as const),
  ),
)(
  "localizes %s event history with protection enabled=%s without changing records",
  async (app, enabled) => {
    vi.mocked(securityApi.getSettings).mockResolvedValue({ enabled });
    const items = [syntheticEvent(0, app), syntheticEvent(6, app)];
    const original = structuredClone(items);
    const { container } = await mountLocalizedEvents("zh", zh, items);
    expect(container.querySelector(".tray-events")).toHaveTextContent(
      "已隐藏后发送",
    );
    expect(container.querySelector(".tray-events")).toHaveTextContent(
      "风险预警",
    );
    expect(container.querySelector(".tray-events")).toHaveTextContent(
      `${app} · 中`,
    );
    expect(
      screen.getByText(
        enabled ? zh.trayPanel.securityStopped : zh.trayPanel.securityOff,
      ),
    ).toBeVisible();
    expect(securityEventsApi.getEvents).toHaveBeenCalledWith(
      { view: "security_events" },
      1,
      8,
    );
    expect(items).toEqual(original);
  },
);
it.each(
  (
    [
      ["en", en],
      ["zh", zh],
      ["ja", ja],
      ["zh-TW", zhTW],
    ] as const
  ).flatMap(([language, tree]) =>
    [0, 6].map((offset) => ({ language, tree, offset })),
  ),
)(
  "renders actual $language translations for event batch $offset",
  async ({ language, tree, offset }) => {
    const items = eventCases
      .slice(offset, offset + 6)
      .map((_, index) => syntheticEvent(index + offset, "codex"));
    const { container, instance } = await mountLocalizedEvents(
      language,
      tree,
      items,
    );
    const rows = container.querySelectorAll(".tray-events > div");
    rows.forEach((row, index) => {
      const [, result, key, level] = eventCases[index + offset];
      expect(instance.exists(key), key).toBe(true);
      expect(row.querySelector("b")).toHaveTextContent(instance.t(key));
      const riskKey =
        level === "CRITICAL"
          ? "security.riskCritical"
          : level === "HIGH"
            ? "security.levelHighShort"
            : level === "MEDIUM"
              ? "security.levelMediumShort"
              : level === "LOW"
                ? "security.levelLowShort"
                : "securityEvents.unknown";
      expect(row.querySelector("small")).toHaveTextContent(
        `codex · ${instance.t(riskKey)}`,
      );
      expect(row.querySelector(".tray-dot")).toHaveAttribute(
        "data-result",
        result,
      );
    });
    expect(container.querySelector(".tray-events")?.textContent).not.toMatch(
      /securityEvents\.|security\.|toolRisk\./,
    );
  },
);
it("shows unknown historical risk and missing Agent identity safely", async () => {
  const event = syntheticEvent(5, null);
  event.riskLevel = "unsupported-level" as SecurityEvent["riskLevel"];
  event.result = "unsupported-result" as SecurityEventResult;
  const { container } = await mountLocalizedEvents("zh", zh, [event]);
  expect(container.querySelector(".tray-events b")).toHaveTextContent("未知");
  expect(container.querySelector(".tray-events small")).toHaveTextContent(
    "— · 未知",
  );
});
