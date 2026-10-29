import { beforeEach, describe, expect, expectTypeOf, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { securityEventsApi } from "./securityEvents";
import { securityEventsApi as exportedApi } from "./index";
import type { AuditLogRecord } from "@/types/security";
import type { ToolRiskEventRecord } from "@/types/toolRisk";
import type {
  SecurityEvent,
  SecurityEventCommon,
  SecurityEventDlpRecord,
  SecurityEventFilter,
  SecurityEventPage,
  SecurityEventResult,
  SecurityEventSummary,
  SecurityEventType,
  SecurityEventView,
  SecurityEventsClearReceipt,
} from "@/types/securityEvents";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const filter: SecurityEventFilter = {
  view: "security_events",
  eventType: "dlp",
  startTimestamp: 0,
  endTimestamp: 1_800_000_000_000,
  appType: "claude-desktop",
  riskLevel: "HIGH",
  result: "unknown",
  category: "dlp:custom",
  sessionId: " session_% ",
  keyword: "  literal_%  ",
};

beforeEach(() => vi.mocked(invoke).mockReset());

describe("unified security event contract", () => {
  it("exports the spec types without widening legacy DLP records", () => {
    expectTypeOf<SecurityEventType>().toEqualTypeOf<"dlp" | "tool_risk">();
    expectTypeOf<SecurityEventView>().toEqualTypeOf<
      "security_events" | "tool_calls"
    >();
    expectTypeOf<SecurityEventResult>().toEqualTypeOf<
      | "redacted"
      | "blocked"
      | "warning"
      | "allowed"
      | "unchecked"
      | "uncertain"
      | "low_risk"
      | "unknown"
    >();
    expectTypeOf<SecurityEventDlpRecord["riskLevel"]>().toEqualTypeOf<string>();
    expectTypeOf<
      SecurityEventDlpRecord["actionTaken"]
    >().toEqualTypeOf<string>();
    expectTypeOf<AuditLogRecord["actionTaken"]>().toEqualTypeOf<
      "block" | "audit" | "redact" | "redact_restore" | "allow"
    >();
    expectTypeOf<
      Extract<SecurityEvent, { eventType: "tool_risk" }>["record"]
    >().toEqualTypeOf<ToolRiskEventRecord>();
    expectTypeOf<SecurityEventCommon["sessionId"]>().toEqualTypeOf<
      string | null
    >();
    expect(exportedApi).toBe(securityEventsApi);
  });

  it("queries one backend-paginated snapshot and preserves every filter", async () => {
    const page: SecurityEventPage = {
      items: [],
      total: 82,
      page: 2,
      pageSize: 50,
    };
    vi.mocked(invoke).mockResolvedValueOnce(page);
    expect(await securityEventsApi.getEvents(filter, 2, 50)).toBe(page);
    expect(invoke).toHaveBeenCalledWith("get_security_events", {
      filter,
      page: 2,
      pageSize: 50,
    });
  });

  it("returns the snapshot summary directly, including other records", async () => {
    const summary: SecurityEventSummary = {
      total: 12,
      dlpCount: 8,
      toolRiskCount: 4,
      redacted: 2,
      warnings: 5,
      protectiveBlocked: 1,
      other: 4,
    };
    vi.mocked(invoke).mockResolvedValueOnce(summary);
    expect(await securityEventsApi.getSummary(filter)).toBe(summary);
    expect(invoke).toHaveBeenCalledWith("get_security_events_summary", {
      filter,
    });
  });

  it("clears one frozen filter atomically and returns actual deletion counts", async () => {
    const toolFilter: SecurityEventFilter = {
      view: "tool_calls",
      toolVerdict: "uncertain",
      endTimestamp: 123,
    };
    const receipt: SecurityEventsClearReceipt = {
      deletedTotal: 3,
      deletedDlp: 0,
      deletedToolRisk: 3,
    };
    vi.mocked(invoke).mockResolvedValueOnce(receipt);
    expect(await securityEventsApi.clearEvents(toolFilter)).toBe(receipt);
    expect(invoke).toHaveBeenCalledWith("clear_security_events", {
      filter: toolFilter,
    });
  });

  it.each(["getEvents", "getSummary", "clearEvents"] as const)(
    "propagates %s failures instead of reporting empty success",
    async (method) => {
      vi.mocked(invoke).mockRejectedValueOnce(
        new Error("security_events_query_failed"),
      );
      await expect(securityEventsApi[method](filter, 1, 50)).rejects.toThrow(
        "security_events_query_failed",
      );
    },
  );
});
