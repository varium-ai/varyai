import {
  render,
  screen,
  fireEvent,
  within,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App";
import { profilesApi } from "@/lib/api/profiles";
import { resetProviderState, setSettings } from "../msw/state";
vi.mock("@/lib/api/profiles", () => ({
  profilesApi: { list: vi.fn(), captureBundle: vi.fn() },
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    list: async () => [
      { app: "codex", enabled: true },
      { app: "kimi", enabled: true },
      { app: "claude", enabled: false },
    ],
  },
}));
vi.mock("@/components/agents/AgentConnectionsPage", () => ({
  AgentConnectionsPage: () => <div data-testid="agents" />,
}));
vi.mock("@/components/providers/RoutingUpstreamsPanel", () => ({
  RoutingUpstreamsPanel: ({
    onEditingChange,
  }: {
    onEditingChange?: (state: { dirty: boolean; busy: boolean }) => void;
  }) => (
    <div data-testid="global-providers">
      <button onClick={() => onEditingChange?.({ dirty: true, busy: false })}>
        Edit fixture draft
      </button>
    </div>
  ),
}));
vi.mock("@/components/routing/RoutingPage", () => ({
  RoutingPage: () => <div data-testid="global-routes" />,
}));
vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: () => <div data-testid="preferences" />,
}));
vi.mock("@/components/security/SecurityCenterPage", () => ({
  SecurityCenterPage: () => <div data-testid="security" />,
}));
vi.mock("@/components/statistics/StatisticsPage", () => ({
  StatisticsPage: () => <div data-testid="stats" />,
}));
vi.mock("@/components/FirstRunNoticeDialog", () => ({
  FirstRunNoticeDialog: () => null,
}));
vi.mock("@/components/UpdateBadge", () => ({ UpdateBadge: () => null }));
vi.mock("@/components/security/SecurityAuditToggle", () => ({
  SecurityAuditToggle: () => null,
}));
vi.mock("@/hooks/useUsageCacheBridge", () => ({
  useUsageCacheBridge: () => {},
}));
describe("Global application shell", () => {
  beforeEach(() => {
    HTMLElement.prototype.scrollIntoView = vi.fn();
    resetProviderState();
    setSettings({ showProfileSwitcher: true });
    vi.mocked(profilesApi.list).mockResolvedValue({
      profiles: [],
      currentIds: { claude: null, claudeDesktop: null, codex: null },
      agents: ["codex", "kimi"].map((app) => ({
        app,
        available: true,
        provider: true,
        mcp: false,
        skills: false,
        instructions: false,
        reason: null,
      })),
    } as never);
    vi.mocked(profilesApi.captureBundle)
      .mockReset()
      .mockResolvedValue({} as never);
  });
  it("creates a multi-agent profile from the main footer using connected agents", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await within(screen.getByRole("contentinfo")).findByRole("combobox"),
    );
    fireEvent.click(await screen.findByText("profiles.createFromCurrent"));
    fireEvent.change(screen.getByPlaceholderText("profiles.namePlaceholder"), {
      target: { value: "Work setup" },
    });
    fireEvent.click(
      screen.getByRole("checkbox", { name: "profiles.bundle.create" }),
    );
    expect(screen.getByRole("checkbox", { name: /apps.codex/ })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /apps.kimi/ })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    await waitFor(() =>
      expect(profilesApi.captureBundle).toHaveBeenCalledWith("Work setup", [
        "codex",
        "kimi",
      ]),
    );
  });
  it("honors the existing profile-switcher visibility preference", async () => {
    setSettings({ showProfileSwitcher: false });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>,
    );
    await waitFor(() =>
      expect(client.getQueryState(["settings"])?.status).toBe("success"),
    );
    expect(
      within(screen.getByRole("contentinfo")).queryByRole("combobox"),
    ).not.toBeInTheDocument();
  });
  it("keeps the current page until discarding an unsaved draft is confirmed", async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <App />
      </QueryClientProvider>,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "workspaceNav.providers" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Edit fixture draft" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "workspaceNav.routing" }),
    );
    expect(screen.getByTestId("global-providers")).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.cancel" }),
    );
    expect(screen.queryByTestId("global-routes")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "workspaceNav.routing" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "providerFlow.discard" }),
    );
    expect(await screen.findByTestId("global-routes")).toBeInTheDocument();
  });
  it("opens the Agent page and navigates global resources without an Agent switcher", async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <App />
      </QueryClientProvider>,
    );
    expect(await screen.findByTestId("agents")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "workspaceNav.providers" }),
    );
    expect(await screen.findByTestId("global-providers")).toBeInTheDocument();
    expect(screen.queryByTestId("app-switcher")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "workspaceNav.routing" }),
    );
    expect(await screen.findByTestId("global-routes")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "workspaceNav.security" }),
    );
    expect(await screen.findByTestId("security")).toBeInTheDocument();
  });
});
