import type { ToolRiskEventRecord } from "@/types/toolRisk";

export type ToolRiskEventGroup = {
  representative: ToolRiskEventRecord;
  members: readonly ToolRiskEventRecord[];
};

function groupKey(event: ToolRiskEventRecord): string | null {
  if (
    (event.verdict !== "uncertain" && event.verdict !== "low_risk") ||
    typeof event.commandSummary !== "string" ||
    event.commandSummary.trim().length === 0 ||
    !Number.isFinite(event.timestamp) ||
    !Number.isSafeInteger(event.timestamp) ||
    !Number.isFinite(new Date(event.timestamp).getTime())
  ) {
    return null;
  }

  const fields = [
    event.verdict,
    event.reasonCode,
    event.policyVersion,
    [...event.ruleIds].sort(),
    event.source,
    event.stage,
    event.clientCategory,
    event.operationCategory,
    event.integrityStatus,
    event.riskCategory,
    event.severity,
    event.commandClass,
    event.detectionScope,
    event.commandSummary,
    Math.floor(event.timestamp / 60_000),
  ];

  // Tag values so JSON serialization cannot collapse undefined into null.
  return JSON.stringify(
    fields.map((value) =>
      value === undefined ? ["undefined"] : ["value", value],
    ),
  );
}

/** Page-local similarity only; members retain original records and define the count. */
export function groupToolRiskEvents(
  events: readonly ToolRiskEventRecord[],
): ToolRiskEventGroup[] {
  const groups: ToolRiskEventGroup[] = [];
  const membersByKey = new Map<string, ToolRiskEventRecord[]>();

  for (const event of events) {
    const key = groupKey(event);
    const members = key === null ? undefined : membersByKey.get(key);
    if (members) {
      members.push(event);
      continue;
    }

    const newMembers = [event];
    groups.push({ representative: event, members: newMembers });
    if (key !== null) {
      membersByKey.set(key, newMembers);
    }
  }

  return groups;
}
