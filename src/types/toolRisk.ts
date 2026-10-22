//

export type ToolRiskRuleOrigin = "builtin" | "custom";

export type ToolRiskOperationType =
  | "process_exec"
  | "shell_exec"
  | "file_read"
  | "file_write"
  | "file_delete"
  | "network_request"
  | "data_send"
  | "database_operation"
  | "unknown_operation";

export type ToolRiskCategory =
  | "destructive_change"
  | "sensitive_read"
  | "data_exfiltration"
  | "remote_code_execution"
  | "privilege_change"
  | "security_config_change"
  | "destructive_database_change";

export type ToolRiskSeverity = "none" | "medium" | "high";

export type ToolRiskLocalReview = "off" | "eligible";

export type ToolRiskConditionOp =
  | "eq"
  | "in"
  | "path_category"
  | "path_glob"
  | "keyword"
  | "regex"
  | "detector";

export type ToolRiskVerdict = "risky" | "unchecked" | "uncertain" | "low_risk";

export interface ToolRiskToolRef {
  client: string;
  namespace?: string | null;
  name: string;
  definitionRef?: string | null;
  serverIdentity?: string | null;
}

export type ToolRiskRuleTarget =
  | { kind: "operation"; types: ToolRiskOperationType[] }
  | { kind: "tool"; tool_refs: ToolRiskToolRef[] };

export function parseToolRiskToolRefs(text: string): ToolRiskToolRef[] | null {
  try {
    const refs: unknown = JSON.parse(text);
    if (!Array.isArray(refs) || refs.length === 0) return null;
    const keys = new Set([
      "client",
      "namespace",
      "name",
      "definitionRef",
      "serverIdentity",
    ]);
    const nonEmpty = (value: unknown): value is string =>
      typeof value === "string" && value.trim().length > 0;
    for (const ref of refs) {
      if (!ref || typeof ref !== "object" || Array.isArray(ref)) return null;
      if (Object.keys(ref).some((key) => !keys.has(key))) return null;
      if (!nonEmpty(ref.client) || !nonEmpty(ref.name)) return null;
      for (const key of ["namespace", "definitionRef", "serverIdentity"]) {
        if (ref[key] != null && !nonEmpty(ref[key])) return null;
      }
      if (!nonEmpty(ref.definitionRef) && !nonEmpty(ref.serverIdentity))
        return null;
      if (ref.namespace === "mcp" && !nonEmpty(ref.serverIdentity)) return null;
      if (nonEmpty(ref.serverIdentity) && !nonEmpty(ref.namespace)) return null;
    }
    return refs;
  } catch {
    return null;
  }
}

export interface ToolRiskRuleSelector {
  sources?: string[] | null;
  clients?: string[] | null;
  target: ToolRiskRuleTarget;
}

export interface ToolRiskRuleCondition {
  field: string;
  op: ToolRiskConditionOp;
  value?: string | null;
  values?: string[] | null;
}

export interface ToolRiskRule {
  schema_version: number;
  id: string;
  revision: number;
  origin: ToolRiskRuleOrigin;
  name: string;
  description?: string | null;
  enabled: boolean;
  selector: ToolRiskRuleSelector;
  conditions: ToolRiskRuleCondition[];
  risk_category: ToolRiskCategory;
  severity: ToolRiskSeverity;
  reason_code: string;
  local_review: ToolRiskLocalReview;
}

export interface ToolRiskRuleItem {
  id: string;
  name: string;
  origin: ToolRiskRuleOrigin;
  enabled: boolean;
  riskCategory: string;
  severity: string;
  reasonCode: string;
  localReview: string;
  /** Legacy list summary, used only when structured data is absent. */
  selectorSummary: string;
  conditionCount: number;
  /** Structured selector for semantic scope labels in the rule list. */
  targetKind: ToolRiskRuleTarget["kind"];
  operationTypes: string[];
  toolRefCount: number;
  /** Structured conditions for semantic summaries rather than a condition count. */
  conditions: ToolRiskRuleCondition[];
  revision: number;
  builtinVersion?: number | null;
}

export interface SaveCustomRuleInput {
  id?: string | null;
  name: string;
  description?: string | null;
  enabled: boolean;
  selector: ToolRiskRuleSelector;
  conditions: ToolRiskRuleCondition[];
  riskCategory: ToolRiskCategory;
  severity: ToolRiskSeverity;
  reasonCode: string;
  localReview: ToolRiskLocalReview;
  expectedRevision?: number | null;
}

export interface ToolRiskSettings {
  enabled: boolean;
  policyVersion: number;
}

export interface ToolRiskEventRecord {
  id: number;
  timestamp: number;
  source: string;
  stage: string;
  clientCategory?: string | null;
  operationCategory?: string | null;

  commandSummary?: string | null;
  commandClass?: ToolRiskCommandClass | null;
  detectionScope: string;
  integrityStatus: string;
  verdict?: string | null;
  riskCategory?: string | null;
  severity?: string | null;
  reasonCode?: string | null;
  policyVersion: number;
  ruleIds: string[];
  suppressedCount: number;
}

export interface SandboxTestInput {
  toolName: string;
  input: unknown;
  client?: string | null;

  definitionRef?: string;
}

export interface SandboxTestResult {
  scope: string;
  verdict?: string | null;
  matchedRuleIds: string[];
  riskCategories: string[];
  applicableRules: number;
}

export const KNOWN_DETECTORS = [
  "destructive_path",
  "sensitive_path",
  "exfil_destination",
  "remote_download_exec",
  "privilege_escalation",
  "security_config_path",
  "destructive_sql",
] as const;

export const DETECTOR_FIELDS: Record<(typeof KNOWN_DETECTORS)[number], string> =
  {
    destructive_path: "/target_path",
    sensitive_path: "/target_path",
    exfil_destination: "/destination",
    remote_download_exec: "/command",
    privilege_escalation: "/command",
    security_config_path: "/target_path",
    destructive_sql: "/command",
  };

export const OPERATION_FIELDS = [
  "/target_path",
  "/command",
  "/destination",
  "/op_type",
  "/recursive",
  "/overwrite",
  "/command_class",
] as const;

/** Shared semantic field slugs; keep aligned with backend OPERATION_FIELDS. */
export const OPERATION_FIELD_SLUGS: Record<string, string> = {
  "/target_path": "target_path",
  "/command": "command",
  "/destination": "destination",
  "/op_type": "op_type",
  "/recursive": "recursive",
  "/overwrite": "overwrite",
  "/command_class": "command_class",
};

/** Supported operators per semantic field; the editor must not generate other combinations. */
export const OPERATION_FIELD_OPS: Record<string, ToolRiskConditionOp[]> = {
  "/target_path": ["eq", "keyword", "regex", "detector"],
  "/command": ["eq", "keyword", "regex", "detector"],
  "/destination": ["eq", "keyword", "regex", "detector"],
  "/command_class": ["eq", "in"],
  "/op_type": ["eq", "in"],
  "/recursive": ["eq"],
  "/overwrite": ["eq"],
};

/** Exact-tool fields use JSON Pointers or `$text`, not operation-only detectors. */
export const TOOL_TARGET_OPS: ToolRiskConditionOp[] = [
  "eq",
  "in",
  "keyword",
  "regex",
];

/** Restrict new operators, but retain and allow saving an already selected legacy operator. */
export function toolRiskConditionOps(
  field: string,
  targetKind: ToolRiskRuleTarget["kind"],
): ToolRiskConditionOp[] {
  if (targetKind === "tool") return [...TOOL_TARGET_OPS];
  return (
    OPERATION_FIELD_OPS[field] ??
    CONDITION_OPS.filter((op) => op !== "detector")
  );
}

/** Prefer structured operation types; use the selector summary only for legacy payloads. */
export function toolRiskScopeTerms(rule: ToolRiskRuleItem): string[] {
  const terms = [...rule.operationTypes];
  const summary = rule.selectorSummary ?? "";
  if (summary.startsWith("operation:")) {
    for (const name of summary.slice("operation:".length).split(",")) {
      const trimmed = name.trim();
      if (trimmed && !terms.includes(trimmed)) terms.push(trimmed);
    }
  }
  return terms;
}

/** Validate supported editor conditions without migrating or rewriting existing rules. */
export function toolRiskConditionIssue(
  condition: ToolRiskRuleCondition,
  targetKind: ToolRiskRuleTarget["kind"],
):
  | "unsupportedCondition"
  | "conditionValueRequired"
  | "invalidConditionShape"
  | null {
  if (!CONDITION_OPS.includes(condition.op)) return "unsupportedCondition";
  if (!condition.field.trim()) return "conditionValueRequired";
  const validField =
    targetKind === "operation"
      ? OPERATION_FIELDS.some((field) => field === condition.field)
      : condition.field === "$text" ||
        (condition.field.startsWith("/") &&
          !/~(?:[^01]|$)/.test(condition.field));
  if (!validField) return "unsupportedCondition";
  if (condition.op === "detector") {
    if (targetKind !== "operation") return "unsupportedCondition";
    if (
      condition.value?.trim() &&
      !KNOWN_DETECTORS.some(
        (id) =>
          id === condition.value && DETECTOR_FIELDS[id] === condition.field,
      )
    )
      return "unsupportedCondition";
  }
  if (condition.op === "in") {
    if (condition.value != null) return "invalidConditionShape";
    if (
      !condition.values?.length ||
      condition.values.some((value) => !value.trim())
    )
      return "conditionValueRequired";
  } else {
    if (condition.values != null) return "invalidConditionShape";
    if (!condition.value?.trim()) return "conditionValueRequired";
  }
  return null;
}

export const KNOWN_REASON_CODES = [
  "destructive_target",
  "sensitive_target",
  "exfil_to_remote",
  "remote_exec_chain",
  "privilege_escalation",
  "security_config_target",
  "destructive_sql",
] as const;

export const EVENT_REASON_CODES = [
  ...KNOWN_REASON_CODES,
  "unclassified_command",
  "unparseable",
  "file_mutate",
  "network_egress",
  "obfuscated",
  "script_exec",
  "privilege",
  "custom_blocklist",
] as const;

export const OPERATION_TYPES: ToolRiskOperationType[] = [
  "process_exec",
  "shell_exec",
  "file_read",
  "file_write",
  "file_delete",
  "network_request",
  "data_send",
  "database_operation",
];

export const RISK_CATEGORIES: ToolRiskCategory[] = [
  "destructive_change",
  "sensitive_read",
  "data_exfiltration",
  "remote_code_execution",
  "privilege_change",
  "security_config_change",
  "destructive_database_change",
];

export const CONDITION_OPS: ToolRiskConditionOp[] = [
  "eq",
  "in",
  "keyword",
  "regex",
  "detector",
];

export const COMMAND_CLASSES = [
  "read_only_query",
  "file_mutate",
  "network_egress",
  "script_exec",
  "privilege",
  "obfuscated",
  "unparseable",
  "unknown",
] as const;
export type ToolRiskCommandClass = (typeof COMMAND_CLASSES)[number];
export type CommandListType = "whitelist" | "blacklist";
export type CommandMatchKind =
  | "program"
  | "prefix"
  | "exact"
  | "keyword"
  | "regex";
export interface CommandListEntry {
  id: string;
  revision: number;
  listType: CommandListType;
  matchKind: CommandMatchKind;
  pattern: string;
  enabled: boolean;
  severity: ToolRiskSeverity;
  riskCategory: ToolRiskCategory;
}
export interface SaveCommandListInput {
  id?: string;
  expectedRevision?: number;
  listType: CommandListType;
  matchKind: CommandMatchKind;
  pattern: string;
  enabled: boolean;
  severity: ToolRiskSeverity;
  riskCategory: ToolRiskCategory;
}

export interface CommandPatternPreview {
  eventId: number;
  matched: boolean;
  summaryOnly: boolean;
}

export interface ToolRiskRuleSeed {
  operationCategory?: ToolRiskOperationType;
  commandClass?: ToolRiskCommandClass;
  riskCategory?: ToolRiskCategory;
  severity?: "medium" | "high";
  reasonCode?: (typeof KNOWN_REASON_CODES)[number];
}
export const RISK_SEVERITIES = ["medium", "high"] as const;

export interface ToolRiskWriteReceipt<T> {
  value: T;
  persistedPolicyVersion: number;
  auditStatus: "recorded" | "not_needed" | "audit_unavailable";
  runtimeStatus: "applied" | "apply_failed";
}

export const POLICY_CHANGE_ACTORS = ["local_ui", "system", "unknown"] as const;
export const POLICY_CHANGE_KINDS = [
  "settings_update",
  "rule_create",
  "rule_update",
  "rule_delete",
  "builtin_toggle",
  "builtin_reset",
  "list_create",
  "list_update",
  "list_delete",
  "unknown",
] as const;
export const POLICY_CHANGE_FIELDS = [
  "enabled",
  "name",
  "description",
  "selector",
  "conditions",
  "risk_category",
  "severity",
  "reason_code",
  "local_review",
  "list_type",
  "match_kind",
  "pattern",
] as const;
export const POLICY_CHANGE_AUDIT_STATUSES = [
  "recorded",
  "audit_unavailable",
  "unknown",
] as const;
const POLICY_CHANGE_BUILTIN_IDS = [
  "builtin.destructive_change",
  "builtin.sensitive_read",
  "builtin.data_exfiltration",
  "builtin.remote_code_execution",
  "builtin.privilege_change",
  "builtin.security_config_change",
  "builtin.destructive_database_change",
] as const;

export interface ToolRiskPolicyChangeRecord {
  id: number;
  at: number;
  policyVersion: number;
  actor: (typeof POLICY_CHANGE_ACTORS)[number];
  kind: (typeof POLICY_CHANGE_KINDS)[number];
  targetId: string | null;
  changedFields: (typeof POLICY_CHANGE_FIELDS)[number][];
  auditStatus: (typeof POLICY_CHANGE_AUDIT_STATUSES)[number];
}

export function projectToolRiskPolicyChanges(
  value: unknown,
): ToolRiskPolicyChangeRecord[] {
  if (!Array.isArray(value)) return [];
  const fixed = <T extends string>(
    input: unknown,
    allowed: readonly T[],
    fallback: T,
  ): T => allowed.find((item) => item === input) ?? fallback;
  const validNumber = (input: unknown): input is number =>
    typeof input === "number" && Number.isSafeInteger(input) && input >= 0;
  return value.flatMap((row: unknown) => {
    if (!row || typeof row !== "object" || Array.isArray(row)) return [];
    const record = row as Record<string, unknown>;
    if (
      !validNumber(record.id) ||
      !validNumber(record.at) ||
      !validNumber(record.policyVersion)
    )
      return [];
    const target = record.targetId;
    const targetId =
      target === null
        ? null
        : typeof target === "string" &&
            (POLICY_CHANGE_BUILTIN_IDS.some((id) => id === target) ||
              /^(?:custom|shell-list)-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
                target,
              ))
          ? target
          : "<unknown_target>";
    return [
      {
        id: record.id,
        at: record.at,
        policyVersion: record.policyVersion,
        actor: fixed(record.actor, POLICY_CHANGE_ACTORS, "unknown"),
        kind: fixed(record.kind, POLICY_CHANGE_KINDS, "unknown"),
        targetId,
        changedFields: Array.isArray(record.changedFields)
          ? [
              ...new Set(
                record.changedFields.filter(
                  (field): field is (typeof POLICY_CHANGE_FIELDS)[number] =>
                    POLICY_CHANGE_FIELDS.some((allowed) => allowed === field),
                ),
              ),
            ]
          : [],
        auditStatus: fixed(
          record.auditStatus,
          POLICY_CHANGE_AUDIT_STATUSES,
          "unknown",
        ),
      },
    ];
  });
}
