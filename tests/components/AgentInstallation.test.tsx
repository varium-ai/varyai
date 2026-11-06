import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { settingsApi } from "@/lib/api/settings";
import { proxyApi } from "@/lib/api/proxy";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { APP_IDS } from "@/config/appConfig";

vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    nativeModels: vi.fn(async () => []),
    nativeModel: vi.fn(async (app: string) => ({
      app,
      model: null,
      available: true,
    })),
    get: vi.fn(async (app: string) => {
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
    }),
    list: vi.fn(async () => []),
    models: vi.fn(async () => []),
    set: vi.fn(),
  },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getAgentInstallations: vi.fn(),
    getProxyStatus: vi.fn(async () => ({ running: true })),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getToolVersions: vi.fn(),
    getAgentVersions: vi.fn(),
    runToolLifecycleAction: vi.fn(),
    probeToolInstallations: vi.fn(),
    openExternal: vi.fn(),
  },
}));
let detected: Record<string, boolean>;
function version(name: string, value: string | null) {
  return [
    {
      name,
      version: value,
      latest_version: null,
      error: null,
      installed_but_broken: false,
      env_type: "macos" as const,
      wsl_distro: null,
    },
  ];
}
function mount(compact = false) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <AgentConnectionsPage compact={compact} />
    </QueryClientProvider>,
  );
}
async function row(app: string) {
  await waitFor(() =>
    expect(proxyApi.getAgentInstallations).toHaveBeenCalled(),
  );
  await waitFor(() =>
    expect(screen.queryByText("common.loading")).not.toBeInTheDocument(),
  );
  const summary = await screen.findByText("agentConnections.moreTools");
  if (!summary.closest("details")?.open) fireEvent.click(summary);
  return screen.getByRole("group", { name: `apps.${app}` });
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([]);
  detected = Object.fromEntries(APP_IDS.map((app) => [app, false]));
  vi.mocked(proxyApi.getAgentInstallations).mockImplementation(async () => ({
    ...detected,
  }));
  vi.mocked(settingsApi.getAgentVersions).mockResolvedValue({});
  vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
    version(tools![0], null),
  );
  vi.mocked(settingsApi.runToolLifecycleAction).mockResolvedValue();
  vi.mocked(settingsApi.openExternal).mockResolvedValue();
});
describe("Agent installation shortcuts", () => {
  it("shows WorkBuddy AI independently when only the international edition is installed", async () => {
    detected["workbuddy-ai"] = true;
    mount();
    const international = await screen.findByRole("article", {
      name: "apps.workbuddy-ai",
    });
    expect(
      within(international).getByRole("switch", {
        name: "apps.workbuddy-ai",
      }),
    ).toBeEnabled();
    const domestic = await row("workbuddy");
    expect(
      within(domestic).getByRole("button", { name: "agentInstall.install" }),
    ).toBeEnabled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it.each([
    ["workbuddy", "https://www.workbuddy.cn/"],
    ["workbuddy-ai", "https://www.workbuddy.ai/"],
  ])("opens %s’s own website from More tools", async (app, website) => {
    mount();
    const desktop = await row(app);
    fireEvent.click(
      within(desktop).getByRole("button", {
        name: "agentConnections.website",
      }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(website),
    );
  });
  it.each(["website", "install"])(
    "opens Doubao Work's product page from the %s shortcut",
    async (shortcut) => {
      mount();
      const desktop = await row("doubaowork");
      fireEvent.click(
        within(desktop).getByRole("button", {
          name:
            shortcut === "website"
              ? "agentConnections.website"
              : "agentInstall.install",
        }),
      );
      if (shortcut === "install") {
        fireEvent.click(
          await screen.findByRole("button", {
            name: "agentInstall.openWebsite",
          }),
        );
      }
      await waitFor(() =>
        expect(settingsApi.openExternal).toHaveBeenCalledWith(
          "https://www.doubao.com/work",
        ),
      );
      expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    },
  );
  it.each([
    { app: "opencode", compact: false, recoveryPending: true },
    { app: "opencode", compact: true, recoveryPending: true },
    { app: "workbuddy", compact: false, recoveryPending: false },
    { app: "workbuddy", compact: true, recoveryPending: false },
    { app: "workbuddy-ai", compact: false, recoveryPending: false },
    { app: "workbuddy-ai", compact: true, recoveryPending: false },
  ] as const)(
    "shows only installation and website actions for missing $app with a saved connection (compact: $compact)",
    async ({ app, compact, recoveryPending }) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app,
          enabled: true,
          configured: !recoveryPending,
          model: null,
          recoveryPending,
          restartRequired: false,
        },
      ]);
      if (app !== "opencode") {
        vi.mocked(settingsApi.getAgentVersions).mockResolvedValue({
          [app]: {
            supported: true,
            installed: true,
            source: "desktop",
            version: "9.8.7",
            latest_version: null,
          },
        });
      }
      const { container } = mount(compact);
      const disclosure = (
        await screen.findByText("agentConnections.moreTools")
      ).closest("details");
      expect(disclosure).not.toHaveAttribute("open");
      fireEvent.click(screen.getByText("agentConnections.moreTools"));
      const missing = await screen.findByRole("group", {
        name: `apps.${app}`,
      });
      expect(disclosure).toContainElement(missing);
      expect(container.querySelector(".mp-list")).not.toContainElement(missing);
      expect(
        within(missing).getByRole("button", { name: "agentInstall.install" }),
      ).toBeEnabled();
      expect(
        within(missing).getByRole("button", {
          name: "agentConnections.website",
        }),
      ).toBeEnabled();
      expect(within(missing).getAllByRole("button")).toHaveLength(2);
      expect(within(missing).queryByRole("switch")).not.toBeInTheDocument();
      expect(within(missing).queryByRole("combobox")).not.toBeInTheDocument();
      expect(
        within(missing).queryByText("9.8.7", { exact: false }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    },
  );
  it("offers installation for missing agents, hides meaningless Connect actions, and never installs on mount", async () => {
    mount();
    const open = await row("opencode");
    expect(
      within(open).getByRole("button", { name: "agentInstall.install" }),
    ).toBeEnabled();
    expect(
      within(open).queryByRole("button", { name: "agentConnections.connect" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "agentInstall.install" }),
    ).toHaveLength(APP_IDS.length);
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
  });
  it("requires install consent, guards duplicate clicks, refreshes detection and version, and never connects automatically", async () => {
    detected.claude = true;
    let finish!: () => void;
    vi.mocked(settingsApi.runToolLifecycleAction).mockImplementation(
      async () => {
        await new Promise<void>((resolve) => {
          finish = resolve;
        });
        detected.opencode = true;
        vi.mocked(settingsApi.getToolVersions).mockImplementation(
          async (tools) => version(tools![0], "1.2.3"),
        );
      },
    );
    mount();
    const open = await row("opencode");
    fireEvent.click(
      within(open).getByRole("button", { name: "agentInstall.install" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText("agentInstall.consent"),
    ).toBeInTheDocument();
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    const confirm = within(dialog).getByRole("button", {
      name: "agentInstall.confirm",
    });
    fireEvent.click(confirm);
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(
        settingsApi.runToolLifecycleAction,
      ).toHaveBeenCalledExactlyOnceWith(["opencode"], "install"),
    );
    expect(
      screen.getByRole("button", { name: "agentInstall.installing" }),
    ).toBeDisabled();
    expect(screen.getByRole("switch", { name: "apps.claude" })).toBeEnabled();
    finish();
    expect(
      await screen.findByText(/agentInstall.installed/),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("article", { name: "apps.opencode" }),
    ).toBeInTheDocument();
    expect(screen.getByText("1.2.3")).toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it.each([
    ["workbuddy", "https://www.workbuddy.cn/"],
    ["workbuddy-ai", "https://www.workbuddy.ai/"],
  ])(
    "cancels without side effects and opens %s’s own installation page",
    async (app, website) => {
      mount();
      const desktop = await row(app);
      fireEvent.click(
        within(desktop).getByRole("button", { name: "agentInstall.install" }),
      );
      const dialog = await screen.findByRole("dialog");
      expect(
        within(dialog).getByText("agentInstall.websiteHint"),
      ).toBeInTheDocument();
      fireEvent.click(
        within(dialog).getByRole("button", { name: "common.cancel" }),
      );
      expect(settingsApi.openExternal).not.toHaveBeenCalled();
      fireEvent.click(
        within(desktop).getByRole("button", { name: "agentInstall.install" }),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "agentInstall.openWebsite" }),
      );
      await waitFor(() =>
        expect(settingsApi.openExternal).toHaveBeenCalledWith(website),
      );
      expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
      expect(
        screen.queryByText(/agentInstall.installed/),
      ).not.toBeInTheDocument();
    },
  );
  it("does not overwrite an installation that appeared before confirmation", async () => {
    mount();
    const open = await row("opencode");
    fireEvent.click(
      within(open).getByRole("button", { name: "agentInstall.install" }),
    );
    detected.opencode = true;
    fireEvent.click(
      await screen.findByRole("button", { name: "agentInstall.confirm" }),
    );
    expect(
      await screen.findByText(/agentInstall.alreadyInstalled/),
    ).toBeInTheDocument();
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
  });
  it.each(["command", "verification", "presence"])(
    "sanitizes %s failures and does not claim installation success",
    async (mode) => {
      if (mode === "command")
        vi.mocked(settingsApi.runToolLifecycleAction).mockRejectedValue(
          "synthetic-private-installer-output",
        );
      if (mode === "presence")
        vi.mocked(proxyApi.getAgentInstallations)
          .mockResolvedValueOnce({ ...detected })
          .mockRejectedValue("synthetic-private-detection-output");
      mount();
      const open = await row("opencode");
      fireEvent.click(
        within(open).getByRole("button", { name: "agentInstall.install" }),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "agentInstall.confirm" }),
      );
      expect(
        await screen.findByText(
          mode === "presence"
            ? /agentInstall.checkFailed/
            : mode === "command"
              ? /agentInstall.failed/
              : /agentInstall.unverified/,
        ),
      ).toBeInTheDocument();
      expect(
        screen.queryByText(/agentInstall.installed/),
      ).not.toBeInTheDocument();
      expect(screen.queryByText(/synthetic-private/)).not.toBeInTheDocument();
      if (mode === "presence")
        expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    },
  );
  it("never offers installation while detection is unavailable", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockRejectedValue(
      "synthetic-private-detection",
    );
    mount();
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    fireEvent.click(screen.getByText("agentConnections.moreTools"));
    expect(
      screen.queryByRole("button", { name: "agentInstall.install" }),
    ).not.toBeInTheDocument();
  });
  it("keeps already installed tools out of the installation flow", async () => {
    detected.hermes = true;
    mount();
    const existing = await screen.findByRole("article", {
      name: "apps.hermes",
    });
    expect(
      within(existing).queryByRole("button", { name: "agentInstall.install" }),
    ).not.toBeInTheDocument();
    expect(
      within(existing).getByRole("switch", { name: "apps.hermes" }),
    ).toBeEnabled();
  });
  it("shares the lifecycle lock with updates throughout consent and installation", async () => {
    detected.gemini = true;
    detected.qwen = true;
    vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
      version(tools![0], "1.0.0").map((v) => ({
        ...v,
        latest_version: "1.1.0",
      })),
    );
    let finish!: () => void;
    vi.mocked(settingsApi.runToolLifecycleAction).mockImplementation(
      async () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    mount();
    const all = await screen.findByRole("button", { name: "agentUpdates.all" });
    const open = await row("opencode");
    fireEvent.click(
      within(open).getByRole("button", { name: "agentInstall.install" }),
    );
    expect(all).toBeDisabled();
    fireEvent.click(
      await screen.findByRole("button", { name: "agentInstall.confirm" }),
    );
    await waitFor(() =>
      expect(settingsApi.runToolLifecycleAction).toHaveBeenCalledOnce(),
    );
    expect(all).toBeDisabled();
    expect(settingsApi.probeToolInstallations).not.toHaveBeenCalled();
    finish();
    expect(
      await screen.findByText(/agentInstall.unverified/),
    ).toBeInTheDocument();
    expect(all).toBeEnabled();
  });
});
