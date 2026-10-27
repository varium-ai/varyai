// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SecurityActivitySummary } from "./SecurityActivitySummary";

const summary = vi.hoisted(() =>
  vi.fn(() => ({
    data: {
      total: 12,
      dlpCount: 8,
      toolRiskCount: 4,
      redacted: 3,
      warnings: 5,
      protectiveBlocked: 4,
    },
    isLoading: false,
    isError: false,
  })),
);
vi.mock("@/hooks/useSecurityEvents", () => ({
  useSecurityEventsSummary: summary,
}));
vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityAuditLogs: vi.fn(() => ({
    logsResult: { total: 0 },
    isLoading: false,
    isError: false,
  })),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

describe("SecurityActivitySummary", () => {
  it("uses today's unified security totals and opens the event view", () => {
    const onOpenSecurity = vi.fn();
    render(<SecurityActivitySummary onOpenSecurity={onOpenSecurity} />);
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("5")).toBeInTheDocument();
    expect(summary).toHaveBeenCalledWith(
      expect.objectContaining({
        view: "security_events",
        startTimestamp: expect.any(Number),
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "securityStats.viewLogs" }),
    );
    expect(onOpenSecurity).toHaveBeenCalledOnce();
  });
});
