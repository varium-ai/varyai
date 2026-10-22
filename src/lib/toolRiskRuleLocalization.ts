import type { TFunction } from "i18next";

/**
 * Localize built-in rules by stable ID, using backend names only as a fallback.
 * Keep custom or unknown names literal rather than treating them as built-in rules.
 */
const TOOL_RISK_BUILTIN_RULE_IDS = new Set([
  "builtin.destructive_change",
  "builtin.sensitive_read",
  "builtin.data_exfiltration",
  "builtin.remote_code_execution",
  "builtin.privilege_change",
  "builtin.security_config_change",
  "builtin.destructive_database_change",
]);

export function getToolRiskRuleName(
  t: TFunction,
  rule: { id: string; origin: string; name: string },
) {
  if (rule.origin !== "builtin" || !TOOL_RISK_BUILTIN_RULE_IDS.has(rule.id)) {
    return rule.name;
  }
  const slug = rule.id.slice("builtin.".length);
  return t(`toolRisk.builtinRule.${slug}.name`, { defaultValue: rule.name });
}
