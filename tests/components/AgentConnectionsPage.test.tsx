import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import { settingsApi } from "@/lib/api/settings";
import { intentRoutingApi } from "@/lib/api/intentRouting";
import { APP_ICON_NAME, APP_IDS } from "@/config/appConfig";
import { getIcon, getIconUrl, isUrlIcon } from "@/icons/extracted";
import { workbuddyCheckinApi } from "@/lib/api/workbuddy-checkin";
import { doubaoCheckinApi } from "@/lib/api/doubao-checkin";
import { agentRewardsApi } from "@/lib/api/agent-rewards";
vi.mock("@/lib/api/workbuddy-checkin", () => ({
  workbuddyCheckinApi: {
    accounts: vi.fn(async () => ({ accounts: [] })),
    checkIn: vi.fn(),
    setAutoEnabled: vi.fn(),
  },
}));
vi.mock("@/lib/api/doubao-checkin", () => ({
  doubaoCheckinApi: {
    accounts: vi.fn(async () => ({ connected: false, accounts: [] })),
    checkIn: vi.fn(),
    setAutoEnabled: vi.fn(),
  },
}));
vi.mock("@/lib/api/agent-rewards", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/agent-rewards")>()),
  agentRewardsApi: { status: vi.fn(), claim: vi.fn(), setAuto: vi.fn() },
}));
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    contextSettings: vi.fn(async () => ({ fullWindow: false })),
    setContextSettings: vi.fn(async () => {}),
  },
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    list: vi.fn(),
    get: vi.fn(),
    diagnoseCodexProxy: vi.fn(),
    nativeModel: vi.fn(),
    models: vi.fn(),
    nativeModels: vi.fn(),
    set: vi.fn(),
    reapply: vi.fn(),
    restart: vi.fn(),
    modelVisibility: vi.fn(),
    setModelVisibility: vi.fn(),
  },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getAgentInstallations: vi.fn(async () => ({
      claude: true,
      codex: true,
      pi: true,
    })),
    getProxyStatus: vi.fn(async () => ({ running: true })),
    launchDoubaoWorkForAudit: vi.fn(async () => {}),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getToolVersions: vi.fn(async () => []),
    getAgentVersions: vi.fn(async () => ({})),
    openExternal: vi.fn(async () => {}),
  },
}));
function mount(
  compact = false,
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
      <AgentConnectionsPage compact={compact} />
    </QueryClientProvider>,
  );
}

describe("tool list sorting", () => {
  const storageKey = "vary-agent-order";
  const order = () =>
    screen.getAllByRole("article").map((row) => row.getAttribute("aria-label"));
  const handle = (app: string) =>
    within(screen.getByRole("article", { name: `apps.${app}` })).getByRole(
      "button",
      { name: "agentConnections.sorting.handle" },
    );
  function drag(app: string, from: number, to: number) {
    fireEvent.pointerDown(handle(app), {
      button: 0,
      clientX: 12,
      clientY: from,
    });
    fireEvent.pointerMove(document, { clientX: 12, clientY: from + 10 });
    fireEvent.pointerMove(document, { clientX: 12, clientY: to });
  }

  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        const row = this.closest(".mp-agent-row") ?? this;
        const rows = Array.from(
          row.parentElement?.querySelectorAll(".mp-agent-row") ?? [],
        );
        const top = Math.max(0, rows.indexOf(row)) * 96;
        return {
          x: 0,
          y: top,
          left: 0,
          top,
          right: 800,
          bottom: top + 96,
          width: 800,
          height: 96,
          toJSON: () => ({}),
        };
      },
    );
    vi.stubGlobal(
      "PointerEvent",
      class extends MouseEvent {
        pointerId = 1;
        isPrimary = true;
        pointerType = "mouse";
      },
    );
  });

  afterEach(async () => {
    // dnd-kit retains its document click guard for 50 ms after a pointer drag.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 60));
    });
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    localStorage.removeItem(storageKey);
  });

  it("reorders from the left handle, saves the order and restores it on remount", async () => {
    const { unmount } = mount();
    await screen.findByRole("article", { name: "apps.codex" });
    const codex = screen.getByRole("article", { name: "apps.codex" });
    expect(codex.firstElementChild).toBe(handle("codex"));
    drag("codex", 144, 48);
    fireEvent.pointerUp(document, { clientX: 12, clientY: 48 });
    await waitFor(() =>
      expect(order()).toEqual(["apps.codex", "apps.claude", "apps.pi"]),
    );
    expect(JSON.parse(localStorage.getItem(storageKey)!)).toContain("gemini");
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    unmount();
    mount();
    await screen.findByRole("article", { name: "apps.codex" });
    expect(order()).toEqual(["apps.codex", "apps.claude", "apps.pi"]);
  });

  it("only starts sorting from the handle, without expanding the row", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.pointerDown(within(row).getByRole("heading"), {
      button: 0,
      clientX: 100,
      clientY: 144,
    });
    fireEvent.pointerMove(document, { clientX: 100, clientY: 48 });
    fireEvent.pointerUp(document, { clientX: 100, clientY: 48 });
    await userEvent.click(handle("codex"));
    expect(order()).toEqual(["apps.claude", "apps.codex", "apps.pi"]);
    expect(localStorage.getItem(storageKey)).toBeNull();
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
  });

  it("uses an inert drag preview without duplicating live rows or element IDs", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(within(row).getByRole("heading"));
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    drag("codex", 144, 48);
    const preview = document.querySelector(".mp-sortable-preview");
    expect(preview).toBeInTheDocument();
    expect(preview).toHaveAttribute("aria-hidden", "true");
    expect(preview).toHaveAttribute("inert");
    expect(preview?.querySelectorAll("[id]")).toHaveLength(0);
    expect(screen.getAllByRole("article")).toHaveLength(3);
    expect(preview).toHaveTextContent("apps.codex");
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    await act(async () => fireEvent.pointerCancel(document));
    expect(
      document.querySelector(".mp-sortable-preview"),
    ).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("cancels sorting without saving or expanding a row", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(within(row).getByRole("heading"));
    drag("codex", 144, 48);
    await act(async () => fireEvent.pointerCancel(document));
    expect(order()).toEqual(["apps.claude", "apps.codex", "apps.pi"]);
    expect(localStorage.getItem(storageKey)).toBeNull();
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
  });

  it("keeps the model picker and switch attached to their tool after a drop", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(within(row).getByRole("heading"));
    const picker = within(row).getByRole("button", {
      name: "agentConnections.model apps.codex",
    });
    const toggle = within(row).getByRole("switch");
    drag("codex", 144, 48);
    fireEvent.pointerUp(document, { clientX: 12, clientY: 48 });
    await waitFor(() => expect(order()[0]).toBe("apps.codex"));
    expect(within(row).getByRole("switch")).toBe(toggle);
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toBe(picker);
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("supports keyboard sorting from the handle", async () => {
    mount();
    await screen.findByRole("article", { name: "apps.codex" });
    handle("codex").focus();
    await userEvent.keyboard("[Space][ArrowUp][Space]");
    await waitFor(() =>
      expect(order()).toEqual(["apps.codex", "apps.claude", "apps.pi"]),
    );
    expect(handle("codex")).toHaveFocus();
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
  });

  it("ignores unknown and duplicate IDs and keeps positions for newly detected tools", async () => {
    localStorage.setItem(
      storageKey,
      JSON.stringify(["pi", "gemini", "removed-app", "pi", 1, "codex"]),
    );
    mount();
    await screen.findByRole("article", { name: "apps.codex" });
    expect(order()).toEqual(["apps.pi", "apps.codex", "apps.claude"]);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      claude: true,
      codex: true,
      pi: true,
      gemini: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    fireEvent.focus(window);
    await screen.findByRole("article", { name: "apps.gemini" });
    expect(order()).toEqual([
      "apps.pi",
      "apps.gemini",
      "apps.codex",
      "apps.claude",
    ]);
  });

  it.each(["invalid json", "{}", "null"])(
    "uses the default order for invalid saved data: %s",
    async (saved) => {
      localStorage.setItem(storageKey, saved);
      mount();
      await screen.findByRole("article", { name: "apps.codex" });
      expect(order()).toEqual(["apps.claude", "apps.codex", "apps.pi"]);
    },
  );

  it("still reorders if local storage is unavailable", async () => {
    mount();
    await screen.findByRole("article", { name: "apps.codex" });
    vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("Storage unavailable");
    });
    drag("codex", 144, 48);
    fireEvent.pointerUp(document, { clientX: 12, clientY: 48 });
    await waitFor(() =>
      expect(order()).toEqual(["apps.codex", "apps.claude", "apps.pi"]),
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("keeps tray rows compact without drag handles", async () => {
    mount(true);
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(
      within(row).queryByRole("button", {
        name: "agentConnections.sorting.handle",
      }),
    ).not.toBeInTheDocument();
  });
});

it("keeps tray controls behind an accessible row expansion", async () => {
  mount(true);
  const row = await screen.findByRole("article", { name: "apps.claude" });
  expect(row).toHaveClass("mp-agent-row");
  expect(
    within(row).getByRole("button", { name: /agentConnections.details/ }),
  ).toHaveAttribute("aria-expanded", "false");
  expect(row.querySelector(".mp-agent-summary-model")).toHaveTextContent(
    "agentConnections.nativeDefault",
  );
  fireEvent.click(
    within(row).getByRole("button", { name: /agentConnections.details/ }),
  );
  expect(within(row).getByRole("region")).toBeInTheDocument();
  expect(within(row).getByRole("switch")).toBeInTheDocument();
  expect(
    within(row).getByRole("region").querySelector(".mp-agent-detail-actions"),
  ).not.toBeInTheDocument();
  expect(
    within(row).queryByRole("button", { name: "agentConnections.website" }),
  ).not.toBeInTheDocument();
  expect(
    within(row).queryByRole("heading", {
      name: "agentConnections.instructions",
    }),
  ).not.toBeInTheDocument();
  fireEvent.click(
    within(row).getByRole("button", { name: /agentConnections.details/ }),
  );
  expect(within(row).queryByRole("region")).not.toBeInTheDocument();
});
it("keeps versions and updates beside the tool title", async () => {
  vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
    (tools ?? []).map((name) => ({
      name,
      version: "1.0.0",
      latest_version: "1.1.0",
      error: null,
      installed_but_broken: false,
      env_type: "macos",
      wsl_distro: null,
    })),
  );
  mount();
  const row = await screen.findByRole("article", { name: "apps.claude" });
  const version = await within(row).findByText("1.0.0");
  const heading = within(row).getByRole("heading");
  expect(heading.parentElement).toContainElement(version);
  expect(heading.parentElement).toContainElement(
    within(row).getByRole("button", { name: "agentUpdates.update" }),
  );
});
it("keeps version updates inside tray details instead of the quick summary", async () => {
  vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
    (tools ?? []).map((name) => ({
      name,
      version: "1.0.0",
      latest_version: "1.1.0",
      error: null,
      installed_but_broken: false,
      env_type: "macos",
      wsl_distro: null,
    })),
  );
  mount(true);
  const row = await screen.findByRole("article", { name: "apps.claude" });
  await waitFor(() => expect(settingsApi.getToolVersions).toHaveBeenCalled());
  expect(
    within(row).queryByRole("button", { name: "agentUpdates.update" }),
  ).not.toBeInTheDocument();
  fireEvent.click(
    within(row).getByRole("button", { name: /agentConnections.details/ }),
  );
  const details = within(row).getByRole("region");
  expect(
    await within(details).findByRole("button", { name: "agentUpdates.update" }),
  ).toBeEnabled();
  expect(within(details).getByText("1.0.0")).toBeInTheDocument();
});
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValue({
    accounts: [],
  } as unknown as Awaited<ReturnType<typeof workbuddyCheckinApi.accounts>>);
  vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
    connected: false,
    accounts: [],
  } as unknown as Awaited<ReturnType<typeof doubaoCheckinApi.accounts>>);
  vi.mocked(agentRewardsApi.status).mockImplementation(async (app) => ({
    app,
    accountId: null,
    label: null,
    autoEnabled: false,
    outcome: "sign_in_required",
    credit: null,
    checkedAt: null,
    nextCheckAt: null,
  }));
  localStorage.removeItem("vary-agent-order");
  vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockResolvedValue(null);
  vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
    running: true,
  } as Awaited<ReturnType<typeof proxyApi.getProxyStatus>>);
  vi.mocked(settingsApi.getToolVersions).mockResolvedValue([]);
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    claude: true,
    codex: true,
    pi: true,
  } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "claude",
      enabled: false,
      configured: false,
      model: null,
      recoveryPending: false,
      restartRequired: false,
    },
  ]);
  vi.mocked(agentConnectionsApi.get).mockImplementation(async (app) => {
    const entries = await agentConnectionsApi.list();
    return (
      entries.find((entry) => entry.app === app) ?? {
        app,
        enabled: false,
        configured: false,
        model: null,
        recoveryPending: false,
        restartRequired: false,
      }
    );
  });
  vi.mocked(agentConnectionsApi.nativeModel).mockImplementation(async (app) => {
    const entries = await agentConnectionsApi.nativeModels();
    return (
      entries.find((entry) => entry.app === app) ?? {
        app,
        model: null,
        available: false,
      }
    );
  });
  vi.mocked(agentConnectionsApi.models).mockResolvedValue([
    { id: "vary-auto/shared", name: "Shared group", contextWindow: 1000000 },
  ]);
  vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
    { app: "claude", model: null, available: true },
    { app: "codex", model: null, available: true },
    { app: "pi", model: null, available: true },
  ]);
  vi.mocked(agentConnectionsApi.set).mockResolvedValue();
  vi.mocked(agentConnectionsApi.reapply).mockResolvedValue();
  vi.mocked(agentConnectionsApi.restart).mockResolvedValue();
  vi.mocked(agentConnectionsApi.modelVisibility).mockResolvedValue({
    only: false,
    ids: [],
  });
  vi.mocked(agentConnectionsApi.setModelVisibility).mockResolvedValue();
});
describe("Agent connections", () => {
  it.each([false, true])(
    "does not label Pi as requiring individual configuration (compact: %s)",
    async (compact) => {
      mount(compact);
      const row = await screen.findByRole("article", { name: "apps.pi" });
      expect(
        within(row).queryByText("agentConnections.individual"),
      ).not.toBeInTheDocument();
    },
  );

  it("keeps a compact page title above the tool list without the introductory subtitle", async () => {
    mount();
    const heading = screen.getByRole("heading", {
      level: 1,
      name: "agentConnections.title",
    });
    expect(heading.closest("header")).toBeInTheDocument();
    expect(
      screen.queryByText("agentConnections.subtitle"),
    ).not.toBeInTheDocument();
    const row = await screen.findByRole("article", { name: "apps.claude" });
    expect(heading.closest("header")).not.toContainElement(row);
  });
  it("moves newly detected Agents into the installed list when the window regains focus", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
      {} as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>,
    );
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([]);
    mount();
    await waitFor(() =>
      expect(proxyApi.getAgentInstallations).toHaveBeenCalledOnce(),
    );
    expect(
      screen.queryByRole("article", { name: "apps.workbuddy" }),
    ).not.toBeInTheDocument();
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
      Object.fromEntries(APP_IDS.map((app) => [app, true])) as Awaited<
        ReturnType<typeof proxyApi.getAgentInstallations>
      >,
    );
    fireEvent.focus(window);
    for (const app of APP_IDS) {
      expect(
        await screen.findByRole("article", { name: `apps.${app}` }),
      ).toBeInTheDocument();
    }
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
  });
  it.each([
    ["workbuddy", "workbuddy.checkin", workbuddyCheckinApi, false],
    ["doubaowork", "doubaowork.checkin", doubaoCheckinApi, false],
    ["workbuddy", "workbuddy.checkin", workbuddyCheckinApi, true],
    ["doubaowork", "doubaowork.checkin", doubaoCheckinApi, true],
  ] as const)(
    "keeps %s account check-in accessible while disconnected without changing routing or claiming rewards",
    async (app, prefix, api, compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([]);
      mount(compact);
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: `agentConnections.details apps.${app}`,
          }),
        );
      } else {
        expect(
          within(row).queryByRole("button", {
            name: `agentConnections.details apps.${app}`,
          }),
        ).not.toBeInTheDocument();
      }
      await waitFor(() => expect(api.accounts).toHaveBeenCalledTimes(1));
      fireEvent.click(
        within(row).getByRole("button", {
          name: `agentRewards.manualLabel apps.${app}`,
        }),
      );
      const panel = await screen.findByRole("region", {
        name: `${prefix}.title`,
      });
      expect(
        within(panel).getByRole("combobox", { name: `${prefix}.account` }),
      ).toBeVisible();
      await waitFor(() => expect(api.accounts).toHaveBeenCalledTimes(1));
      const trigger = within(row).getByRole("button", {
        name: `agentRewards.manualLabel apps.${app}`,
      });
      fireEvent.click(trigger);
      expect(panel).not.toBeVisible();
      fireEvent.click(trigger);
      expect(panel).toBeVisible();
      expect(api.accounts).toHaveBeenCalledTimes(1);
      expect(api.checkIn).not.toHaveBeenCalled();
      expect(api.setAutoEnabled).not.toHaveBeenCalled();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it.each(
    (["workbuddy", "doubaowork", "qoder", "trae", "zcode"] as const).flatMap(
      (app) => [false, true].map((compact) => ({ app, compact })),
    ),
  )(
    "reflects saved and changed check-in modes for $app (compact=$compact)",
    async ({ app, compact }) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      const status = {
        autoEnabled: true,
        autoConfigured: true,
        day: "2026-11-10",
        outcome: "not_checked" as const,
        checkedAt: null,
        credit: null,
        streak: null,
        creditDelayed: false,
      };
      if (app === "workbuddy") {
        const accounts = [
          {
            id: "manual",
            label: "Manual account",
            source: "browser" as const,
            status: { ...status, autoEnabled: false },
          },
          {
            id: "auto",
            label: "Auto account",
            source: "browser" as const,
            status,
          },
        ];
        vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValue({
          accounts,
          nativeStatus: status,
        });
        vi.mocked(workbuddyCheckinApi.setAutoEnabled).mockImplementation(
          async (enabled) => ({
            ...status,
            autoEnabled: enabled,
            autoConfigured: enabled,
          }),
        );
      } else if (app === "doubaowork") {
        const snapshot = {
          connected: true,
          autoConfigured: true,
          day: status.day,
          outcome: status.outcome,
          accounts: [
            {
              id: "manual",
              label: "Manual account",
              current: false,
              ...status,
              autoEnabled: false,
              totalDays: null,
            },
            {
              id: "auto",
              label: "Auto account",
              current: true,
              ...status,
              totalDays: null,
            },
          ],
        };
        vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue(snapshot);
        vi.mocked(doubaoCheckinApi.setAutoEnabled).mockImplementation(
          async (_, enabled) => ({
            ...snapshot,
            autoConfigured: enabled,
            accounts: snapshot.accounts.map((account) => ({
              ...account,
              autoEnabled: account.id === "auto" && enabled,
            })),
          }),
        );
      } else {
        const reward = {
          app,
          accountId: "auto",
          label: "Auto account",
          autoEnabled: true,
          autoConfigured: true,
          outcome: status.outcome,
          credit: null,
          checkedAt: null,
          nextCheckAt: null,
        };
        vi.mocked(agentRewardsApi.status).mockResolvedValue(reward);
        vi.mocked(agentRewardsApi.setAuto).mockImplementation(
          async (_, __, enabled) => ({
            ...reward,
            autoEnabled: enabled,
            autoConfigured: enabled,
          }),
        );
      }
      mount(compact);
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      if (compact)
        fireEvent.click(
          within(row).getByRole("button", {
            name: `agentConnections.details apps.${app}`,
          }),
        );
      const trigger = await within(row).findByRole("button", {
        name: `agentRewards.autoLabel apps.${app}`,
      });
      expect(trigger).toHaveTextContent("agentRewards.autoLabel");
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      fireEvent.click(trigger);
      const panel = within(row).getByRole("region", {
        name:
          app === "workbuddy"
            ? "workbuddy.checkin.title"
            : app === "doubaowork"
              ? "doubaowork.checkin.title"
              : "agentRewards.title",
      });
      if (app === "workbuddy")
        fireEvent.change(within(panel).getByRole("combobox"), {
          target: { value: "auto" },
        });
      fireEvent.click(within(panel).getByRole("switch"));
      await waitFor(() =>
        expect(trigger).toHaveAccessibleName(
          `agentRewards.manualLabel apps.${app}`,
        ),
      );
      expect(trigger).toHaveTextContent("agentRewards.manualLabel");
      await waitFor(() =>
        expect(within(panel).getByRole("switch")).toBeEnabled(),
      );
      fireEvent.click(within(panel).getByRole("switch"));
      await waitFor(() =>
        expect(trigger).toHaveAccessibleName(
          `agentRewards.autoLabel apps.${app}`,
        ),
      );
      expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
      expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
      expect(agentRewardsApi.claim).not.toHaveBeenCalled();
    },
  );

  it.each(["workbuddy", "doubaowork", "qoder", "trae", "zcode"] as const)(
    "keeps %s automatic mode after a fresh mount without an available account",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      const status = {
        autoEnabled: false,
        autoConfigured: true,
        day: "2026-11-10",
        outcome: "connection_required" as const,
        checkedAt: null,
        credit: null,
        streak: null,
        creditDelayed: false,
      };
      if (app === "workbuddy") {
        vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValue({
          accounts: [],
          nativeStatus: { ...status, outcome: "credentials_protected" },
        });
      } else if (app === "doubaowork") {
        vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
          connected: false,
          accounts: [],
          ...status,
        });
      } else {
        vi.mocked(agentRewardsApi.status).mockResolvedValue({
          app,
          accountId: null,
          label: null,
          nextCheckAt: null,
          ...status,
        });
      }
      for (let attempt = 0; attempt < 2; attempt++) {
        const view = mount();
        const row = await screen.findByRole("article", { name: `apps.${app}` });
        const trigger = await within(row).findByRole("button", {
          name: `agentRewards.autoLabel apps.${app}`,
        });
        fireEvent.click(trigger);
        const panel = within(row).getByRole("region", {
          name:
            app === "workbuddy"
              ? "workbuddy.checkin.title"
              : app === "doubaowork"
                ? "doubaowork.checkin.title"
                : "agentRewards.title",
        });
        expect(within(panel).getByRole("switch")).toBeChecked();
        expect(within(panel).getByRole("switch")).toBeDisabled();
        view.unmount();
      }
      expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
      expect(doubaoCheckinApi.setAutoEnabled).not.toHaveBeenCalled();
      expect(workbuddyCheckinApi.setAutoEnabled).not.toHaveBeenCalled();
    },
  );

  it.each(["pending", "failed"])(
    "does not invent manual check-in mode while preferences are %s",
    async (state) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        qoder: true,
      });
      vi.mocked(agentRewardsApi.status).mockImplementation(() =>
        state === "pending"
          ? new Promise(() => {})
          : Promise.reject(new Error("synthetic-unavailable-status")),
      );
      mount();
      const row = await screen.findByRole("article", { name: "apps.qoder" });
      expect(
        await within(row).findByRole("button", {
          name: "agentRewards.title apps.qoder",
        }),
      ).toBeInTheDocument();
      expect(
        within(row).queryByRole("button", {
          name: "agentRewards.manualLabel apps.qoder",
        }),
      ).not.toBeInTheDocument();
    },
  );

  it.each(["qoder", "trae", "zcode"] as const)(
    "places %s check-in before its model picker",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([]);
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      const trigger = within(row).getByRole("button", {
        name: `agentRewards.manualLabel apps.${app}`,
      });
      const picker = within(row).getByLabelText(
        `agentConnections.${app === "qoder" ? "modelList" : "model"} apps.${app}`,
      );
      expect(
        trigger.compareDocumentPosition(picker) &
          Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(
        screen.queryByRole("region", { name: "agentRewards.title" }),
      ).not.toBeInTheDocument();
      fireEvent.click(trigger);
      const panel = await screen.findByRole("region", {
        name: "agentRewards.title",
      });
      expect(
        within(panel).getByRole("button", { name: "agentRewards.claim" }),
      ).toBeVisible();
      expect(
        within(panel).queryByRole("button", { name: "agentRewards.title" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it.each(["claude-desktop", "workbuddy"])(
    "confirms a stale %s restart and never restarts from rendering or cancel",
    async (app) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: true,
          desktopState: "restart_required",
        },
      ]);
      mount();
      const row = await screen.findByRole("article", {
        name: `apps.${app}`,
      });
      const restart = within(row).getByRole("button", {
        name: "agentConnections.restart",
      });
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.restartRequired",
      );
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(restart);
      let dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveTextContent("agentConnections.restartConfirmHint");
      fireEvent.click(
        within(dialog).getByRole("button", { name: "common.cancel" }),
      );
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(restart);
      dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.restart",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith(app, true),
      );
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it.each(["claude-desktop", "workbuddy"])(
    "offers a confirmed %s restart after restoring its native configuration",
    async (app) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: false,
          configured: false,
          recoveryPending: false,
          model: null,
          restartRequired: true,
          desktopState: "restart_required",
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      expect(within(row).getByRole("switch")).not.toBeChecked();
      fireEvent.click(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      );
      const dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.restart",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith(app, true),
      );
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it.each(["stale_catalog", "failed"] as const)(
    "offers a confirmed Codex reload when the background service is %s",
    async (reloadState) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "codex",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: true,
          reloadState,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      expect(within(row).getByRole("status")).toHaveTextContent(
        reloadState === "failed"
          ? "agentConnections.states.reloadFailed"
          : "agentConnections.states.catalogStale",
      );
      fireEvent.click(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      );
      let dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveTextContent(
        "agentConnections.codexRestartConfirmHint",
      );
      fireEvent.click(
        within(dialog).getByRole("button", { name: "common.cancel" }),
      );
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      );
      dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.restart",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith("codex", true),
      );
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    },
  );

  it.each(["waiting_for_idle", "restarting"] as const)(
    "shows Codex reload progress %s without offering another restart",
    async (reloadState) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "codex",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: false,
          reloadState,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      expect(within(row).getByRole("status")).toHaveTextContent(
        reloadState === "waiting_for_idle"
          ? "agentConnections.states.waitingForIdle"
          : "agentConnections.states.reloading",
      );
      expect(
        within(row).queryByRole("button", { name: "agentConnections.restart" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it("keeps Codex configuration drift ahead of reload errors", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: false,
        configured: false,
        recoveryPending: true,
        recoveryReason: "configuration_changed",
        model: null,
        restartRequired: true,
        reloadState: "failed",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.recovery",
    );
    expect(
      within(row).queryByRole("button", { name: "agentConnections.restart" }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
  });

  it.each(["current", "stopped", "unknown", "unsupported"] as const)(
    "does not offer a restart when Desktop state is %s",
    async (desktopState) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "claude-desktop",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: false,
          desktopState,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", {
        name: "apps.claude-desktop",
      });
      expect(
        within(row).queryByRole("button", { name: "agentConnections.restart" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it("keeps configuration drift distinct from a stale running Desktop", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "configuration_changed",
        model: "vary-auto/shared",
        restartRequired: true,
        desktopState: "restart_required",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    expect(
      within(row).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
    expect(
      within(row).queryByRole("button", { name: "agentConnections.restart" }),
    ).not.toBeInTheDocument();
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.recovery",
    );
  });

  it.each([false, true])(
    "keeps normal TRAE bridge waiting free of repair actions (compact: %s)",
    async (compact) => {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const connection = {
        app: "trae",
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "bridge_pending" as const,
        restartRequired: true,
        desktopState: "stopped" as const,
        model: "vary-auto/shared",
      };
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        trae: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connection]);
      mount(compact, client);
      const row = await screen.findByRole("article", { name: "apps.trae" });
      for (const desktopState of [
        "stopped",
        "current",
        "unknown",
        "unsupported",
      ] as const) {
        await act(async () => {
          client.setQueryData(["agent-connections", "trae"], {
            ...connection,
            desktopState,
          });
        });
        expect(within(row).getByRole("status")).toHaveTextContent(
          "agentConnections.states.bridgePending",
        );
        expect(
          within(row).queryByRole("button", {
            name: "agentConnections.reapply",
          }),
        ).not.toBeInTheDocument();
        expect(
          within(row).queryByRole("button", {
            name: "agentConnections.restart",
          }),
        ).not.toBeInTheDocument();
      }
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it("offers only restart when a running TRAE predates its configured bridge", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({ trae: true });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "trae",
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "bridge_pending",
        restartRequired: true,
        desktopState: "restart_required",
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.trae" });
    expect(
      within(row).queryByRole("button", { name: "agentConnections.reapply" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "agentConnections.restart" }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.restart).toHaveBeenCalledWith("trae", true),
    );
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
  });

  it("keeps TRAE repair available for confirmed native configuration drift", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({ trae: true });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "trae",
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "configuration_changed",
        restartRequired: true,
        desktopState: "restart_required",
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.trae" });
    expect(
      within(row).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
    expect(
      within(row).queryByRole("button", { name: "agentConnections.restart" }),
    ).not.toBeInTheDocument();
  });

  it.each(["trae", "claude-desktop", "workbuddy", "codex"] as const)(
    "clears the %s restart action after observing a manual reopen",
    async (app) => {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const connection = {
        app,
        enabled: true,
        configured: app !== "trae",
        recoveryPending: app === "trae",
        recoveryReason: app === "trae" ? ("bridge_pending" as const) : null,
        restartRequired: true,
        desktopState: "restart_required" as const,
        model: "vary-auto/shared",
      };
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connection]);
      mount(false, client);
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      expect(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      ).toBeEnabled();
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          ...connection,
          configured: true,
          recoveryPending: false,
          recoveryReason: null,
          restartRequired: false,
          desktopState: "current",
        },
      ]);
      await act(async () => {
        await client.invalidateQueries({
          queryKey: ["agent-connections", app],
        });
      });
      await waitFor(() => {
        expect(
          within(row).queryByRole("button", {
            name: "agentConnections.restart",
          }),
        ).not.toBeInTheDocument();
        expect(
          within(row).queryByRole("button", {
            name: "agentConnections.reapply",
          }),
        ).not.toBeInTheDocument();
      });
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    },
  );

  it("keeps other Agent switches and model pickers independent while connections overlap", async () => {
    let finishClaude!: () => void;
    let finishCodex!: () => void;
    vi.mocked(agentConnectionsApi.set).mockImplementation(
      (app) =>
        new Promise<void>((resolve) => {
          if (app === "claude") finishClaude = resolve;
          if (app === "codex") finishCodex = resolve;
        }),
    );
    mount();
    const claude = await screen.findByRole("switch", { name: "apps.claude" });
    const codex = screen.getByRole("switch", { name: "apps.codex" });
    const pi = screen.getByRole("switch", { name: "apps.pi" });
    await waitFor(() => expect(claude).toBeEnabled());
    fireEvent.click(claude);
    expect(claude).toBeDisabled();
    expect(claude).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getByRole("article", { name: "apps.claude" }),
    ).toHaveTextContent("agentConnections.states.connecting");
    expect(codex).toBeEnabled();
    expect(pi).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "agentConnections.model apps.pi" }),
    ).toBeEnabled();
    fireEvent.click(codex);
    expect(codex).toBeDisabled();
    expect(agentConnectionsApi.set).toHaveBeenCalledTimes(2);
    await act(async () => finishClaude());
    await waitFor(() => expect(claude).toBeEnabled());
    expect(codex).toBeDisabled();
    expect(pi).toBeEnabled();
    await act(async () => finishCodex());
    await waitFor(() => expect(codex).toBeEnabled());
  });

  it("does not hold a completed connection behind a slow native-model refresh", async () => {
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "agentConnections.model Claude Code",
        }),
      ).toHaveTextContent("agentConnections.nativeDefault"),
    );
    vi.mocked(agentConnectionsApi.nativeModels).mockImplementationOnce(
      () => new Promise(() => {}),
    );
    fireEvent.click(toggle);
    await waitFor(() => expect(agentConnectionsApi.set).toHaveBeenCalledOnce());
    await waitFor(() => expect(toggle).toBeEnabled());
  });

  it("does not close another Agent's tray setup when an earlier connection completes", async () => {
    let finish!: () => void;
    vi.mocked(agentConnectionsApi.set).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        restartRequired: true,
      },
    ]);
    mount(true);
    fireEvent.click(
      await screen.findByRole("article", { name: "apps.claude" }),
    );
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole("article", { name: "apps.codex" }));
    fireEvent.click(
      screen.getByRole("button", { name: "agentConnections.individual" }),
    );
    const dialog = screen.getByRole("dialog");
    await act(async () => finish());
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(dialog).toBeInTheDocument();
  });

  it("leaves unrelated switches usable after a failed connection", async () => {
    vi.mocked(agentConnectionsApi.set).mockRejectedValueOnce(
      "synthetic-private-failure",
    );
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "apps.codex" })).toBeEnabled();
    expect(
      screen.queryByText("synthetic-private-failure"),
    ).not.toBeInTheDocument();
  });
  it("does not load hidden detail settings when workspace rows are clicked", async () => {
    mount();
    const codex = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(codex);
    const pi = screen.getByRole("article", { name: "apps.pi" });
    fireEvent.click(pi);
    expect(
      screen.queryByRole("radiogroup", { name: "longConversations.title" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(intentRoutingApi.contextSettings).not.toHaveBeenCalled();
    expect(intentRoutingApi.setContextSettings).not.toHaveBeenCalled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it("shows the native default when a disconnected tool has no explicit model", async () => {
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model Claude Code",
    });
    await waitFor(() =>
      expect(button).toHaveTextContent("agentConnections.nativeDefault"),
    );
    expect(button).not.toHaveTextContent("agentConnections.pickModel");
    fireEvent.click(button);
    const native = await screen.findByRole("radio", {
      name: /agentConnections.nativeDefault/,
    });
    expect(native).toBeChecked();
    expect(native).toBeDisabled();
    expect(screen.getByRole("dialog")).toHaveClass("mp-model-picker-popover");
    expect(native.closest(".mp-shared-picker .mp-am-body")).not.toBeNull();
    expect(screen.getByRole("searchbox")).toHaveFocus();
    fireEvent.scroll(native);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.scroll(document);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(button);
    expect(await screen.findByRole("searchbox")).toHaveFocus();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "Shared" },
    });
    fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("shows a disconnected tool's configured native model instead of a shared default", async () => {
    vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
      { app: "codex", model: "native-sol", available: true },
    ]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model apps.codex",
    });
    await waitFor(() => expect(button).toHaveTextContent("native-sol"));
    fireEvent.click(button);
    expect(
      await screen.findByRole("radio", { name: /native-sol/ }),
    ).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /Shared group/ }),
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: /Shared group/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "codex",
        true,
        "vary-auto/shared",
      ),
    );
  });

  it.each([false, true])(
    "shows only OpenCode's native model ID in the row and picker (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        codex: true,
        opencode: true,
      });
      vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
        {
          app: "opencode",
          model: "typex/zai-org/GLM-5-FP8",
          available: true,
        },
        { app: "codex", model: "auto/gpt-6.1-sol", available: true },
      ]);
      mount(compact);
      const row = await screen.findByRole("article", { name: "apps.opencode" });
      if (compact) {
        await waitFor(() =>
          expect(
            row.querySelector(".mp-agent-summary-model"),
          ).toHaveTextContent("zai-org/GLM-5-FP8"),
        );
        expect(
          row.querySelector(".mp-agent-summary-model"),
        ).not.toHaveTextContent("typex/");
        fireEvent.click(
          within(row).getByRole("button", { name: /agentConnections.details/ }),
        );
      }
      const button = within(row).getByRole("button", {
        name: "agentConnections.model apps.opencode",
      });
      await waitFor(() =>
        expect(button).toHaveTextContent("zai-org/GLM-5-FP8"),
      );
      expect(button).not.toHaveTextContent("typex/");
      expect(button.title).not.toContain("typex/");
      expect(
        screen.getByRole("article", { name: "apps.codex" }),
      ).toHaveTextContent("auto/gpt-6.1-sol");
      fireEvent.click(button);
      const native = await screen.findByRole("radio", {
        name: /zai-org\/GLM-5-FP8/,
      });
      expect(native).toBeChecked();
      expect(native).toBeDisabled();
      expect(native).not.toHaveTextContent("typex/");
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("radio", { name: /Shared group/ }));
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          "opencode",
          true,
          "vary-auto/shared",
        ),
      );
    },
  );

  it("does not label an unreadable native config as Default", async () => {
    vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
      { app: "claude", model: null, available: false },
    ]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model Claude Code",
    });
    await waitFor(() =>
      expect(button).toHaveTextContent("agentConnections.nativeUnavailable"),
    );
    expect(button).not.toHaveTextContent("agentConnections.nativeDefault");
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("keeps native defaults visible even when there are no shared models", async () => {
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model Claude Code",
    });
    await waitFor(() =>
      expect(button).toHaveTextContent("agentConnections.nativeDefault"),
    );
    fireEvent.click(button);
    expect(
      await screen.findByText("agentConnections.noModels"),
    ).toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  async function desktop() {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: true,
      },
    ]);
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([
      { id: "vary-auto/shared", name: "Shared group", contextWindow: 1000000 },
      { id: "direct-model", name: "Direct model", contextWindow: 200000 },
    ]);
    mount();
    return screen.findByRole("article", { name: "apps.claude-desktop" });
  }

  async function openDesktopList(row: HTMLElement) {
    const trigger = within(row).getByRole("button", {
      name: "agentConnections.modelList apps.claude-desktop",
    });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    return screen.findByRole("dialog");
  }

  it("applies Desktop visibility immediately without setting a starting model", async () => {
    const row = await desktop();
    const dialog = await openDesktopList(row);
    const shared = within(dialog).getByRole("menuitemcheckbox", {
      name: "Shared group",
    });
    fireEvent.click(shared);
    expect(shared).toHaveAttribute("aria-checked", "false");
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
        "claude-desktop",
        { only: false, ids: ["vary-auto/shared"] },
      ),
    );
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Direct model" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      within(dialog).queryByRole("button", { name: "common.save" }),
    ).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("keeps disconnected Desktop visibility independent from its connection switch", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: false,
        configured: false,
        recoveryPending: false,
        model: null,
        restartRequired: false,
      },
    ]);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      "claude-desktop": true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    const dialog = await openDesktopList(row);
    fireEvent.click(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
        "claude-desktop",
        { only: false, ids: ["vary-auto/shared"] },
      ),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(within(row).getByRole("switch")).not.toBeChecked();
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.click(within(row).getByRole("switch"));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude-desktop",
        true,
        "vary-auto/shared",
      ),
    );
  });

  it("saves an explicit Desktop allowlist and confirms bulk hiding", async () => {
    const dialog = await openDesktopList(await desktop());
    fireEvent.click(
      within(dialog).getByRole("switch", {
        name: "agentConnections.onlyPicked",
      }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "agentConnections.hideAll" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.confirmHideAll",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
        "claude-desktop",
        { only: true, ids: [] },
      ),
    );
  });

  it("keeps failed Desktop edits open with a safe error", async () => {
    vi.mocked(agentConnectionsApi.setModelVisibility).mockRejectedValue(
      "synthetic-private-error",
    );
    const dialog = await openDesktopList(await desktop());
    fireEvent.click(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(dialog).not.toHaveTextContent("synthetic-private-error");
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("keeps the Desktop row flat without exposing its internal model ID", async () => {
    const row = await desktop();
    fireEvent.click(within(row).getByRole("heading"));
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    expect(row).not.toHaveTextContent("vary-auto/shared");
  });

  it("preserves the visible subset when switching to only-picked mode", async () => {
    vi.mocked(agentConnectionsApi.modelVisibility).mockResolvedValue({
      only: false,
      ids: ["vary-auto/shared"],
    });
    const row = await desktop();
    const dialog = await openDesktopList(row);
    fireEvent.click(
      within(dialog).getByRole("switch", {
        name: "agentConnections.onlyPicked",
      }),
    );
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    ).toHaveAttribute("aria-checked", "false");
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Direct model" }),
    ).toHaveAttribute("aria-checked", "true");
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
        "claude-desktop",
        { only: true, ids: ["direct-model"] },
      ),
    );
  });

  it("closes an unchanged Desktop list without saving or changing another agent", async () => {
    const dialog = await openDesktopList(await desktop());
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("disables Desktop visibility changes during connection recovery", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: false,
        recoveryPending: true,
        model: "vary-auto/shared",
        restartRequired: false,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.modelList apps.claude-desktop",
      }),
    ).toBeDisabled();
  });

  it.each(["off", "connected", "recovery"] as const)(
    "keeps a %s workspace row flat without a Details button or expansion",
    async (status) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "claude",
          enabled: status !== "off",
          configured: status === "connected",
          recoveryPending: status === "recovery",
          restartRequired: false,
          model: status === "off" ? null : "vary-auto/shared",
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.claude" });
      expect(
        within(row).queryByRole("button", { name: /agentConnections.details/ }),
      ).not.toBeInTheDocument();
      for (const target of [
        row,
        within(row).getByRole("status"),
        within(row).getByRole("heading", { name: "Claude Code" }),
      ]) {
        fireEvent.click(target);
        expect(within(row).queryByRole("region")).not.toBeInTheDocument();
        expect(row).not.toHaveClass("expanded");
      }
      expect(
        within(row).getByRole("button", {
          name: "agentConnections.model Claude Code",
        }),
      ).toBeInTheDocument();
      expect(within(row).getByRole("switch")).toBeEnabled();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    },
  );

  it("keeps recovery actions in the flat row and disconnects through the switch", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: false,
        recoveryPending: true,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const claude = await screen.findByRole("article", { name: "apps.claude" });
    const codex = screen.getByRole("article", { name: "apps.codex" });
    fireEvent.click(claude);
    fireEvent.click(codex);
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(
      within(codex).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
    expect(
      within(codex).queryByRole("button", { name: "agentConnections.website" }),
    ).not.toBeInTheDocument();
    expect(settingsApi.openExternal).not.toHaveBeenCalled();
    expect(
      within(codex).queryByRole("button", { name: "agentConnections.restore" }),
    ).not.toBeInTheDocument();
    expect(codex.querySelector(".mp-agent-detail-actions")).toBeNull();
    fireEvent.click(within(codex).getByRole("switch"));
    const dialog = await screen.findByRole("dialog");
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "codex",
        false,
        null,
      ),
    );
  });

  it.each([
    { compact: false, recoveryPending: false },
    { compact: false, recoveryPending: true },
    { compact: true, recoveryPending: false },
    { compact: true, recoveryPending: true },
  ])(
    "uses the switch to disconnect Desktop without a duplicate details action (compact: $compact, recovery: $recoveryPending)",
    async ({ compact, recoveryPending }) => {
      const connected = {
        app: "claude-desktop",
        enabled: true,
        configured: !recoveryPending,
        recoveryPending,
        restartRequired: false,
        model: "vary-auto/shared",
      };
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        "claude-desktop": true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connected]);
      vi.mocked(agentConnectionsApi.set).mockImplementationOnce(async () => {
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          {
            ...connected,
            enabled: false,
            configured: false,
            recoveryPending: false,
          },
        ]);
      });
      mount(compact);
      const row = await screen.findByRole("article", {
        name: "apps.claude-desktop",
      });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", { name: /agentConnections.details/ }),
        );
        expect(within(row).getByRole("region")).toBeInTheDocument();
      } else {
        expect(
          within(row).queryByRole("button", {
            name: /agentConnections.details/,
          }),
        ).not.toBeInTheDocument();
      }
      expect(
        within(row).queryByRole("button", {
          name: "agentConnections.restore",
        }),
      ).not.toBeInTheDocument();
      expect(!!row.querySelector(".mp-agent-detail-actions")).toBe(
        compact && !recoveryPending,
      );
      const toggle = within(row).getByRole("switch");
      await waitFor(() => expect(toggle).toBeEnabled());
      fireEvent.click(toggle);
      const cancelled = await screen.findByRole("dialog");
      expect(cancelled).toHaveTextContent("agentConnections.disconnectHint");
      fireEvent.click(
        within(cancelled).getByRole("button", { name: "common.cancel" }),
      );
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(toggle).toBeChecked();
      fireEvent.click(toggle);
      const confirmed = await screen.findByRole("dialog");
      fireEvent.click(
        within(confirmed).getByRole("button", {
          name: "agentConnections.disconnectRestore",
        }),
      );
      await waitFor(() => expect(toggle).not.toBeChecked());
      expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
        "claude-desktop",
        false,
        null,
      );
    },
  );

  it("keeps individual settings in connected tray details without collapsing the row", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        recoveryPending: false,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount(true);
    const row = await screen.findByRole("article", { name: "apps.claude" });
    fireEvent.click(row);
    const panel = within(row).getByRole("region");
    fireEvent.click(
      within(panel).getByRole("button", {
        name: "agentConnections.individual",
      }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("agentConnections.setupHint");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(panel).toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("does not toggle a row while its text is being selected", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.claude" });
    const selection = vi.spyOn(window, "getSelection").mockReturnValue({
      isCollapsed: false,
    } as Selection);
    try {
      fireEvent.click(
        within(row).getByRole("heading", { name: "Claude Code" }),
      );
      expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    } finally {
      selection.mockRestore();
    }
  });

  it.each([false, true])(
    "shows each agent's icon in more tools (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
        Object.fromEntries(APP_IDS.map((app) => [app, false])),
      );
      mount(compact);
      await waitFor(() =>
        expect(document.querySelectorAll(".mp-agent-install-row")).toHaveLength(
          APP_IDS.length,
        ),
      );
      for (const app of APP_IDS) {
        const label = app === "claude" ? "Claude Code" : `apps.${app}`;
        const row = screen
          .getByText(label, {
            selector: ".mp-agent-install-name",
          })
          .closest("[role='group']")!;
        const icon = row.querySelector(".mp-agent-install-icon [title]");
        expect(icon).toHaveAttribute("title", label);
        expect(icon).toHaveStyle({
          width: compact ? "22px" : "32px",
          height: compact ? "22px" : "32px",
        });
        const iconName = APP_ICON_NAME[app];
        if (isUrlIcon(iconName)) {
          expect(icon).toHaveAttribute("src", getIconUrl(iconName));
        } else {
          const expected = document.createElement("span");
          expected.innerHTML = getIcon(iconName);
          expect(icon?.innerHTML).toBe(expected.innerHTML);
        }
      }
    },
  );

  it("keeps the installation disclosure separate from the installed tool card", async () => {
    const { container } = mount();
    await screen.findByRole("article", { name: "apps.claude" });
    const disclosure = screen
      .getByText("agentConnections.moreTools")
      .closest("details");
    expect(disclosure).toHaveClass("mp-agent-more");
    expect(disclosure?.closest(".mp-list")).toBeNull();
    expect(container.querySelector(".mp-list")).toContainElement(
      screen.getByRole("article", { name: "apps.claude" }),
    );
  });
  describe.each(["workbuddy", "qoder", "trae"] as const)(
    "%s shared catalog",
    (app) => {
      it.each([false, true])(
        "browses and searches models without changing the connection when enabled is %s",
        async (enabled) => {
          vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
            [app]: true,
          } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
          const pending = app === "trae" && enabled;
          vi.mocked(agentConnectionsApi.list).mockResolvedValue([
            {
              app,
              enabled,
              configured: enabled && !pending,
              recoveryPending: pending,
              recoveryReason: pending ? "bridge_pending" : null,
              restartRequired: false,
              model: enabled ? "vary-auto/shared" : null,
            },
          ]);
          vi.mocked(agentConnectionsApi.models).mockResolvedValue([
            {
              id: "vary-auto/shared",
              name: "Shared group",
              contextWindow: 1000000,
            },
            { id: "direct-model", name: "Direct model", contextWindow: 200000 },
          ]);
          mount(app === "trae");
          const row = await screen.findByRole("article", {
            name: `apps.${app}`,
          });
          if (app === "trae") {
            fireEvent.click(
              within(row).getByRole("button", {
                name: "agentConnections.details apps.trae",
              }),
            );
          }
          const trigger = within(row).getByRole("button", {
            name: `agentConnections.modelList apps.${app}`,
          });
          await waitFor(() => expect(trigger).toBeEnabled());
          fireEvent.click(trigger);
          const dialog = await screen.findByRole("dialog");
          expect(dialog).toHaveTextContent(
            pending
              ? "agentConnections.nativeCatalogPendingHint"
              : enabled
                ? "agentConnections.nativeCatalogHint"
                : "agentConnections.nativeCatalogDisconnectedHint",
          );
          fireEvent.click(
            within(dialog).getByRole("listitem", { name: "Shared group" }),
          );
          fireEvent.change(within(dialog).getByRole("searchbox"), {
            target: { value: "Direct" },
          });
          expect(
            within(dialog).getByRole("listitem", { name: "Direct model" }),
          ).toBeVisible();
          expect(
            within(dialog).queryByRole("listitem", { name: "Shared group" }),
          ).not.toBeInTheDocument();
          expect(within(dialog).queryByRole("radio")).not.toBeInTheDocument();
          expect(agentConnectionsApi.set).not.toHaveBeenCalled();
          expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
          expect(agentConnectionsApi.modelVisibility).not.toHaveBeenCalled();
          expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
          expect(within(row).getByRole("switch")).toHaveAttribute(
            "aria-checked",
            String(enabled),
          );
        },
      );
    },
  );

  describe.each([false, true])("TRAE details (compact: %s)", (compact) => {
    it.each([
      {
        mode: "connected",
        enabled: true,
        configured: true,
        recoveryPending: false,
        status: "configured",
      },
      {
        mode: "disconnected",
        enabled: false,
        configured: false,
        recoveryPending: false,
        status: "off",
      },
      {
        mode: "pending",
        enabled: true,
        configured: false,
        recoveryPending: true,
        status: "bridgePending",
      },
    ])(
      "keeps one starting picker in the flat workspace or useful compact $mode details",
      async ({ enabled, configured, recoveryPending, status }) => {
        vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
          trae: true,
        } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          {
            app: "trae",
            enabled,
            configured,
            recoveryPending,
            recoveryReason: recoveryPending ? "bridge_pending" : null,
            restartRequired: recoveryPending,
            model: enabled ? "vary-auto/shared" : null,
          },
        ]);
        mount(compact);
        const row = await screen.findByRole("article", { name: "apps.trae" });
        expect(
          within(row).getAllByRole("button", {
            name: "agentConnections.model apps.trae",
            hidden: true,
          }),
        ).toHaveLength(1);
        expect(
          within(row).queryByRole("button", {
            name: "agentConnections.modelList apps.trae",
            hidden: true,
          }),
        ).not.toBeInTheDocument();
        if (!compact) {
          expect(
            within(row).queryByRole("button", {
              name: "agentConnections.details apps.trae",
            }),
          ).not.toBeInTheDocument();
          fireEvent.click(within(row).getByRole("heading"));
          expect(within(row).queryByRole("region")).not.toBeInTheDocument();
          expect(within(row).getByRole("switch")).toBeEnabled();
          return;
        }
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.trae",
          }),
        );
        const details = within(row).getByRole("region", {
          name: "agentConnections.details apps.trae",
        });
        expect(
          within(details).getByText("agentConnections.bridgeHint"),
        ).toBeVisible();
        expect(
          within(details).getByText(`agentConnections.states.${status}`),
        ).toBeVisible();
        const catalog = within(details).getByRole("button", {
          name: "agentConnections.modelList apps.trae",
        });
        await waitFor(() => expect(catalog).toBeEnabled());
        fireEvent.click(catalog);
        const dialog = await screen.findByRole("dialog");
        expect(
          within(dialog).getByRole("listitem", { name: "Shared group" }),
        ).toBeVisible();
        expect(within(dialog).queryByRole("radio")).not.toBeInTheDocument();
        expect(agentConnectionsApi.set).not.toHaveBeenCalled();
        expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
      },
    );
  });

  it("shows a safe catalog error instead of a false model count", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      qoder: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.models).mockRejectedValue(
      new Error("synthetic-private-catalog-error"),
    );
    mount();
    const row = await screen.findByRole("article", { name: "apps.qoder" });
    const trigger = within(row).getByRole("button", {
      name: "agentConnections.modelList apps.qoder",
    });
    await waitFor(() =>
      expect(trigger).toHaveTextContent("agentConnections.failed"),
    );
    fireEvent.click(trigger);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(screen.getByRole("dialog")).not.toHaveTextContent(
      "synthetic-private-catalog-error",
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("labels Doubao's official models without offering an unsupported shared selector", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      doubaowork: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    mount();
    const row = await screen.findByRole("article", { name: "apps.doubaowork" });
    const note = within(row).getByRole("note", {
      name: "agentConnections.modelList apps.doubaowork",
    });
    expect(note).toHaveTextContent("agentConnections.officialModels");
    expect(note).toHaveAttribute("title", "agentConnections.auditOnlyHint");
    expect(
      within(row).queryByRole("button", {
        name: "agentConnections.modelList apps.doubaowork",
      }),
    ).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "offers the explicit Doubao audit launcher and explains a running client (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        doubaowork: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "doubaowork",
          enabled: true,
          configured: false,
          model: null,
          recoveryPending: false,
          restartRequired: false,
        },
      ]);
      vi.mocked(proxyApi.launchDoubaoWorkForAudit).mockRejectedValueOnce(
        "homeAudit.doubaoQuitFirst",
      );
      mount(compact);
      const row = await screen.findByRole("article", {
        name: "apps.doubaowork",
      });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.doubaowork",
          }),
        );
      }
      const launch = within(row).getByRole("button", {
        name: "homeAudit.doubaoLaunch",
      });
      expect(proxyApi.launchDoubaoWorkForAudit).not.toHaveBeenCalled();
      fireEvent.click(launch);
      await waitFor(() =>
        expect(proxyApi.launchDoubaoWorkForAudit).toHaveBeenCalledOnce(),
      );
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "homeAudit.doubaoQuitFirst",
      );
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it.each([false, true])(
    "enables disconnected Doubao inspection before its explicit launcher (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        doubaowork: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "doubaowork",
          enabled: false,
          configured: false,
          model: null,
          recoveryPending: false,
          restartRequired: false,
        },
      ]);
      const order: string[] = [];
      vi.mocked(agentConnectionsApi.set).mockImplementationOnce(async () => {
        order.push("enable");
      });
      vi.mocked(proxyApi.launchDoubaoWorkForAudit).mockImplementationOnce(
        async () => {
          order.push("launch");
        },
      );
      mount(compact);
      const row = await screen.findByRole("article", {
        name: "apps.doubaowork",
      });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.doubaowork",
          }),
        );
      }
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(proxyApi.launchDoubaoWorkForAudit).not.toHaveBeenCalled();
      fireEvent.click(
        within(row).getByRole("button", { name: "homeAudit.doubaoLaunch" }),
      );
      await waitFor(() => expect(order).toEqual(["enable", "launch"]));
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "doubaowork",
        true,
        null,
      );
      expect(agentConnectionsApi.set).toHaveBeenCalledOnce();
      expect(proxyApi.launchDoubaoWorkForAudit).toHaveBeenCalledOnce();
    },
  );

  it.each([false, true])(
    "does not offer the Doubao launcher after the inspection connection is active (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        doubaowork: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "doubaowork",
          enabled: true,
          configured: true,
          model: null,
          recoveryPending: false,
          restartRequired: false,
        },
      ]);
      mount(compact);
      const row = await screen.findByRole("article", {
        name: "apps.doubaowork",
      });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.doubaowork",
          }),
        );
      }
      expect(
        within(row).queryByRole("button", { name: "homeAudit.doubaoLaunch" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(proxyApi.launchDoubaoWorkForAudit).not.toHaveBeenCalled();
    },
  );

  it.each([false, true])(
    "keeps unrecognized Doubao launch errors out of the UI (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        doubaowork: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "doubaowork",
          enabled: true,
          configured: false,
          model: null,
          recoveryPending: false,
          restartRequired: false,
        },
      ]);
      vi.mocked(proxyApi.launchDoubaoWorkForAudit).mockRejectedValueOnce(
        "synthetic-private-launch-detail",
      );
      mount(compact);
      const row = await screen.findByRole("article", {
        name: "apps.doubaowork",
      });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.doubaowork",
          }),
        );
      }
      fireEvent.click(
        within(row).getByRole("button", { name: "homeAudit.doubaoLaunch" }),
      );
      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("homeAudit.doubaoLaunchFailed");
      expect(alert).not.toHaveTextContent("synthetic-private-launch-detail");
    },
  );

  it("shows installed native adapters in the main list and distinguishes audit-only tools", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      gemini: true,
      grokbuild: true,
      openclaw: true,
      hermes: true,
      qoder: true,
      kimi: true,
      qwen: true,
      zcode: true,
      trae: true,
      doubaowork: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    mount();
    for (const app of [
      "gemini",
      "grokbuild",
      "openclaw",
      "hermes",
      "qoder",
      "kimi",
      "qwen",
      "zcode",
      "trae",
      "doubaowork",
    ]) {
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      expect(
        within(row).getByRole("switch", { name: `apps.${app}` }),
      ).toBeEnabled();
      if (["qoder", "doubaowork"].includes(app)) {
        expect(
          within(row).queryByRole("button", {
            name: `agentConnections.model apps.${app}`,
          }),
        ).not.toBeInTheDocument();
      }
    }
    expect(screen.getByText("agentConnections.auditOnly")).toBeInTheDocument();
  });
  it.each([false, true])(
    "selects TRAE's starting model with a catalog preview only in expanded details (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        trae: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "trae",
          enabled: true,
          configured: true,
          recoveryPending: false,
          restartRequired: false,
          model: "vary-auto/shared",
        },
      ]);
      vi.mocked(agentConnectionsApi.models).mockResolvedValue([
        { id: "vary-auto/shared", name: "Shared group", contextWindow: null },
        { id: "vary-model/second", name: "Second model", contextWindow: null },
      ]);
      mount(compact);
      const row = await screen.findByRole("article", { name: "apps.trae" });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.trae",
          }),
        );
      }
      const preview = within(row).queryByRole("button", {
        name: "agentConnections.modelList apps.trae",
      });
      if (compact) expect(preview).toBeInTheDocument();
      else expect(preview).not.toBeInTheDocument();
      fireEvent.click(
        within(row).getByRole("button", {
          name: "agentConnections.model apps.trae",
        }),
      );
      expect(
        await screen.findByText("agentConnections.traeStartingModelHint"),
      ).toBeInTheDocument();
      fireEvent.click(
        await screen.findByRole("radio", { name: /Second model/ }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          "trae",
          true,
          "vary-model/second",
        ),
      );
    },
  );
  it("connects the audit-only client without requiring a shared catalog", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      doubaowork: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    mount();
    fireEvent.click(
      await screen.findByRole("switch", { name: "apps.doubaowork" }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "doubaowork",
        true,
        null,
      ),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("allows changing a saved TRAE preference while its bridge is pending", async () => {
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([
      { id: "vary-auto/shared", name: "Shared group", contextWindow: null },
      { id: "vary-model/second", name: "Second model", contextWindow: null },
    ]);
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "trae",
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "bridge_pending",
        restartRequired: true,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.trae" });
    const picker = within(row).getByRole("button", {
      name: "agentConnections.model apps.trae",
    });
    expect(picker).toBeEnabled();
    fireEvent.click(picker);
    fireEvent.click(await screen.findByRole("radio", { name: /Second model/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "trae",
        true,
        "vary-model/second",
      ),
    );
  });
  it("can connect by picking a remembered model while the native adapter is disconnected", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      kimi: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "kimi",
        enabled: false,
        configured: false,
        recoveryPending: false,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model apps.kimi",
    });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    const model = await screen.findByRole("radio", { name: /Shared group/ });
    expect(model).not.toBeChecked();
    fireEvent.click(model);
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "kimi",
        true,
        "vary-auto/shared",
      ),
    );
  });
  it.each(["provider_missing", "profile_override", "catalog_missing"] as const)(
    "explains the Codex configuration issue %s instead of showing a configured connection",
    async (configurationIssue) => {
      const connection = {
        app: "codex",
        enabled: true,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        recoveryReason: "configuration_changed" as const,
        restartRequired: false,
        configurationIssue,
      };
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connection]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      await waitFor(() =>
        expect(within(row).getByRole("status")).toHaveTextContent(
          `agentConnections.states.${configurationIssue}`,
        ),
      );
      expect(
        within(row).getByRole("button", { name: "agentConnections.reapply" }),
      ).toBeEnabled();
    },
  );
  it.each([
    "unsupported",
    "revoked",
    "reconnect_required",
    "recovery_pending",
  ] as const)(
    "offers reapply for unavailable Codex native compatibility (%s) while keeping the model menu connected",
    async (nativeCompatibility) => {
      const connection = {
        app: "codex",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        restartRequired: true,
        nativeCompatibility,
      };
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connection]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      await waitFor(() =>
        expect(within(row).getByRole("status")).toHaveTextContent(
          "agentConnections.states.nativeUnavailable",
        ),
      );
      expect(within(row).getByRole("switch")).toBeChecked();
      expect(
        within(row).queryByText("agentConnections.codexNativeHint"),
      ).not.toBeInTheDocument();
      expect(
        within(row).getByRole("button", { name: "agentConnections.reapply" }),
      ).toBeEnabled();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );
  it("offers Apply again directly on a drifted Codex row and refreshes its status", async () => {
    const pending = {
      app: "codex",
      enabled: true,
      configured: false,
      model: "vary-auto/shared",
      recoveryPending: true,
      recoveryReason: "configuration_changed" as const,
      restartRequired: false,
    };
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([pending]);
    vi.mocked(agentConnectionsApi.reapply).mockImplementation(async () => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        { ...pending, configured: true, recoveryPending: false },
      ]);
    });
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    const retry = await within(row).findByRole("button", {
      name: "agentConnections.reapply",
    });
    expect(retry.closest(".mp-agent-name")).not.toBeNull();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toHaveTextContent("Shared group");
    fireEvent.click(retry);
    await waitFor(() =>
      expect(agentConnectionsApi.reapply).toHaveBeenCalledWith("codex"),
    );
    await waitFor(() =>
      expect(
        within(row).queryByRole("button", {
          name: "agentConnections.reapply",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it.each([false, true])(
    "clears only the inactive Codex connection request after foreign takeover (compact=%s)",
    async (compact) => {
      const inactive = {
        app: "codex",
        enabled: false,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        recoveryReason: "connection_inactive" as const,
        restartRequired: false,
      };
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([inactive]);
      vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
        { app: "codex", available: true, model: "magpie-test-model" },
      ]);
      vi.mocked(agentConnectionsApi.set).mockImplementation(async () => {
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          {
            ...inactive,
            model: null,
            recoveryPending: false,
            recoveryReason: null,
          },
        ]);
      });
      mount(compact);
      const row = await screen.findByRole("article", { name: "apps.codex" });
      if (compact) {
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.codex",
          }),
        );
      }
      const picker = within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      });
      await waitFor(() => expect(picker).toBeEnabled());
      expect(within(row).getByRole("switch")).not.toBeChecked();
      expect(picker).toHaveTextContent("magpie-test-model");
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      fireEvent.click(picker);
      fireEvent.click(
        await screen.findByRole("button", {
          name: "agentConnections.disconnectOnly",
        }),
      );
      const dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveTextContent(
        "agentConnections.inactiveCodexDisconnectHint",
      );
      expect(dialog).not.toHaveTextContent("agentConnections.disconnectHint");
      expect(dialog).not.toHaveTextContent(
        "agentConnections.orphanDisconnectHint",
      );
      expect(
        within(dialog).queryByRole("button", {
          name: "agentConnections.disconnectRestore",
        }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.disconnectOnly",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
          "codex",
          false,
          null,
        ),
      );
      await waitFor(() =>
        expect(within(row).getByRole("status")).toHaveTextContent(
          "agentConnections.states.off",
        ),
      );
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(within(row).getByRole("switch")).not.toBeChecked();
      expect(picker).toHaveTextContent("magpie-test-model");
      expect(
        within(row).queryByRole("button", {
          name: "agentConnections.reapply",
        }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    },
  );
  it("keeps a changed Codex configuration protected from the inactive-intent disconnect path", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: false,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        recoveryReason: "configuration_changed",
        restartRequired: false,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", {
        name: "agentConnections.disconnectOnly",
      }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
  });
  it("keeps owned Codex recovery and restoration when the gateway is stopped", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        recoveryReason: "connection_inactive",
        restartRequired: false,
      },
    ]);
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: false,
    } as Awaited<ReturnType<typeof proxyApi.getProxyStatus>>);
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    await waitFor(() =>
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.gatewayStopped",
      ),
    );
    expect(within(row).getByRole("switch")).toBeChecked();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toBeDisabled();
    fireEvent.click(within(row).getByRole("switch"));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("agentConnections.disconnectHint");
    expect(dialog).not.toHaveTextContent(
      "agentConnections.inactiveCodexDisconnectHint",
    );
    expect(
      within(dialog).getByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    ).toBeEnabled();
    expect(
      within(dialog).queryByRole("button", {
        name: "agentConnections.disconnectOnly",
      }),
    ).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it("distinguishes inactive routing from changed configuration and sanitizes reapply failures", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        recoveryReason: "connection_inactive",
        restartRequired: false,
      },
    ]);
    vi.mocked(agentConnectionsApi.reapply).mockRejectedValue(
      "synthetic-private-reapply-error",
    );
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.inactive",
    );
    fireEvent.click(
      within(row).getByRole("button", {
        name: "agentConnections.reapply",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(
      screen.queryByText("synthetic-private-reapply-error"),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.reapply",
      }),
    ).toBeEnabled();
  });
  it.each([false, true])(
    "selects the DeepSeek Harness starting model when connected is %s",
    async (enabled) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        "deepseek-harness": true,
      });
      vi.mocked(agentConnectionsApi.models).mockResolvedValue([
        {
          id: "vary-auto/shared",
          name: "Shared group",
          contextWindow: 1000000,
        },
        {
          id: "vary-auto/another",
          name: "Another group",
          contextWindow: 1000000,
        },
      ]);
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "deepseek-harness",
          enabled,
          configured: enabled,
          model: enabled ? "vary-auto/another" : null,
          recoveryPending: false,
          restartRequired: false,
        },
      ]);
      vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
        { app: "deepseek-harness", available: true, model: "deepseek-flash" },
      ]);
      mount();
      const row = await screen.findByRole("article", {
        name: "apps.deepseek-harness",
      });
      fireEvent.click(
        await within(row).findByRole("button", {
          name: "agentConnections.model apps.deepseek-harness",
        }),
      );
      fireEvent.click(
        await screen.findByRole("radio", { name: /Shared group/ }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          "deepseek-harness",
          true,
          "vary-auto/shared",
        ),
      );
    },
  );
  it("connects a disconnected tool immediately when a starting model is picked", async () => {
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    );
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("radio", { name: /Shared group/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        true,
        "vary-auto/shared",
      ),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("does not reconnect through the model picker while configuration recovery is pending", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        restartRequired: false,
      },
    ]);
    mount();
    expect(
      await screen.findByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    ).toBeDisabled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it.each([false, true])(
    "can choose a model to recover receiptless Codex configuration when enabled is %s",
    async (enabled) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "codex",
          enabled,
          configured: false,
          model: null,
          recoveryPending: true,
          recoveryReason: "orphaned",
          restartRequired: false,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      const picker = within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      });
      await waitFor(() => expect(picker).toBeEnabled());
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
      fireEvent.click(picker);
      expect(
        await screen.findByText("agentConnections.orphanReconnectHint"),
      ).toBeInTheDocument();
      fireEvent.click(
        await screen.findByRole("radio", { name: /Shared group/ }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          "codex",
          true,
          "vary-auto/shared",
        ),
      );
    },
  );
  it("keeps receiptless Codex disconnected after refresh and explains that its old configuration cannot be restored", async () => {
    const orphaned = {
      app: "codex",
      enabled: true,
      configured: false,
      model: null,
      recoveryPending: true,
      recoveryReason: "orphaned" as const,
      restartRequired: false,
    };
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([orphaned]);
    vi.mocked(agentConnectionsApi.set).mockImplementation(async () => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        { ...orphaned, enabled: false },
      ]);
    });
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.codex" });
    fireEvent.click(toggle);
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("agentConnections.orphanDisconnectHint");
    expect(dialog).not.toHaveTextContent("agentConnections.disconnectHint");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.disconnectOnly",
      }),
    );
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(agentConnectionsApi.set).toHaveBeenCalledWith("codex", false, null);
    const row = screen.getByRole("article", { name: "apps.codex" });
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.orphaned",
    );
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toBeEnabled();
    fireEvent.click(row);
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
  });
  it("changes a connected tool's starting model directly from its flat row", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        restartRequired: false,
      },
    ]);
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([
      { id: "vary-auto/shared", name: "Shared group", contextWindow: 1000000 },
      {
        id: "vary-provider/second",
        name: "Second model",
        contextWindow: 128000,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.claude" });
    fireEvent.click(row);
    expect(
      within(row).queryByRole("button", { name: /agentConnections.details/ }),
    ).not.toBeInTheDocument();
    expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    );
    fireEvent.click(await screen.findByRole("radio", { name: /Second model/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        true,
        "vary-provider/second",
      ),
    );
  });
  it("connects an installed Agent directly from its switch without a model or provider dialog", async () => {
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        true,
        "vary-auto/shared",
      ),
    );
  });
  it("opens the shared provider flow when no model is available", async () => {
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    const add = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <AgentConnectionsPage onAddProvider={add} />
      </QueryClientProvider>,
    );
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", { name: "modelProviders.add" }),
    );
    expect(add).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("confirms a model-menu disconnect before changing the connection", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        recoveryPending: false,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    );
    const menu = await screen.findByRole("dialog");
    expect(menu).toHaveAttribute("data-variant", "agent");
    fireEvent.click(
      within(menu).getByRole("button", {
        name: "agentConnections.restore",
      }),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(
      await screen.findByText("agentConnections.disconnectHint"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
        "claude",
        false,
        null,
      ),
    );
  });

  it("disconnects independently of global providers and security settings", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        recoveryPending: false,
        restartRequired: true,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeChecked());
    fireEvent.click(toggle);
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        false,
        null,
      ),
    );
  });
  it("reports failed status reads and never presents them as connected", async () => {
    vi.mocked(agentConnectionsApi.list).mockRejectedValue(
      new Error("synthetic-private-configuration"),
    );
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(screen.getByRole("switch", { name: "apps.claude" })).toBeDisabled();
    expect(
      screen.queryByText(/synthetic-private-configuration/),
    ).not.toBeInTheDocument();
  });
});

describe("per-Agent detection", () => {
  it.each([false, true])(
    "unlocks completed Agents while another is checking (compact=%s)",
    async (compact) => {
      const get = vi.mocked(agentConnectionsApi.get).getMockImplementation()!;
      let finish!: (
        value: Awaited<ReturnType<typeof agentConnectionsApi.get>>,
      ) => void;
      vi.mocked(agentConnectionsApi.get).mockImplementation((app) =>
        app === "codex"
          ? new Promise((resolve) => {
              finish = resolve;
            })
          : get(app),
      );
      mount(compact);
      const codex = await screen.findByRole("article", { name: "apps.codex" });
      await waitFor(() =>
        expect(within(codex).getByRole("status")).toHaveTextContent(
          "agentConnections.states.checking",
        ),
      );
      expect(
        within(codex).queryByText("agentConnections.states.off"),
      ).not.toBeInTheDocument();
      expect(
        within(codex).getByRole("switch", { hidden: compact }),
      ).toBeDisabled();
      const claude = screen.getByRole("article", { name: "apps.claude" });
      await waitFor(() =>
        expect(
          within(claude).getByRole("switch", { hidden: compact }),
        ).toBeEnabled(),
      );
      expect(document.querySelector(".mp-list > p[role='status']")).toBeNull();
      if (compact) {
        const unconfigured = screen.getByText("trayPanel.notConfigured");
        expect(
          codex.compareDocumentPosition(unconfigured) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      }
      await act(async () =>
        finish({
          app: "codex",
          enabled: false,
          configured: false,
          recoveryPending: false,
          model: null,
          restartRequired: false,
        }),
      );
      await waitFor(() =>
        expect(within(codex).getByRole("status")).toHaveTextContent(
          "agentConnections.states.off",
        ),
      );
      expect(
        within(codex).getByRole("switch", { hidden: compact }),
      ).toBeEnabled();
    },
  );

  it("keeps native model detection independent for each Agent", async () => {
    const native = vi
      .mocked(agentConnectionsApi.nativeModel)
      .getMockImplementation()!;
    vi.mocked(agentConnectionsApi.nativeModel).mockImplementation((app) =>
      app === "codex" ? new Promise(() => {}) : native(app),
    );
    mount();
    const claude = await screen.findByRole("article", { name: "apps.claude" });
    await waitFor(() =>
      expect(
        within(claude).getByRole("button", {
          name: "agentConnections.model Claude Code",
        }),
      ).toHaveTextContent("agentConnections.nativeDefault"),
    );
    const codex = screen.getByRole("article", { name: "apps.codex" });
    expect(
      within(codex).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toHaveTextContent("common.loading");
    expect(within(claude).getByRole("switch")).toBeEnabled();
    expect(within(codex).getByRole("switch")).toBeEnabled();
  });

  it("isolates detection failures to their Agent", async () => {
    const get = vi.mocked(agentConnectionsApi.get).getMockImplementation()!;
    vi.mocked(agentConnectionsApi.get).mockImplementation((app) =>
      app === "codex"
        ? Promise.reject(new Error("synthetic failure"))
        : get(app),
    );
    mount();
    const codex = await screen.findByRole("article", { name: "apps.codex" });
    await waitFor(() =>
      expect(within(codex).getByRole("status")).toHaveTextContent(
        "agentConnections.states.unavailable",
      ),
    );
    expect(within(codex).getByRole("switch")).toBeDisabled();
    const claude = screen.getByRole("article", { name: "apps.claude" });
    expect(within(claude).getByRole("status")).toHaveTextContent(
      "agentConnections.states.off",
    );
    expect(within(claude).getByRole("switch")).toBeEnabled();
  });

  it("retains confirmed connection and model during a slow background refresh", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.mocked(agentConnectionsApi.get).mockResolvedValue({
      app: "codex",
      enabled: true,
      configured: true,
      model: "vary-auto/shared",
      recoveryPending: false,
      restartRequired: false,
    });
    mount(false, client);
    const codex = await screen.findByRole("article", { name: "apps.codex" });
    await waitFor(() =>
      expect(within(codex).getByRole("status")).toHaveTextContent(
        "agentConnections.states.configured",
      ),
    );
    vi.mocked(agentConnectionsApi.get).mockImplementation(
      () => new Promise(() => {}),
    );
    vi.mocked(agentConnectionsApi.nativeModel).mockImplementation(
      () => new Promise(() => {}),
    );
    await act(async () => {
      void client.invalidateQueries({ queryKey: ["agent-connections"] });
      void client.invalidateQueries({ queryKey: ["agent-native-models"] });
    });
    await waitFor(() =>
      expect(
        client.isFetching({ queryKey: ["agent-connections", "codex"] }),
      ).toBe(1),
    );
    expect(within(codex).getByRole("status")).toHaveTextContent(
      "agentConnections.states.configured",
    );
    expect(within(codex).getByRole("switch")).toBeEnabled();
    expect(
      within(codex).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toHaveTextContent("Shared group");
  });
});

describe("Codex proxy diagnostic integration", () => {
  const connected = {
    app: "codex",
    enabled: true,
    configured: true,
    model: "vary-auto/shared",
    recoveryPending: false,
    restartRequired: false,
  };
  const proxyIssue = {
    target: "http://127.0.0.1:15722",
    proxy: "http://127.0.0.1:7890",
    source: "environment" as const,
  };

  it.each([false, true])(
    "replaces the healthy status and keeps the proxy action visible (compact=%s)",
    async (compact) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connected]);
      vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockResolvedValue(
        proxyIssue,
      );
      mount(compact);
      const row = await screen.findByRole("article", { name: "apps.codex" });
      const action = await within(row).findByRole("button", {
        name: "agentConnections.codexProxy.howToFix",
      });
      expect(within(row).getAllByRole("status")).toHaveLength(1);
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.codexProxyBlocked",
      );
      expect(
        within(row).queryByText("agentConnections.states.configured"),
      ).not.toBeInTheDocument();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      await userEvent.click(action);
      const warning = screen.getByRole("dialog", {
        name: "agentConnections.codexProxy.title",
      });
      expect(
        within(warning).getByText(
          "agentConnections.codexProxy.sourceEnvironment",
        ),
      ).toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      await userEvent.click(
        within(warning).getByRole("button", {
          name: "agentConnections.disconnectRestore",
        }),
      );
      const confirmation = screen.getByRole("dialog", {
        name: "agentConnections.disconnectTitle",
      });
      expect(
        screen.queryByRole("dialog", {
          name: "agentConnections.codexProxy.title",
        }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      await userEvent.click(
        within(confirmation).getByRole("button", {
          name: "agentConnections.disconnectRestore",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
          "codex",
          false,
          null,
        ),
      );
    },
  );

  it("discards a confirmed diagnostic when the running gateway changes address or port", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([connected]);
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: true,
      address: "127.0.0.1",
      port: 15722,
    } as Awaited<ReturnType<typeof proxyApi.getProxyStatus>>);
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockResolvedValueOnce(
      proxyIssue,
    );
    mount(false, client);
    const action = await screen.findByRole("button", {
      name: "agentConnections.codexProxy.howToFix",
    });
    await userEvent.click(action);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    let resolveFresh!: (value: null) => void;
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveFresh = resolve;
      }),
    );
    await act(async () => {
      client.setQueryData(["proxyStatus"], {
        running: true,
        address: "127.0.0.2",
        port: 15723,
      });
    });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", {
        name: "agentConnections.codexProxy.howToFix",
      }),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getByRole("article", { name: "apps.codex" })).getByRole(
        "status",
      ),
    ).not.toHaveTextContent("agentConnections.states.codexProxyBlocked");
    await waitFor(() =>
      expect(agentConnectionsApi.diagnoseCodexProxy).toHaveBeenCalledTimes(2),
    );
    await act(async () => {
      resolveFresh(null);
    });
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it.each([
    "disabled",
    "unconfigured",
    "recovery",
    "configurationIssue",
    "connectionError",
    "gatewayStopped",
    "gatewayError",
  ])(
    "does not diagnose an ineligible Codex connection: %s",
    async (scenario) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          ...connected,
          enabled: scenario !== "disabled",
          configured: scenario !== "unconfigured",
          recoveryPending: scenario === "recovery",
          configurationIssue:
            scenario === "configurationIssue" ? "provider_missing" : null,
        },
      ]);
      if (scenario === "connectionError") {
        vi.mocked(agentConnectionsApi.get).mockRejectedValue(
          new Error("synthetic-unavailable"),
        );
      }
      if (scenario === "gatewayStopped") {
        vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
          running: false,
        } as Awaited<ReturnType<typeof proxyApi.getProxyStatus>>);
      }
      if (scenario === "gatewayError") {
        vi.mocked(proxyApi.getProxyStatus).mockRejectedValue(
          new Error("synthetic-unavailable"),
        );
      }
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      await waitFor(() =>
        expect(within(row).getByRole("status")).not.toHaveTextContent(
          "agentConnections.states.checking",
        ),
      );
      expect(agentConnectionsApi.diagnoseCodexProxy).not.toHaveBeenCalled();
      expect(
        within(row).queryByRole("button", {
          name: "agentConnections.codexProxy.howToFix",
        }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );
});

it("updates the switch after external takeover without writing another manager's configuration", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const original = {
    app: "codex",
    enabled: true,
    configured: true,
    model: "vary-auto/shared",
    recoveryPending: false,
    restartRequired: false,
  };
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([original]);
  mount(false, client);
  const toggle = await screen.findByRole("switch", { name: "apps.codex" });
  await waitFor(() => expect(toggle).toBeChecked());
  const external = {
    ...original,
    enabled: false,
    configured: false,
    recoveryPending: true,
    recoveryReason: "configuration_changed" as const,
  };
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([external]);
  await act(async () => {
    await client.invalidateQueries({
      queryKey: ["agent-connections", "codex"],
    });
  });
  await waitFor(() => expect(toggle).not.toBeChecked());
  expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
  fireEvent.click(toggle);
  await waitFor(() =>
    expect(agentConnectionsApi.reapply).toHaveBeenCalledWith("codex"),
  );
});
