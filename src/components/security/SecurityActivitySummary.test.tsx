// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SecurityActivitySummary } from "./SecurityActivitySummary";

const summary = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/useSecurityEvents", () => ({
  useSecurityEventsSummary: summary,
}));
vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityAuditLogs: () => ({ logsResult: { total: 0 } }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

describe("SecurityActivitySummary", () => {
  it("uses the unified total including tool risks and other DLP actions without summing cards", () => {
    summary.mockReturnValue({
      data: {
        total: 17,
        dlpCount: 12,
        toolRiskCount: 5,
        protectiveBlocked: 4,
        redacted: 3,
        warnings: 7,
        other: 3,
      },
      isLoading: false,
      isError: false,
    });
    const onOpenSecurity = vi.fn();
    render(<SecurityActivitySummary onOpenSecurity={onOpenSecurity} />);
    for (const value of ["17", "4", "3", "7"])
      expect(screen.getByText(value)).toBeInTheDocument();
    expect(
      screen.getByText("securityStats.audited").parentElement,
    ).toHaveTextContent("7");
    expect(summary).toHaveBeenCalledWith(
      expect.objectContaining({
        view: "security_events",
        startTimestamp: expect.any(Number),
      }),
    );
    expect(summary.mock.calls[0][0].appType).toBeUndefined();
    fireEvent.click(
      screen.getByRole("button", { name: "securityStats.viewLogs" }),
    );
    expect(onOpenSecurity).toHaveBeenCalledOnce();
  });
  it("does not render errors as successful zero totals", () => {
    summary.mockReturnValue({ isError: true });
    render(<SecurityActivitySummary onOpenSecurity={() => {}} />);
    expect(screen.getAllByText("—")).toHaveLength(4);
  });
});
