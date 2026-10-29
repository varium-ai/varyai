import type { AuditLogRecord, RiskLevel } from "./security";
import type { ToolRiskEventRecord, ToolRiskVerdict } from "./toolRisk";

/** Product event type, distinct from the tool collection source in record.source. */
export type SecurityEventType = "dlp" | "tool_risk";
export type SecurityEventView = "security_events" | "tool_calls";
export type SecurityEventResult =
  | "redacted"
  | "blocked"
  | "warning"
  | "allowed"
  | "unchecked"
  | "uncertain"
  | "low_risk"
  | "unknown";

export interface SecurityEventCommon {
  key: string;
  timestamp: number;
  lastSeenAt: number;
  triggerCount: number;
  appType: string | null;
  sessionId: string | null;
  riskLevel: RiskLevel | null;
  result: SecurityEventResult;
}

/** Preserve unknown historical values without widening the legacy AuditLogRecord contract. */
export type SecurityEventDlpRecord = Omit<
  AuditLogRecord,
  "riskLevel" | "actionTaken"
> & {
  riskLevel: string;
  actionTaken: string;
};

export type SecurityEvent = SecurityEventCommon &
  (
    | { eventType: "dlp"; record: SecurityEventDlpRecord }
    | { eventType: "tool_risk"; record: ToolRiskEventRecord }
  );

export interface SecurityEventFilter {
  view: SecurityEventView;
  eventType?: SecurityEventType;
  startTimestamp?: number;
  endTimestamp?: number;
  appType?: string;
  riskLevel?: RiskLevel;
  result?: SecurityEventResult;
  category?: string;
  sessionId?: string;
  keyword?: string;
  toolVerdict?: ToolRiskVerdict;
}

export interface SecurityEventPage {
  items: SecurityEvent[];
  total: number;
  page: number;
  pageSize: number;
}

export interface SecurityEventSummary {
  total: number;
  dlpCount: number;
  toolRiskCount: number;
  redacted: number;
  warnings: number;
  protectiveBlocked: number;
  other: number;
}

export interface SecurityEventsClearReceipt {
  deletedTotal: number;
  deletedDlp: number;
  deletedToolRisk: number;
}
