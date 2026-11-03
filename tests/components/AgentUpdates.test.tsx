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

vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    nativeModels: vi.fn(async () => []),
    list: vi.fn(async () => []),
    models: vi.fn(async () => []),
  },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getAgentInstallations: vi.fn(async () => ({ claude: true, codex: true })),
    getProxyStatus: vi.fn(async () => ({ running: true })),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getToolVersions: vi.fn(),
    getAgentVersions: vi.fn(),
    probeToolInstallations: vi.fn(),
    runToolLifecycleAction: vi.fn(),
    openExternal: vi.fn(),
  },
}));
const versions = (name: string, version = "1.0.0") => [
  {
    name,
    version,
    latest_version: "1.1.0",
    error: null,
    installed_but_broken: false,
    env_type: "macos" as const,
    wsl_distro: null,
  },
];
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
          },
        })
      }
    >
      <AgentConnectionsPage />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
    versions(tools![0]),
  );
  vi.mocked(settingsApi.getAgentVersions).mockResolvedValue({
    codex: {
      supported: true,
      installed: true,
      version: "26.1000.100",
      latest_version: null,
      source: "desktop",
    },
  });
  vi.mocked(settingsApi.probeToolInstallations).mockImplementation(
    async (tools) =>
      tools.map((tool) => ({
        tool,
        installs: [],
        is_conflict: false,
        needs_confirmation: false,
        command: "synthetic updater",
        anchored: true,
      })),
  );
  vi.mocked(settingsApi.runToolLifecycleAction).mockResolvedValue();
});
describe("Agent versions and updates", () => {
  it("loads CLI and App versions separately without performing any updates on mount", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(await within(row).findByText("CLI 1.0.0")).toBeInTheDocument();
    expect(within(row).getByText("App 26.1000.100")).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "agentUpdates.all" }),
    ).toBeInTheDocument();
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    expect(settingsApi.probeToolInstallations).not.toHaveBeenCalled();
  });
  it("does not block connections when version probes fail or remain pending", async () => {
    vi.mocked(settingsApi.getToolVersions).mockRejectedValue(
      "synthetic-private-probe",
    );
    vi.mocked(settingsApi.getAgentVersions).mockImplementation(
      () => new Promise(() => {}),
    );
    mount();
    expect(
      await screen.findByRole("switch", { name: "apps.codex" }),
    ).toBeEnabled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.queryByText("synthetic-private-probe"),
    ).not.toBeInTheDocument();
  });
  it("serializes all updates, blocks duplicate clicks, continues after failure and verifies actual versions", async () => {
    let finish!: () => void;
    let active = 0;
    const changed = new Set<string>();
    vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
      versions(tools![0], changed.has(tools![0]) ? "1.1.0" : "1.0.0"),
    );
    vi.mocked(settingsApi.runToolLifecycleAction).mockImplementation(
      async (tools) => {
        expect(active++).toBe(0);
        try {
          if (tools[0] === "claude") {
            await new Promise<void>((resolve) => {
              finish = resolve;
            });
            throw new Error("synthetic-private-upgrade-output");
          }
          changed.add(tools[0]);
        } finally {
          active--;
        }
      },
    );
    mount();
    const all = await screen.findByRole("button", { name: "agentUpdates.all" });
    fireEvent.click(all);
    fireEvent.click(all);
    await waitFor(() =>
      expect(settingsApi.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    expect(all).toBeDisabled();
    finish();
    await waitFor(() =>
      expect(settingsApi.runToolLifecycleAction).toHaveBeenCalledTimes(2),
    );
    expect(settingsApi.runToolLifecycleAction).toHaveBeenNthCalledWith(
      1,
      ["claude"],
      "update",
    );
    expect(settingsApi.runToolLifecycleAction).toHaveBeenNthCalledWith(
      2,
      ["codex"],
      "update",
    );
    expect(await screen.findByText(/agentUpdates.summary/)).toBeInTheDocument();
    expect(screen.getByText("CLI 1.1.0")).toBeInTheDocument();
    expect(screen.getByText(/agentUpdates.changed/)).toBeInTheDocument();
    expect(
      screen.queryByText(/synthetic-private-upgrade-output/),
    ).not.toBeInTheDocument();
  });
  it.each(["unchanged", "unreadable"])(
    "does not report an %s version as a successful upgrade",
    async (state) => {
      if (state === "unreadable")
        vi.mocked(settingsApi.runToolLifecycleAction).mockImplementation(
          async () => {
            vi.mocked(settingsApi.getToolVersions).mockRejectedValue(
              "synthetic-private-version-output",
            );
          },
        );
      mount();
      const row = await screen.findByRole("article", { name: "apps.codex" });
      fireEvent.click(
        await within(row).findByRole("button", { name: "agentUpdates.update" }),
      );
      expect(
        await screen.findByText(/agentUpdates.unverified/),
      ).toBeInTheDocument();
      expect(
        screen.queryByText(/agentUpdates.changed/),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText(/synthetic-private-version-output/),
      ).not.toBeInTheDocument();
    },
  );
  it("does not offer updates when the installed version is already ahead", async () => {
    vi.mocked(settingsApi.getToolVersions).mockImplementation(async (tools) =>
      versions(tools![0], "1.2.0"),
    );
    mount();
    expect(await screen.findByText("CLI 1.2.0")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "agentUpdates.update" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "agentUpdates.all" }),
    ).not.toBeInTheDocument();
  });
  it("requires confirmation for multiple or unanchored installations and allows cancel without updating", async () => {
    vi.mocked(settingsApi.probeToolInstallations).mockResolvedValue([
      {
        tool: "codex",
        installs: [],
        is_conflict: true,
        needs_confirmation: true,
        command: "synthetic-private-command-not-for-display",
        anchored: false,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(
      await within(row).findByRole("button", { name: "agentUpdates.update" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    expect(
      screen.queryByText(/synthetic-private-command-not-for-display/),
    ).not.toBeInTheDocument();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(
      within(row).getByRole("button", { name: "agentUpdates.update" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "agentUpdates.confirm" }),
    );
    await waitFor(() =>
      expect(settingsApi.runToolLifecycleAction).toHaveBeenCalledOnce(),
    );
  });
  it("does not upgrade if preflight fails", async () => {
    vi.mocked(settingsApi.probeToolInstallations).mockRejectedValue(
      "synthetic-private-preflight",
    );
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(
      await within(row).findByRole("button", { name: "agentUpdates.update" }),
    );
    expect(
      await screen.findByText(/agentUpdates.preflightFailed/),
    ).toBeInTheDocument();
    expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
  });
});
