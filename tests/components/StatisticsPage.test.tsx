// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StatisticsPage } from "@/components/statistics/StatisticsPage";

const settingsMock = vi.hoisted(() => ({
  settings: {
    usageDashboardRefreshIntervalMs: 30000,
    sessionAutoSyncEnabled: true,
  },
  updateSettings: vi.fn(),
  autoSaveSettings: vi.fn().mockResolvedValue({ requiresRestart: false }),
}));

const dashboardProps = vi.hoisted(() => ({ current: null as any }));

vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => settingsMock,
}));

vi.mock("@/components/security/SecurityActivitySummary", () => ({
  SecurityActivitySummary: () => <section aria-label="today-summary" />,
}));

vi.mock("@/components/usage/UsageDashboard", () => ({
  UsageDashboard: (props: any) => {
    dashboardProps.current = props;
    return (
      <div>
        <span data-testid="refresh-interval">
          {String(props.refreshIntervalMs)}
        </span>
        <span data-testid="session-auto-sync">
          {String(props.sessionAutoSyncEnabled)}
        </span>
        <button onClick={() => props.onRefreshIntervalChange(10000)}>
          change-refresh
        </button>
        <button onClick={() => props.onSessionAutoSyncEnabledChange(false)}>
          change-session-sync
        </button>
      </div>
    );
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("StatisticsPage", () => {
  beforeEach(() => {
    settingsMock.updateSettings.mockReset();
    settingsMock.autoSaveSettings.mockReset();
    settingsMock.autoSaveSettings.mockResolvedValue({ requiresRestart: false });
    dashboardProps.current = null;
  });

  it("renders usage statistics outside the settings page", () => {
    const onOpenSecurity = vi.fn();

    render(<StatisticsPage onOpenSecurity={onOpenSecurity} />);

    expect(screen.getByTestId("refresh-interval")).toHaveTextContent("30000");
    expect(screen.getByTestId("session-auto-sync")).toHaveTextContent("true");
    const heading = screen.getByRole("heading", {
      level: 1,
      name: "usage.title",
    });
    expect(
      heading.compareDocumentPosition(
        screen.getByRole("region", { name: "today-summary" }),
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "securityStats.viewLogs" }),
    );
    expect(onOpenSecurity).toHaveBeenCalledOnce();
  });

  it("persists statistics preferences through the settings service", async () => {
    render(<StatisticsPage />);

    fireEvent.click(screen.getByRole("button", { name: "change-refresh" }));
    fireEvent.click(
      screen.getByRole("button", { name: "change-session-sync" }),
    );

    await waitFor(() => {
      expect(settingsMock.updateSettings).toHaveBeenCalledWith({
        usageDashboardRefreshIntervalMs: 10000,
      });
      expect(settingsMock.updateSettings).toHaveBeenCalledWith({
        sessionAutoSyncEnabled: false,
      });
      expect(settingsMock.autoSaveSettings).toHaveBeenCalledWith({
        usageDashboardRefreshIntervalMs: 10000,
      });
      expect(settingsMock.autoSaveSettings).toHaveBeenCalledWith({
        sessionAutoSyncEnabled: false,
      });
    });
  });
});
