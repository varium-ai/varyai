// @vitest-environment jsdom
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SecurityCenterPage } from "./SecurityCenterPage";

const state = vi.hoisted(() => ({ panel: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityRules: () => ({ rules: [], isLoading: false }),
  useSecurityAuditLogs: () => ({
    logsResult: { items: [], total: 0 },
    isLoading: false,
    isError: false,
  }),
}));
vi.mock("./SecurityEventsPanel", () => ({
  SecurityEventsPanel: (props: unknown) => {
    state.panel(props);
    return <div data-testid="unified-events" />;
  },
}));
vi.mock("./ToolRiskRulesPanel", () => ({
  ToolRiskRulesPanel: () => <div data-testid="tool-risk-rules" />,
}));

describe("SecurityCenterPage unified events", () => {
  it("shows the mixed event view, preserves rule tabs, and routes rule creation", () => {
    render(<SecurityCenterPage />);
    expect(screen.getByTestId("unified-events")).toBeInTheDocument();
    const props = state.panel.mock.lastCall?.[0];
    expect(props.filter).toEqual({ view: "security_events" });
    act(() => props.onCreateRule({ operationCategory: "shell_exec" }));
    expect(screen.getByTestId("tool-risk-rules")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("security-tab-rules"));
    expect(screen.getByText("security.sandboxTitle")).toBeInTheDocument();
  });
});
