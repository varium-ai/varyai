export type RiskLevel = "CRITICAL" | "HIGH" | "MEDIUM" | "LOW";
export type AuditAction = "block" | "audit" | "redact" | "allow";
export type RuleType = "keyword" | "regex";

export interface SecurityAuditSettings {
  enabled: boolean;
}

export interface SecurityRuleItem {
  id: string;
  name: string;
  category: string;
  ruleType?: RuleType;
  riskLevel: RiskLevel;
  action: AuditAction;
  enabled: boolean;
  isBuiltin: boolean;
  actionOverride?: AuditAction | null;
  description?: string;
  pattern?: string;
}

export interface CustomSecurityRule {
  id: string;
  name: string;
  ruleType: RuleType;
  pattern: string;
  riskLevel: RiskLevel;
  action: AuditAction;
  enabled: boolean;
  description?: string;
  updatedAt: number;
}

export interface AuditLogRecord {
  id: number;
  timestamp: number;
  triggerCount?: number;
  lastSeenAt?: number;
  requestId?: string;
  sessionId?: string | null;
  appType: string;
  providerId: string;
  ruleId: string;
  ruleName: string;
  ruleCategory: string;
  riskLevel: RiskLevel;
  actionTaken: AuditAction;
  matchedSnippetMasked: string;
  location?: string;
  createdAt?: string;
}

export interface AuditLogFilter {
  sessionId?: string;
  appType?: string;
  riskLevel?: string;
  ruleCategory?: string;
  action?: string;
  startTimestamp?: number;
  endTimestamp?: number;
  keyword?: string;
}

export interface AuditLogPageResult {
  items: AuditLogRecord[];
  total: number;
  page: number;
  pageSize: number;
}

export interface RuleMatchResult {
  ruleId: string;
  ruleName: string;
  category: string;
  riskLevel: RiskLevel;
  action: AuditAction;
  matchedSnippetMasked: string;
  location: string;
}
