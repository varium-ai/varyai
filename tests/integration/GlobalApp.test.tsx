import { render, screen, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "@/App";
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
