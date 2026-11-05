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
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <AgentConnectionsPage />
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
  it("offers installation in the main row when a saved connection outlives the CLI", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "opencode",
        enabled: true,
        configured: false,
        model: null,
        recoveryPending: true,
        restartRequired: false,
      },
    ]);
    mount();
    const main = await screen.findByRole("article", { name: "apps.opencode" });
    expect(
      within(main).getByRole("button", { name: "agentInstall.install" }),
    ).toBeEnabled();
  });
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
  it("cancels without side effects and uses a distinct official install-page flow for desktop apps", async () => {
    mount();
    const desktop = await row("workbuddy");
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
      expect(settingsApi.openExternal).toHaveBeenCalledWith(
        "https://www.workbuddy.ai/",
      ),
    );
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    expect(
      screen.queryByText(/agentInstall.installed/),
    ).not.toBeInTheDocument();
  });
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
