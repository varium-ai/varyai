import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, beforeEach, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
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
import { readFileSync } from "node:fs";
import { calendarDays, localDateKey } from "@/components/tray/trayUsageData";
import { trayPanelApi } from "@/lib/trayPanel";
import { trayUsageApi, type TrayUsageRow } from "@/lib/api/trayUsage";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { securityEventsApi } from "@/lib/api/securityEvents";
import { settingsApi } from "@/lib/api/settings";
import type { Settings } from "@/types";

const updateState = vi.hoisted(() => ({
  isChecking: false,
  hasUpdate: false,
  checkUpdate: vi.fn(async () => false),
  resetDismiss: vi.fn(),
}));

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

vi.mock("@/components/agents/AgentConnectionsPage", () => ({
  AgentConnectionsPage: ({ compact }: { compact: boolean }) => (
    <div className="tray-agents" data-testid="compact-agents">
      {String(compact)}
      <div className="mp-agent-row">
        <div className="mp-agent-checkin" data-testid="compact-checkin" />
      </div>
    </div>
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
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { get: vi.fn(async () => ({})) },
}));
vi.mock("@/lib/api/trayUsage", async (original) => ({
  ...(await original<typeof import("@/lib/api/trayUsage")>()),
  trayUsageApi: { read: vi.fn(async () => []) },
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
  vi.mocked(securityEventsApi.getEvents).mockResolvedValue({
    items: [],
    total: 0,
    page: 1,
    pageSize: 8,
  });
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
  vi.mocked(trayUsageApi.read).mockResolvedValue([]);
});
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
it("opens Usage through the third tab and hands off to workspace statistics", async () => {
  mount();
  expect(screen.getAllByRole("tab")).toHaveLength(4);
  expect(screen.getByTestId("compact-agents")).toHaveTextContent("true");
  expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  expect(screen.getByText("top")).toBeInTheDocument();
  expect(trayUsageApi.read).not.toHaveBeenCalled();
  expect(screen.queryByRole("tab", { name: "trayPanel.routing" })).toBeNull();
  const usage = screen.getByRole("tab", { name: "trayPanel.usage" });
  expect(screen.getAllByRole("tab")[2]).toBe(usage);
  fireEvent.click(usage);
  expect(await screen.findByText("trayUsage.emptyToday")).toBeVisible();
  expect(localStorage.getItem("vary:tray-tab")).toBe("usage");
  expect(routingTraceApi.snapshot).not.toHaveBeenCalled();
  const details = screen.getByRole("button", {
    name: "trayUsage.open",
  });
  expect(details.closest("main")).toBeInTheDocument();
  fireEvent.click(details);
  expect(trayPanelApi.open).toHaveBeenNthCalledWith(1, "stats");
  fireEvent.click(screen.getByRole("button", { name: "trayPanel.openMain" }));
  expect(trayPanelApi.open).toHaveBeenNthCalledWith(2, "stats");
});
it("gives expanded check-in cards the full compact Agent row", () => {
  const sheet = document.createElement("style");
  sheet.textContent = readFileSync(
    "src/components/tray/tray-panel.css",
    "utf8",
  );
  document.head.appendChild(sheet);
  try {
    mount();
    const card = screen.getByTestId("compact-checkin");
    expect(getComputedStyle(card).gridColumn).toBe("1 / -1");
    expect(getComputedStyle(card).minWidth).toBe("0");
  } finally {
    sheet.remove();
  }
});
it("restores the former Routing selection into Usage and refreshes the current period", async () => {
  localStorage.setItem("vary:tray-tab", "routing");
  mount();
  expect(screen.getByRole("tab", { name: "trayPanel.usage" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await screen.findByText("trayUsage.emptyToday");
  fireEvent.click(screen.getByRole("button", { name: "trayUsage.week" }));
  await screen.findByText("trayUsage.empty");
  expect(trayUsageApi.read).toHaveBeenLastCalledWith("week");
  fireEvent.click(screen.getByRole("button", { name: "trayUsage.refresh" }));
  await waitFor(() =>
    expect(
      vi
        .mocked(trayUsageApi.read)
        .mock.calls.map(([period]) => period)
        .filter((period) => period === "week"),
    ).toHaveLength(2),
  );
});
it("refreshes Usage from the header and shows progress while it refetches", async () => {
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "trayPanel.usage" }));
  await screen.findByText("trayUsage.emptyToday");

  const refresh = screen.getByRole("button", { name: "trayPanel.refresh" });
  fireEvent.click(refresh);

  expect(refresh).toBeDisabled();
  expect(refresh).toHaveAttribute("aria-busy", "true");
  expect(refresh.querySelector("svg")).toHaveClass("animate-spin");
  await waitFor(() => {
    expect(refresh).toBeEnabled();
    expect(refresh).toHaveAttribute("aria-busy", "false");
  });
  const periods = vi.mocked(trayUsageApi.read).mock.calls.flat();
  expect(periods.filter((period) => period === "today")).toHaveLength(2);
  expect(periods.filter((period) => period === "activity")).toHaveLength(2);
});
it("confirms a toolbar refresh even when the totals have not changed", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  mount();
  await screen.findByText("trayUsage.emptyToday");
  let finishActivity!: (rows: TrayUsageRow[]) => void;
  const activity = new Promise<TrayUsageRow[]>((resolve) => {
    finishActivity = resolve;
  });
  vi.mocked(trayUsageApi.read).mockImplementation(async (period) =>
    period === "activity" ? activity : [],
  );
  const refresh = screen.getByRole("button", { name: "trayUsage.refresh" });
  fireEvent.click(refresh);
  expect(refresh).toBeDisabled();
  expect(refresh).toHaveAttribute("aria-busy", "true");
  expect(toast.success).not.toHaveBeenCalled();
  finishActivity([]);
  await waitFor(() =>
    expect(toast.success).toHaveBeenCalledWith("settings.autoReload"),
  );
  expect(refresh).toBeEnabled();
  expect(vi.mocked(trayUsageApi.read).mock.calls.flat().sort()).toEqual([
    "activity",
    "activity",
    "today",
    "today",
  ]);
});

it("reports a failed heatmap refresh without losing totals and allows retry", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  mount();
  await screen.findByText("trayUsage.emptyToday");
  vi.mocked(trayUsageApi.read).mockImplementation(async (period) => {
    if (period === "activity") throw new Error("fixture");
    return [usageFixture];
  });
  const refresh = screen.getByRole("button", { name: "trayUsage.refresh" });
  fireEvent.click(refresh);
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("trayUsage.error"),
  );
  expect(toast.success).not.toHaveBeenCalled();
  expect(screen.getByText("trayUsage.noneFailed")).toBeVisible();
  vi.mocked(trayUsageApi.read).mockResolvedValue([]);
  fireEvent.click(refresh);
  await screen.findByText("trayUsage.emptyToday");
  expect(toast.success).toHaveBeenCalledWith("settings.autoReload");
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
  const usage = screen.getByRole("tab", { name: "trayPanel.usage" });
  expect(usage).toHaveFocus();
  expect(usage).toHaveAttribute("aria-selected", "true");
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
    fireEvent.click(screen.getByRole("tab", { name: "trayPanel.usage" }));
    await screen.findByText("trayUsage.emptyToday");
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

const usageFixture: TrayUsageRow = {
  bucket: "09:00",
  providerId: "_codex_session",
  providerName: "Codex (Session)",
  model: "synthetic-session-model",
  requests: 3,
  failed: 0,
  input: 10,
  output: 5,
  reads: 80,
  writes: 5,
  cost: 0.03,
  unpriced: 0,
  firstMs: 1500,
  firstCount: 3,
  timedOutput: 5,
  generationMs: 1000,
  timedCount: 2,
};

it("allows long model IDs to shrink without widening the usage ranking", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  localStorage.setItem("vary:tray-usage-split", "model");
  const model = "syntheticmodel".repeat(16);
  vi.mocked(trayUsageApi.read).mockImplementation(async (period) =>
    period === "activity" ? [] : [{ ...usageFixture, model }],
  );
  const sheet = document.createElement("style");
  sheet.textContent = readFileSync(
    "src/components/tray/tray-usage.css",
    "utf8",
  );
  document.head.appendChild(sheet);
  try {
    mount();
    const label = await screen.findByTitle(model);
    const ranking = label.closest("ul");
    expect(ranking).not.toBeNull();
    expect(getComputedStyle(ranking!).gridTemplateColumns).toBe(
      "minmax(0, 1fr)",
    );
    expect(getComputedStyle(label).textOverflow).toBe("ellipsis");
  } finally {
    sheet.remove();
  }
});

it("shows a labeled 53-week heatmap with its own metric even if today is empty", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  const now = new Date();
  const weekday = (now.getDay() + 6) % 7;
  const dates = calendarDays(365 + weekday, now);
  vi.mocked(trayUsageApi.read).mockImplementation(async (period) =>
    period === "activity"
      ? [
          { ...usageFixture, bucket: dates[0], input: 900, requests: 1 },
          { ...usageFixture, bucket: dates.at(-1)!, input: 0, requests: 20 },
        ]
      : [],
  );
  mount();
  expect(await screen.findByText("trayUsage.emptyToday")).toBeVisible();
  const heatmap = await screen.findByRole("region", {
    name: "trayUsage.activity",
  });
  expect(within(heatmap).getByText("trayUsage.activityRange")).toBeVisible();
  expect(within(heatmap).getByText("周一")).toBeVisible();
  expect(within(heatmap).getByText("trayUsage.activityLess")).toBeVisible();
  expect(within(heatmap).getByText("trayUsage.activityMore")).toBeVisible();
  const oldest = heatmap.querySelector(`[data-date="${dates[0]}"]`);
  const today = heatmap.querySelector(`[data-date="${localDateKey(now)}"]`);
  expect(heatmap.querySelectorAll("[data-date]")).toHaveLength(dates.length);
  expect(oldest).toHaveAttribute("data-level", "4");
  expect(today).toHaveAttribute("data-level", "1");
  fireEvent.click(
    within(heatmap).getByRole("button", { name: "trayUsage.requests" }),
  );
  expect(oldest).toHaveAttribute("data-level", "1");
  expect(today).toHaveAttribute("data-level", "4");
  expect(localStorage.getItem("vary:tray-usage-heat-metric")).toBe("requests");
  expect(localStorage.getItem("vary:tray-usage-metric")).toBeNull();
  fireEvent.focus(today!);
  expect(within(heatmap).getByRole("tooltip")).toHaveTextContent(
    "trayUsage.callCount",
  );
  fireEvent.keyDown(today!, { key: "ArrowLeft" });
  expect(heatmap.querySelector(`[data-date="${dates.at(-8)}"]`)).toHaveFocus();
});

it("uses cache-inclusive totals, switches chart metrics and drills into provider models", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  vi.mocked(trayUsageApi.read).mockImplementation(async (period) =>
    period === "activity"
      ? []
      : [
          usageFixture,
          {
            ...usageFixture,
            providerId: "fixture",
            providerName: "Synthetic provider",
            model: "synthetic-gateway-model",
            requests: 1,
            failed: 1,
          },
        ],
  );
  const { container } = mount();
  const summary = (metric: string) =>
    container.querySelector(`[data-metric="${metric}"]`);
  const provider = await screen.findByRole("button", {
    name: /Synthetic provider/,
  });
  expect(summary("tokens")).toHaveTextContent("200");
  expect(summary("requests")).toHaveTextContent("4");
  expect(summary("cache")).toHaveTextContent("84%");
  expect(summary("cost")).toHaveTextContent("$0.06");
  fireEvent.click(screen.getByRole("button", { name: "trayUsage.cost" }));
  expect(screen.getByRole("img", { name: "09:00 · $0.06" })).toBeVisible();
  fireEvent.click(provider);
  expect(await screen.findByText("synthetic-gateway-model")).toBeVisible();
  expect(summary("tokens")).toHaveTextContent("100");
  expect(summary("requests")).toHaveTextContent("1");
  expect(screen.queryByRole("group", { name: "trayUsage.split" })).toBeNull();
  expect(localStorage.getItem("vary:tray-usage-metric")).toBe("cost");
});

it("dismisses the provider menu with Escape before hiding the panel", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  mount();
  const user = userEvent.setup();
  const picker = await screen.findByRole("combobox", {
    name: "trayUsage.provider",
  });
  await user.click(picker);
  expect(screen.getByRole("listbox")).toBeVisible();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(trayPanelApi.hide).not.toHaveBeenCalled();
  expect(picker).toHaveFocus();
  await user.keyboard("{Escape}");
  expect(trayPanelApi.hide).toHaveBeenCalledOnce();
});

it.each([
  ["tokens", "250", "150", "100"],
  ["cost", "$0.10", "$0.07", "$0.03"],
  ["requests", "8", "5", "3"],
] as const)(
  "shows the hourly %s breakdown and total on hover",
  async (metric, total, providerValue, sessionValue) => {
    localStorage.setItem("vary:tray-tab", "usage");
    localStorage.setItem("vary:tray-usage-metric", metric);
    vi.mocked(trayUsageApi.read).mockResolvedValue([
      usageFixture,
      {
        ...usageFixture,
        providerId: "fixture",
        providerName: "Synthetic provider",
        model: "synthetic-gateway-model",
        input: 60,
        requests: 5,
        cost: 0.07,
      },
      {
        ...usageFixture,
        bucket: "11:00",
        output: 1_000,
        requests: 50,
        cost: 1,
      },
    ]);
    mount();
    const column = await screen.findByRole("img", { name: `09:00 · ${total}` });
    fireEvent.pointerEnter(column);
    const tooltip = within(screen.getByRole("tooltip"));
    expect(
      tooltip.getByText(
        `${localDateKey(new Date()).replaceAll("-", "/")} 09:00`,
      ),
    ).toBeVisible();
    expect(
      tooltip.getAllByRole("listitem").map((item) => item.textContent),
    ).toEqual([
      `Synthetic provider${providerValue}`,
      `trayUsage.sessions${sessionValue}`,
    ]);
    expect(tooltip.getByText("trayUsage.total")).toBeVisible();
    expect(
      tooltip.getByText(total, { exact: metric === "requests" }),
    ).toBeVisible();
    if (metric !== "requests")
      expect(screen.getByRole("tooltip")).toHaveTextContent(
        "trayUsage.callCount",
      );
    fireEvent.pointerLeave(
      screen.getByRole("group", { name: "trayUsage.trend" }),
    );
    expect(screen.queryByRole("tooltip")).toBeNull();
  },
);

it("narrows the summaries to a selected day without losing the trend", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  localStorage.setItem("vary:tray-usage-period", "week");
  const [yesterday, today] = calendarDays(2);
  vi.mocked(trayUsageApi.read).mockResolvedValue([
    { ...usageFixture, bucket: yesterday },
    { ...usageFixture, bucket: today },
  ]);
  const { container } = mount();
  const selectedDay = await screen.findByRole("button", {
    name: `${yesterday} · 100`,
  });
  const totalRequests = container.querySelector('[data-metric="requests"]');
  expect(totalRequests).toHaveTextContent("6");
  fireEvent.focus(selectedDay);
  expect(screen.getByRole("tooltip")).toHaveTextContent(
    yesterday.replaceAll("-", "/"),
  );
  expect(selectedDay).toHaveAccessibleDescription(/trayUsage.sessions/);
  fireEvent.keyDown(selectedDay, { key: "Escape" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(trayPanelApi.hide).not.toHaveBeenCalled();
  fireEvent.click(selectedDay);
  expect(selectedDay).toHaveAttribute("aria-pressed", "true");
  expect(totalRequests).toHaveTextContent("3");
  expect(screen.getByRole("button", { name: `${today} · 100` })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "trayUsage.clearDay" }));
  expect(totalRequests).toHaveTextContent("6");
});

it("reports refresh errors and allows a retry", async () => {
  localStorage.setItem("vary:tray-tab", "usage");
  let failed = false;
  vi.mocked(trayUsageApi.read).mockImplementation(async (period) => {
    if (period === "today" && !failed) {
      failed = true;
      throw new Error("fixture");
    }
    return [];
  });
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent("trayUsage.error");
  fireEvent.click(screen.getByRole("button", { name: "trayUsage.refresh" }));
  expect(await screen.findByText("trayUsage.emptyToday")).toBeVisible();
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
  const timestamp = Date.UTC(2026, 10, 23, 2, 13 + index, 17);
  const common = {
    key: `synthetic:${index}`,
    timestamp,
    lastSeenAt: timestamp,
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
    const times = Array.from(container.querySelectorAll(".tray-events time"));
    expect(times.map((time) => time.textContent)).toEqual(
      items.map(({ timestamp }) =>
        new Date(timestamp).toLocaleTimeString("zh", {
          hour: "2-digit",
          minute: "2-digit",
        }),
      ),
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
      expect(row.querySelector("time")).toHaveTextContent(
        new Date(items[index].timestamp).toLocaleTimeString(language, {
          hour: "2-digit",
          minute: "2-digit",
        }),
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

it("spans embedded check-in panels across all columns of a tray agent row", () => {
  const sheet = document.createElement("style");
  sheet.textContent = readFileSync(
    "src/components/tray/tray-panel.css",
    "utf8",
  );
  document.head.appendChild(sheet);
  const host = document.createElement("div");
  host.className = "tray-panel";
  host.innerHTML =
    '<div class="tray-agents"><div class="mp-agent-row"><div class="mp-agent-checkin"></div></div></div>';
  document.body.appendChild(host);
  try {
    const checkin = host.querySelector<HTMLElement>(".mp-agent-checkin")!;
    const styles = getComputedStyle(checkin);
    expect(styles.gridColumn).toBe("1 / -1");
    expect(styles.minWidth).toBe("0");
    expect(styles.paddingLeft).toBe("0px");
  } finally {
    host.remove();
    sheet.remove();
  }
});

it("syncs TrayPanel language when settings language changes", async () => {
  const instance = createInstance();
  await instance.init({
    lng: "zh",
    fallbackLng: false,
    resources: {
      zh: { translation: zh },
      en: { translation: en },
    },
  });
  const getSettingsSpy = vi
    .spyOn(settingsApi, "get")
    .mockResolvedValue({ language: "en" } as Settings);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  try {
    const { container } = render(
      <I18nextProvider i18n={instance}>
        <QueryClientProvider client={queryClient}>
          <TrayPanel />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    await waitFor(() => {
      expect(instance.language).toBe("en");
      expect(container.querySelector(".tray-panel")).toHaveAttribute(
        "lang",
        "en",
      );
    });
    expect(
      screen.getByRole("button", { name: en.trayPanel.refresh }),
    ).toBeVisible();

    getSettingsSpy.mockResolvedValue({ language: "zh" } as Settings);
    await queryClient.invalidateQueries({ queryKey: ["settings"] });

    await waitFor(() => {
      expect(instance.language).toBe("zh");
      expect(container.querySelector(".tray-panel")).toHaveAttribute(
        "lang",
        "zh",
      );
    });
    expect(
      screen.getByRole("button", { name: zh.trayPanel.refresh }),
    ).toBeVisible();
  } finally {
    getSettingsSpy.mockRestore();
  }
});
