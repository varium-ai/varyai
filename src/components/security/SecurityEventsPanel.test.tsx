// @vitest-environment jsdom
import { useState } from "react";
import { toast } from "sonner";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SecurityEvent,
  SecurityEventFilter,
} from "@/types/securityEvents";
import type { UsageRangeSelection } from "@/types/usage";
import { SecurityEventsPanel } from "./SecurityEventsPanel";

const state = vi.hoisted(() => ({
  items: [] as SecurityEvent[],
  total: 57,
  loading: false,
  error: false,
  summaryError: false,
  hasUpdates: false,
  query: vi.fn(),
  summary: vi.fn(),
  refetch: vi.fn(),
  summaryRefetch: vi.fn(),
  reset: vi.fn(),
  clear: vi.fn(),
  legacyQuery: vi.fn(),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@/hooks/useSecurityEvents", () => ({
  useSecurityEvents: (
    filter: SecurityEventFilter,
    page: number,
    pageSize: number,
  ) => {
    state.query(filter, page, pageSize);
    return {
      eventsResult: { items: state.items, total: state.total, page, pageSize },
      isLoading: state.loading,
      isError: state.error,
      refetch: state.refetch,
      clearEvents: state.clear,
      isClearing: false,
    };
  },
  useSecurityEventsSummary: (filter: SecurityEventFilter) => {
    state.summary(filter);
    return {
      data: {
        total: 101,
        dlpCount: 80,
        toolRiskCount: 21,
        redacted: 10,
        warnings: 51,
        protectiveBlocked: 30,
        other: 10,
      },
      isLoading: state.loading,
      isError: state.summaryError,
      refetch: state.summaryRefetch,
    };
  },
  useSecurityEventsLiveRefresh: () => ({
    hasUpdates: state.hasUpdates,
    resetUpdates: state.reset,
  }),
}));
vi.mock("@/hooks/useToolRisk", () => ({
  useToolRiskEvents: (...args: unknown[]) => {
    state.legacyQuery(...args);
    return { events: [], refetch: vi.fn() };
  },
}));
vi.mock("./ShellCommandListsPanel", () => ({
  ShellCommandListDialog: () => null,
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
const tool: SecurityEvent = {
  key: "tool_risk:7",
  eventType: "tool_risk",
  timestamp: 1000,
  lastSeenAt: 1000,
  triggerCount: 1,
  appType: "openai",
  sessionId: null,
  riskLevel: "HIGH",
  result: "warning",
  record: {
    id: 7,
    timestamp: 1000,
    source: "proxy",
    stage: "model_response",
    clientCategory: "openai",
    operationCategory: "shell_exec",
    commandSummary: "ordinary-command <redacted>",
    detectionScope: "in_scope",
    integrityStatus: "complete",
    verdict: "risky",
    riskCategory: "data_exfiltration",
    severity: "high",
    policyVersion: 1,
    ruleIds: [],
    suppressedCount: 0,
  },
};
const dlp: SecurityEvent = {
  key: "dlp:7",
  eventType: "dlp",
  timestamp: 2000,
  lastSeenAt: 8000,
  triggerCount: 7,
  appType: "codex",
  sessionId: "session-a",
  riskLevel: "CRITICAL",
  result: "redacted",
  record: {
    id: 7,
    timestamp: 2000,
    lastSeenAt: 8000,
    triggerCount: 7,
    appType: "codex",
    providerId: "",
    sessionId: "session-a",
    ruleId: "r",
    ruleName: "DLP token",
    ruleCategory: "custom",
    riskLevel: "CRITICAL",
    actionTaken: "redact",
    matchedSnippetMasked: "***",
  },
};
function Harness({
  onCreateRule = vi.fn(),
  initialRange = { preset: "today" },
}: {
  onCreateRule?: (seed: unknown) => void;
  initialRange?: UsageRangeSelection;
}) {
  const [filter, setFilter] = useState<SecurityEventFilter>({
    view: "security_events",
  });
  const [page, setPage] = useState(1);
  const [range, setRange] = useState<UsageRangeSelection>(initialRange);
  return (
    <SecurityEventsPanel
      filter={filter}
      onFilterChange={setFilter}
      page={page}
      onPageChange={setPage}
      range={range}
      onRangeChange={setRange}
      onCreateRule={onCreateRule}
    />
  );
}
const choose = (label: string, option: string) => {
  fireEvent.click(screen.getByRole("combobox", { name: label }));
  fireEvent.click(screen.getByRole("option", { name: option }));
};
const latestFilter = () =>
  state.query.mock.calls.at(-1)![0] as SecurityEventFilter;
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
beforeEach(() => {
  vi.clearAllMocks();
  state.refetch.mockReset();
  state.summaryRefetch.mockReset();
  state.items = [dlp, tool];
  state.total = 57;
  state.loading = false;
  state.error = false;
  state.summaryError = false;
  state.hasUpdates = false;
  state.clear.mockResolvedValue({
    deletedTotal: 3,
    deletedDlp: 2,
    deletedToolRisk: 1,
  });
});
describe("unified security event list", () => {
  it("uses composite keys to select same-id sources, retains DLP grouping, and never claims tools were blocked", () => {
    render(<Harness />);
    expect(screen.getByTestId("security-event-dlp:7")).toHaveTextContent("7");
    const row = screen.getByTestId("security-event-tool_risk:7");
    expect(row).toHaveTextContent("securityEvents.notRecorded");
    expect(row).toHaveTextContent("securityEvents.toolObservation");
    expect(row).not.toHaveTextContent("security.outboundSent");
    fireEvent.click(row);
    expect(
      within(screen.getByRole("dialog")).getByText(
        "securityEvents.toolWarning",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("dialog")).queryByText("DLP token"),
    ).toBeNull();
  });
  it("queries summary by time only and shows real total/source counts rather than page-sampled sensitive types", () => {
    render(<Harness />);
    choose("security.allRisks", "security.levelHighShort");
    expect(latestFilter().riskLevel).toBe("HIGH");
    expect(state.summary.mock.calls.at(-1)![0]).toEqual({
      view: "security_events",
      startTimestamp: expect.any(Number),
      endTimestamp: undefined,
    });
    expect(
      screen.getByTestId("security-events-summary-total"),
    ).toHaveTextContent("101");
    expect(screen.getByTestId("security-events-summary-dlp")).toHaveTextContent(
      "80",
    );
    expect(
      screen.getByTestId("security-events-summary-tool-risk"),
    ).toHaveTextContent("21");
    expect(screen.queryByText("securityStats.sensitiveTypesSample")).toBeNull();
  });
  it("switches views keeping public filters, clearing source-specific ones, and never requests the standalone tool query", () => {
    render(<Harness />);
    choose("securityEvents.source", "securityEvents.dlp");
    choose("security.allApps", "OpenAI");
    choose("security.allRisks", "security.levelHighShort");
    fireEvent.change(
      screen.getByRole("textbox", { name: "security.sessionFilter" }),
      { target: { value: "session-a" } },
    );
    fireEvent.change(
      screen.getByPlaceholderText("security.searchPlaceholder"),
      { target: { value: "token" } },
    );
    fireEvent.click(screen.getByText("securityEvents.toolCallsView"));
    expect(latestFilter()).toMatchObject({
      view: "tool_calls",
      eventType: "tool_risk",
      appType: "openai",
      riskLevel: "HIGH",
    });
    expect(latestFilter().sessionId).toBeUndefined();
    expect(latestFilter().keyword).toBeUndefined();
    expect(latestFilter().toolVerdict).toBeUndefined();
    expect(
      screen.queryByRole("textbox", { name: "security.sessionFilter" }),
    ).toBeNull();
    expect(state.legacyQuery).not.toHaveBeenCalled();
    choose("toolRisk.eventVerdictFilter", "toolRisk.verdict.uncertain");
    expect(latestFilter().toolVerdict).toBe("uncertain");
    fireEvent.click(screen.getByText("securityEvents.detailsView"));
    expect(latestFilter()).toMatchObject({
      view: "security_events",
      appType: "openai",
      riskLevel: "HIGH",
    });
    expect(latestFilter().eventType).toBeUndefined();
    expect(latestFilter().toolVerdict).toBeUndefined();
  });
  it("selecting pure tool source hides and clears DLP-only search and session", () => {
    render(<Harness />);
    choose("securityEvents.source", "securityEvents.dlp");
    fireEvent.change(
      screen.getByRole("textbox", { name: "security.sessionFilter" }),
      { target: { value: "session" } },
    );
    choose("securityEvents.source", "securityEvents.toolRisk");
    expect(latestFilter().sessionId).toBeUndefined();
    expect(
      screen.queryByPlaceholderText("security.searchPlaceholder"),
    ).toBeNull();
  });
  it("shows source-specific search only when the selected source is DLP", () => {
    render(<Harness />);
    expect(
      screen.queryByPlaceholderText("security.searchPlaceholder"),
    ).toBeNull();
    choose("securityEvents.source", "securityEvents.dlp");
    fireEvent.change(
      screen.getByPlaceholderText("security.searchPlaceholder"),
      {
        target: { value: "token" },
      },
    );
    expect(latestFilter()).toMatchObject({
      eventType: "dlp",
      keyword: "token",
    });
    choose("securityEvents.source", "securityEvents.allSources");
    expect(
      screen.queryByPlaceholderText("security.searchPlaceholder"),
    ).toBeNull();
    expect(latestFilter().keyword).toBeUndefined();
  });
  it("does not display stale total as zero or old records on query errors, and supports retry", () => {
    state.error = true;
    state.summaryError = true;
    render(<Harness />);
    expect(screen.queryByTestId("security-event-dlp:7")).toBeNull();
    expect(
      screen.getByTestId("security-events-summary-total"),
    ).toHaveTextContent("—");
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.retry" }),
    );
    expect(state.refetch).toHaveBeenCalled();
    expect(state.summaryRefetch).toHaveBeenCalled();
  });
  it("explicit refresh returns to first page and resets update warning", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByText("security.nextPage"));
    expect(state.query.mock.calls.at(-1)![1]).toBe(2);
    fireEvent.click(
      screen.getByRole("button", { name: "security.refreshLogs" }),
    );
    await waitFor(() => expect(state.refetch).toHaveBeenCalled());
    expect(state.query.mock.calls.at(-1)![1]).toBe(1);
    expect(state.reset).toHaveBeenCalled();
  });
  it("requires confirmation, freezes filters and caps clear end at submission time", async () => {
    vi.spyOn(Date, "now").mockReturnValue(1_800_000_000_000);
    render(<Harness />);
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    expect(state.clear).not.toHaveBeenCalled();
    expect(screen.getByText("securityEvents.clearTitle")).toBeInTheDocument();
    expect(
      screen.getByText("securityEvents.clearRiskOnly"),
    ).toBeInTheDocument();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByText("common.confirm"),
    );
    await waitFor(() =>
      expect(state.clear).toHaveBeenCalledWith(
        expect.objectContaining({
          view: "security_events",
          endTimestamp: 1_800_000_000_000,
        }),
      ),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    vi.restoreAllMocks();
  });
  it("preserves failed deletion confirmation and uses safe errors without IPC detail", async () => {
    state.clear.mockRejectedValue(new Error("secret-sql-input"));
    render(<Harness />);
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByText("common.confirm"),
    );
    await waitFor(() =>
      expect(
        screen.getByText("securityEvents.clearFailed"),
      ).toBeInTheDocument(),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain("secret-sql-input");
  });
});

describe("unified event snapshot and clear boundaries", () => {
  it.each(["today", "7d", "30d"] as const)(
    "keeps the %s deep-page query snapshot across midnight until explicit refresh",
    async (preset) => {
      const clock = vi
        .spyOn(Date, "now")
        .mockReturnValue(new Date(2026, 9, 21, 23, 59).getTime());
      const { rerender } = render(<Harness initialRange={{ preset }} />);
      fireEvent.click(screen.getByText("security.nextPage"));
      const before = { ...latestFilter() };
      clock.mockReturnValue(new Date(2026, 9, 22, 0, 1).getTime());
      state.hasUpdates = true;
      rerender(<Harness initialRange={{ preset }} />);
      expect(latestFilter()).toEqual(before);
      expect(state.query.mock.calls.at(-1)![1]).toBe(2);
      expect(screen.getByTestId("security-event-dlp:7")).toHaveTextContent(
        "DLP token",
      );
      expect(
        screen.getByText("securityEvents.updatesAvailable"),
      ).toBeInTheDocument();
      state.refetch.mockImplementation(() => {
        expect(state.query.mock.calls.at(-1)![1]).toBe(1);
        expect(latestFilter().startTimestamp).toBeGreaterThan(
          before.startTimestamp!,
        );
      });
      state.summaryRefetch.mockImplementation(() => {
        expect(
          state.summary.mock.calls.at(-1)![0].startTimestamp,
        ).toBeGreaterThan(before.startTimestamp!);
      });
      fireEvent.click(
        screen.getByRole("button", { name: "security.refreshLogs" }),
      );
      await waitFor(() => expect(state.refetch).toHaveBeenCalled());
      expect(state.summaryRefetch).toHaveBeenCalled();
    },
  );
  it("shows update warnings without replacing a selected deep-page snapshot", () => {
    state.hasUpdates = true;
    const { rerender } = render(<Harness />);
    fireEvent.click(screen.getByText("security.nextPage"));
    fireEvent.click(screen.getByTestId("security-event-dlp:7"));
    state.items = [{ ...dlp, triggerCount: 99 }];
    rerender(<Harness />);
    expect(
      screen.getByText("securityEvents.updatesAvailable"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("dialog")).getByText("7"),
    ).toBeInTheDocument();
    expect(within(screen.getByRole("dialog")).queryByText("99")).toBeNull();
  });
  it("updates first-page details only by source-aware event key", () => {
    const { rerender } = render(<Harness />);
    fireEvent.click(screen.getByTestId("security-event-dlp:7"));
    state.items = [{ ...dlp, triggerCount: 99 }, tool];
    rerender(<Harness />);
    expect(
      within(screen.getByRole("dialog")).getByText("99"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("dialog")).queryByText(
        "securityEvents.toolWarning",
      ),
    ).toBeNull();
  });
  it("describes irreversible all-page clearing, cancellation does not delete", () => {
    render(<Harness />);
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    expect(
      screen.getByText("securityEvents.clearIrreversible"),
    ).toBeInTheDocument();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByText("common.cancel"),
    );
    expect(state.clear).not.toHaveBeenCalled();
  });
  it("keeps a fixed custom end instead of extending it to confirmation time", async () => {
    const fixedEnd = new Date(2020, 0, 2, 18).getTime();
    render(
      <Harness
        initialRange={{
          preset: "custom",
          customStartDate: new Date(2020, 0, 1).getTime() / 1000,
          customEndDate: fixedEnd / 1000,
        }}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByText("common.confirm"),
    );
    await waitFor(() =>
      expect(state.clear).toHaveBeenCalledWith(
        expect.objectContaining({ endTimestamp: fixedEnd + 999 }),
      ),
    );
  });
  it("all tools clear explicitly includes non-risk calls and preserves the selected verdict", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByText("securityEvents.toolCallsView"));
    choose("toolRisk.eventVerdictFilter", "toolRisk.verdict.uncertain");
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    expect(
      screen.getByText("securityEvents.clearIncludesNonRisk"),
    ).toBeInTheDocument();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByText("common.confirm"),
    );
    await waitFor(() =>
      expect(state.clear).toHaveBeenCalledWith(
        expect.objectContaining({
          view: "tool_calls",
          eventType: "tool_risk",
          toolVerdict: "uncertain",
        }),
      ),
    );
  });
});

describe("manual refresh feedback", () => {
  it.each(["security_events", "tool_calls"] as const)(
    "keeps initial loading below the headers in %s",
    (view) => {
      state.loading = true;
      render(<Harness />);
      if (view === "tool_calls")
        fireEvent.click(screen.getByText("securityEvents.toolCallsView"));
      const loading = screen.getByTestId("security-events-loading");
      const table = screen.getByRole("table");
      expect(loading.previousElementSibling).toContainElement(table);
      expect(loading).not.toHaveClass("absolute");
      expect(table.querySelector("tbody")).toBeEmptyDOMElement();
      expect(within(table).getAllByRole("columnheader").length).toBeGreaterThan(
        0,
      );
    },
  );

  it.each(["security_events", "tool_calls"] as const)(
    "keeps the original icon spinning and table loading for 300 ms in %s",
    async (view) => {
      vi.useFakeTimers();
      render(<Harness />);
      if (view === "tool_calls")
        fireEvent.click(screen.getByText("securityEvents.toolCallsView"));
      const button = screen.getByRole("button", {
        name: "security.refreshLogs",
      });
      fireEvent.click(button);
      expect(button).toHaveAttribute("aria-busy", "true");
      expect(button.querySelector("svg")).toHaveClass(
        "lucide-refresh-cw",
        "animate-spin",
      );
      const loading = screen.getByTestId("security-events-loading");
      expect(loading).toHaveAttribute("role", "status");
      expect(loading).toHaveTextContent("security.loadingLogs");
      expect(loading.querySelector("svg")).toHaveClass("animate-spin");
      expect(loading.closest("table")).toBeNull();
      const table = screen.getByRole("table");
      expect(within(table).getAllByRole("columnheader").length).toBeGreaterThan(
        0,
      );
      expect(loading.previousElementSibling).toContainElement(table);
      expect(loading).not.toHaveClass("absolute");
      expect(table.querySelector("tbody")).toBeEmptyDOMElement();
      expect(screen.getByText("security.nextPage")).toBeDisabled();
      fireEvent.click(button);
      expect(state.refetch).toHaveBeenCalledTimes(1);
      await act(async () => vi.advanceTimersByTimeAsync(299));
      expect(button).toBeDisabled();
      await act(async () => vi.advanceTimersByTimeAsync(1));
      expect(button).toBeEnabled();
      expect(button).toHaveAttribute("aria-busy", "false");
      expect(button.querySelector("svg")).not.toHaveClass("animate-spin");
      expect(screen.queryByTestId("security-events-loading")).toBeNull();
      expect(toast.success).not.toHaveBeenCalled();
    },
  );

  it("waits for all requests after a partial failure, reports a safe error and permits retry", async () => {
    vi.useFakeTimers();
    let finishSummary!: () => void;
    state.refetch.mockRejectedValueOnce(new Error("synthetic-private-detail"));
    state.summaryRefetch.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        finishSummary = resolve;
      }),
    );
    render(<Harness />);
    const button = screen.getByRole("button", { name: "security.refreshLogs" });
    fireEvent.click(button);
    await act(async () => vi.advanceTimersByTimeAsync(300));
    expect(button).toBeDisabled();
    expect(toast.error).not.toHaveBeenCalled();
    await act(async () => finishSummary());
    expect(button).toBeEnabled();
    expect(toast.error).toHaveBeenCalledExactlyOnceWith(
      "security.refreshFailed",
    );
    expect(document.body.innerHTML).not.toContain("synthetic-private-detail");
    expect(toast.success).not.toHaveBeenCalled();
    fireEvent.click(button);
    await act(async () => vi.advanceTimersByTimeAsync(300));
    expect(button).toBeEnabled();
    expect(state.refetch).toHaveBeenCalledTimes(2);
  });

  it("treats query result errors as refresh failures", async () => {
    vi.useFakeTimers();
    state.summaryRefetch.mockResolvedValue({ isError: true });
    render(<Harness />);
    fireEvent.click(
      screen.getByRole("button", { name: "security.refreshLogs" }),
    );
    await act(async () => vi.advanceTimersByTimeAsync(300));
    expect(toast.error).toHaveBeenCalledWith("security.refreshFailed");
    expect(toast.success).not.toHaveBeenCalled();
  });
});

describe("clear action feedback", () => {
  it.each([
    ["security.refreshLogs", 0],
    ["security.refreshLogs", 57],
    ["securityEvents.clearFiltered", 0],
    ["securityEvents.clearFiltered", 57],
  ] as const)(
    "provides the native %s title with %s records, including disabled clear",
    (name, total) => {
      state.total = total;
      render(<Harness />);
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("title", name);
      expect(button.parentElement).toHaveAttribute("title", name);
      expect(button.parentElement).not.toHaveClass("pointer-events-none");
      if (total === 0) {
        expect(
          screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
        ).toBeDisabled();
      }
    },
  );

  it("retains the disabled clear title without enabling deletion", () => {
    state.total = 0;
    render(<Harness />);
    const button = screen.getByRole("button", {
      name: "securityEvents.clearFiltered",
    });
    expect(button).toBeDisabled();
    expect(button.parentElement).toHaveAttribute(
      "title",
      "securityEvents.clearFiltered",
    );
    fireEvent.click(button);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(state.clear).not.toHaveBeenCalled();
  });

  it("retains the refresh title while loading and keeps clear disabled", () => {
    state.loading = true;
    render(<Harness />);
    const button = screen.getByRole("button", { name: "security.refreshLogs" });
    expect(button).toBeDisabled();
    expect(button.parentElement).toHaveAttribute(
      "title",
      "security.refreshLogs",
    );
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(state.clear).not.toHaveBeenCalled();
  });

  it("focuses cancel when confirmation opens", async () => {
    render(<Harness />);
    const button = screen.getByRole("button", {
      name: "securityEvents.clearFiltered",
    });
    fireEvent.click(button);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "common.cancel" }),
      ).toHaveFocus(),
    );
    expect(state.clear).not.toHaveBeenCalled();
  });

  it("blocks duplicate submissions and dismissal while deleting, then allows retry after failure", async () => {
    let fail!: (error: Error) => void;
    state.clear.mockReturnValueOnce(
      new Promise((_, reject) => {
        fail = reject;
      }),
    );
    render(<Harness />);
    fireEvent.click(
      screen.getByRole("button", { name: "securityEvents.clearFiltered" }),
    );
    const dialog = screen.getByRole("dialog");
    const confirm = within(dialog).getByRole("button", {
      name: "common.confirm",
    });
    fireEvent.click(confirm);
    expect(confirm).toBeDisabled();
    expect(within(dialog).getByText("common.cancel")).toBeDisabled();
    fireEvent.click(confirm);
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(state.clear).toHaveBeenCalledTimes(1);
    await act(async () => fail(new Error("synthetic-clear-error")));
    expect(confirm).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "securityEvents.clearFailed",
    );
    fireEvent.click(confirm);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(state.clear).toHaveBeenCalledTimes(2);
  });
});
