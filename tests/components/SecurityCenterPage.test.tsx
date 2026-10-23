// @vitest-environment jsdom
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import postcss from "postcss";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import tailwindcss from "tailwindcss";
import { resolveUsageRange } from "@/lib/usageRange";
import { SecurityCenterPage } from "@/components/security/SecurityCenterPage";

const auditRows = vi.hoisted(() => ({
  items: [] as Array<Record<string, unknown>>,
  filters: [] as Array<Record<string, unknown>>,
  queries: [] as Array<{
    filter: Record<string, unknown>;
    page: number;
    pageSize: number;
  }>,
  total: undefined as number | undefined,
  isLoading: false,
  isError: false,
  redactedLoading: false,
  refetchByAction: {
    block: vi.fn(),
    redact: vi.fn(),
    audit: vi.fn(),
  } as Record<string, ReturnType<typeof vi.fn>>,
}));

const updateCustomAction = vi.hoisted(() => vi.fn());
const saveRule = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const deleteRule = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

vi.mock("@/components/security/SecurityAuditToggle", () => ({
  SecurityAuditToggle: () => null,
}));

vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityRules: () => ({
    rules: [
      {
        id: "rule-1",
        name: "Internal token",
        category: "custom",
        ruleType: "regex",
        pattern: "INTERNAL_[0-9]+",
        riskLevel: "HIGH",
        action: "redact",
        enabled: true,
        isBuiltin: false,
        description: "Internal token rule",
      },
      {
        id: "builtin-token",
        name: "Built-in token",
        category: "api_token",
        riskLevel: "HIGH",
        action: "block",
        enabled: true,
        isBuiltin: true,
      },
    ],
    isLoading: false,
    toggleRule: vi.fn(),
    saveRule,
    deleteRule,
    updateBuiltinAction: vi.fn(),
    updateCustomAction,
    isSaving: false,
  }),
  useSecurityAuditLogs: (
    filter: { action?: string },
    page: number,
    pageSize: number,
  ) => {
    auditRows.queries.push({ filter, page, pageSize });
    auditRows.filters.push(filter);
    const totalByAction: Record<string, number> = {
      block: 4,
      redact: 3,
      audit: 2,
      allow: 1,
    };
    const total = filter.action
      ? (totalByAction[filter.action] ?? 0)
      : (auditRows.total ?? 1);
    return {
      logsResult: {
        items: auditRows.items,
        total,
        page: 1,
        pageSize: 20,
      },
      isError: auditRows.isError,
      isLoading:
        auditRows.isLoading ||
        (filter.action === "redact" && auditRows.redactedLoading),
      refetch: auditRows.refetchByAction[filter.action ?? ""] ?? vi.fn(),
      clearLogs: vi.fn(),
      isClearing: false,
    };
  },
}));

vi.mock("@/lib/api", () => ({
  securityApi: {
    testRuleMatch: vi.fn(),
  },
}));

describe("SecurityCenterPage", () => {
  it.each([
    ["empty", "security.noFilteredRecords"],
    ["loading", "security.loadingLogs"],
    ["error", "common.error"],
  ])(
    "centers the %s state within the visible table panel",
    async (state, key) => {
      auditRows.isLoading = state === "loading";
      auditRows.isError = state === "error";
      render(<SecurityCenterPage />);

      const table = screen.getByRole("table");
      const scroller = table.parentElement!;
      const message = screen.getByText(key);
      // A centered cell in the 1552px table drifts right in narrower windows.
      expect(scroller).not.toContainElement(message);
      expect(scroller.parentElement).toContainElement(message);
      expect(table.querySelector("tbody")).toBeEmptyDOMElement();

      const status = message.closest('[role="status"]')!;
      const sheet = document.createElement("style");
      sheet.textContent = (
        await postcss([
          tailwindcss({
            content: [{ raw: document.body.innerHTML, extension: "html" }],
            corePlugins: { preflight: false },
          }),
        ]).process("@tailwind utilities;", { from: undefined })
      ).css;
      document.head.appendChild(sheet);
      try {
        expect(getComputedStyle(status).display).toBe("flex");
        expect(getComputedStyle(status).alignItems).toBe("center");
        expect(getComputedStyle(status).justifyContent).toBe("center");
        expect(getComputedStyle(status).textAlign).toBe("center");
      } finally {
        sheet.remove();
      }
    },
  );

  it("shows interception tabs and today's security counts", () => {
    render(<SecurityCenterPage />);

    const indicator = screen.getByTestId("security-tabs-indicator");
    expect(indicator).toHaveStyle({ transform: "translateX(0%)" });
    expect(
      screen.getByRole("tab", { name: /security\.tabRecords/ }),
    ).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("securityStats.warnings")).toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    expect(
      screen.getByRole("tab", { name: /security\.tabRules/ }),
    ).toHaveAttribute("aria-selected", "true");
    expect(indicator).toHaveStyle({ transform: "translateX(100%)" });
    expect(screen.getByText("security.ruleHeading")).toBeInTheDocument();
    expect(screen.getByText("security.addRule")).toBeInTheDocument();
  });

  it("shows redacted, warned and protectively blocked event totals in order", () => {
    auditRows.total = 0;
    render(<SecurityCenterPage />);
    const labels = ["redacted", "warnings", "protectiveBlocked"];
    const cards = labels.map(
      (label) => screen.getByText(`securityStats.${label}`).parentElement!,
    );
    expect(cards.map((card) => card.firstElementChild?.textContent)).toEqual([
      "3",
      "2",
      "4",
    ]);
    expect(Array.from(cards[0].parentElement!.children)).toEqual(cards);
    expect(screen.queryByText("securityStats.blocked")).not.toBeInTheDocument();
    const summary = screen.getByText("securityStats.sensitiveTypesSummary");
    for (const card of cards) expect(card).not.toContainElement(summary);
  });

  it("includes redaction totals when refreshing and waiting for statistics", () => {
    auditRows.redactedLoading = true;
    const { rerender } = render(<SecurityCenterPage />);
    expect(
      screen.getByText("securityStats.redacted").parentElement,
    ).toHaveTextContent("…");
    auditRows.redactedLoading = false;
    rerender(<SecurityCenterPage />);
    expect(
      screen.getByText("securityStats.redacted").parentElement,
    ).toHaveTextContent("3");
    fireEvent.click(
      screen.getByRole("button", { name: "security.refreshLogs" }),
    );
    for (const refetch of Object.values(auditRows.refetchByAction)) {
      expect(refetch).toHaveBeenCalledOnce();
    }
  });

  it("explains the refresh button on hover", () => {
    render(<SecurityCenterPage />);
    expect(
      screen.getByRole("button", { name: "security.refreshLogs" }),
    ).toHaveAttribute("title", "security.refreshLogs");
  });

  it("labels sensitive type counts as a sample when the summary is paginated", () => {
    auditRows.total = 150;
    render(<SecurityCenterPage />);
    expect(
      screen.getByText("securityStats.sensitiveTypesSample"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("securityStats.sensitiveTypesSummary"),
    ).not.toBeInTheDocument();
  });

  it.each([
    ["zh", zh],
    ["en", en],
    ["ja", ja],
    ["zh-TW", zhTW],
  ] as const)(
    "uses the same protective block wording in %s results and stats",
    (_, locale) => {
      expect(locale.security.resultBlocked).toBe(
        locale.securityStats.protectiveBlocked,
      );
    },
  );

  it("keeps blocked outcomes searchable through the result filter", () => {
    render(<SecurityCenterPage />);
    fireEvent.click(
      screen.getByRole("combobox", { name: "security.allActions" }),
    );
    fireEvent.click(
      screen.getByRole("option", { name: /security.resultBlocked/ }),
    );
    expect(
      auditRows.queries.filter((query) => query.pageSize === 20).at(-1)?.filter
        .action,
    ).toBe("block");
  });

  it("does not render the removed global policy controls", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    expect(
      screen.queryByText("security.globalDefaultAction"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("security.blockCriticalTitle"),
    ).not.toBeInTheDocument();
  });

  it("labels the redact rule action explicitly", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    expect(
      screen.getAllByRole("button", { name: "security.ruleActionRedact" }),
    ).toHaveLength(2);
  });

  it("keeps both filter focus rings consistent and inside the scroll area", () => {
    render(<SecurityCenterPage />);
    const session = screen.getByRole("textbox", {
      name: "security.sessionFilter",
    });
    const search = screen.getByPlaceholderText("security.searchPlaceholder");
    for (const input of [session, search]) {
      expect(input).toHaveClass(
        "h-8",
        "rounded-full",
        "focus:border-border-active",
        "focus:ring-2",
        "focus:ring-blue-500/20",
      );
      expect(input).not.toHaveClass("focus:ring-0", "focus-visible:ring-0");
    }
    const scroller = session.parentElement?.parentElement;
    expect(scroller).toContainElement(search);
    expect(scroller).toHaveClass("overflow-x-auto", "p-1");
  });

  it("shows and updates action buttons for custom rules", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));

    const customRuleRow = screen.getByTestId("security-rule-rule-1");
    expect(
      within(customRuleRow).queryByRole("button", {
        name: "security.ruleActionBlock",
      }),
    ).not.toBeInTheDocument();
    expect(
      within(customRuleRow)
        .getAllByRole("button")
        .slice(0, 3)
        .map((button) => button.textContent),
    ).toEqual([
      "security.ruleActionRedact",
      "security.ruleActionAlert",
      "security.ruleActionAllow",
    ]);
    expect(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionAlert",
      }),
    ).toBeInTheDocument();
    expect(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionRedact",
      }),
    ).toBeInTheDocument();
    expect(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionAllow",
      }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(customRuleRow).getByRole("button", {
        name: "security.ruleActionAlert",
      }),
    );
    expect(updateCustomAction).toHaveBeenCalledWith("rule-1", "audit");
  });

  it("shows hide and send as selected for a legacy blocking rule", () => {
    render(<SecurityCenterPage />);
    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    const row = screen.getByTestId("security-rule-builtin-token");
    expect(
      within(row).queryByRole("button", { name: "security.ruleActionBlock" }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: "security.ruleActionRedact" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("offers hide and send first and no block option in the rule editor", () => {
    render(<SecurityCenterPage />);
    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    fireEvent.click(screen.getByRole("button", { name: "security.addRule" }));
    const actionField = screen.getByText(
      "security.dialogAction",
    ).parentElement!;
    fireEvent.click(within(actionField).getByRole("combobox"));
    expect(
      screen.getAllByRole("option").map((option) => option.textContent),
    ).toEqual([
      "security.dialogActionRedact",
      "security.dialogActionAudit",
      "security.dialogActionAllow",
    ]);
  });

  it("requires confirmation before deleting a custom rule", () => {
    deleteRule.mockClear();
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    fireEvent.click(
      within(screen.getByTestId("security-rule-rule-1")).getByRole("button", {
        name: "common.delete",
      }),
    );

    expect(deleteRule).not.toHaveBeenCalled();
    expect(
      screen.getByText("security.deleteRuleConfirmTitle"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "security.deleteRuleConfirm" }),
    );
    expect(deleteRule).toHaveBeenCalledWith("rule-1");
  });

  it("opens existing custom rules in the editor and preserves their id", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: /security\.tabRules/ }));
    fireEvent.click(
      within(screen.getByTestId("security-rule-rule-1")).getByRole("button", {
        name: "common.edit",
      }),
    );

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Internal token")).toBeInTheDocument();
    expect(screen.getByDisplayValue("INTERNAL_[0-9]+")).toBeInTheDocument();
    expect(screen.getByText("security.dialogEditTitle")).toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue("Internal token"), {
      target: { value: "Updated token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "security.saveRule" }));

    expect(saveRule).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "rule-1",
        name: "Updated token",
        ruleType: "regex",
        pattern: "INTERNAL_[0-9]+",
        action: "redact",
      }),
    );
  });

  it("keeps the add-rule form inset inside a bounded dialog", () => {
    render(<SecurityCenterPage />);

    fireEvent.click(screen.getByRole("tab", { name: "security.tabRules" }));
    fireEvent.click(screen.getByRole("button", { name: "security.addRule" }));

    expect(screen.getByRole("dialog")).toHaveClass("overflow-hidden", "z-[60]");
    expect(screen.getByTestId("security-rule-form")).toHaveClass(
      "px-6",
      "py-5",
      "overflow-y-auto",
    );
  });
});

afterEach(() => {
  auditRows.items = [];
  auditRows.filters = [];
  auditRows.queries = [];
  auditRows.total = undefined;
  auditRows.isLoading = false;
  auditRows.isError = false;
  auditRows.redactedLoading = false;
  Object.values(auditRows.refetchByAction).forEach((refetch) =>
    refetch.mockClear(),
  );
  vi.restoreAllMocks();
});

it("shows persisted aggregation counts and first/last hit times in the list and detail", () => {
  auditRows.items = [
    {
      id: 900,
      timestamp: 1000,
      lastSeenAt: 8000,
      triggerCount: 7,
      appType: "claude",
      providerId: "",
      ruleId: "custom-token",
      ruleName: "Repeated token",
      ruleCategory: "custom",
      riskLevel: "HIGH",
      actionTaken: "audit",
      matchedSnippetMasked: "***",
    },
  ];
  render(<SecurityCenterPage />);
  expect(screen.getByText("security.firstSeenAt")).toBeInTheDocument();
  expect(screen.getByText("security.lastSeenAt")).toBeInTheDocument();
  expect(screen.getByText("7")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Repeated token"));
  const detail = screen.getByRole("dialog");
  expect(within(detail).getByText("security.triggerCount")).toBeInTheDocument();
  expect(within(detail).getByText("7")).toBeInTheDocument();
  expect(
    within(detail).getByText(new Date(8000).toLocaleString("en")),
  ).toBeInTheDocument();
});

it("shows session IDs and unknown sessions and filters by exact session ID", () => {
  const row = {
    id: 900,
    timestamp: 1000,
    appType: "claude",
    providerId: "",
    ruleId: "r",
    ruleName: "Session token",
    ruleCategory: "custom",
    riskLevel: "HIGH",
    actionTaken: "audit",
    matchedSnippetMasked: "***",
  };
  auditRows.items = [
    { ...row, sessionId: "session-a" },
    { ...row, id: 901, ruleName: "Unknown token", sessionId: null },
  ];
  render(<SecurityCenterPage />);
  expect(screen.getByText("session-a")).toBeInTheDocument();
  expect(screen.getByText("security.sessionUnknown")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Session token"));
  expect(
    within(screen.getByRole("dialog")).getByText("session-a"),
  ).toBeInTheDocument();
  fireEvent.click(within(screen.getByRole("dialog")).getByText("common.done"));
  fireEvent.change(
    screen.getByRole("textbox", { name: "security.sessionFilter" }),
    { target: { value: "session-a" } },
  );
  expect(
    auditRows.filters.some((filter) => filter.sessionId === "session-a"),
  ).toBe(true);
  fireEvent.click(screen.getByText("security.resetFilters"));
  expect(
    screen.getByRole("textbox", { name: "security.sessionFilter" }),
  ).toHaveValue("");
});

it("keeps audit details and the backdrop above navigation with a scrollable body", async () => {
  auditRows.items = [
    {
      id: 900,
      timestamp: 1000,
      appType: "codex",
      providerId: "",
      ruleId: "r",
      ruleName: "Detail token",
      ruleCategory: "custom",
      riskLevel: "HIGH",
      actionTaken: "audit",
      matchedSnippetMasked: "***",
      sessionId: "session-a",
    },
  ];
  // Reproduce the app navigation's stacking layer, rather than testing class names.
  render(
    <>
      <header className="fixed z-[60]">Navigation</header>
      <SecurityCenterPage />
    </>,
  );
  fireEvent.click(screen.getByText("Detail token"));
  const dialog = screen.getByRole("dialog");
  const backdrop = dialog.previousElementSibling as HTMLElement;
  const body =
    within(dialog).getByText("security.sessionId").parentElement!
      .parentElement!;
  const sheet = document.createElement("style");
  sheet.textContent = (
    await postcss([
      tailwindcss({
        content: [{ raw: document.body.innerHTML, extension: "html" }],
        corePlugins: { preflight: false },
      }),
    ]).process("@tailwind utilities;", { from: undefined })
  ).css;
  document.head.appendChild(sheet);
  try {
    const navigationLayer = Number(
      getComputedStyle(screen.getByText("Navigation")).zIndex,
    );
    expect(Number(getComputedStyle(dialog).zIndex)).toBeGreaterThan(
      navigationLayer,
    );
    expect(Number(getComputedStyle(backdrop).zIndex)).toBeGreaterThan(
      navigationLayer,
    );
    expect(getComputedStyle(body).overflowY).toBe("auto");
    fireEvent.click(within(dialog).getByText("common.done"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  } finally {
    sheet.remove();
  }
});

it("keeps audit headers on one line with reserved column widths and bounded long content", async () => {
  const sessionId = `codex_${"a".repeat(200)}`;
  const ruleName = "A very long detection rule name ".repeat(8);
  auditRows.items = [
    {
      id: 902,
      timestamp: 1000,
      lastSeenAt: 8000,
      triggerCount: 12345,
      appType: "codex",
      ruleId: "long-rule",
      ruleName,
      ruleCategory: "custom",
      riskLevel: "HIGH",
      actionTaken: "audit",
      matchedSnippetMasked: "***",
      sessionId,
    },
  ];
  render(<SecurityCenterPage />);
  const table = screen.getByRole("table");
  const sheet = document.createElement("style");
  sheet.textContent = (
    await postcss([
      tailwindcss({
        content: [{ raw: document.body.innerHTML, extension: "html" }],
        corePlugins: { preflight: false },
      }),
    ]).process("@tailwind utilities;", { from: undefined })
  ).css;
  document.head.appendChild(sheet);
  const pixels = (value: string) =>
    parseFloat(value) * (value.endsWith("rem") ? 16 : 1);
  try {
    expect(getComputedStyle(table).tableLayout).toBe("fixed");
    for (const header of within(table).getAllByRole("columnheader")) {
      expect(getComputedStyle(header).whiteSpace).toBe("nowrap");
    }
    const widths = Array.from(table.querySelectorAll("col"), (col) =>
      pixels(getComputedStyle(col).width),
    );
    expect(widths).toHaveLength(10);
    expect(widths[0]).toBeGreaterThanOrEqual(208);
    expect(widths[1]).toBeGreaterThanOrEqual(208);
    expect(widths[2]).toBeGreaterThanOrEqual(112);
    expect(widths[3]).toBeGreaterThanOrEqual(208);
    expect(widths.slice(4, 9).every((width) => width >= 96)).toBe(true);
    expect(pixels(getComputedStyle(table).minWidth)).toBeGreaterThanOrEqual(
      widths.reduce((sum, width) => sum + width, 0),
    );
    expect(getComputedStyle(table.parentElement!).overflowX).toBe("auto");
    for (const text of [sessionId, ruleName]) {
      const content = within(table).getByTitle(text, {
        normalizer: (value) => value,
      });
      expect(content.textContent).toBe(text);
      expect(content).toHaveAttribute("title", text);
      expect(getComputedStyle(content).overflow).toBe("hidden");
      expect(getComputedStyle(content).textOverflow).toBe("ellipsis");
    }
  } finally {
    sheet.remove();
  }
});

describe("security event date ranges", () => {
  const now = new Date(2026, 9, 13, 12, 0, 0).getTime();
  const openRange = () =>
    fireEvent.click(
      screen.getByRole("button", { name: "security.todayFilter" }),
    );

  it.each(["7d", "30d"] as const)(
    "applies %s to all stats and the event list, preserving filters and resetting pagination",
    (preset) => {
      vi.spyOn(Date, "now").mockReturnValue(now);
      auditRows.total = 50;
      render(<SecurityCenterPage />);
      fireEvent.change(
        screen.getByRole("textbox", { name: "security.sessionFilter" }),
        { target: { value: "session-a" } },
      );
      fireEvent.click(
        screen.getByRole("button", { name: "security.nextPage" }),
      );
      expect(
        auditRows.queries.filter((q) => q.pageSize === 20).at(-1)?.page,
      ).toBe(2);
      openRange();
      expect(
        screen.queryByRole("button", { name: "usage.preset14d" }),
      ).not.toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", {
          name: preset === "7d" ? "security.last7Days" : "security.last30Days",
        }),
      );
      const startTimestamp =
        resolveUsageRange({ preset }, now).startDate * 1000;
      const queries = auditRows.queries.slice(-5);
      expect(queries).toHaveLength(5);
      for (const query of queries) {
        expect(query.filter.startTimestamp).toBe(startTimestamp);
        expect(query.filter.endTimestamp).toBeUndefined();
        expect(query.page).toBe(1);
      }
      expect(queries[0].filter.sessionId).toBe("session-a");
      expect(queries[1].filter.sessionId).toBeUndefined();
      fireEvent.click(screen.getByText("security.resetFilters"));
      expect(
        screen.getByRole("textbox", { name: "security.sessionFilter" }),
      ).toHaveValue("");
      expect(auditRows.queries.slice(-5)[0].filter.startTimestamp).toBe(
        startTimestamp,
      );
    },
  );

  it("applies both custom boundaries to all queries and can return to today", () => {
    vi.spyOn(Date, "now").mockReturnValue(now);
    render(<SecurityCenterPage />);
    openRange();
    fireEvent.change(screen.getByLabelText("usage.startDate"), {
      target: { value: "2026-10-01" },
    });
    fireEvent.change(screen.getByLabelText("usage.startTime"), {
      target: { value: "09:30" },
    });
    fireEvent.change(screen.getByLabelText("usage.endDate"), {
      target: { value: "2026-10-03" },
    });
    fireEvent.change(screen.getByLabelText("usage.endTime"), {
      target: { value: "18:45" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));
    for (const query of auditRows.queries.slice(-5)) {
      expect(query.filter.startTimestamp).toBe(
        new Date(2026, 9, 1, 9, 30).getTime(),
      );
      expect(query.filter.endTimestamp).toBe(
        new Date(2026, 9, 3, 18, 45).getTime() + 999,
      );
    }
    fireEvent.click(
      screen.getByRole("button", { name: "security.customDateRange" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "security.todayFilter" }),
    );
    for (const query of auditRows.queries.slice(-5)) {
      expect(query.filter.startTimestamp).toBe(
        resolveUsageRange({ preset: "today" }, now).startDate * 1000,
      );
      expect(query.filter.endTimestamp).toBeUndefined();
    }
  });

  it("does not apply a cancelled custom draft", () => {
    vi.spyOn(Date, "now").mockReturnValue(now);
    render(<SecurityCenterPage />);
    openRange();
    fireEvent.change(screen.getByLabelText("usage.startDate"), {
      target: { value: "2026-10-01" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(
      screen.getByRole("button", { name: "security.todayFilter" }),
    ).toBeInTheDocument();
    for (const query of auditRows.queries.slice(-5)) {
      expect(query.filter.startTimestamp).toBe(
        resolveUsageRange({ preset: "today" }, now).startDate * 1000,
      );
    }
  });
});
