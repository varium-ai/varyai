import { selectAppOption } from "../utils/appSelect";
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
import { toast } from "sonner";
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { updatePromptState } from "@/components/agents/updatePrompts";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import { settingsApi } from "@/lib/api/settings";
import { intentRoutingApi } from "@/lib/api/intentRouting";
import { blankRoutingGroup } from "@/lib/routingGroupTransfer";
import { APP_ICON_NAME, MANAGED_APP_IDS } from "@/config/appConfig";
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
    list: vi.fn(async () => ({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    })),
    contextSettings: vi.fn(async () => ({ fullWindow: false })),
    setContextSettings: vi.fn(async () => { }),
  },
}));
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(async () => []),
  },
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    list: vi.fn(),
    get: vi.fn(),
    diagnoseCodexProxy: vi.fn(),
    nativeModel: vi.fn(),
    modelControl: vi.fn(async () => ({
      authorized: false,
      ready: false,
      pending: false,
    })),
    authorizeModelControl: vi.fn(),
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
    launchDoubaoWorkForAudit: vi.fn(async () => { }),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getToolVersions: vi.fn(async () => []),
    getAgentVersions: vi.fn(async () => ({})),
    openExternal: vi.fn(async () => { }),
  },
}));
function mount(
  compact = false,
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  onEditingChange?: (state: { dirty: boolean; busy: boolean }) => void,
) {
  return render(
    <QueryClientProvider client={client}>
      <AgentConnectionsPage
        compact={compact}
        onEditingChange={onEditingChange}
      />
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

  it("reorders from the trailing handle, saves the order and restores it on remount", async () => {
    const { unmount } = mount();
    await screen.findByRole("article", { name: "apps.codex" });
    const codex = screen.getByRole("article", { name: "apps.codex" });
    expect(codex.lastElementChild).toBe(handle("codex"));
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
    const toggle = within(row).getByRole("switch", {
      name: row.getAttribute("aria-label") ?? undefined,
    });
    drag("codex", 144, 48);
    fireEvent.pointerUp(document, { clientX: 12, clientY: 48 });
    await waitFor(() => expect(order()[0]).toBe("apps.codex"));
    expect(
      within(row).getByRole("switch", {
        name: row.getAttribute("aria-label") ?? undefined,
      }),
    ).toBe(toggle);
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
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
  });
});

describe("desktop Agent directory", () => {
  const layoutKey = "vary-agent-layout";

  afterEach(() => localStorage.removeItem(layoutKey));

  it("contains scrolling in the active tab panel below the directory controls", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      claude: true,
      gemini: false,
    });
    mount();
    const controls = [
      screen.getByRole("heading", { name: "agentConnections.title" }),
      screen.getByRole("button", { name: "agentConnections.rescan" }),
      screen.getByRole("tablist"),
      screen.getByRole("searchbox"),
    ];
    const installedPanel = screen.getByRole("tabpanel");
    await within(installedPanel).findByRole("article", { name: "apps.claude" });
    expect(installedPanel).toHaveClass("min-h-0", "overflow-y-auto");
    expect(installedPanel).toHaveAttribute("tabindex", "0");
    for (const control of controls) {
      expect(installedPanel).not.toContainElement(control);
    }

    await userEvent.click(
      screen.getByRole("tab", {
        name: /^agentConnections.directory.notInstalled/,
      }),
    );
    const missingPanel = screen.getByRole("tabpanel");
    expect(missingPanel).not.toBe(installedPanel);
    expect(installedPanel).not.toBeVisible();
    expect(missingPanel).toHaveClass("min-h-0", "overflow-y-auto");
    expect(missingPanel).toHaveAttribute("tabindex", "0");
    expect(
      within(missingPanel).getByRole("group", { name: "apps.gemini" }),
    ).toBeVisible();
    for (const control of controls) {
      expect(missingPanel).not.toContainElement(control);
    }
  });

  it.each(["Macintosh", "Windows NT 10.0"])(
    "uses consistent list rows across installed Agents on %s",
    async (platform) => {
      const userAgent = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(platform);
      const restartApps = new Set([
        "claude-desktop",
        "codex",
        "workbuddy",
        "workbuddy-ai",
        "qoder",
        "trae",
        "trae-intl",
        "kimi",
        "zcode",
        "deepseek-harness",
        "t3code",
      ]);
      try {
        vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
          Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, true])),
        );
        vi.mocked(agentConnectionsApi.list).mockResolvedValue(
          MANAGED_APP_IDS.map((app, index) => ({
            app,
            enabled: index % 2 === 0,
            configured: index % 2 === 0,
            model: null,
            recoveryPending: false,
            restartRequired: restartApps.has(app),
            desktopState: restartApps.has(app) ? "restart_required" : null,
          })),
        );
        vi.mocked(settingsApi.getAgentVersions).mockResolvedValueOnce({
          "deepseek-harness": {
            supported: true,
            installed: true,
            version: "1.0.0",
            latest_version: "1.1.0",
            source: "desktop",
          },
        });
        mount();
        const panel = within(screen.getByRole("tabpanel"));
        const harness = within(
          await panel.findByRole("article", { name: "apps.deepseek-harness" }),
        );
        const version = await harness.findByText("App 1.0.0");
        const update = harness.getByRole("button", {
          name: "agentUpdates.update",
        });
        expect(version.compareDocumentPosition(update)).toBe(
          Node.DOCUMENT_POSITION_FOLLOWING,
        );
        for (const [index, app] of MANAGED_APP_IDS.entries()) {
          const row = panel.getByRole("article", { name: `apps.${app}` });
          const rowQueries = within(row);
          const title = rowQueries.getByRole("heading");
          const status = rowQueries.getByRole("status");
          const statusText = status.querySelector(".mp-agent-status-text");
          const toggle = rowQueries.getByRole("switch");
          const restart = rowQueries.queryByRole("button", {
            name: "agentConnections.restart",
          });
          expect(row.closest(".agent-directory-rows")).toBeInTheDocument();
          expect(title.parentElement?.nextElementSibling).toBe(status);
          if (restartApps.has(app)) {
            expect(
              index % 2 === 0 ? title.parentElement : status,
            ).toContainElement(restart);
          } else {
            expect(restart).not.toBeInTheDocument();
          }
          expect(statusText).toHaveTextContent(/\S/);
          expect(status).toHaveAttribute("title", statusText?.textContent);
          expect(toggle.closest(".agent-list-controls")).toBeInTheDocument();
          expect(toggle).toHaveAttribute(
            "aria-checked",
            String(index % 2 === 0),
          );
        }
        expect(
          panel
            .getByRole("article", { name: "apps.workbuddy" })
            .querySelector(".mp-agent-name .mp-agent-checkin-trigger"),
        ).toBeInTheDocument();
        expect(agentConnectionsApi.set).not.toHaveBeenCalled();
        expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      } finally {
        userAgent.mockRestore();
      }
    },
  );

  it("omits GLM from both Agent directory tabs even with stored state", async () => {
    localStorage.setItem("vary-agent-order", JSON.stringify(["glm", "claude"]));
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      claude: true,
      glm: true,
    });
    mount();
    await screen.findByRole("article", { name: "apps.claude" });
    expect(
      screen.queryByRole("article", { name: "apps.glm" }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("tab", {
        name: /^agentConnections.directory.notInstalled/,
      }),
    );
    expect(
      screen.queryByRole("group", { name: "apps.glm" }),
    ).not.toBeInTheDocument();
  });

  it("omits repeated captions and keeps each Agent's controls accessible", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
      Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, true])),
    );
    mount();
    const tabPanel = screen.getByRole("tabpanel");
    await within(tabPanel).findByRole("article", { name: "apps.claude" });

    expect(
      within(tabPanel).queryByText("agentConnections.model"),
    ).not.toBeInTheDocument();
    expect(
      within(tabPanel).queryByText("agentConnections.directory.connection"),
    ).not.toBeInTheDocument();
    for (const app of MANAGED_APP_IDS) {
      const name = `apps.${app}`;
      const row = within(within(tabPanel).getByRole("article", { name }));
      expect(row.getByRole("switch", { name })).toBeInTheDocument();
      expect(
        row.getByRole("button", { name: "agentConnections.sorting.handle" }),
      ).toBeInTheDocument();
      expect(
        app === "doubaowork"
          ? row.getByRole("note", { name: /^agentConnections\.modelList / })
          : row.getByRole("button", { name: /^agentConnections\.model / }),
      ).toBeInTheDocument();
    }
  });

  it.each(["cards", "list", "invalid"])(
    "keeps both tabs in list view with a saved %s preference",
    async (saved) => {
      localStorage.setItem(layoutKey, saved);
      mount();
      const claude = await screen.findByRole("article", {
        name: "apps.claude",
      });
      expect(claude.closest(".agent-directory-rows")).toBeInTheDocument();
      expect(
        screen.queryByRole("group", {
          name: "agentConnections.directory.layout",
        }),
      ).not.toBeInTheDocument();
      await userEvent.click(
        screen.getByRole("tab", {
          name: /^agentConnections.directory.notInstalled/,
        }),
      );
      const gemini = await screen.findByRole("group", { name: "apps.gemini" });
      expect(gemini.closest(".agent-directory-rows")).toBeInTheDocument();
      expect(
        within(gemini).getByRole("button", {
          name: "agentConnections.website",
        }),
      ).toBeEnabled();
      expect(localStorage.getItem(layoutKey)).toBe(saved);
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it("keeps the tray compact with a saved card preference", async () => {
    localStorage.setItem(layoutKey, "cards");
    mount(true);
    const trayRow = await screen.findByRole("article", { name: "apps.claude" });
    expect(trayRow.closest(".mp-list")).toBeInTheDocument();
    expect(
      screen.queryByRole("group", {
        name: "agentConnections.directory.layout",
      }),
    ).not.toBeInTheDocument();
  });

  it.each(
    ["Macintosh", "Windows NT 10.0"].flatMap((platform) =>
      [true, false].map((installed) => ({ platform, installed })),
    ),
  )(
    "shows the illustrated search empty state without a button on $platform (installed=$installed)",
    async ({ platform, installed }) => {
      const userAgent = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(platform);
      try {
        vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
          Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, installed])),
        );
        mount();
        if (!installed) {
          await userEvent.click(
            screen.getByRole("tab", {
              name: /^agentConnections.directory.notInstalled/,
            }),
          );
        }
        const tabPanel = screen.getByRole("tabpanel");
        const panel = within(tabPanel);
        const rowRole = installed ? "article" : "group";
        await panel.findByRole(rowRole, { name: "apps.codex" });
        const search = screen.getByRole("searchbox");
        fireEvent.change(search, { target: { value: "  " } });
        expect(panel.getByRole(rowRole, { name: "apps.codex" })).toBeVisible();
        expect(
          panel.queryByText("agentConnections.directory.noMatches"),
        ).not.toBeInTheDocument();

        fireEvent.change(search, { target: { value: "tool-with-no-match" } });
        const title = panel.getByRole("heading", {
          name: "agentConnections.directory.noMatches",
        });
        expect(title.closest('[role="status"]')).toHaveTextContent(
          "agentConnections.directory.noMatchesHint",
        );
        const illustration = panel.getByAltText("");
        expect(illustration).toHaveAttribute(
          "src",
          expect.stringContaining("agent-search-empty"),
        );
        expect(illustration.closest(".search-empty-state")?.parentElement).toBe(
          tabPanel,
        );
        expect(panel.queryByRole("img")).not.toBeInTheDocument();
        expect(panel.queryByRole(rowRole, { name: "apps.codex" })).toBeNull();

        expect(panel.queryByRole("button")).not.toBeInTheDocument();
        await userEvent.clear(search);
        expect(search).toHaveValue("");
        expect(search).toHaveFocus();
        expect(title).not.toBeInTheDocument();
        for (const app of MANAGED_APP_IDS) {
          const name =
            !installed && app === "claude" ? "Claude Code" : `apps.${app}`;
          expect(panel.getByRole(rowRole, { name })).toBeVisible();
        }
        expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      } finally {
        userAgent.mockRestore();
      }
    },
  );

  it("searches each installation tab and restores the saved Agent order", async () => {
    localStorage.setItem("vary-agent-order", JSON.stringify(["pi", "codex"]));
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      claude: true,
      codex: true,
      pi: true,
      gemini: false,
    });
    mount();
    const codex = await screen.findByRole("article", { name: "apps.codex" });
    const installed = screen.getByRole("tab", {
      name: /^agentConnections.directory.installed/,
    });
    const missing = screen.getByRole("tab", {
      name: /^agentConnections.directory.notInstalled/,
    });
    const search = screen.getByRole("searchbox", {
      name: "agentConnections.directory.search",
    });
    expect(installed).toHaveAttribute("aria-selected", "true");
    expect(within(codex).getByRole("switch")).not.toBeChecked();
    fireEvent.change(search, { target: { value: "  CODEX " } });
    expect(
      within(screen.getByRole("tabpanel")).getAllByRole("article"),
    ).toEqual([codex]);
    expect(
      within(codex).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toBeEnabled();
    expect(
      within(codex).queryByRole("button", {
        name: "agentConnections.sorting.handle",
      }),
    ).not.toBeInTheDocument();
    installed.focus();
    await userEvent.keyboard("[ArrowRight]");
    expect(missing).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("article")).not.toBeInTheDocument();
    expect(
      screen.getByText("agentConnections.directory.noMatches"),
    ).toBeInTheDocument();
    await userEvent.clear(search);
    expect(search).toHaveFocus();
    expect(missing).toHaveAttribute("aria-selected", "true");
    fireEvent.change(search, { target: { value: "gemini" } });
    const gemini = within(screen.getByRole("tabpanel")).getByRole("group", {
      name: "apps.gemini",
    });
    expect(
      within(gemini).getByRole("button", { name: "agentConnections.website" }),
    ).toBeVisible();
    expect(within(gemini).queryByRole("switch")).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "" } });
    await userEvent.click(installed);
    expect(
      screen.getAllByRole("article").map((card) => card.ariaLabel),
    ).toEqual(["apps.pi", "apps.codex", "apps.claude"]);
    fireEvent.change(search, { target: { value: "claude code" } });
    expect(screen.getByRole("article", { name: "apps.claude" })).toBeVisible();
    expect(localStorage.getItem("vary-agent-order")).toBe('["pi","codex"]');
  });

  it("moves a newly detected tool to Installed after a rescan", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      claude: true,
      gemini: false,
    });
    mount();
    await screen.findByRole("article", { name: "apps.claude" });
    await userEvent.click(
      screen.getByRole("tab", {
        name: /^agentConnections.directory.notInstalled/,
      }),
    );
    expect(screen.getByRole("group", { name: "apps.gemini" })).toBeVisible();
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      claude: true,
      gemini: true,
    });
    await userEvent.click(
      screen.getByRole("button", { name: "agentConnections.rescan" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("group", { name: "apps.gemini" }),
      ).not.toBeInTheDocument(),
    );
    await userEvent.click(
      screen.getByRole("tab", {
        name: /^agentConnections.directory.installed/,
      }),
    );
    expect(screen.getByRole("article", { name: "apps.gemini" })).toBeVisible();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
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
  expect(
    within(row).getByRole("switch", {
      name: row.getAttribute("aria-label") ?? undefined,
    }),
  ).toBeInTheDocument();
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
it("keeps versions and updates beside the Agent name", async () => {
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
  const metadata = row.querySelector(".mp-agent-cli");
  expect(heading.nextElementSibling).toBe(metadata);
  expect(metadata).toContainElement(version);
  expect(metadata).toContainElement(
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
  updatePromptState.visible = true;
  vi.clearAllMocks();
  vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValue({
    accounts: [],
    nativeStatus: {
      autoEnabled: false,
      day: "2026-11-11",
      outcome: "credentials_unavailable",
      checkedAt: null,
      credit: null,
      streak: null,
      creditDelayed: false,
    },
  });
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
  vi.mocked(agentConnectionsApi.modelControl).mockResolvedValue({
    authorized: false,
    ready: false,
    pending: false,
  });
  vi.mocked(agentConnectionsApi.authorizeModelControl).mockResolvedValue();
  vi.mocked(agentConnectionsApi.reapply).mockResolvedValue();
  vi.mocked(agentConnectionsApi.restart).mockResolvedValue();
  vi.mocked(agentConnectionsApi.modelVisibility).mockResolvedValue({
    only: false,
    ids: [],
  });
  vi.mocked(agentConnectionsApi.setModelVisibility).mockResolvedValue();
});

describe("installation rescan feedback", () => {
  it.each(["connect", "reapply"])(
    "offers a confirmed restart after a stale desktop %s",
    async (operation) => {
      const app = "claude-desktop";
      const connection = {
        app,
        enabled: false,
        configured: false,
        model: null,
        recoveryPending: operation === "reapply",
        recoveryReason: "configuration_changed" as const,
        restartRequired: false,
      };
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([connection]);
      const applied = async () => {
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          {
            ...connection,
            enabled: true,
            configured: true,
            recoveryPending: false,
            restartRequired: true,
            desktopState: "restart_required",
          },
        ]);
      };
      vi.mocked(agentConnectionsApi.set).mockImplementation(applied);
      vi.mocked(agentConnectionsApi.reapply).mockImplementation(applied);
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      fireEvent.click(within(row).getByRole("switch"));
      const dialog = await screen.findByRole("dialog");
      const restart = within(dialog).getByRole("button", {
        name: "agentConnections.restart",
      });
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(restart);
      await waitFor(() =>
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith(app, true),
      );
    },
  );
  it("clears the busy state after a failed refresh and allows retry", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    mount(false, client);
    await waitFor(() => expect(client.isFetching()).toBe(0));
    const button = screen.getByRole("button", {
      name: "agentConnections.rescan",
    });
    let failRefresh!: () => void;
    const refresh = vi.spyOn(client, "invalidateQueries").mockReturnValueOnce(
      new Promise((_resolve, reject) => {
        failRefresh = () => reject(new Error("Synthetic refresh failure"));
      }),
    );
    const notify = vi.spyOn(toast, "error").mockReturnValue("rescan-error");
    fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(button.querySelector("svg")).toHaveClass("animate-spin");
    await act(async () => failRefresh());
    await waitFor(() => expect(button).toBeEnabled());
    expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
    expect(notify).toHaveBeenCalledWith("agentConnections.rescanFailed");
    refresh.mockRestore();
    notify.mockRestore();

    const previousCalls = vi.mocked(proxyApi.getAgentInstallations).mock.calls
      .length;
    fireEvent.click(button);
    await waitFor(() => expect(button).toBeEnabled());
    expect(proxyApi.getAgentInstallations).toHaveBeenCalledTimes(
      previousCalls + 1,
    );
  });

  it.each([
    { compact: false, empty: false },
    { compact: true, empty: false },
    { compact: false, empty: true },
    { compact: true, empty: true },
  ])(
    "keeps all rescan buttons spinning until the full refresh completes ($compact, $empty)",
    async ({ compact, empty }) => {
      const success = vi
        .spyOn(toast, "success")
        .mockReturnValue("rescan-toast");
      const presence: Record<string, boolean> = empty ? {} : { claude: true };
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(presence);
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const { container } = mount(compact, client);
      await waitFor(() => expect(client.isFetching()).toBe(0));
      if (compact) {
        fireEvent.click(
          container.querySelector("details.mp-agent-more summary")!,
        );
      }
      const buttons = screen.getAllByRole("button", {
        name: "agentConnections.rescan",
      });
      expect(buttons).toHaveLength(compact && empty ? 2 : 1);
      for (const button of buttons) {
        expect(button).toBeEnabled();
        expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
      }

      let finishDetection!: () => void;
      let finishVersions!: () => void;
      vi.mocked(proxyApi.getAgentInstallations).mockReturnValueOnce(
        new Promise((resolve) => {
          finishDetection = () => resolve(presence);
        }),
      );
      vi.mocked(settingsApi.getAgentVersions).mockReturnValueOnce(
        new Promise((resolve) => {
          finishVersions = () => resolve({});
        }),
      );
      fireEvent.click(buttons[0]);
      await waitFor(() => {
        for (const button of buttons) {
          expect(button).toBeDisabled();
          expect(button).toHaveAttribute("aria-busy", "true");
          expect(button.querySelector("svg")).toHaveClass("animate-spin");
        }
      });
      const detectionCalls = vi.mocked(proxyApi.getAgentInstallations).mock
        .calls.length;
      fireEvent.click(buttons[buttons.length - 1]);
      expect(proxyApi.getAgentInstallations).toHaveBeenCalledTimes(
        detectionCalls,
      );

      await act(async () => finishDetection());
      await waitFor(() =>
        expect(client.isFetching({ queryKey: ["agent-installations"] })).toBe(
          0,
        ),
      );
      for (const button of buttons) {
        expect(button).toBeDisabled();
        expect(button.querySelector("svg")).toHaveClass("animate-spin");
      }
      expect(success).not.toHaveBeenCalledWith(
        "agentConnections.rescanSuccess",
      );

      await act(async () => finishVersions());
      await waitFor(() => {
        for (const button of buttons) {
          expect(button).toBeEnabled();
          expect(button).toHaveAttribute("aria-busy", "false");
          expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
        }
      });
      expect(success).toHaveBeenCalledWith("agentConnections.rescanSuccess");
      success.mockRestore();
    },
  );
});

describe("tool discovery empty state", () => {
  it.each([false, true])(
    "offers installation and rescan actions (tray: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
        {} as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>,
      );
      const { container } = mount(compact);
      const empty = await screen.findByRole("region", {
        name: "agentConnections.emptyTitle",
      });
      expect(
        within(empty).getByText("agentConnections.empty"),
      ).toBeInTheDocument();
      expect(screen.queryByRole("article")).not.toBeInTheDocument();
      const disclosure = container.querySelector("details.mp-agent-more");
      const browse = within(empty).getByRole("button", {
        name: "agentConnections.browseTools",
      });
      const installedTab = () =>
        screen.getByRole("tab", {
          name: /^agentConnections.directory.installed/,
        });
      if (compact) {
        expect(disclosure).not.toHaveAttribute("open");
        fireEvent.click(browse);
        expect(disclosure).toHaveAttribute("open");
        expect(disclosure?.querySelector("summary")).toHaveFocus();
      } else {
        await userEvent.click(browse);
        const missing = screen.getByRole("tab", {
          name: /^agentConnections.directory.notInstalled/,
        });
        expect(missing).toHaveAttribute("aria-selected", "true");
        expect(missing).toHaveFocus();
        await userEvent.click(installedTab());
      }

      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        claude: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      fireEvent.click(
        (compact ? within(empty) : screen).getByRole("button", {
          name: "agentConnections.rescan",
        }),
      );
      await screen.findByRole("article", { name: "apps.claude" });
      expect(
        screen.queryByRole("region", { name: "agentConnections.emptyTitle" }),
      ).not.toBeInTheDocument();
    },
  );

  it("does not mistake pending detection for an empty installation", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockReturnValue(
      new Promise(() => { }),
    );
    mount();
    expect(
      screen.getByRole("status", { name: "agentConnections.states.checking" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("agentConnections.emptyTitle"),
    ).not.toBeInTheDocument();
  });

  it("does not show the empty state when installation detection fails", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockRejectedValue(
      new Error("Synthetic detection failure"),
    );
    mount();
    await screen.findByRole("alert");
    expect(
      screen.queryByText("agentConnections.empty"),
    ).not.toBeInTheDocument();
  });
});

describe("Agent connections", () => {
  it.each(["workbuddy-ai", "codex", "trae-intl", "claude"] as const)(
    "reopens %s with its previous model or group, falling back only when it is unavailable",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      let enabled = true;
      const remembered = "vary-auto/second";
      vi.mocked(agentConnectionsApi.get).mockImplementation(async (id) => ({
        app: id,
        enabled: id === app && enabled,
        configured: id === app && enabled,
        recoveryPending: false,
        restartRequired: id === "codex" && !enabled,
        reloadState:
          id === "codex" && !enabled ? "waiting_for_idle" : undefined,
        model: id === app ? remembered : null,
      }));
      vi.mocked(agentConnectionsApi.models).mockResolvedValue([
        { id: "vary-auto/first", name: "First group", contextWindow: null },
        { id: remembered, name: "Second group", contextWindow: null },
      ]);
      vi.mocked(agentConnectionsApi.set).mockImplementation(
        async (_app, value) => {
          enabled = value;
        },
      );
      const { unmount } = mount();
      const toggle = await screen.findByRole("switch", { name: `apps.${app}` });
      await waitFor(() => expect(toggle).toBeChecked());
      fireEvent.click(toggle);
      const dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.disconnectRestore",
        }),
      );
      await waitFor(() => expect(toggle).not.toBeChecked());
      if (app === "codex") {
        const row = within(screen.getByRole("article", { name: "apps.codex" }));
        expect(row.getByRole("status")).toHaveTextContent(
          "agentConnections.states.off",
        );
        expect(
          row.queryByRole("button", { name: "agentConnections.restart" }),
        ).not.toBeInTheDocument();
        expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      }
      unmount();
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      mount(false, client);
      const reopened = await screen.findByRole("switch", {
        name: `apps.${app}`,
      });
      await waitFor(() => expect(reopened).toBeEnabled());
      fireEvent.click(reopened);
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenLastCalledWith(
          app,
          true,
          remembered,
        ),
      );
      await act(async () => {
        enabled = false;
        client.setQueryData(["agent-connections", app], {
          app,
          enabled: false,
          configured: false,
          recoveryPending: false,
          restartRequired: false,
          model: remembered,
        });
        client.setQueryData(
          ["agent-model-catalog"],
          [{ id: "vary-auto/first", name: "First group", contextWindow: null }],
        );
      });
      await waitFor(() => {
        expect(reopened).not.toBeChecked();
        expect(reopened).toBeEnabled();
      });
      fireEvent.click(reopened);
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenLastCalledWith(
          app,
          true,
          "vary-auto/first",
        ),
      );
    },
  );
  it.each([false, true])(
    "shows connection errors below the affected Agent without a popup (compact: %s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        "workbuddy-ai": true,
        claude: true,
      });
      vi.mocked(agentConnectionsApi.set).mockImplementation(async (app) => {
        if (app === "workbuddy-ai") throw "agentConnections.conflict";
      });
      mount(compact);
      const international = await screen.findByRole("article", {
        name: "apps.workbuddy-ai",
      });
      if (compact) fireEvent.click(international);
      const toggle = within(international).getByRole("switch");
      await waitFor(() => expect(toggle).toBeEnabled());
      fireEvent.click(toggle);
      expect(await within(international).findByRole("alert")).toHaveTextContent(
        "agentConnections.conflict",
      );
      expect(toggle).not.toBeChecked();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      if (compact) fireEvent.click(international);
      expect(within(international).getByRole("alert")).toBeVisible();
      const claude = screen.getByRole("article", { name: "apps.claude" });
      if (compact) fireEvent.click(claude);
      fireEvent.click(within(claude).getByRole("switch"));
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledTimes(2),
      );
      expect(within(claude).queryByRole("alert")).not.toBeInTheDocument();
      expect(within(international).getByRole("alert")).toHaveTextContent(
        "agentConnections.conflict",
      );
      vi.mocked(agentConnectionsApi.set).mockResolvedValue();
      if (compact) fireEvent.click(international);
      fireEvent.click(toggle);
      await waitFor(() =>
        expect(
          within(international).queryByRole("alert"),
        ).not.toBeInTheDocument(),
      );
    },
  );

  it.each([
    ["agentConnections.noModels", "agentConnections.noModels"],
    [new Error("synthetic-private-error"), "agentConnections.failed"],
  ])(
    "keeps safe connection failure details on the Agent row: %s",
    async (cause, message) => {
      vi.mocked(agentConnectionsApi.set).mockRejectedValueOnce(cause);
      mount();
      const row = await screen.findByRole("article", { name: "apps.claude" });
      const toggle = within(row).getByRole("switch");
      await waitFor(() => expect(toggle).toBeEnabled());
      fireEvent.click(toggle);
      expect(await within(row).findByRole("alert")).toHaveTextContent(message);
      expect(
        screen.queryByText("synthetic-private-error"),
      ).not.toBeInTheDocument();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    },
  );
  it.each(["workbuddy", "workbuddy-ai"] as const)(
    "does not reconnect or request a restart when %s's active default is selected again",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: true,
          recoveryPending: false,
          restartRequired: false,
          desktopState: "current",
          model: "vary-auto/shared",
        },
      ]);
      vi.mocked(agentConnectionsApi.modelControl).mockResolvedValue({
        authorized: true,
        ready: true,
        pending: false,
      });
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      fireEvent.click(
        await within(row).findByRole("button", {
          name: `agentConnections.model apps.${app}`,
        }),
      );
      fireEvent.click(
        await screen.findByRole("radio", { name: /Shared group/ }),
      );
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
      );
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      expect(
        within(row).queryByRole("button", { name: "agentConnections.restart" }),
      ).not.toBeInTheDocument();
    },
  );

  it.each(["workbuddy", "workbuddy-ai", "qoder"] as const)(
    "requires model-control consent before saving %s's new-chat default",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: true,
          recoveryPending: false,
          restartRequired: false,
          model: "vary-auto/shared",
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      const trigger = within(row).getByRole("button", {
        name: `agentConnections.model apps.${app}`,
      });
      await waitFor(() => expect(trigger).toBeEnabled());
      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("radio", { name: /Shared group/ }),
      );
      const dialog = await screen.findByRole("dialog", {
        name: "agentConnections.modelControlTitle",
      });
      expect(dialog).toHaveAccessibleDescription(
        app === "qoder"
          ? "agentConnections.modelControlConsent"
          : "agentConnections.workbuddyModelControlConsent",
      );
      expect(trigger).toHaveAttribute(
        "title",
        app === "qoder"
          ? "agentConnections.modelControlHint"
          : "agentConnections.workbuddyModelControlHint",
      );
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.authorizeModelControl).not.toHaveBeenCalled();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.modelControlAllow",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          app,
          true,
          "vary-auto/shared",
        ),
      );
      expect(agentConnectionsApi.authorizeModelControl).toHaveBeenCalledWith(
        app,
        true,
      );
      await waitFor(() => expect(dialog).not.toBeInTheDocument());
      expect(
        screen.queryByRole("dialog", {
          name: "agentConnections.restartTitle",
        }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it.each(["workbuddy", "workbuddy-ai", "qoder"] as const)(
    "saves an authorized %s model without opening restart confirmation",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: true,
          recoveryPending: false,
          restartRequired: false,
          model: "vary-auto/previous",
        },
      ]);
      vi.mocked(agentConnectionsApi.modelControl).mockResolvedValue({
        authorized: true,
        ready: false,
        pending: true,
      });
      vi.mocked(agentConnectionsApi.models).mockResolvedValue([
        {
          id: "vary-auto/previous",
          name: "Previous group",
          contextWindow: 1000000,
        },
        {
          id: "vary-auto/shared",
          name: "Shared group",
          contextWindow: 1000000,
        },
        {
          id: "relay/alternative",
          name: "Alternative model",
          contextWindow: 128000,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      const trigger = within(row).getByRole("button", {
        name: `agentConnections.model apps.${app}`,
      });
      await waitFor(() => expect(trigger).toBeEnabled());
      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("radio", { name: /Previous group/ }),
      );
      await waitFor(() =>
        expect(trigger).toHaveAttribute("aria-expanded", "false"),
      );
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("radio", { name: /Shared group/ }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          app,
          true,
          "vary-auto/shared",
        ),
      );
      await waitFor(() =>
        expect(trigger).toHaveAttribute("aria-expanded", "false"),
      );
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      );
      expect(
        await screen.findByRole("dialog", {
          name: "agentConnections.restartTitle",
        }),
      ).toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it("keeps a pending model-control restart available after a manual client reopen", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      qoder: true,
    });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "qoder",
        enabled: true,
        configured: true,
        recoveryPending: false,
        restartRequired: false,
        desktopState: "current",
        model: "vary-auto/shared",
      },
    ]);
    vi.mocked(agentConnectionsApi.modelControl).mockResolvedValue({
      authorized: true,
      ready: false,
      pending: true,
    });
    mount();
    const row = await screen.findByRole("article", { name: "apps.qoder" });
    await waitFor(() =>
      expect(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      ).toBeEnabled(),
    );
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.modelControlPending",
    );
  });
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
      Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, true])) as Awaited<
        ReturnType<typeof proxyApi.getAgentInstallations>
      >,
    );
    fireEvent.focus(window);
    for (const app of MANAGED_APP_IDS) {
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

  it.each([false, true])(
    "follows the selected Doubao account in the reward summary (compact=%s)",
    async (compact) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        doubaowork: true,
      });
      vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
        connected: true,
        autoConfigured: true,
        day: "2026-11-16",
        outcome: "claimed",
        accounts: [
          {
            id: "first",
            label: "First account",
            current: true,
            autoEnabled: true,
            outcome: "claimed",
            credit: 400,
            checkedAt: null,
            streak: 1,
            totalDays: 4,
          },
          {
            id: "second",
            label: "Second account",
            current: false,
            autoEnabled: false,
            outcome: "not_checked",
            credit: null,
            checkedAt: null,
            streak: null,
            totalDays: null,
          },
        ],
      });
      mount(compact);
      const row = await screen.findByRole("article", {
        name: "apps.doubaowork",
      });
      if (compact)
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.doubaowork",
          }),
        );
      const trigger = await within(row).findByRole("button", {
        name: "agentRewards.autoLabel apps.doubaowork",
      });
      expect(trigger).toHaveTextContent("dailyCheckin.todayReward");
      fireEvent.click(trigger);
      const select = within(row).getByRole("combobox", {
        name: "doubaowork.checkin.account",
      });
      selectAppOption(select, "second");
      expect(trigger).toHaveAttribute("data-complete", "false");
      expect(trigger).toHaveTextContent("agentRewards.manualLabel");
      expect(trigger).not.toHaveTextContent("dailyCheckin.todayReward");
      fireEvent.click(trigger);
      fireEvent.click(trigger);
      expect(select).toHaveValue("second");
      selectAppOption(select, "first");
      expect(trigger).toHaveAttribute("data-complete", "true");
      expect(trigger).toHaveTextContent("dailyCheckin.todayReward");
      expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
      expect(doubaoCheckinApi.setAutoEnabled).not.toHaveBeenCalled();
    },
  );

  it.each(
    (["workbuddy", "doubaowork"] as const).flatMap((app) =>
      [false, true].map((allComplete) => ({ app, allComplete })),
    ),
  )(
    "keeps $app account selection available even when every account is checked in ($allComplete)",
    async ({ app, allComplete }) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      const status = {
        autoEnabled: true,
        day: "2026-11-10",
        outcome: "claimed" as const,
        checkedAt: null,
        credit: null,
        streak: null,
        creditDelayed: false,
      };
      const secondStatus = {
        ...status,
        outcome: allComplete
          ? ("already_checked" as const)
          : ("not_checked" as const),
      };
      if (app === "workbuddy") {
        vi.mocked(workbuddyCheckinApi.accounts).mockResolvedValue({
          nativeStatus: status,
          accounts: [
            { id: "first", label: "First account", source: "native", status },
            {
              id: "second",
              label: "Second account",
              source: "browser",
              status: secondStatus,
            },
          ],
        });
      } else {
        vi.mocked(doubaoCheckinApi.accounts).mockResolvedValue({
          connected: true,
          day: status.day,
          outcome: status.outcome,
          accounts: [
            {
              ...status,
              id: "first",
              label: "First account",
              current: true,
              totalDays: null,
            },
            {
              ...secondStatus,
              id: "second",
              label: "Second account",
              current: false,
              totalDays: null,
            },
          ],
        });
      }
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      const trigger = await within(row).findByRole("button", {
        name: `agentRewards.autoLabel apps.${app}`,
      });
      expect(trigger).toBeEnabled();
      const selectedComplete = app === "doubaowork" || allComplete;
      expect(trigger).toHaveAttribute(
        "data-complete",
        String(selectedComplete),
      );
      expect(trigger).toHaveTextContent(
        selectedComplete
          ? "dailyCheckin.todayChecked"
          : "agentRewards.autoLabel",
      );
      fireEvent.click(trigger);
      expect(trigger).toHaveAttribute("aria-expanded", "true");
      expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
      expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
    },
  );

  it.each(
    [false, true].flatMap((compact) =>
      [false, true].map((autoEnabled) => ({ compact, autoEnabled })),
    ),
  )(
    "keeps inactive ZCode rewards expandable with a muted activity label (compact=$compact, auto=$autoEnabled)",
    async ({ compact, autoEnabled }) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        zcode: true,
      });
      const snapshot = {
        app: "zcode" as const,
        accountId: "synthetic-account",
        label: "Synthetic account",
        autoEnabled,
        outcome: "inactive" as const,
        credit: null,
        checkedAt: null,
        nextCheckAt: null,
      };
      vi.mocked(agentRewardsApi.status).mockResolvedValue(snapshot);
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      mount(compact, client);
      const row = await screen.findByRole("article", { name: "apps.zcode" });
      if (compact)
        fireEvent.click(
          within(row).getByRole("button", {
            name: "agentConnections.details apps.zcode",
          }),
        );
      const trigger = await within(row).findByRole("button", {
        name: "agentRewards.noActivity apps.zcode",
      });
      expect(trigger).toBeEnabled();
      expect(trigger).toHaveTextContent(
        "dailyCheckin.title · agentRewards.noActivity",
      );
      expect(trigger).toHaveAttribute("data-inactive", "true");
      fireEvent.click(trigger);
      const panel = await within(row).findByRole("region", {
        name: "agentRewards.title",
      });
      expect(panel).toBeVisible();
      expect(
        within(panel).getByText("agentRewards.outcomes.inactive"),
      ).toBeVisible();
      expect(
        within(panel).getByRole("button", { name: "agentRewards.claim" }),
      ).toBeDisabled();
      fireEvent.click(trigger);
      expect(panel).not.toBeVisible();
      fireEvent.click(trigger);
      expect(panel).toBeVisible();
      const mode = autoEnabled
        ? "agentRewards.autoLabel"
        : "agentRewards.manualLabel";
      vi.mocked(agentRewardsApi.status).mockResolvedValue({
        ...snapshot,
        outcome: "not_checked",
      });
      await act(async () => {
        await client.invalidateQueries({ queryKey: ["agentRewards", "zcode"] });
      });
      await waitFor(() =>
        expect(trigger).toHaveAttribute("data-inactive", "false"),
      );
      expect(trigger).toHaveTextContent(mode);
      expect(trigger).toHaveAttribute("aria-label", `${mode} apps.zcode`);
      await act(async () => {
        client.setQueryData(["agentRewards", "zcode"], {
          ...snapshot,
          outcome: "claimed",
        });
      });
      await waitFor(() =>
        expect(trigger).toHaveTextContent("dailyCheckin.claimed"),
      );
      await act(async () => {
        client.setQueryData(["agentRewards", "zcode"], snapshot);
      });
      await waitFor(() =>
        expect(trigger).toHaveTextContent("agentRewards.noActivity"),
      );
      expect(panel).toBeVisible();
      vi.mocked(agentRewardsApi.status).mockRejectedValue(
        new Error("synthetic-unavailable"),
      );
      await act(async () => {
        await client.invalidateQueries({ queryKey: ["agentRewards", "zcode"] });
      });
      await waitFor(() =>
        expect(trigger).toHaveAttribute("data-inactive", "false"),
      );
      expect(trigger).toHaveTextContent(mode);
      expect(agentRewardsApi.claim).not.toHaveBeenCalled();
      expect(agentRewardsApi.setAuto).not.toHaveBeenCalled();
    },
  );

  it.each(
    (["workbuddy", "doubaowork", "qoder", "trae", "zcode"] as const).flatMap(
      (app) =>
        (["claimed", "already_checked"] as const).flatMap((outcome) =>
          [false, true].map((compact) => ({ app, outcome, compact })),
        ),
    ),
  )(
    "keeps the $app dropdown available while only the completed detail claim is disabled ($outcome, compact=$compact)",
    async ({ app, outcome: initialOutcome, compact }) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
      let outcome: "claimed" | "already_checked" | "not_checked" =
        initialOutcome;
      const status = () => ({
        autoEnabled: true,
        day: outcome === "not_checked" ? "2026-11-11" : "2026-11-10",
        outcome,
        checkedAt: null,
        credit: null,
        streak: null,
        creditDelayed: false,
      });
      if (app === "workbuddy") {
        vi.mocked(workbuddyCheckinApi.accounts).mockImplementation(
          async () => ({
            nativeStatus: status(),
            accounts: [
              {
                id: "synthetic",
                label: "Synthetic account",
                source: "native",
                status: status(),
              },
            ],
          }),
        );
      } else if (app === "doubaowork") {
        vi.mocked(doubaoCheckinApi.accounts).mockImplementation(async () => ({
          connected: true,
          day: status().day,
          outcome,
          accounts: [
            {
              ...status(),
              id: "synthetic",
              label: "Synthetic account",
              current: true,
              totalDays: null,
            },
          ],
        }));
      } else {
        vi.mocked(agentRewardsApi.status).mockImplementation(async (app) => ({
          ...status(),
          app,
          accountId: "synthetic",
          label: "Synthetic account",
          nextCheckAt: null,
        }));
      }
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      mount(compact, client);
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
      expect(trigger).toBeEnabled();
      fireEvent.click(trigger);
      expect(trigger).toHaveAttribute("aria-expanded", "true");
      const claim = await within(row).findByRole("button", {
        name: /^dailyCheckin\.(todayReward|todayChecked|claimedReward|claimed)$/,
      });
      expect(claim).toBeDisabled();
      fireEvent.click(claim);
      fireEvent.click(trigger);
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      outcome = "not_checked";
      await act(async () => {
        await client.invalidateQueries({
          queryKey:
            app === "workbuddy"
              ? ["workbuddyCheckin", "accounts"]
              : app === "doubaowork"
                ? ["doubaoCheckin"]
                : ["agentRewards", app],
        });
      });
      await waitFor(() =>
        expect(
          within(row).getByRole("button", {
            name: `agentRewards.autoLabel apps.${app}`,
          }),
        ).toBeEnabled(),
      );
      expect(workbuddyCheckinApi.checkIn).not.toHaveBeenCalled();
      expect(doubaoCheckinApi.checkIn).not.toHaveBeenCalled();
      expect(agentRewardsApi.claim).not.toHaveBeenCalled();
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
      expect(trigger).toHaveTextContent("dailyCheckin.title");
      if (!compact) {
        expect(trigger.closest(".mp-agent-name")).toBeInTheDocument();
      }
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
        selectAppOption(within(panel).getByRole("combobox"), "auto");
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
          ? new Promise(() => { })
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
        `agentConnections.model apps.${app}`,
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

  it.each([
    ["macOS", "Macintosh", "Mac"],
    ["Windows", "Windows NT 10.0", "Windows"],
    ["Linux", "X11; Linux x86_64", ""],
  ])("shows only %s restart guidance", async (_platform, userAgent, suffix) => {
    const ua = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue(userAgent);
    const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("");
    try {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "qoder",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: true,
          desktopState: "restart_required",
        },
        {
          app: "codex",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: false,
          reloadState: "stale_catalog",
        },
      ]);
      mount();
      for (const app of ["qoder", "codex"]) {
        const row = await screen.findByRole("article", { name: `apps.${app}` });
        fireEvent.click(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        );
        const dialog = await screen.findByRole("dialog");
        const key =
          app === "codex" ? "codexRestartConfirmHint" : "restartConfirmHint";
        const platformSuffix =
          app === "codex" && suffix !== "Windows" ? "" : suffix;
        expect(dialog).toHaveAccessibleDescription(
          `agentConnections.${key}${platformSuffix}`,
        );
        expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
        fireEvent.click(
          within(dialog).getByRole("button", { name: "common.cancel" }),
        );
      }
    } finally {
      ua.mockRestore();
      platform.mockRestore();
    }
  });

  it.each(["Macintosh", "Windows NT 10.0"])(
    "closes restart confirmation and keeps other agents and navigation available on %s",
    async (userAgent) => {
      const ua = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      let finish!: () => void;
      vi.mocked(agentConnectionsApi.restart).mockReturnValueOnce(
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
      );
      vi.mocked(agentConnectionsApi.list).mockResolvedValue(
        ["workbuddy", "claude-desktop"].map((app) => ({
          app,
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: true,
          desktopState: "restart_required" as const,
        })),
      );
      const editing = vi.fn();
      try {
        mount(false, undefined, editing);
        const row = await screen.findByRole("article", {
          name: "apps.workbuddy",
        });
        fireEvent.click(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        );
        fireEvent.click(
          within(await screen.findByRole("dialog")).getByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        await waitFor(() =>
          expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
        );
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith(
          "workbuddy",
          true,
        );
        expect(within(row).getByRole("status")).toHaveTextContent(
          "agentConnections.states.restarting",
        );
        expect(within(row).getByRole("switch")).toBeDisabled();
        expect(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        ).toBeDisabled();
        const restartButton = within(row).getByRole("button", {
          name: "agentConnections.restart",
        });
        expect(restartButton).toHaveAttribute("aria-busy", "true");
        expect(restartButton.querySelector("svg")).toHaveClass(
          "animate-spin",
          "motion-reduce:animate-none",
        );
        expect(editing).toHaveBeenLastCalledWith({ dirty: false, busy: false });
        const other = screen.getByRole("article", {
          name: "apps.claude-desktop",
        });
        expect(within(other).getByRole("switch")).toBeEnabled();
        fireEvent.click(
          within(other).getByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        const dialog = await screen.findByRole("dialog");
        expect(dialog).toHaveAccessibleName("agentConnections.restartTitle");
        await act(async () => finish());
        await waitFor(() =>
          expect(
            within(row).getByRole("switch", { hidden: true }),
          ).toBeEnabled(),
        );
        expect(restartButton).toHaveAttribute("aria-busy", "false");
        expect(restartButton.querySelector("svg")).not.toHaveClass(
          "animate-spin",
        );
        expect(dialog).toBeInTheDocument();
        expect(agentConnectionsApi.restart).toHaveBeenCalledTimes(1);
      } finally {
        await act(async () => finish());
        ua.mockRestore();
      }
    },
  );

  it.each([false, true])(
    "tracks concurrent restart spinners independently (compact=%s)",
    async (compact) => {
      const finish = new Map<string, () => void>();
      vi.mocked(agentConnectionsApi.restart).mockImplementation(
        (app) =>
          new Promise<void>((resolve) => {
            finish.set(app, resolve);
          }),
      );
      const entries = ["codex", "workbuddy"].map((app) => ({
        app,
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: true,
        desktopState: "restart_required" as const,
      }));
      vi.mocked(agentConnectionsApi.list).mockResolvedValue(entries);
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      mount(compact, client);
      for (const { app } of entries) {
        const row = await screen.findByRole("article", { name: `apps.${app}` });
        const button = within(row).getByRole("button", {
          name: "agentConnections.restart",
        });
        expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
        fireEvent.click(button);
        fireEvent.click(
          within(await screen.findByRole("dialog")).getByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        await waitFor(() =>
          expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
        );
        expect(button.querySelector("svg")).toHaveClass("animate-spin");
      }
      // Polling can clear restart-required before the native restart has finished.
      await act(async () => {
        for (const entry of entries)
          client.setQueryData(["agent-connections", entry.app], {
            ...entry,
            restartRequired: false,
            desktopState: "current",
          });
      });
      const codex = screen.getByRole("article", { name: "apps.codex" });
      const workbuddy = screen.getByRole("article", { name: "apps.workbuddy" });
      expect(
        within(codex)
          .getByRole("button", { name: "agentConnections.restart" })
          .querySelector("svg"),
      ).toHaveClass("animate-spin");
      await act(async () => finish.get("codex")?.());
      await waitFor(() =>
        expect(
          within(codex).getByRole("switch", { hidden: true }),
        ).toBeEnabled(),
      );
      expect(within(codex).getByRole("status")).not.toHaveTextContent(
        "agentConnections.states.restarting",
      );
      const pending = within(workbuddy).getByRole("button", {
        name: "agentConnections.restart",
      });
      expect(pending).toBeDisabled();
      expect(pending.querySelector("svg")).toHaveClass("animate-spin");
      expect(within(workbuddy).getByRole("status")).toHaveTextContent(
        "agentConnections.states.restarting",
      );
      await act(async () => finish.get("workbuddy")?.());
      await waitFor(() =>
        expect(
          within(workbuddy).getByRole("switch", { hidden: true }),
        ).toBeEnabled(),
      );
      expect(agentConnectionsApi.restart).toHaveBeenCalledTimes(2);
    },
  );

  it("preserves a pending restart across navigation and releases the row after failure", async () => {
    const notify = vi.spyOn(toast, "error");
    let fail!: (cause: unknown) => void;
    vi.mocked(agentConnectionsApi.restart).mockReturnValueOnce(
      new Promise<void>((_resolve, reject) => {
        fail = reject;
      }),
    );
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "workbuddy",
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: true,
        desktopState: "restart_required",
      },
    ]);
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const view = mount(false, client);
    let row = await screen.findByRole("article", { name: "apps.workbuddy" });
    fireEvent.click(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    );
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "agentConnections.restart",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    view.unmount();
    mount(false, client);
    row = await screen.findByRole("article", { name: "apps.workbuddy" });
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.restarting",
    );
    expect(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    ).toBeDisabled();
    await act(async () => fail(new Error("synthetic private detail")));
    await waitFor(() => expect(within(row).getByRole("switch")).toBeEnabled());
    expect(within(row).getByRole("status")).not.toHaveTextContent(
      "agentConnections.states.restarting",
    );
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.restartFailed",
    );
    expect(notify).not.toHaveBeenCalled();
    expect(
      screen.queryByText(/synthetic private detail/),
    ).not.toBeInTheDocument();
    notify.mockRestore();
    fireEvent.click(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    );
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(agentConnectionsApi.restart).toHaveBeenCalledTimes(1);
  });

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
      const status = within(row).getByRole("status");
      expect(status).toHaveTextContent("agentConnections.states.off");
      fireEvent.click(
        within(status).getByRole("button", {
          name: "agentConnections.restart",
        }),
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

  it.each(
    (["Macintosh", "Windows NT 10.0", "Linux"] as const).flatMap((platform) =>
      [false, true].map((compact) => ({ platform, compact })),
    ),
  )(
    "uses native freshness for disconnected Agents on $platform (compact=$compact)",
    async ({ platform, compact }) => {
      const nativeRestartApps =
        platform === "Linux"
          ? []
          : [
            "codex",
            "claude-desktop",
            "workbuddy",
            "workbuddy-ai",
            "qoder",
            "trae",
            "trae-intl",
            "zcode",
            "t3code",
            ...(platform === "Macintosh" ? ["kimi", "deepseek-harness"] : []),
          ];
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
        Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, true])),
      );
      vi.mocked(agentConnectionsApi.list).mockResolvedValue(
        MANAGED_APP_IDS.map((app) => ({
          app,
          enabled: false,
          configured: false,
          recoveryPending: false,
          model: null,
          restartRequired: true,
          desktopState: nativeRestartApps.includes(app)
            ? "restart_required"
            : "unsupported",
          reloadState: app === "codex" ? "waiting_for_idle" : undefined,
        })),
      );
      mount(compact);
      for (const app of MANAGED_APP_IDS) {
        const row = await screen.findByRole("article", { name: `apps.${app}` });
        const status = within(row).getByRole("status");
        await waitFor(() =>
          expect(status).toHaveTextContent("agentConnections.states.off"),
        );
        const restart = within(row).queryByRole("button", {
          name: "agentConnections.restart",
        });
        if (app !== "codex" && nativeRestartApps.includes(app)) {
          expect(status).toContainElement(restart);
        } else {
          expect(restart).not.toBeInTheDocument();
        }
      }
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it.each(["claude", "codex", "gemini", "pi"] as const)(
    "puts the reopen note in the %s disconnect confirmation only where applicable",
    async (app) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: true,
          recoveryPending: false,
          restartRequired: false,
          model: "vary-auto/shared",
        },
      ]);
      mount();
      const toggle = await screen.findByRole("switch", { name: `apps.${app}` });
      await waitFor(() => expect(toggle).toBeChecked());
      fireEvent.click(toggle);
      const dialog = await screen.findByRole("dialog");
      if (app === "pi") {
        expect(dialog).not.toHaveTextContent(
          "agentConnections.disconnectReopenNote",
        );
      } else {
        expect(dialog).toHaveTextContent(
          "agentConnections.disconnectReopenNote",
        );
      }
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it.each([
    ["current", "off"],
    ["waiting_for_idle", "off"],
    ["restarting", "off"],
    ["stale_catalog", "off"],
    ["stopped", "off"],
    ["failed", "disconnectedReloadFailed"],
  ] as const)(
    "keeps disconnected Codex free of a manual restart action (%s)",
    async (reloadState, status) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "codex",
          enabled: false,
          configured: false,
          recoveryPending: false,
          model: null,
          restartRequired: true,
          desktopState: "restart_required",
          reloadState,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      expect(
        await within(row).findByText(`agentConnections.states.${status}`),
      ).toBeVisible();
      expect(
        within(row).queryByRole("button", { name: "agentConnections.restart" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  const restartableDesktopApps = [
    "claude-desktop",
    "workbuddy",
    "workbuddy-ai",
    "codex",
    "qoder",
    "trae",
    "trae-intl",
    "kimi",
    "zcode",
    "deepseek-harness",
  ] as const;
  it.each(
    restartableDesktopApps.flatMap((app) =>
      ["Macintosh", "Windows NT 10.0"].flatMap((userAgent) =>
        [false, true].map((compact) => ({ app, userAgent, compact })),
      ),
    ),
  )(
    "shows status-line restart guidance for $app on $userAgent (compact=$compact)",
    async ({ app, userAgent, compact }) => {
      const ua = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      const platform = vi
        .spyOn(navigator, "platform", "get")
        .mockReturnValue("");
      try {
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          {
            app,
            enabled: app === "codex" || !compact,
            configured: app === "codex" || !compact,
            recoveryPending: false,
            model: app === "codex" || !compact ? "vary-auto/shared" : null,
            restartRequired: true,
            desktopState: "restart_required",
          },
        ]);
        vi.mocked(agentConnectionsApi.restart).mockRejectedValueOnce(
          "agentConnections.restartBlocked",
        );
        mount(compact);
        const row = await screen.findByRole("article", { name: `apps.${app}` });
        fireEvent.click(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        );
        const dialog = await screen.findByRole("dialog");
        fireEvent.click(
          within(dialog).getByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        const guidance = await screen.findByText(
          "agentConnections.restartBlocked",
        );
        const status = guidance.closest('[role="status"]');
        expect(status).toHaveClass("mp-agent-status");
        expect(guidance).toHaveClass("agent-feedback");
        expect(guidance).not.toHaveClass("err");
        expect(status).not.toHaveClass("text-destructive");
        expect(status?.closest("footer")).toBeNull();
        expect(status?.closest("article")).toBe(row);
        expect(within(row).queryByRole("alert")).not.toBeInTheDocument();
        expect(toast.info).not.toHaveBeenCalled();
        expect(toast.error).not.toHaveBeenCalled();
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith(app, true);
        expect(agentConnectionsApi.set).not.toHaveBeenCalled();
        expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
        expect(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        ).toBeEnabled();
        if (!compact) expect(within(row).getByRole("switch")).toBeChecked();
      } finally {
        ua.mockRestore();
        platform.mockRestore();
      }
    },
  );

  it.each(
    restartableDesktopApps.flatMap((app) =>
      [
        "agentConnections.restartUnsupported",
        "agentConnections.restartQuitFailed",
        "agentConnections.restartOpenFailed",
        "agentConnections.restartFailed",
        "private native error",
        null,
      ].map((cause) => ({ app, cause })),
    ),
  )(
    "guides manual restart for $app ($cause) in its status line without private details",
    async ({ app, cause }) => {
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
      vi.mocked(agentConnectionsApi.restart).mockRejectedValueOnce(cause);
      mount();
      const row = await screen.findByRole("article", {
        name: `apps.${app}`,
      });
      fireEvent.click(
        within(row).getByRole("button", { name: "agentConnections.restart" }),
      );
      const dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.restart",
        }),
      );
      const key =
        typeof cause === "string" &&
          [
            "agentConnections.restartUnsupported",
            "agentConnections.restartQuitFailed",
            "agentConnections.restartOpenFailed",
          ].includes(cause)
          ? cause
          : "agentConnections.restartFailed";
      const guidance = await screen.findByText(key);
      const status = guidance.closest('[role="status"]');
      expect(status).toHaveClass("mp-agent-status");
      expect(guidance).toHaveClass("agent-feedback");
      expect(guidance).not.toHaveClass("err");
      expect(status).toHaveTextContent(key);
      expect(status).not.toHaveClass("text-destructive");
      expect(within(row).queryByRole("alert")).not.toBeInTheDocument();
      expect(toast.info).not.toHaveBeenCalled();
      expect(toast.error).not.toHaveBeenCalled();
      expect(
        screen.queryByText("private native error"),
      ).not.toBeInTheDocument();
    },
  );

  it("reports restart failures on the row and prevents duplicate calls during a confirmed retry", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: true,
        desktopState: "restart_required",
      },
    ]);
    let completeRestart!: () => void;
    vi.mocked(agentConnectionsApi.restart)
      .mockRejectedValueOnce("agentConnections.restartBlocked")
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            completeRestart = resolve;
          }),
      );
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    );
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", {
      name: "agentConnections.restart",
    });
    fireEvent.click(confirm);
    expect(
      await screen.findByText("agentConnections.restartBlocked"),
    ).toBeVisible();
    expect(toast.info).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(agentConnectionsApi.restart).toHaveBeenCalledTimes(1);
    fireEvent.click(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    );
    const retryDialog = await screen.findByRole("dialog");
    const retry = within(retryDialog).getByRole("button", {
      name: "agentConnections.restart",
    });
    fireEvent.click(retry);
    fireEvent.click(retry);
    await waitFor(() =>
      expect(agentConnectionsApi.restart).toHaveBeenCalledTimes(2),
    );
    expect(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    ).toBeDisabled();
    await act(async () => completeRestart());
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("never renders the internal restart state-change error or traps the confirmation", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        desktopState: "restart_required",
        restartRequired: true,
      },
    ]);
    vi.mocked(agentConnectionsApi.restart).mockRejectedValue(
      "agentConnections.restartChanged",
    );
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "agentConnections.restart" }),
    );
    expect(
      await screen.findByText("agentConnections.restartFailed"),
    ).toBeVisible();
    expect(
      screen.queryByText("agentConnections.restartChanged"),
    ).not.toBeInTheDocument();
    expect(toast.error).not.toHaveBeenCalledWith(
      "agentConnections.restartChanged",
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    ).toBeEnabled();
  });

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

  it.each([
    ["Macintosh", "current", "stale_catalog"],
    ["Windows NT 10.0", "current", "stale_catalog"],
    ["Macintosh", "restart_required", "stopped"],
    ["Windows NT 10.0", "restart_required", "stopped"],
    ["Macintosh", "current", "waiting_for_idle"],
    ["Windows NT 10.0", "current", "waiting_for_idle"],
  ] as const)(
    "confirms an immediate Codex restart on %s (%s, %s) and waits for its actual result",
    async (userAgent, desktopState, reloadState) => {
      const ua = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      const platform = vi
        .spyOn(navigator, "platform", "get")
        .mockReturnValue("");
      let fail!: (cause: string) => void;
      vi.mocked(agentConnectionsApi.restart).mockImplementationOnce(
        () =>
          new Promise<void>((_resolve, reject) => {
            fail = reject;
          }),
      );
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "codex",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: true,
          desktopState,
          reloadState,
        },
      ]);
      try {
        mount();
        const row = await screen.findByRole("article", { name: "apps.codex" });
        fireEvent.click(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        );
        const dialog = await screen.findByRole("dialog");
        expect(dialog).toHaveAccessibleDescription(
          userAgent.startsWith("Windows")
            ? "agentConnections.codexRestartConfirmHintWindows"
            : "agentConnections.codexRestartConfirmHint",
        );
        expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
        fireEvent.click(
          within(dialog).getByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        await waitFor(() =>
          expect(agentConnectionsApi.restart).toHaveBeenCalledWith(
            "codex",
            true,
          ),
        );
        expect(within(row).getByRole("status")).toHaveTextContent(
          "agentConnections.states.restarting",
        );
        expect(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        ).toBeDisabled();
        await act(async () => fail("agentConnections.restartBlocked"));
        await waitFor(() =>
          expect(within(row).getByRole("status")).toHaveTextContent(
            "agentConnections.restartBlocked",
          ),
        );
        expect(
          within(row).getByRole("button", { name: "agentConnections.restart" }),
        ).toBeEnabled();
      } finally {
        if (fail)
          await act(async () => fail("agentConnections.restartBlocked"));
        ua.mockRestore();
        platform.mockRestore();
      }
    },
  );

  it.each(["waiting_for_idle", "restarting"] as const)(
    "shows Codex automatic reload progress %s and permits manual override only while waiting",
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
      const restart = within(row).queryByRole("button", {
        name: "agentConnections.restart",
      });
      if (reloadState === "waiting_for_idle") expect(restart).toBeEnabled();
      else expect(restart).not.toBeInTheDocument();
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
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    ).toBeEnabled();
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.recovery",
    );
  });

  it.each([false, true])(
    "keeps TRAE bridge waiting free of reapply while honoring required restarts (compact: %s)",
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
          within(row).getByRole("button", {
            name: "agentConnections.restart",
          }),
        ).toBeEnabled();
      }
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it.each(["trae", "trae-intl"] as const)(
    "shows a loading indicator while %s applies its model catalog or starting model",
    async (app) => {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const pending = {
        app,
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "bridge_pending" as const,
        restartRequired: false,
        desktopState: "current" as const,
        model: "vary-auto/shared",
      };
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([pending]);
      mount(false, client);
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      const status = within(row).getByRole("status");
      await waitFor(() =>
        expect(status).toHaveTextContent(
          "agentConnections.states.bridgePending",
        ),
      );
      const indicator = () => status.querySelector("svg");
      expect(indicator()).toHaveClass("motion-safe:animate-spin");
      await act(async () => {
        client.setQueryData(["agent-connections", app], {
          ...pending,
          configured: true,
          recoveryReason: "bridge_selection_pending",
        });
      });
      await waitFor(() =>
        expect(status).toHaveTextContent(
          "agentConnections.states.bridgeSelectionPending",
        ),
      );
      expect(indicator()).toHaveClass("motion-safe:animate-spin");
      await act(async () => {
        client.setQueryData(["agent-connections", app], {
          ...pending,
          enabled: false,
        });
      });
      await waitFor(() =>
        expect(status).toHaveTextContent(
          "agentConnections.states.bridgeCleanupPending",
        ),
      );
      expect(indicator()).toHaveClass("motion-safe:animate-spin");
      await act(async () => {
        client.setQueryData(["agent-connections", app], {
          ...pending,
          configured: true,
          recoveryPending: false,
          recoveryReason: null,
        });
      });
      await waitFor(() =>
        expect(status).toHaveTextContent("agentConnections.states.configured"),
      );
      expect(indicator()).toBeNull();
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
      within(row).getByRole("button", { name: "agentConnections.restart" }),
    ).toBeEnabled();
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
      () => new Promise(() => { }),
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
    fireEvent.click(screen.getByRole("switch", { name: "apps.codex" }));
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
    expect(
      within(screen.getByRole("dialog")).getByRole("searchbox"),
    ).toHaveFocus();
    fireEvent.scroll(native);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.scroll(document);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(button);
    const modelSearch = await screen.findByRole("searchbox", {
      name: "modelPicker.search",
    });
    expect(modelSearch).toHaveFocus();
    fireEvent.change(modelSearch, {
      target: { value: "Shared" },
    });
    fireEvent.keyDown(modelSearch, { key: "Escape" });
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
    const hint = await screen.findByRole("note");
    expect(hint).toHaveTextContent("agentConnections.noModels");
    const dialog = screen.getByRole("dialog");
    expect(dialog).toContainElement(hint);
    expect(within(dialog).queryByRole("navigation")).not.toBeInTheDocument();
    const native = within(dialog).getByRole("radio", {
      name: "agentConnections.nativeDefault",
    });
    expect(native).toBeChecked();
    expect(native).toBeDisabled();
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
      name: "agentConnections.model apps.claude-desktop",
    });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.manageVisibleModels",
      }),
    );
    return screen.findByRole("dialog");
  }

  it("keeps Desktop visibility management inside its single model dropdown", async () => {
    const row = await desktop();
    expect(
      within(row).queryByRole("button", {
        name: "agentConnections.modelList apps.claude-desktop",
      }),
    ).not.toBeInTheDocument();
    const dialog = await openDesktopList(row);
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: /Direct model/ }),
    ).toBeInTheDocument();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.backToModels",
      }),
    );
    expect(
      within(dialog).getByRole("radio", { name: /Direct model/ }),
    ).toBeInTheDocument();
    expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
  });

  it("selects Desktop's new-session default independently of model visibility", async () => {
    const row = await desktop();
    const trigger = within(row).getByRole("button", {
      name: "agentConnections.model apps.claude-desktop",
    });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("radio", { name: /Direct model/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude-desktop",
        true,
        "direct-model",
      ),
    );
    expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
  });

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
      within(dialog).getByRole("menuitemcheckbox", {
        name: "Direct model",
      }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      within(dialog).queryByRole("button", { name: "common.save" }),
    ).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("keeps Desktop visibility independent from its connection switch", async () => {
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
      within(dialog).getByRole("menuitemcheckbox", {
        name: "Shared group",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
        "claude-desktop",
        { only: false, ids: ["vary-auto/shared"] },
      ),
    );
    expect(
      within(row).getByRole("switch", { name: "apps.claude-desktop" }),
    ).not.toBeChecked();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
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
      within(dialog).getByRole("menuitemcheckbox", {
        name: "Shared group",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(dialog).not.toHaveTextContent("synthetic-private-error");
    expect(
      within(dialog).getByRole("menuitemcheckbox", {
        name: "Shared group",
      }),
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
      within(dialog).getByRole("menuitemcheckbox", {
        name: "Shared group",
      }),
    ).toHaveAttribute("aria-checked", "false");
    expect(
      within(dialog).getByRole("menuitemcheckbox", {
        name: "Direct model",
      }),
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
        name: "agentConnections.model apps.claude-desktop",
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
      expect(
        within(row).getByRole("switch", {
          name: row.getAttribute("aria-label") ?? undefined,
        }),
      ).toBeEnabled();
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
    "shows the Desktop connection switch without a context-window switch and preserves recovery status (compact: $compact, recovery: $recoveryPending)",
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
      expect(row.querySelector(".mp-agent-detail-actions")).toBeNull();
      expect(
        within(row).getByRole("switch", { name: "apps.claude-desktop" }),
      ).toBeChecked();
      expect(
        within(row).queryByText("agentConnections.desktopLongest"),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );

  it("does not render individual settings in connected tray details", async () => {
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
    expect(
      within(panel).queryByRole("button", {
        name: "agentConnections.individual",
      }),
    ).not.toBeInTheDocument();
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
        Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, false])),
      );
      mount(compact);
      if (!compact) {
        await userEvent.click(
          screen.getByRole("tab", {
            name: /^agentConnections.directory.notInstalled/,
          }),
        );
      }
      await waitFor(() =>
        expect(document.querySelectorAll(".mp-agent-install-row")).toHaveLength(
          MANAGED_APP_IDS.length,
        ),
      );
      for (const app of MANAGED_APP_IDS) {
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

  it("shows all installed feedback in the empty Not installed tab", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
      Object.fromEntries(MANAGED_APP_IDS.map((app) => [app, true])),
    );
    mount();
    await screen.findByRole("article", { name: "apps.claude" });
    await userEvent.click(
      screen.getByRole("tab", {
        name: /^agentConnections.directory.notInstalled/,
      }),
    );
    expect(
      screen.getByText("agentConnections.directory.allInstalled"),
    ).toBeVisible();
    expect(screen.queryByRole("article")).not.toBeInTheDocument();
  });
  describe.each(["workbuddy", "workbuddy-ai", "qoder", "trae"] as const)(
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
            name: `agentConnections.${app === "trae" ? "modelList" : "model"} apps.${app}`,
          });
          if (app !== "trae") {
            expect(
              within(row).queryByRole("button", {
                name: `agentConnections.modelList apps.${app}`,
              }),
            ).not.toBeInTheDocument();
          }
          await waitFor(() => expect(trigger).toBeEnabled());
          fireEvent.click(trigger);
          const dialog = await screen.findByRole("dialog");
          if (app === "trae")
            expect(dialog).toHaveTextContent(
              pending
                ? "agentConnections.nativeCatalogPendingHint"
                : enabled
                  ? "agentConnections.nativeCatalogHint"
                  : "agentConnections.nativeCatalogDisconnectedHint",
            );
          if (app === "trae")
            fireEvent.click(
              within(dialog).getByRole("listitem", {
                name: "Shared group",
              }),
            );
          fireEvent.change(within(dialog).getByRole("searchbox"), {
            target: { value: "Direct" },
          });
          expect(
            within(dialog).getByRole(app === "trae" ? "listitem" : "radio", {
              name: /Direct model/,
            }),
          ).toBeVisible();
          expect(
            within(dialog).queryByRole(app === "trae" ? "listitem" : "radio", {
              name: /Shared group/,
            }),
          ).not.toBeInTheDocument();
          if (app === "trae")
            expect(within(dialog).queryByRole("radio")).not.toBeInTheDocument();
          expect(agentConnectionsApi.set).not.toHaveBeenCalled();
          expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
          expect(agentConnectionsApi.modelVisibility).not.toHaveBeenCalledWith(
            app,
          );
          expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
          expect(
            within(row).getByRole("switch", {
              name: row.getAttribute("aria-label") ?? undefined,
            }),
          ).toHaveAttribute("aria-checked", String(enabled));
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
          expect(
            within(row).getByRole("switch", {
              name: row.getAttribute("aria-label") ?? undefined,
            }),
          ).toBeEnabled();
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
          within(dialog).getByRole("listitem", {
            name: "Shared group",
          }),
        ).toBeVisible();
        expect(within(dialog).queryByRole("radio")).not.toBeInTheDocument();
        expect(agentConnectionsApi.set).not.toHaveBeenCalled();
        expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
      },
    );
  });

  it("shows a safe catalog error instead of a false model count", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      trae: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.models).mockRejectedValue(
      new Error("synthetic-private-catalog-error"),
    );
    mount(true);
    const row = await screen.findByRole("article", { name: "apps.trae" });
    fireEvent.click(
      within(row).getByRole("button", {
        name: "agentConnections.details apps.trae",
      }),
    );
    const trigger = within(row).getByRole("button", {
      name: "agentConnections.modelList apps.trae",
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

  it.each([false, true])(
    "shows one audit action for Doubao with stale restart flags (compact: %s)",
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
          restartRequired: true,
          desktopState: "restart_required",
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
        within(row).queryByRole("button", {
          name: "agentConnections.restart",
        }),
      ).not.toBeInTheDocument();
      expect(
        within(row).queryByRole("button", {
          name: "homeAudit.doubaoLaunch",
        }),
      ).not.toBeInTheDocument();
      expect(
        within(row).getAllByRole("button", {
          name: "homeAudit.doubaoRestart",
        }),
      ).toHaveLength(1);
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.auditWaiting",
      );
      fireEvent.click(
        within(row).getByRole("button", {
          name: "homeAudit.doubaoRestart",
        }),
      );
      await waitFor(() =>
        expect(proxyApi.launchDoubaoWorkForAudit).toHaveBeenCalledOnce(),
      );
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

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
    "offers the explicit Doubao audit launcher for opening or restarting (compact: %s)",
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
      expect(
        screen.queryByText("homeAudit.doubaoQuitFirst"),
      ).not.toBeInTheDocument();
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
          restartRequired: true,
          desktopState: "restart_required",
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
      expect(
        within(row).queryByRole("button", { name: "homeAudit.doubaoRestart" }),
      ).not.toBeInTheDocument();
      expect(
        within(row).queryByRole("button", { name: "agentConnections.restart" }),
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
      if (app === "doubaowork") {
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
        nativeCompatibility: "unsupported" as const,
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
    "disabled",
    "pending",
    "active",
    "unsupported",
    "revoked",
    "reconnect_required",
    "recovery_pending",
  ] as const)(
    "keeps configured Codex model routing independent of official-login compatibility (%s)",
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
          "agentConnections.states.restartRequired",
        ),
      );
      expect(
        within(row).getByRole("switch", {
          name: row.getAttribute("aria-label") ?? undefined,
        }),
      ).toBeChecked();
      expect(
        within(row).queryByText("agentConnections.codexNativeHint"),
      ).not.toBeInTheDocument();
      expect(
        within(row).queryByRole("button", { name: "agentConnections.reapply" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    },
  );
  it("offers reconnect beside the red bypass status and refreshes the Codex row", async () => {
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
    const status = within(row).getByRole("status");
    expect(status).toHaveClass("bad");
    expect(status).toHaveTextContent("agentConnections.states.recovery");
    expect(status).toContainElement(retry);
    expect(status.querySelector(".lucide-x")).not.toBeNull();
    expect(status.querySelector(".mp-agent-dot")).toBeNull();
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
      expect(
        within(row).getByRole("switch", {
          name: row.getAttribute("aria-label") ?? undefined,
        }),
      ).not.toBeChecked();
      expect(picker).toHaveTextContent("magpie-test-model");
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      fireEvent.click(picker);
      fireEvent.click(
        await screen.findByRole("button", {
          name: "agentConnections.disconnectOnly",
        }),
      );
      const dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveTextContent("agentConnections.disconnectHint");
      expect(dialog).not.toHaveTextContent(
        "agentConnections.inactiveCodexDisconnectHint",
      );
      expect(dialog).not.toHaveTextContent(
        "agentConnections.orphanDisconnectHint",
      );
      expect(
        within(dialog).getByRole("button", {
          name: "agentConnections.disconnectRestore",
        }),
      ).toBeEnabled();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
      fireEvent.click(
        within(dialog).getByRole("button", {
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
      await waitFor(() =>
        expect(within(row).getByRole("status")).toHaveTextContent(
          "agentConnections.states.off",
        ),
      );
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(
        within(row).getByRole("switch", {
          name: row.getAttribute("aria-label") ?? undefined,
        }),
      ).not.toBeChecked();
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
    ).toBeEnabled();
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
    expect(
      within(row).getByRole("switch", {
        name: row.getAttribute("aria-label") ?? undefined,
      }),
    ).toBeChecked();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toBeEnabled();
    fireEvent.click(
      within(row).getByRole("switch", {
        name: row.getAttribute("aria-label") ?? undefined,
      }),
    );
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
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "apps.codex: agentConnections.failed",
        expect.objectContaining({ closeButton: true }),
      ),
    );
    expect(toast.error).toHaveBeenCalledTimes(1);
    expect(within(row).queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByText("synthetic-private-reapply-error"),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.reapply",
      }),
    ).toBeEnabled();
  });
  it.each(["t3code"])(
    "connects %s to a shared model without a persistent setup note",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      mount();
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      expect(
        within(row).queryByText("agentConnections.t3codeHint"),
      ).not.toBeInTheDocument();
      const picker = within(row).getByRole("button", {
        name: `agentConnections.model apps.${app}`,
      });
      await waitFor(() => expect(picker).toBeEnabled());
      fireEvent.click(picker);
      fireEvent.click(
        await screen.findByRole("radio", { name: /Shared group/ }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.set).toHaveBeenCalledWith(
          app,
          true,
          "vary-auto/shared",
        ),
      );
    },
  );
  it.each([
    [true, "current"],
    [true, "stopped"],
    [false, "stopped"],
  ] as const)(
    "offers a confirmed Harness restart when enabled is %s and the desktop is %s",
    async (enabled, desktopState) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        "deepseek-harness": true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "deepseek-harness",
          enabled,
          configured: enabled,
          model: "vary-auto/shared",
          desktopState,
          restartRequired: false,
          recoveryPending: false,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", {
        name: "apps.deepseek-harness",
      });
      fireEvent.click(
        await within(row).findByRole("button", {
          name: "agentConnections.restart",
        }),
      );
      const dialog = screen.getByRole("dialog", {
        name: "agentConnections.restartTitle",
      });
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "agentConnections.restart",
        }),
      );
      await waitFor(() =>
        expect(agentConnectionsApi.restart).toHaveBeenCalledWith(
          "deepseek-harness",
          true,
        ),
      );
      await waitFor(() =>
        expect(
          within(row).queryByRole("button", {
            name: "agentConnections.restart",
          }),
        ).not.toBeInTheDocument(),
      );
    },
  );
  it.each([
    {
      userAgent: "Macintosh",
      enabled: true,
      compact: false,
      desktopState: "unknown",
    },
    {
      userAgent: "Windows NT 10.0",
      enabled: false,
      compact: true,
      desktopState: undefined,
    },
  ] as const)(
    "offers a confirmed Harness desktop restart without freshness on $userAgent",
    async ({ userAgent, enabled, compact, desktopState }) => {
      const os = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      try {
        vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
          "deepseek-harness": true,
        });
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          {
            app: "deepseek-harness",
            enabled,
            configured: enabled,
            model: "vary-auto/shared",
            desktopState,
            restartRequired: false,
            recoveryPending: false,
          },
        ]);
        mount(compact);
        const row = await screen.findByRole("article", {
          name: "apps.deepseek-harness",
        });
        fireEvent.click(
          await within(row).findByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
        fireEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: "agentConnections.restart",
          }),
        );
        await waitFor(() =>
          expect(agentConnectionsApi.restart).toHaveBeenCalledWith(
            "deepseek-harness",
            true,
          ),
        );
        await waitFor(() =>
          expect(
            within(row).queryByRole("button", {
              name: "agentConnections.restart",
            }),
          ).not.toBeInTheDocument(),
        );
      } finally {
        os.mockRestore();
      }
    },
  );
  it.each(["model change", "disconnect"] as const)(
    "keeps a successful Harness restart acknowledged until a %s",
    async (change) => {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const app = "deepseek-harness";
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: true,
          model: "vary-auto/previous",
          desktopState: "current",
          restartRequired: false,
          recoveryPending: false,
        },
      ]);
      vi.mocked(agentConnectionsApi.restart).mockRejectedValueOnce(
        "agentConnections.restartOpenFailed",
      );
      const view = mount(false, client);
      const rowName = { name: `apps.${app}` };
      const tools = within(await screen.findByRole("article", rowName));
      const restartButton = { name: "agentConnections.restart" };
      const confirmRestart = async () => {
        fireEvent.click(await tools.findByRole("button", restartButton));
        fireEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", restartButton),
        );
      };
      await confirmRestart();
      await screen.findByText("agentConnections.restartOpenFailed");
      await waitFor(() =>
        expect(tools.getByRole("button", restartButton)).toBeEnabled(),
      );
      await confirmRestart();
      await waitFor(() =>
        expect(
          tools.queryByRole("button", restartButton),
        ).not.toBeInTheDocument(),
      );
      view.unmount();
      mount(true, client);
      const compactTools = within(await screen.findByRole("article", rowName));
      expect(
        compactTools.queryByRole("button", restartButton),
      ).not.toBeInTheDocument();
      fireEvent.click(
        compactTools.getByRole("button", {
          name: `agentConnections.details apps.${app}`,
        }),
      );
      if (change === "model change") {
        fireEvent.click(
          compactTools.getByRole("button", {
            name: `agentConnections.model apps.${app}`,
          }),
        );
        fireEvent.click(
          await screen.findByRole("radio", { name: /Shared group/ }),
        );
        await waitFor(() =>
          expect(agentConnectionsApi.set).toHaveBeenCalledWith(
            app,
            true,
            "vary-auto/shared",
          ),
        );
      } else {
        fireEvent.click(compactTools.getByRole("switch", rowName));
        fireEvent.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: "agentConnections.disconnectRestore",
          }),
        );
        await waitFor(() =>
          expect(agentConnectionsApi.set).toHaveBeenCalledWith(
            app,
            false,
            null,
          ),
        );
      }
      expect(
        await compactTools.findByRole("button", restartButton),
      ).toBeEnabled();
    },
  );
  it("keeps the Harness restart hidden on an unsupported desktop platform", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      "deepseek-harness": true,
    });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "deepseek-harness",
        enabled: false,
        configured: false,
        model: "vary-auto/shared",
        desktopState: "unsupported",
        restartRequired: false,
        recoveryPending: false,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.deepseek-harness",
    });
    await waitFor(() =>
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.off",
      ),
    );
    expect(
      within(row).queryByRole("button", { name: "agentConnections.restart" }),
    ).not.toBeInTheDocument();
  });
  it.each(MANAGED_APP_IDS)(
    "toasts for each manual reconnect attempt for %s",
    async (app) => {
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: false,
          configured: false,
          model: "vary-auto/shared",
          recoveryPending: true,
          recoveryReason: "configuration_changed",
          restartRequired: false,
        },
      ]);
      vi.mocked(agentConnectionsApi.reapply)
        .mockRejectedValueOnce("agentConnections.conflict")
        .mockRejectedValueOnce(new Error("synthetic-private-error"))
        .mockResolvedValueOnce();
      mount(
        app !== "deepseek-harness" && MANAGED_APP_IDS.indexOf(app) % 2 === 0,
      );
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      const retry = await within(row).findByRole("button", {
        name: "agentConnections.reapply",
      });
      expect(toast.error).not.toHaveBeenCalled();
      expect(toast.success).not.toHaveBeenCalled();
      for (const [index, key] of [
        "agentConnections.conflict",
        "agentConnections.failed",
        "agentConnections.reapplySubmitted",
      ].entries()) {
        await waitFor(() => expect(retry).toBeEnabled());
        fireEvent.click(
          app === "deepseek-harness" ? within(row).getByRole("switch") : retry,
        );
        const notify = index === 2 ? toast.success : toast.error;
        await waitFor(() =>
          expect(notify).toHaveBeenLastCalledWith(
            expect.stringContaining(key),
            expect.anything(),
          ),
        );
        expect(agentConnectionsApi.reapply).toHaveBeenCalledTimes(index + 1);
        expect(within(row).queryByText(key)).not.toBeInTheDocument();
      }
      expect(toast.error).toHaveBeenCalledTimes(2);
      expect(toast.success).toHaveBeenCalledTimes(1);
    },
  );
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
  it("uses the same Magpie disconnect confirmation for receiptless Codex and keeps it disconnected after refresh", async () => {
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
    expect(dialog).toHaveTextContent("agentConnections.disconnectHint");
    expect(dialog).toHaveTextContent("agentConnections.codexDisconnectNote");
    expect(dialog).not.toHaveTextContent(
      "agentConnections.orphanDisconnectHint",
    );
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.disconnectRestore",
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
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(
        "Claude Code: agentConnections.sessionHint",
        expect.objectContaining({ closeButton: true }),
      ),
    );
  });
  it("shows the saved routing group name while its executable catalog entry is missing", async () => {
    const group = { ...blankRoutingGroup(), id: "named", name: "中文路由组" };
    const model = "vary-auto/named";
    const apps = MANAGED_APP_IDS.filter(
      (app) => app !== "doubaowork" && app !== "zcode",
    );
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue(
      Object.fromEntries(apps.map((app) => [app, true])),
    );
    vi.mocked(agentConnectionsApi.list).mockResolvedValue(
      apps.map((app) => ({
        app,
        enabled: true,
        configured: true,
        model,
        recoveryPending: false,
        restartRequired: false,
      })),
    );
    vi.mocked(agentConnectionsApi.modelControl).mockResolvedValue({
      authorized: true,
      ready: true,
      pending: false,
    });
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    vi.mocked(intentRoutingApi.list).mockResolvedValueOnce({
      groups: [group],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount(true);
    for (const app of apps) {
      const row = await screen.findByRole("article", {
        name: app === "claude" ? "apps.claude" : `apps.${app}`,
      });
      expect(
        await within(row).findByText(group.name, {
          selector: ".mp-agent-summary-model",
        }),
      ).toHaveAttribute("title", group.name);
      expect(within(row).queryByText(model)).not.toBeInTheDocument();
    }
  });
  it("shows a disabled unsupported-default-model selector for ZCode", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      zcode: true,
    });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "zcode",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        restartRequired: false,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.zcode" });
    const selector = await within(row).findByRole("button", {
      name: "agentConnections.model apps.zcode",
    });
    expect(selector).toBeDisabled();
    expect(selector).toHaveTextContent("agentConnections.defaultModelUnsupported");
    expect(selector).toHaveAttribute(
      "title",
      "agentConnections.defaultModelUnsupportedHint",
    );
    fireEvent.click(selector);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
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
    "shows a loading skeleton while other Agents finish checking (compact=%s)",
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
      const codexPlaceholder = await screen.findByRole("status", {
        name: "apps.codex: agentConnections.states.checking",
      });
      expect(codexPlaceholder).toHaveClass("mp-agent-placeholder");
      expect(
        codexPlaceholder.querySelector(".mp-agent-placeholder-name"),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("article", { name: "apps.codex" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("switch", { name: "apps.codex", hidden: compact }),
      ).not.toBeInTheDocument();
      expect(
        codexPlaceholder.querySelector(".mp-agent-placeholder-status"),
      ).toBeInTheDocument();
      const claude = await screen.findByRole("article", {
        name: "apps.claude",
      });
      await waitFor(() =>
        expect(within(claude).getByRole("status")).toHaveTextContent(
          "agentConnections.states.off",
        ),
      );
      await waitFor(() =>
        expect(
          within(claude).getByRole("switch", { hidden: compact }),
        ).toBeEnabled(),
      );
      expect(document.querySelector(".mp-list > p[role='status']")).toBeNull();
      if (compact) {
        const unconfigured = screen.getByText("trayPanel.notConfigured");
        expect(
          codexPlaceholder.compareDocumentPosition(unconfigured) &
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
      const codex = await screen.findByRole("article", { name: "apps.codex" });
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

  it("loads native models in one batch and maps them to their Agents", async () => {
    vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValueOnce([
      { app: "claude", model: "native-claude", available: true },
      { app: "codex", model: "native-codex", available: true },
      { app: "pi", model: "native-pi", available: true },
    ]);
    mount();
    const claude = await screen.findByRole("article", { name: "apps.claude" });
    await waitFor(() =>
      expect(
        within(claude).getByRole("button", {
          name: "agentConnections.model Claude Code",
        }),
      ).toHaveTextContent("native-claude"),
    );
    const codex = screen.getByRole("article", { name: "apps.codex" });
    await waitFor(() =>
      expect(
        within(codex).getByRole("button", {
          name: "agentConnections.model apps.codex",
        }),
      ).toHaveTextContent("native-codex"),
    );
    expect(within(claude).getByRole("switch")).toBeEnabled();
    expect(within(codex).getByRole("switch")).toBeEnabled();
    expect(agentConnectionsApi.nativeModels).toHaveBeenCalledTimes(1);
    expect(agentConnectionsApi.nativeModel).not.toHaveBeenCalled();
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
      () => new Promise(() => { }),
    );
    vi.mocked(agentConnectionsApi.nativeModel).mockImplementation(
      () => new Promise(() => { }),
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

it.each([false, true])(
  "shows bypass and reconnect after external takeover without writing configuration (compact=%s)",
  async (compact) => {
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
    mount(compact, client);
    if (compact) {
      fireEvent.click(
        await screen.findByRole("button", {
          name: "agentConnections.details apps.codex",
        }),
      );
    }
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
    const row = screen.getByRole("article", { name: "apps.codex" });
    const status = within(row).getByRole("status");
    const reconnect = within(row).getByRole("button", {
      name: "agentConnections.reapply",
    });
    expect(status).toHaveClass("bad");
    expect(status).toHaveTextContent("agentConnections.states.recovery");
    expect(status).toContainElement(reconnect);
    expect(status.querySelector(".lucide-x")).not.toBeNull();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    fireEvent.click(reconnect);
    await waitFor(() =>
      expect(agentConnectionsApi.reapply).toHaveBeenCalledWith("codex"),
    );
  },
);

it.each([false, true])(
  "keeps Desktop disconnected until its connection switch is enabled (compact: %s)",
  async (compact) => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      "claude-desktop": true,
    });
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: false,
        configured: false,
        recoveryPending: false,
        restartRequired: false,
        model: null,
      },
    ]);
    mount(compact);
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    if (compact) {
      fireEvent.click(
        within(row).getByRole("button", { name: /agentConnections.details/ }),
      );
    }
    const toggle = within(row).getByRole("switch", {
      name: "apps.claude-desktop",
    });
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).not.toBeChecked();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(
      within(row).queryByText("agentConnections.desktopLongest"),
    ).not.toBeInTheDocument();
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
        "claude-desktop",
        true,
        "vary-auto/shared",
      ),
    );
  },
);

it.each([false, true])(
  "closes disconnect confirmation immediately and keeps other agents and navigation available (compact: %s)",
  async (compact) => {
    const state = {
      app: "codex",
      enabled: true,
      configured: true,
      recoveryPending: false,
      restartRequired: false,
      model: "vary-auto/shared",
    };
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([state]);
    let finish!: () => void;
    vi.mocked(agentConnectionsApi.set).mockReturnValueOnce(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    const editing = vi.fn();
    mount(compact, undefined, editing);
    const row = await screen.findByRole("article", { name: "apps.codex" });
    if (compact) {
      fireEvent.click(
        within(row).getByRole("button", { name: /agentConnections.details/ }),
      );
    }
    const toggle = within(row).getByRole("switch");
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", {
      name: "agentConnections.disconnectRestore",
    });
    fireEvent.click(confirm);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.disconnecting",
      ),
    );
    expect(toggle).toBeDisabled();
    expect(toggle).toHaveAttribute("aria-busy", "true");
    expect(editing).toHaveBeenLastCalledWith({ dirty: false, busy: false });
    const other = screen.getByRole("article", { name: "apps.claude" });
    if (compact) {
      fireEvent.click(
        within(other).getByRole("button", { name: /agentConnections.details/ }),
      );
    }
    expect(within(other).getByRole("switch")).toBeEnabled();
    expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
      "codex",
      false,
      null,
    );
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      { ...state, enabled: false, configured: false },
    ]);
    await act(async () => finish());
    await waitFor(() =>
      expect(within(row).getByRole("status")).toHaveTextContent(
        "agentConnections.states.off",
      ),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  },
);

it("preserves a background disconnect across navigation and allows retry after failure", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "codex",
      enabled: true,
      configured: true,
      recoveryPending: false,
      restartRequired: false,
      model: "vary-auto/shared",
    },
  ]);
  let fail!: (error: unknown) => void;
  vi.mocked(agentConnectionsApi.set).mockReturnValueOnce(
    new Promise<void>((_resolve, reject) => {
      fail = reject;
    }),
  );
  const view = mount(false, client);
  fireEvent.click(await screen.findByRole("switch", { name: "apps.codex" }));
  fireEvent.click(
    within(await screen.findByRole("dialog")).getByRole("button", {
      name: "agentConnections.disconnectRestore",
    }),
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  view.unmount();
  mount(false, client);
  const row = await screen.findByRole("article", { name: "apps.codex" });
  expect(within(row).getByRole("status")).toHaveTextContent(
    "agentConnections.states.disconnecting",
  );
  expect(within(row).getByRole("switch")).toBeDisabled();
  await act(async () => fail(new Error("synthetic-private-disconnect-error")));
  await waitFor(() => expect(within(row).getByRole("switch")).toBeEnabled());
  expect(within(row).getByRole("status")).not.toHaveTextContent(
    "agentConnections.states.disconnecting",
  );
  expect(toast.error).toHaveBeenCalledWith(
    "apps.codex: agentConnections.failed",
  );
  expect(screen.queryByText(/synthetic-private/)).not.toBeInTheDocument();
  fireEvent.click(within(row).getByRole("switch"));
  expect(await screen.findByRole("dialog")).toBeInTheDocument();
  expect(agentConnectionsApi.set).toHaveBeenCalledTimes(1);
});

it("reports the failed disconnect action on its row without exposing the internal configuration conflict", async () => {
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "codex",
      enabled: true,
      configured: true,
      recoveryPending: false,
      restartRequired: false,
      model: "vary-auto/shared",
    },
  ]);
  vi.mocked(agentConnectionsApi.set).mockRejectedValueOnce(
    "agentConnections.conflict",
  );
  mount();
  fireEvent.click(await screen.findByRole("switch", { name: "apps.codex" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.disconnectRestore",
    }),
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  const row = screen.getByRole("article", { name: "apps.codex" });
  expect(await within(row).findByRole("alert")).toHaveTextContent(
    "agentConnections.failed",
  );
  expect(row).not.toHaveTextContent("agentConnections.conflict");
  expect(within(row).getByRole("switch")).toBeEnabled();
  expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
    "codex",
    false,
    null,
  );
});

it("keeps external Desktop configuration conflicts under explicit recovery", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    "claude-desktop": true,
  });
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "claude-desktop",
      enabled: false,
      configured: false,
      recoveryPending: true,
      recoveryReason: "configuration_changed",
      restartRequired: false,
      model: "vary-auto/shared",
    },
  ]);
  mount();
  const row = await screen.findByRole("article", {
    name: "apps.claude-desktop",
  });
  await waitFor(() =>
    expect(
      within(row).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled(),
  );
  expect(
    within(row).getByRole("switch", { name: "apps.claude-desktop" }),
  ).not.toBeChecked();
  expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
});

it("allows direct model selection during configuration_changed recovery without forcing reapply first", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    claude: true,
  });
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "claude",
      enabled: false,
      configured: false,
      recoveryPending: true,
      recoveryReason: "configuration_changed",
      restartRequired: false,
      model: "vary-auto/shared",
    },
  ]);
  mount();
  const row = await screen.findByRole("article", { name: "apps.claude" });
  const picker = within(row).getByRole("button", {
    name: "agentConnections.model Claude Code",
  });
  expect(picker).toBeEnabled();
  fireEvent.click(picker);
  const option = await screen.findByRole("radio", { name: /Shared group/ });
  fireEvent.click(option);
  await waitFor(() =>
    expect(agentConnectionsApi.set).toHaveBeenCalledWith(
      "claude",
      true,
      "vary-auto/shared",
    ),
  );
  expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
});

it("preserves specific invalidConfig and permissionDenied errors on connect and disconnect", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    claude: true,
    codex: true,
  });
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "claude",
      enabled: false,
      configured: false,
      recoveryPending: false,
      restartRequired: false,
      model: null,
    },
    {
      app: "codex",
      enabled: true,
      configured: true,
      recoveryPending: false,
      restartRequired: false,
      model: "vary-auto/shared",
    },
  ]);
  vi.mocked(agentConnectionsApi.set).mockImplementation(
    async (app, enabled) => {
      if (app === "claude" && enabled) {
        throw "agentConnections.invalidConfig";
      }
      if (app === "codex" && !enabled) {
        throw "agentConnections.permissionDenied";
      }
    },
  );
  mount();
  const claudeRow = await screen.findByRole("article", { name: "apps.claude" });
  fireEvent.click(within(claudeRow).getByRole("switch"));
  expect(await within(claudeRow).findByRole("alert")).toHaveTextContent(
    "agentConnections.invalidConfig",
  );

  const codexRow = screen.getByRole("article", { name: "apps.codex" });
  fireEvent.click(within(codexRow).getByRole("switch"));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.disconnectRestore",
    }),
  );
  expect(await within(codexRow).findByRole("alert")).toHaveTextContent(
    "agentConnections.permissionDenied",
  );
});

it("reports a failed explicit Desktop connection without exposing the backend error or retrying on render", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    "claude-desktop": true,
  });
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "claude-desktop",
      enabled: false,
      configured: false,
      recoveryPending: false,
      restartRequired: false,
      model: null,
    },
  ]);
  vi.mocked(agentConnectionsApi.set).mockRejectedValueOnce(
    new Error("synthetic-private-default-failure"),
  );
  mount();
  const toggle = await screen.findByRole("switch", {
    name: "apps.claude-desktop",
  });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "agentConnections.failed",
  );
  expect(
    screen.queryByText(/synthetic-private-default-failure/),
  ).not.toBeInTheDocument();
  expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
    "claude-desktop",
    true,
    "vary-auto/shared",
  );
});

it.each([false, true])(
  "disconnects Desktop only after confirmation and stays off after remount (compact: %s)",
  async (compact) => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      "claude-desktop": true,
    });
    const state = {
      app: "claude-desktop",
      enabled: true,
      configured: true,
      recoveryPending: false,
      restartRequired: false,
      model: "vary-auto/shared",
    };
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([state]);
    vi.mocked(agentConnectionsApi.set).mockImplementation(
      async (_app, enabled) => {
        vi.mocked(agentConnectionsApi.list).mockResolvedValue([
          { ...state, enabled, configured: enabled },
        ]);
      },
    );
    const { unmount } = mount(compact);
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    if (compact)
      fireEvent.click(
        within(row).getByRole("button", { name: /agentConnections.details/ }),
      );
    const toggle = within(row).getByRole("switch", {
      name: "apps.claude-desktop",
    });
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    const dialog = await screen.findByRole("dialog");
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    );
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(agentConnectionsApi.set).toHaveBeenCalledExactlyOnceWith(
      "claude-desktop",
      false,
      null,
    );
    unmount();
    mount(compact);
    const remountedRow = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    if (compact)
      fireEvent.click(
        within(remountedRow).getByRole("button", {
          name: /agentConnections.details/,
        }),
      );
    const remountedToggle = within(remountedRow).getByRole("switch", {
      name: "apps.claude-desktop",
    });
    await waitFor(() => expect(remountedToggle).toBeEnabled());
    expect(remountedToggle).not.toBeChecked();
    expect(agentConnectionsApi.set).toHaveBeenCalledTimes(1);
  },
);

it.each([
  ["current", "configured", false],
  ["stale_catalog", "catalogStale", true],
] as const)(
  "keeps the desktop Codex %s freshness reason separate from connection availability",
  async (reloadState, status, canRefresh) => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: canRefresh,
        reloadState,
        desktopState: "current",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(
      await within(row).findByText(`agentConnections.states.${status}`),
    ).toBeVisible();
    expect(
      within(row).queryByText("agentConnections.states.restartRequired"),
    ).not.toBeInTheDocument();
    expect(
      within(row).queryAllByRole("button", {
        name: "agentConnections.restart",
      }),
    ).toHaveLength(canRefresh ? 1 : 0);
    expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
  },
);

it.each(MANAGED_APP_IDS)(
  "exposes the correct restart or audit-launch action for %s during recovery",
  async (app) => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      [app]: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app,
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "configuration_changed",
        restartRequired: true,
        desktopState: "unknown",
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: `apps.${app}` });
    expect(
      await within(row).findByRole("button", {
        name:
          app === "doubaowork"
            ? "homeAudit.doubaoLaunch"
            : "agentConnections.restart",
      }),
    ).toBeEnabled();
    expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
  },
);

it("allows connecting supported agents even when installation detection returns false", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    claude: true,
    gemini: false,
  });
  mount();
  await screen.findByRole("article", { name: "apps.claude" });

  await userEvent.click(
    screen.getByRole("tab", {
      name: /^agentConnections.directory.notInstalled/,
    }),
  );
  const missingPanel = screen.getByRole("tabpanel");
  const geminiGroup = within(missingPanel).getByRole("group", {
    name: "apps.gemini",
  });
  const connectButton = within(geminiGroup).getByRole("button", {
    name: "agentConnections.connect",
  });
  expect(connectButton).toBeEnabled();
  await userEvent.click(connectButton);
  expect(agentConnectionsApi.set).toHaveBeenCalledWith(
    "gemini",
    true,
    expect.any(String),
  );
});
