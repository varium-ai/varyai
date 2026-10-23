// @vitest-environment jsdom
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ToolRiskEventRecord } from "@/types/toolRisk";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { ToolRiskEventsPanel } from "./ToolRiskEventsPanel";

const saveEntry = vi.fn().mockResolvedValue("entry-1");
const clearEvents = vi.fn().mockResolvedValue(0);
const refetchEvents = vi.fn().mockResolvedValue(undefined);
const eventQuery = vi.fn();
let mockIsClearing = false;
let mockEvents: ToolRiskEventRecord[] = [];
let mockToolRiskTranslations: Record<string, unknown> = en.toolRisk;
const assignmentName = ["API", "KEY"].join("_");
const redactedAssignment = `${assignmentName}=<redacted>`;
const rawAssignment = `${assignmentName}=raw-secret-not-for-ui`;

vi.mock("@/hooks/useToolRisk", () => ({
  useToolRiskCommandLists: () => ({
    entries: [],
    isLoading: false,
    isError: false,
    saveEntry,
    deleteEntry: vi.fn(),
    isSaving: false,
    isDeleting: false,
  }),
  useToolRiskEvents: (...args: unknown[]) => ({
    ...eventQuery(...args),
    events: mockEvents,
    isLoading: false,
    refetch: refetchEvents,
    clearEvents,
    isClearing: mockIsClearing,
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      const localKey = key.replace(/^toolRisk\./, "");
      const value = localKey.startsWith("commandLists.")
        ? undefined
        : localKey
            .split(".")
            .reduce<unknown>(
              (node, part) =>
                node && typeof node === "object"
                  ? (node as Record<string, unknown>)[part]
                  : undefined,
              mockToolRiskTranslations,
            );
      return typeof value === "string"
        ? value.replace(/{{(\w+)}}/g, (_, name: string) =>
            String(options?.[name] ?? `{{${name}}}`),
          )
        : key;
    },
  }),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}));

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn();
});
const verdictLabel = (value: string) =>
  value === "all"
    ? (mockToolRiskTranslations.eventFilterAll as string)
    : (mockToolRiskTranslations.verdict as Record<string, string>)[value];
const chooseVerdict = (value: string) => {
  fireEvent.keyDown(screen.getByTestId("tool-risk-events-verdict-filter"), {
    key: "Enter",
  });
  fireEvent.click(
    within(screen.getByRole("listbox")).getByRole("option", {
      name: verdictLabel(value),
    }),
  );
};

const riskyRecord: ToolRiskEventRecord = {
  id: 7,
  timestamp: 1_700_000_000_000,
  source: "proxy",
  stage: "model_response",
  clientCategory: "codex",
  operationCategory: "shell_exec",
  commandSummary: "rm -rf <path>",
  detectionScope: "in_scope",
  integrityStatus: "complete",
  verdict: "risky",
  riskCategory: "remote_code_execution",
  severity: "high",
  reasonCode: "remote_exec_chain",
  policyVersion: 1,
  ruleIds: ["builtin-remote-exec"],
  suppressedCount: 0,
};

describe("ToolRiskEventsPanel", () => {
  beforeEach(() => {
    clearEvents.mockClear();
    mockEvents = [];
    mockToolRiskTranslations = en.toolRisk;
  });

  it("renders recorded tool-call events", () => {
    mockEvents = [riskyRecord];
    render(<ToolRiskEventsPanel />);

    expect(screen.getByTestId("tool-risk-events-table")).toBeTruthy();
    expect(screen.getByText("codex")).toBeTruthy();
    expect(screen.getByText("shell_exec")).toBeTruthy();
    expect(screen.getByText("rm -rf <path>")).toBeTruthy();
  });

  it.each([
    "ls -la /workspace/project",
    "unknown-program --input /workspace/project --url https://example.com/docs",
  ])(
    "preserves the backend summary without a program allowlist: %s",
    (summary) => {
      mockEvents = [{ ...riskyRecord, commandSummary: summary }];
      render(<ToolRiskEventsPanel />);

      const cell = screen.getByText(summary);
      expect(cell.textContent).toBe(summary);
      expect(cell).toHaveAttribute("title", summary);
    },
  );

  it("uses only the sanitized backend summary in the cell and title", () => {
    const summary = `${redactedAssignment} unknown-program --input /workspace/project`;
    // Extra raw payload fields must not become a fallback or tooltip source.
    const recordWithRawPayload = {
      ...riskyRecord,
      commandSummary: summary,
      command: `${rawAssignment} unknown-program`,
      input: { command: `${rawAssignment} unknown-program` },
    };
    mockEvents = [recordWithRawPayload];
    const { container } = render(<ToolRiskEventsPanel />);

    const cell = screen.getByText(summary);
    expect(cell.textContent).toBe(summary);
    expect(cell).toHaveAttribute("title", summary);
    expect(container.innerHTML).not.toContain("raw-secret-not-for-ui");
    expect(container.querySelector("details")).toBeNull();
  });

  it("keeps a long bounded backend summary available in the title", () => {
    const summary = `unknown-program ${"ordinary-argument ".repeat(10)}${redactedAssignment}`;
    mockEvents = [{ ...riskyRecord, commandSummary: summary }];
    render(<ToolRiskEventsPanel />);

    const cell = screen.getByText(summary);
    expect(cell).toHaveClass("truncate");
    expect(cell.textContent).toBe(summary);
    expect(cell).toHaveAttribute("title", summary);
  });

  it.each([null, undefined, "", "   "])(
    "does not fabricate an unavailable summary (%s) from raw fields",
    (commandSummary) => {
      const recordWithRawPayload = {
        ...riskyRecord,
        commandSummary,
        command: `${rawAssignment} ls -la /workspace/project`,
      };
      mockEvents = [recordWithRawPayload];
      const { container } = render(<ToolRiskEventsPanel />);

      const cell = screen.getByText(
        "Not collected; historical summaries cannot be recovered",
      );
      expect(cell).not.toHaveAttribute("title");
      expect(cell).not.toHaveClass("truncate");
      expect(container.innerHTML).not.toContain("raw-secret-not-for-ui");
      expect(screen.queryByText("ls -la /workspace/project")).toBeNull();
    },
  );

  it.each([
    {
      locale: "zh",
      translations: zh.toolRisk,
      description:
        "展示后端处理后的命令摘要：保留普通命令、路径和 URL，仅尽力替换可识别的隐私与敏感值；可能遗漏，请勿视为完整脱敏保证。",
      unavailable: "未采集；历史摘要无法恢复",
    },
    {
      locale: "en",
      translations: en.toolRisk,
      description:
        "Shows backend-processed command summaries: ordinary commands, paths and URLs are preserved; only identifiable private information and sensitive values are replaced on a best-effort basis. Some may be missed; complete sanitization is not guaranteed.",
      unavailable: "Not collected; historical summaries cannot be recovered",
    },
    {
      locale: "ja",
      translations: ja.toolRisk,
      description:
        "バックエンドで処理したコマンド概要を表示します。通常のコマンド、パス、URL は保持し、識別可能な個人情報と機密値のみを可能な範囲で置き換えます。見落としの可能性があり、完全なマスキングは保証されません。",
      unavailable: "未収集。過去の概要は復元できません",
    },
    {
      locale: "zh-TW",
      translations: zhTW.toolRisk,
      description:
        "顯示後端處理後的命令摘要：保留一般命令、路徑和 URL，僅盡力替換可識別的隱私與敏感值；可能遺漏，請勿視為完整去識別化保證。",
      unavailable: "未收集；歷史摘要無法復原",
    },
  ])(
    "explains the privacy boundary and unavailable history in $locale",
    ({ translations, description, unavailable }) => {
      mockToolRiskTranslations = translations;
      mockEvents = [{ ...riskyRecord, commandSummary: null }];
      render(<ToolRiskEventsPanel />);

      expect(screen.getByText(description)).toBeTruthy();
      expect(screen.getByText(unavailable)).toBeTruthy();
      expect(screen.getByText(translations.colCommandSummary)).toBeTruthy();
    },
  );

  it("shows an empty state when there are no events", () => {
    render(<ToolRiskEventsPanel />);
    expect(screen.getByText(en.toolRisk.noEvents)).toBeTruthy();
  });

  it("clears events when the clear button is clicked", async () => {
    mockEvents = [riskyRecord];
    render(<ToolRiskEventsPanel />);

    await act(async () => {
      fireEvent.click(screen.getByTestId("tool-risk-events-clear"));
    });
    expect(clearEvents).toHaveBeenCalled();
  });
});

describe("audit quick actions", () => {
  beforeEach(() => {
    mockToolRiskTranslations = en.toolRisk;
    saveEntry.mockClear();
    mockEvents = [{ ...riskyRecord, commandSummary: "python -c '[REDACTED]'" }];
  });
  it("opens scoped suggestions rather than immediately whitelisting a summary program", () => {
    render(<ToolRiskEventsPanel />);
    fireEvent.click(screen.getByTestId("tool-risk-whitelist-7"));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(
      screen.getByLabelText("toolRisk.commandLists.matchKind"),
    ).toHaveTextContent("toolRisk.commandLists.kind.prefix");
    expect(screen.getByLabelText("toolRisk.commandLists.pattern")).toHaveValue(
      "python",
    );
    expect(saveEntry).not.toHaveBeenCalled();
  });
  it("offers a blacklist dialog and disables summary actions when history is unavailable", () => {
    mockEvents = [riskyRecord, { ...riskyRecord, id: 8, commandSummary: null }];
    render(<ToolRiskEventsPanel />);
    expect(screen.getByTestId("tool-risk-whitelist-8")).toBeDisabled();
    expect(screen.getByTestId("tool-risk-blacklist-8")).toBeDisabled();
    fireEvent.click(screen.getByTestId("tool-risk-blacklist-7"));
    expect(
      screen.getByLabelText("toolRisk.commandLists.listType"),
    ).toHaveTextContent("toolRisk.commandLists.blacklist");
    expect(saveEntry).not.toHaveBeenCalled();
  });
  it("passes only allowlisted enum metadata for risk-rule creation, never raw or summary payload", () => {
    const onCreateRule = vi.fn();
    mockEvents = [
      {
        ...riskyRecord,
        commandClass: "script_exec",
        commandSummary: "python -c '[REDACTED]'",
        command: rawAssignment,
      } as ToolRiskEventRecord,
    ];
    render(<ToolRiskEventsPanel onCreateRule={onCreateRule} />);
    fireEvent.click(screen.getByTestId("tool-risk-create-rule-7"));
    expect(onCreateRule).toHaveBeenCalledWith({
      operationCategory: "shell_exec",
      commandClass: "script_exec",
      riskCategory: "remote_code_execution",
      severity: "high",
      reasonCode: "remote_exec_chain",
    });
    expect(JSON.stringify(onCreateRule.mock.calls)).not.toContain("python");
    expect(JSON.stringify(onCreateRule.mock.calls)).not.toContain(
      "raw-secret-not-for-ui",
    );
  });
});

describe("audit command class compatibility", () => {
  it.each([
    ["script_exec", "script_exec"],
    ["network_egress", "network_egress"],
    ["file_mutate", "file_mutate"],
    ["privilege", "privilege"],
    ["obfuscated", "obfuscated"],
    ["unparseable", "unparseable"],
    ["unclassified_command", "unknown"],
  ])(
    "derives class from fixed reason code %s without a new event field",
    (reasonCode, commandClass) => {
      mockToolRiskTranslations = en.toolRisk;
      mockEvents = [{ ...riskyRecord, reasonCode }];
      const onCreateRule = vi.fn();
      render(<ToolRiskEventsPanel onCreateRule={onCreateRule} />);
      fireEvent.click(screen.getByTestId("tool-risk-create-rule-7"));
      expect(onCreateRule).toHaveBeenCalledWith(
        expect.objectContaining({ commandClass }),
      );
      expect(onCreateRule.mock.calls[0][0]).not.toHaveProperty("reasonCode");
    },
  );
  it("does not pass arbitrary metadata strings as rule seeds", () => {
    mockToolRiskTranslations = en.toolRisk;
    mockEvents = [
      {
        ...riskyRecord,
        operationCategory: rawAssignment,
        riskCategory: rawAssignment,
        reasonCode: rawAssignment,
        severity: rawAssignment,
        commandClass: rawAssignment as never,
      },
    ];
    const onCreateRule = vi.fn();
    render(<ToolRiskEventsPanel onCreateRule={onCreateRule} />);
    fireEvent.click(screen.getByTestId("tool-risk-create-rule-7"));
    expect(onCreateRule).toHaveBeenCalledWith({});
  });
});

const expectedEventReasons = [
  "destructive_target",
  "sensitive_target",
  "exfil_to_remote",
  "remote_exec_chain",
  "privilege_escalation",
  "security_config_target",
  "destructive_sql",
  "unclassified_command",
  "unparseable",
  "file_mutate",
  "network_egress",
  "obfuscated",
  "script_exec",
  "privilege",
  "custom_blocklist",
] as const;

const reasonLocales = [
  ["zh", zh.toolRisk],
  ["en", en.toolRisk],
  ["ja", ja.toolRisk],
  ["zh-TW", zhTW.toolRisk],
] as const;

describe("event reason display", () => {
  beforeEach(() => {
    mockEvents = [];
    mockToolRiskTranslations = en.toolRisk;
  });

  it.each(reasonLocales)(
    "renders all fixed event reasons using %s translations",
    (_locale, translations) => {
      mockToolRiskTranslations = translations;
      mockEvents = expectedEventReasons.map((reasonCode, index) => ({
        ...riskyRecord,
        id: index + 1,
        reasonCode,
      }));
      render(<ToolRiskEventsPanel />);

      const strings = translations as unknown as Record<string, unknown>;
      const reasons = strings.reason as Record<string, string>;
      expect(strings.colReason, `${_locale}: reason column`).toEqual(
        expect.any(String),
      );
      expect(screen.getByText(strings.colReason as string)).toBeInTheDocument();
      expectedEventReasons.forEach((code) => {
        const expected = reasons[code];
        expect(expected, `${_locale}: ${code}`).toEqual(expect.any(String));
        expect(expected).not.toBe(code);
        expect(screen.getByText(expected)).toBeInTheDocument();
        expect(screen.queryByText(code)).not.toBeInTheDocument();
      });
    },
  );

  it.each([null, undefined, "", "toString", "constructor", rawAssignment])(
    "uses a fixed unknown label without echoing an unknown reason (%s)",
    (reasonCode) => {
      mockEvents = [{ ...riskyRecord, reasonCode }];
      const { container } = render(<ToolRiskEventsPanel />);
      const strings = en.toolRisk as unknown as Record<string, string>;

      expect(strings.reasonUnknown).toEqual(expect.any(String));
      expect(screen.getByText(strings.reasonUnknown)).toBeInTheDocument();
      expect(container.innerHTML).not.toContain("raw-secret-not-for-ui");
      if (reasonCode)
        expect(screen.queryByText(reasonCode)).not.toBeInTheDocument();
    },
  );

  it("keeps the reason column aligned in the empty state", () => {
    render(<ToolRiskEventsPanel />);
    const table = screen.getByTestId("tool-risk-events-table");
    const columns = table.querySelectorAll("thead th");
    expect(columns).toHaveLength(9);
    expect(table.querySelector("tbody td")).toHaveAttribute("colspan", "9");
  });
});

describe("P2-10 verdict filters and current-page groups", () => {
  beforeEach(() => {
    mockEvents = [];
    mockToolRiskTranslations = en.toolRisk;
    mockIsClearing = false;
    eventQuery.mockClear();
    clearEvents.mockClear();
    refetchEvents.mockClear();
  });

  function similar(count: number, verdict = "uncertain") {
    return Array.from({ length: count }, (_, index) => ({
      ...riskyRecord,
      id: 100 + index,
      timestamp: 1_800_000_000_000 + index,
      verdict,
      commandSummary: "example-program README.md",
      reasonCode: "unclassified_command",
      suppressedCount: index,
    }));
  }

  it("defaults to risky and exposes all four verdicts plus all", () => {
    render(<ToolRiskEventsPanel />);
    expect(eventQuery).toHaveBeenLastCalledWith(50, 0, "risky");
    const filter = screen.getByTestId("tool-risk-events-verdict-filter");
    expect(filter).toHaveTextContent(verdictLabel("risky"));
    fireEvent.keyDown(filter, { key: "Enter" });
    const options = screen.getByRole("listbox");
    expect(within(options).getAllByRole("option")).toHaveLength(5);
    fireEvent.click(
      within(options).getByRole("option", { name: verdictLabel("risky") }),
    );
    for (const value of [
      "unchecked",
      "uncertain",
      "low_risk",
      "all",
      "risky",
    ]) {
      chooseVerdict(value);
      expect(eventQuery).toHaveBeenLastCalledWith(
        50,
        0,
        value === "all" ? undefined : value,
      );
    }
  });

  it("resets offset atomically when a verdict is changed", () => {
    mockEvents = similar(50, "risky");
    render(<ToolRiskEventsPanel />);
    fireEvent.click(screen.getByText("common.next"));
    expect(eventQuery).toHaveBeenLastCalledWith(50, 50, "risky");
    chooseVerdict("uncertain");
    expect(eventQuery).toHaveBeenLastCalledWith(50, 0, "uncertain");
    expect(
      eventQuery.mock.calls.some(
        (args) => args[1] === 50 && args[2] === "uncertain",
      ),
    ).toBe(false);
  });

  it.each(["uncertain", "low_risk"])(
    "groups ten %s records and expands every original ID and action",
    (verdict) => {
      mockEvents = similar(10, verdict);
      const onCreateRule = vi.fn();
      const original = JSON.stringify(mockEvents);
      render(<ToolRiskEventsPanel onCreateRule={onCreateRule} />);
      const toggle = screen.getByTestId("tool-risk-event-group-toggle-100");
      expect(toggle).toHaveAttribute("aria-expanded", "false");
      expect(screen.getByText("Similar records on this page: 10")).toBeTruthy();
      expect(screen.queryByTestId("tool-risk-blacklist-109")).toBeNull();
      fireEvent.click(toggle);
      for (let id = 100; id < 110; id++) {
        expect(screen.getByTestId(`tool-risk-event-${id}`)).toHaveTextContent(
          `ID ${id}`,
        );
        expect(screen.getByTestId(`tool-risk-blacklist-${id}`)).toBeEnabled();
        expect(screen.getByTestId(`tool-risk-create-rule-${id}`)).toBeEnabled();
      }
      fireEvent.click(screen.getByTestId("tool-risk-create-rule-109"));
      expect(onCreateRule).toHaveBeenCalledTimes(1);
      expect(JSON.stringify(mockEvents)).toBe(original);
      fireEvent.click(toggle);
      expect(screen.queryByTestId("tool-risk-event-109")).toBeNull();
    },
  );

  it.each(["risky", "unchecked"])(
    "keeps every %s event visible individually",
    (verdict) => {
      mockEvents = similar(10, verdict);
      render(<ToolRiskEventsPanel />);
      for (let id = 100; id < 110; id++)
        expect(screen.getByTestId(`tool-risk-blacklist-${id}`)).toBeTruthy();
      expect(
        screen.queryByTestId("tool-risk-event-group-toggle-100"),
      ).toBeNull();
    },
  );

  it("paginates fifty raw events even when they display as one group", () => {
    mockEvents = similar(50);
    render(<ToolRiskEventsPanel />);
    expect(screen.getByText("Similar records on this page: 50")).toBeTruthy();
    fireEvent.click(screen.getByText("common.next"));
    expect(eventQuery).toHaveBeenLastCalledWith(50, 50, "risky");
    expect(screen.getByText(/Only this page/)).toBeTruthy();
  });

  it("does not merge missing summaries or distinct policy/client/integrity metadata", () => {
    mockEvents = [
      ...similar(1),
      { ...similar(1)[0], id: 101, policyVersion: 2 },
      { ...similar(1)[0], id: 102, clientCategory: "claude_code" },
      { ...similar(1)[0], id: 103, integrityStatus: "incomplete" },
      { ...similar(1)[0], id: 104, commandSummary: null },
      { ...similar(1)[0], id: 105, commandSummary: null },
    ];
    render(<ToolRiskEventsPanel />);
    expect(screen.queryByTestId("tool-risk-event-group-toggle-100")).toBeNull();
    expect(screen.getByTestId("tool-risk-blacklist-104")).toBeDisabled();
    expect(screen.getByTestId("tool-risk-blacklist-105")).toBeDisabled();
  });

  it("can clear all history from an empty filtered page and resets pagination", async () => {
    mockEvents = similar(50, "risky");
    const view = render(<ToolRiskEventsPanel />);
    fireEvent.click(screen.getByText("common.next"));
    mockEvents = [];
    view.rerender(<ToolRiskEventsPanel />);
    expect(screen.getByTestId("tool-risk-events-clear")).toBeEnabled();
    expect(screen.getByTestId("tool-risk-events-clear")).toHaveTextContent(
      "Clear all records",
    );
    fireEvent.click(screen.getByTestId("tool-risk-events-clear"));
    await waitFor(() =>
      expect(eventQuery).toHaveBeenLastCalledWith(50, 0, "risky"),
    );
    expect(clearEvents).toHaveBeenLastCalledWith();
  });

  it("disables global clear while it is pending", () => {
    mockIsClearing = true;
    render(<ToolRiskEventsPanel />);
    expect(screen.getByTestId("tool-risk-events-clear")).toBeDisabled();
  });

  it("refreshes the active filter and rebuilds groups when new records arrive", () => {
    mockEvents = similar(10);
    const view = render(<ToolRiskEventsPanel />);
    chooseVerdict("uncertain");
    fireEvent.click(screen.getByText("common.refresh"));
    expect(refetchEvents).toHaveBeenCalledTimes(1);
    mockEvents = similar(11);
    view.rerender(<ToolRiskEventsPanel />);
    expect(screen.getByText("Similar records on this page: 11")).toBeTruthy();
    expect(eventQuery).toHaveBeenLastCalledWith(50, 0, "uncertain");
  });

  it.each(reasonLocales)(
    "provides filter, retention and group-boundary text in %s",
    (_locale, translations) => {
      mockToolRiskTranslations = translations;
      render(<ToolRiskEventsPanel />);
      for (const key of [
        "eventVerdictFilter",
        "eventRetentionHint",
        "eventPageGroupingHint",
        "clearAllEvents",
      ] as const) {
        const value = (translations as Record<string, unknown>)[key];
        expect(typeof value).toBe("string");
        expect(screen.getByText(value as string)).toBeTruthy();
      }
      fireEvent.keyDown(screen.getByTestId("tool-risk-events-verdict-filter"), {
        key: "Enter",
      });
      expect(
        screen.getByRole("option", {
          name: translations.eventFilterAll as string,
        }),
      ).toBeInTheDocument();
    },
  );
});

describe("controlled unified tool calls", () => {
  it("uses provided events, grouping and details without invoking the standalone query", () => {
    eventQuery.mockClear();
    const onSelectEvent = vi.fn();
    const controlled = {
      events: [riskyRecord],
      isLoading: false,
      isError: false,
      onSelectEvent,
    };
    render(<ToolRiskEventsPanel controlled={controlled} />);
    expect(eventQuery).not.toHaveBeenCalled();
    expect(screen.getByText("rm -rf <path>")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("tool-risk-event-7"));
    expect(onSelectEvent).toHaveBeenCalledWith(riskyRecord);
    expect(screen.queryByTestId("tool-risk-events-clear")).toBeNull();
    expect(screen.queryByTestId("tool-risk-events-verdict-filter")).toBeNull();
  });
  it("shows unknown and absent verdicts distinctly without exposing arbitrary enum strings", () => {
    render(
      <ToolRiskEventsPanel
        controlled={{
          events: [
            { ...riskyRecord, id: 81, verdict: null },
            {
              ...riskyRecord,
              id: 82,
              verdict: "future-verdict",
              riskCategory: "future-category",
              severity: "future-severity",
            },
          ],
          isLoading: false,
          isError: false,
        }}
      />,
    );
    expect(screen.getByTestId("tool-risk-event-81")).toHaveTextContent(
      "securityEvents.notRecorded",
    );
    expect(screen.getByTestId("tool-risk-event-82")).toHaveTextContent(
      "securityEvents.unknown",
    );
    expect(document.body.innerHTML).not.toContain("future-verdict");
    expect(document.body.innerHTML).not.toContain("future-category");
    expect(document.body.innerHTML).not.toContain("future-severity");
  });
});

describe("controlled tool-call labels and observation boundary", () => {
  beforeEach(() => {
    mockToolRiskTranslations = en.toolRisk;
  });

  it.each(reasonLocales)(
    "localizes every controlled operation in %s and adds an observation-only status column",
    (_locale, translations) => {
      mockToolRiskTranslations = translations;
      const operations = Object.keys(translations.operation);
      render(
        <ToolRiskEventsPanel
          controlled={{
            events: operations.map((operationCategory, index) => ({
              ...riskyRecord,
              id: index + 1,
              operationCategory,
            })),
            isLoading: false,
            isError: false,
          }}
        />,
      );
      const table = screen.getByTestId("tool-risk-events-table");
      expect(within(table).getAllByRole("columnheader")).toHaveLength(10);
      expect(
        within(table).getByRole("columnheader", {
          name: "security.detailOutbound",
        }),
      ).toBeInTheDocument();
      operations.forEach((operation, index) => {
        const row = screen.getByTestId(`tool-risk-event-${index + 1}`);
        expect(within(row).getAllByRole("cell")).toHaveLength(10);
        expect(row).toHaveTextContent(
          translations.operation[
            operation as keyof typeof translations.operation
          ],
        );
        expect(row).toHaveTextContent("securityEvents.toolObservation");
        expect(row).not.toHaveTextContent(operation);
        expect(row).not.toHaveTextContent("security.outboundSent");
        expect(row).not.toHaveTextContent("security.outboundBlocked");
      });
    },
  );

  it.each([
    null,
    undefined,
    "",
    "__proto__",
    "constructor",
    "toString",
    rawAssignment,
  ])(
    "does not echo an unknown or unavailable controlled operation (%s)",
    (operationCategory) => {
      const { container } = render(
        <ToolRiskEventsPanel
          controlled={{
            events: [{ ...riskyRecord, operationCategory }],
            isLoading: false,
            isError: false,
          }}
        />,
      );
      const operationCell = within(
        screen.getByTestId("tool-risk-event-7"),
      ).getAllByRole("cell")[2];
      expect(operationCell).toHaveTextContent(
        operationCategory
          ? "securityEvents.unknown"
          : "securityEvents.notRecorded",
      );
      expect(container.innerHTML).not.toContain("raw-secret-not-for-ui");
      if (operationCategory)
        expect(operationCell).not.toHaveTextContent(operationCategory);
    },
  );

  it("replaces the standalone retention hint with an observation-only warning in controlled mode", () => {
    render(
      <ToolRiskEventsPanel
        controlled={{ events: [riskyRecord], isLoading: false, isError: false }}
      />,
    );
    const warning = screen.getByText("securityEvents.toolWarning", {
      exact: false,
    });
    expect(warning).toBeInTheDocument();
    expect(screen.getByTestId("tool-risk-event-7")).toHaveTextContent(
      "securityEvents.toolObservation",
    );
    expect(screen.queryByText(en.toolRisk.eventRetentionHint)).toBeNull();
    expect(
      screen.getByText(en.toolRisk.eventPageGroupingHint, { exact: false }),
    ).toBeInTheDocument();
  });

  it.each(["empty", "loading", "error"])(
    "keeps all controlled columns aligned for the %s state",
    (state) => {
      render(
        <ToolRiskEventsPanel
          controlled={{
            events: [],
            isLoading: state === "loading",
            isError: state === "error",
          }}
        />,
      );
      const table = screen.getByTestId("tool-risk-events-table");
      expect(table.querySelectorAll("thead th")).toHaveLength(10);
      expect(table.querySelector("tbody td")).toHaveAttribute("colspan", "10");
    },
  );
});

describe("tool-call table layout", () => {
  it.each([false, true])(
    "keeps explicit readable column widths and local overflow (controlled=%s)",
    (controlled) => {
      mockEvents = [riskyRecord];
      render(
        <ToolRiskEventsPanel
          onCreateRule={vi.fn()}
          controlled={
            controlled
              ? { events: [riskyRecord], isLoading: false, isError: false }
              : undefined
          }
        />,
      );
      const table = screen.getByTestId("tool-risk-events-table");
      expect(table).toHaveClass(
        "table-fixed",
        controlled ? "min-w-[1760px]" : "min-w-[1544px]",
      );
      expect(table.parentElement).toHaveClass("overflow-auto");
      const columns = Array.from(table.querySelectorAll("colgroup col"));
      expect(columns.map((column) => column.getAttribute("style"))).toEqual(
        [
          176,
          104,
          128,
          320,
          112,
          152,
          104,
          184,
          ...(controlled ? [216] : []),
          264,
        ].map((width) => `width: ${width}px;`),
      );
      for (const heading of within(table).getAllByRole("columnheader")) {
        expect(heading).toHaveClass("whitespace-nowrap");
      }
      const cells = within(
        screen.getByTestId("tool-risk-event-7"),
      ).getAllByRole("cell");
      expect(cells[4]).toHaveClass("whitespace-nowrap");
      expect(cells[4].firstElementChild).toHaveClass("whitespace-nowrap");
      expect(cells[6]).toHaveClass("whitespace-nowrap");
      expect(cells[5]).toHaveClass("break-words", "leading-5");
      expect(cells[7]).toHaveClass("break-words", "leading-5");
      const command = within(cells[3]).getByText(riskyRecord.commandSummary!);
      expect(command.tagName).toBe("DIV");
      expect(command).toHaveClass("truncate", "font-mono");
      const actions = cells.at(-1)!;
      expect(within(actions).getAllByRole("button")).toHaveLength(3);
    },
  );
});
