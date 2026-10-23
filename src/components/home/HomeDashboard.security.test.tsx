// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SecurityStatusRow } from "./HomeDashboard";

const summary = vi.hoisted(() =>
  vi.fn(() => ({
    data: {
      total: 14,
      dlpCount: 8,
      toolRiskCount: 6,
      redacted: 3,
      warnings: 9,
      protectiveBlocked: 2,
    },
    isLoading: false,
    isError: false,
  })),
);
vi.mock("@/hooks/useSecurityEvents", () => ({
  useSecurityEventsSummary: summary,
}));
vi.mock("@/hooks/useSecurityProtection", () => ({
  useSecurityProtection: () => ({
    settings: { enabled: true },
    connected: true,
    isLoading: false,
  }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      `${key} ${values?.handled ?? ""} ${values?.warned ?? ""}`,
    i18n: { language: "en" },
  }),
}));

describe("HomeDashboard security status", () => {
  it("counts handled and warnings from the unified summary for the active app", () => {
    render(
      <SecurityStatusRow
        appType="codex"
        onOpenSecurity={vi.fn()}
        onOpenRoutingSettings={vi.fn()}
      />,
    );
    expect(summary).toHaveBeenCalledWith(
      expect.objectContaining({
        view: "security_events",
        appType: "codex",
        startTimestamp: expect.any(Number),
      }),
    );
    expect(screen.getByText(/homeAudit.auditSummary 5 9/)).toBeInTheDocument();
  });
});
