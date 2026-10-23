import {
  COMMAND_CLASSES,
  KNOWN_REASON_CODES,
  OPERATION_TYPES,
  RISK_CATEGORIES,
  type ToolRiskEventRecord,
  type ToolRiskRuleSeed,
} from "@/types/toolRisk";

export function toolRiskRuleSeed(
  record: ToolRiskEventRecord,
): ToolRiskRuleSeed {
  const commandClass = COMMAND_CLASSES.includes(record.commandClass as never)
    ? record.commandClass!
    : record.reasonCode === "unclassified_command"
      ? "unknown"
      : COMMAND_CLASSES.includes(record.reasonCode as never)
        ? (record.reasonCode as NonNullable<ToolRiskRuleSeed["commandClass"]>)
        : undefined;
  return {
    ...(OPERATION_TYPES.includes(record.operationCategory as never)
      ? {
          operationCategory:
            record.operationCategory as ToolRiskRuleSeed["operationCategory"],
        }
      : {}),
    ...(commandClass ? { commandClass } : {}),
    ...(RISK_CATEGORIES.includes(record.riskCategory as never)
      ? {
          riskCategory: record.riskCategory as ToolRiskRuleSeed["riskCategory"],
        }
      : {}),
    ...(record.severity === "medium" || record.severity === "high"
      ? { severity: record.severity }
      : {}),
    ...(KNOWN_REASON_CODES.includes(record.reasonCode as never)
      ? { reasonCode: record.reasonCode as ToolRiskRuleSeed["reasonCode"] }
      : {}),
  };
}
