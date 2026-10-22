// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SecurityActivitySummary } from "./SecurityActivitySummary";

const logs = vi.hoisted(() =>
  vi.fn((filter: { action: string; startTimestamp: number }) => ({
    logsResult: { total: { block: 4, redact: 3, audit: 2 }[filter.action] },
    isLoading: false,
    isError: false,
  })),
);
vi.mock("@/hooks/useSecurityAudit", () => ({ useSecurityAuditLogs: logs }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

describe("SecurityActivitySummary", () => {
  it("uses today's real action totals and opens the audit view", () => {
    const onOpenSecurity = vi.fn();
    render(<SecurityActivitySummary onOpenSecurity={onOpenSecurity} />);
    expect(screen.getByText("9")).toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(
      screen.getByText("securityStats.protectiveBlocked").parentElement,
    ).toHaveTextContent("4");
    expect(screen.queryByText("securityStats.blocked")).not.toBeInTheDocument();
    expect(logs.mock.calls.map(([filter]) => filter.action)).toEqual([
      "block",
      "redact",
      "audit",
    ]);
    expect(logs.mock.calls.every(([filter]) => filter.startTimestamp > 0)).toBe(
      true,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "securityStats.viewLogs" }),
    );
    expect(onOpenSecurity).toHaveBeenCalledOnce();
  });
});
