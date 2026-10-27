import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HomeDashboard } from "@/components/home/HomeDashboard";
import { useSecurityEventsSummary } from "@/hooks/useSecurityEvents";
import type { Provider } from "@/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";

vi.mock("@/hooks/useSecurityProtection", () => ({
  useSecurityProtection: () => ({
    settings: { enabled: true },
    connected: true,
    isLoading: false,
  }),
}));
vi.mock("@/hooks/useSecurityEvents", () => ({
  useSecurityEventsSummary: vi.fn(() => ({
    data: {
      total: 9,
      dlpCount: 7,
      toolRiskCount: 2,
      redacted: 3,
      warnings: 4,
      protectiveBlocked: 2,
    },
    isError: false,
    isLoading: false,
  })),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: "en" },
    t: (key: string, values?: Record<string, unknown>) =>
      key === "homeAudit.auditSummary"
        ? `Handled ${values?.handled}, warned ${values?.warned}`
        : key,
  }),
}));

describe("WorkBuddy home configuration", () => {
  it("uses WorkBuddy daily counts and switches between configurations using the shared provider actions", async () => {
    const providers: Record<string, Provider> = Object.fromEntries(
      ["enabled", "disabled"].map((id) => [
        id,
        {
          id,
          name: id,
          settingsConfig: {
            id: `model-${id}`,
            url: "https://example.com/v1/chat/completions",
            apiKey: "synthetic-secret",
          },
        },
      ]),
    );
    const onSwitch = vi.fn();
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    const onDuplicate = vi.fn();
    const onOpenSecurity = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <HomeDashboard
          activeApp="workbuddy"
          visibleApps={DEFAULT_VISIBLE_APPS}
          providers={providers}
          currentProviderId="enabled"
          onAppChange={vi.fn()}
          onCreate={vi.fn()}
          onEdit={onEdit}
          onDelete={onDelete}
          onDuplicate={onDuplicate}
          onSwitch={onSwitch}
          onOpenSecurity={onOpenSecurity}
          onOpenRoutingSettings={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(screen.getByText("Handled 5, warned 4")).toBeInTheDocument();
    expect(useSecurityEventsSummary).toHaveBeenCalledWith(
      expect.objectContaining({
        view: "security_events",
        appType: "workbuddy",
        startTimestamp: expect.any(Number),
      }),
    );
    expect(screen.getByText("provider.inUse")).toBeInTheDocument();
    expect(
      screen.queryByRole("switch", { name: "workbuddy.routing.title" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("synthetic-secret")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "homeAudit.switchTo" }));
    expect(onSwitch).toHaveBeenCalledWith(providers.disabled);
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    expect(onEdit).toHaveBeenCalledWith(providers.enabled);
    fireEvent.click(screen.getByRole("button", { name: "provider.duplicate" }));
    expect(onDuplicate).toHaveBeenCalledWith(providers.enabled);
    fireEvent.click(
      screen.getByRole("button", { name: "provider.duplicate disabled" }),
    );
    expect(onDuplicate).toHaveBeenLastCalledWith(providers.disabled);
    expect(
      screen.queryByRole("button", { name: "homeAudit.copyEnv" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "common.delete disabled" }),
    );
    expect(onDelete).toHaveBeenCalledWith(providers.disabled);
    fireEvent.click(screen.getByRole("button", { name: "homeAudit.viewLogs" }));
    expect(onOpenSecurity).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", { name: "workbuddy.importModels" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "homeAudit.addAlternative" }),
    ).toBeInTheDocument();
  });
});
