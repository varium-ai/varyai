// @vitest-environment jsdom
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SecurityEvent } from "@/types/securityEvents";
import { SecurityEventDetailsDialog } from "./SecurityEventDetailsDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("./ShellCommandListsPanel", () => ({
  ShellCommandListDialog: ({
    open,
    commandSummary,
    initialListType,
  }: {
    open: boolean;
    commandSummary?: string;
    initialListType?: string;
  }) =>
    open ? (
      <div data-testid="list-draft">
        {" "}
        {initialListType}:{commandSummary}{" "}
      </div>
    ) : null,
}));

export const toolEvent: SecurityEvent = {
  key: "tool_risk:7",
  eventType: "tool_risk",
  timestamp: 1000,
  lastSeenAt: 1000,
  triggerCount: 1,
  appType: "codex",
  sessionId: null,
  riskLevel: "HIGH",
  result: "warning",
  record: {
    id: 7,
    timestamp: 1000,
    source: "proxy",
    stage: "model_response",
    clientCategory: "codex",
    operationCategory: "shell_exec",
    commandClass: "network_egress",
    commandSummary: "curl https://example.com <redacted>",
    detectionScope: "in_scope",
    integrityStatus: "complete",
    verdict: "risky",
    riskCategory: "data_exfiltration",
    severity: "high",
    reasonCode: "exfil_to_remote",
    policyVersion: 1,
    ruleIds: ["rule-1"],
    suppressedCount: 0,
  },
};
const dlpEvent: SecurityEvent = {
  key: "dlp:7",
  eventType: "dlp",
  timestamp: 1000,
  lastSeenAt: 8000,
  triggerCount: 7,
  appType: "claude",
  sessionId: "session-a",
  riskLevel: null,
  result: "unknown",
  record: {
    id: 7,
    timestamp: 1000,
    lastSeenAt: 8000,
    triggerCount: 7,
    appType: "claude",
    providerId: "",
    ruleId: "r",
    ruleName: "Legacy rule",
    ruleCategory: "custom",
    riskLevel: "FUTURE",
    actionTaken: "future",
    matchedSnippetMasked: "***",
    sessionId: "session-a",
  },
};

describe("SecurityEventDetailsDialog", () => {
  it("shows observation-only tool metadata and only the bounded backend summary", () => {
    const record = {
      ...toolEvent.record,
      input: { command: "raw-secret-never-display" },
    };
    const { container } = render(
      <SecurityEventDetailsDialog
        event={{ ...toolEvent, record } as SecurityEvent}
        onClose={vi.fn()}
      />,
    );
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByText("securityEvents.toolWarning")).toBeInTheDocument();
    expect(
      dialog.getByText("securityEvents.toolObservation"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("curl https://example.com <redacted>"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("securityEvents.commandPrivacy"),
    ).toBeInTheDocument();
    expect(dialog.getByText("rule-1")).toBeInTheDocument();
    expect(
      dialog.getByText("securityEvents.integrityComplete"),
    ).toBeInTheDocument();
    expect(dialog.queryByText("security.outboundSent")).toBeNull();
    expect(container.innerHTML).not.toContain("raw-secret-never-display");
  });
  it("creates only an enum seed and opens a list draft rather than saving", () => {
    const createRule = vi.fn();
    render(
      <SecurityEventDetailsDialog
        event={toolEvent}
        onClose={vi.fn()}
        onCreateRule={createRule}
      />,
    );
    fireEvent.click(screen.getByText("toolRisk.createRuleFromEvent"));
    expect(createRule).toHaveBeenCalledWith({
      operationCategory: "shell_exec",
      commandClass: "network_egress",
      riskCategory: "data_exfiltration",
      severity: "high",
      reasonCode: "exfil_to_remote",
    });
    fireEvent.click(screen.getByText("toolRisk.commandLists.addTo.blacklist"));
    expect(screen.getByTestId("list-draft")).toHaveTextContent(
      "blacklist:curl https://example.com <redacted>",
    );
  });
  it("disables list drafts for unavailable historical summaries and never reconstructs raw fields", () => {
    render(
      <SecurityEventDetailsDialog
        event={
          {
            ...toolEvent,
            record: {
              ...toolEvent.record,
              commandSummary: null,
              command: "raw-secret-never-display",
            },
          } as SecurityEvent
        }
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByText("toolRisk.commandSummaryUnavailable"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("toolRisk.commandLists.addTo.blacklist"),
    ).toBeDisabled();
    expect(
      screen.getByText("toolRisk.commandLists.addTo.whitelist"),
    ).toBeDisabled();
  });
  it("retains DLP aggregation and shows unknown values without claiming LOW, allowed or sent", () => {
    render(<SecurityEventDetailsDialog event={dlpEvent} onClose={vi.fn()} />);
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByText("7")).toBeInTheDocument();
    expect(
      dialog.getByText(new Date(8000).toLocaleString("en")),
    ).toBeInTheDocument();
    expect(dialog.getByText("session-a")).toBeInTheDocument();
    expect(
      dialog.getAllByText("securityEvents.unknown").length,
    ).toBeGreaterThanOrEqual(3);
    expect(dialog.queryByText("security.levelLowShort")).toBeNull();
    expect(dialog.queryByText("security.outboundSent")).toBeNull();
    expect(dialog.queryByText("security.actionAllow")).toBeNull();
  });
  it("keeps CRITICAL visible rather than collapsing the original level", () => {
    render(
      <SecurityEventDetailsDialog
        event={
          {
            ...dlpEvent,
            riskLevel: "CRITICAL",
            record: { ...dlpEvent.record, riskLevel: "CRITICAL" },
          } as SecurityEvent
        }
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("security.riskCritical")).toBeInTheDocument();
  });
});

it.each(["__proto__", "constructor", "toString"])(
  "handles inherited object names as unknown action and metadata: %s",
  (value) => {
    const { rerender } = render(
      <SecurityEventDetailsDialog
        event={
          {
            ...dlpEvent,
            record: { ...dlpEvent.record, actionTaken: value },
          } as SecurityEvent
        }
        onClose={vi.fn()}
      />,
    );
    expect(
      within(screen.getByRole("dialog")).getAllByText("securityEvents.unknown")
        .length,
    ).toBeGreaterThanOrEqual(3);
    rerender(
      <SecurityEventDetailsDialog
        event={
          {
            ...toolEvent,
            record: {
              ...toolEvent.record,
              source: value,
              stage: value,
              integrityStatus: value,
              detectionScope: value,
            },
          } as SecurityEvent
        }
        onClose={vi.fn()}
      />,
    );
    expect(
      within(screen.getByRole("dialog")).getAllByText("securityEvents.unknown")
        .length,
    ).toBeGreaterThanOrEqual(2);
  },
);

it.each([
  [
    "model_response",
    "proposed",
    "securityEvents.sourceModelResponse",
    "securityEvents.stageProposed",
  ],
  [
    "client_hook",
    "pre_execution",
    "securityEvents.sourceHook",
    "securityEvents.stagePreExecution",
  ],
  [
    "mcp_rpc",
    "rpc_submitted",
    "securityEvents.sourceMcpRpc",
    "securityEvents.stageRpcSubmitted",
  ],
])(
  "shows real backend source/stage %s/%s",
  (source, stage, sourceKey, stageKey) => {
    render(
      <SecurityEventDetailsDialog
        event={
          {
            ...toolEvent,
            record: { ...toolEvent.record, source, stage },
          } as SecurityEvent
        }
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText(`${sourceKey} / ${stageKey}`)).toBeInTheDocument();
  },
);
