import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { SecurityCenterPage } from "@/components/security/SecurityCenterPage";
import type { AuditLogPageResult } from "@/types/security";

const { getAuditLogs } = vi.hoisted(() => ({ getAuditLogs: vi.fn() }));

vi.mock("@/lib/api", () => ({
  securityApi: {
    getRules: vi.fn().mockResolvedValue([]),
    getAuditLogs,
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const emptyPage: AuditLogPageResult = {
  items: [],
  total: 0,
  page: 1,
  pageSize: 20,
};

async function renderLoadedPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <SecurityCenterPage />
    </QueryClientProvider>,
  );
  const button = screen.getByRole("button", { name: "security.refreshLogs" });
  await waitFor(() => expect(client.isFetching()).toBe(0));
  expect(getAuditLogs).toHaveBeenCalledTimes(5);
  expect(button).toBeEnabled();
  getAuditLogs.mockClear();
  return button;
}

describe("Security Center manual refresh", () => {
  beforeEach(() => {
    getAuditLogs.mockReset().mockResolvedValue(emptyPage);
  });

  afterEach(() => vi.useRealTimers());

  it("shows progress until the list and every statistic finish, then displays updated data", async () => {
    const button = await renderLoadedPage();
    const pending: Array<(page: AuditLogPageResult) => void> = [];
    getAuditLogs.mockImplementation(
      () => new Promise<AuditLogPageResult>((resolve) => pending.push(resolve)),
    );

    fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button.querySelector("svg")).toHaveClass("animate-spin");
    expect(button.querySelector("svg")).toHaveClass("lucide-refresh-cw");
    const table = screen.getByRole("table");
    expect(table).toHaveAttribute("aria-busy", "true");
    const loading = screen.getByRole("status");
    expect(loading).toHaveTextContent("security.loadingLogs");
    expect(loading.querySelector("svg")).toHaveClass("animate-spin");
    expect(table.parentElement!.parentElement).toContainElement(loading);
    expect(table.parentElement).not.toContainElement(loading);
    expect(
      screen.queryByText("security.noFilteredRecords"),
    ).not.toBeInTheDocument();
    expect(getAuditLogs).toHaveBeenCalledTimes(5);
    fireEvent.click(button);
    expect(getAuditLogs).toHaveBeenCalledTimes(5);

    const updatedPage: AuditLogPageResult = {
      ...emptyPage,
      total: 1,
      items: [
        {
          id: 1,
          timestamp: Date.now(),
          appType: "claude",
          providerId: "synthetic-provider",
          ruleId: "synthetic-rule",
          ruleName: "Synthetic finding",
          ruleCategory: "custom",
          riskLevel: "HIGH",
          actionTaken: "redact",
          matchedSnippetMasked: "[REDACTED]",
        },
      ],
    };
    await act(async () => {
      pending.slice(0, -1).forEach((resolve) => resolve(updatedPage));
    });
    expect(button).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "security.loadingLogs",
    );
    expect(screen.queryByText("Synthetic finding")).not.toBeInTheDocument();
    expect(toast.success).not.toHaveBeenCalled();

    await act(async () => pending.at(-1)!(updatedPage));
    await waitFor(() => expect(button).toBeEnabled());
    expect(button).toHaveAttribute("aria-busy", "false");
    expect(table).toHaveAttribute("aria-busy", "false");
    expect(screen.queryByText("security.loadingLogs")).not.toBeInTheDocument();
    expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
    expect(button.querySelector("svg")).toHaveClass("lucide-refresh-cw");
    expect(screen.getByText("Synthetic finding")).toBeInTheDocument();
    for (const label of ["redacted", "warnings", "protectiveBlocked"]) {
      expect(
        screen.getByText(`securityStats.${label}`).parentElement,
      ).toHaveTextContent("1");
    }
    expect(toast.success).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("keeps table loading visible for at least 300 ms even when requests finish immediately", async () => {
    const button = await renderLoadedPage();
    vi.useFakeTimers();
    fireEvent.click(button);
    await act(async () => vi.advanceTimersByTimeAsync(299));
    expect(button).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      "security.loadingLogs",
    );
    expect(screen.getByRole("table")).toHaveAttribute("aria-busy", "true");
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(button).toBeEnabled();
    expect(screen.getByRole("table")).toHaveAttribute("aria-busy", "false");
    expect(screen.queryByText("security.loadingLogs")).not.toBeInTheDocument();
    expect(screen.getByText("security.noFilteredRecords")).toBeInTheDocument();
    expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
    expect(toast.success).not.toHaveBeenCalled();
    expect(getAuditLogs).toHaveBeenCalledTimes(5);
  });

  it("reports a partial failure without exposing the backend error and permits retry", async () => {
    const button = await renderLoadedPage();
    getAuditLogs.mockRejectedValueOnce(new Error("synthetic-private-detail"));
    fireEvent.click(button);

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("security.logsRefreshFailed"),
    );
    expect(toast.success).not.toHaveBeenCalled();
    expect(button).toBeEnabled();
    expect(getAuditLogs).toHaveBeenCalledTimes(5);

    fireEvent.click(button);
    expect(button).toBeDisabled();
    await waitFor(() => expect(button).toBeEnabled());
    expect(toast.success).not.toHaveBeenCalled();
    expect(getAuditLogs).toHaveBeenCalledTimes(10);
  });
});
