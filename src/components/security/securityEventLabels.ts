import type { TFunction } from "i18next";
import type { SecurityEvent } from "@/types/securityEvents";
import { RISK_CATEGORIES, EVENT_REASON_CODES } from "@/types/toolRisk";
import { getBuiltinRuleName } from "@/lib/securityRuleLocalization";

export function securityEventRiskLabel(
  t: TFunction,
  level: string | null,
  exact = false,
): string {
  if (level === "CRITICAL")
    return t(exact ? "security.riskCritical" : "security.levelHighShort");
  if (level === "HIGH") return t("security.levelHighShort");
  if (level === "MEDIUM") return t("security.levelMediumShort");
  if (level === "LOW") return t("security.levelLowShort");
  return t("securityEvents.unknown");
}
export function toolVerdictLabel(
  t: TFunction,
  verdict?: string | null,
): string {
  if (!verdict) return t("securityEvents.notRecorded");
  if (["risky", "unchecked", "uncertain", "low_risk"].includes(verdict))
    return t(`toolRisk.verdict.${verdict}`);
  return t("securityEvents.unknown");
}
export function toolCategoryLabel(
  t: TFunction,
  category?: string | null,
): string {
  return RISK_CATEGORIES.includes(category as never)
    ? t(`toolRisk.category.${category}`)
    : t(category ? "securityEvents.unknown" : "securityEvents.notRecorded");
}
export function toolSeverityLabel(
  t: TFunction,
  severity?: string | null,
): string {
  if (severity === "high" || severity === "medium")
    return t(`toolRisk.severity.${severity}`);
  if (severity === "none") return t("securityEvents.severityNone");
  return t(severity ? "securityEvents.unknown" : "securityEvents.notRecorded");
}
export function toolReasonLabel(t: TFunction, reason?: string | null): string {
  return EVENT_REASON_CODES.includes(reason as never)
    ? t(`toolRisk.reason.${reason}`)
    : t("toolRisk.reasonUnknown");
}
export function securityEventName(t: TFunction, event: SecurityEvent): string {
  return event.eventType === "dlp"
    ? getBuiltinRuleName(t, event.record.ruleId, event.record.ruleName)
    : toolCategoryLabel(t, event.record.riskCategory);
}
export function securityEventResultLabel(
  t: TFunction,
  event: SecurityEvent,
): string {
  if (event.eventType === "tool_risk")
    return event.record.verdict === "risky"
      ? t("securityEvents.resultWarning")
      : toolVerdictLabel(t, event.record.verdict);
  const keys: Record<string, string> = {
    redact: "security.resultRedacted",
    redact_restore: "security.resultRedactRestore",
    block: "security.resultBlocked",
    audit: "security.resultAlert",
    allow: "security.actionAllow",
  };
  return t(
    Object.prototype.hasOwnProperty.call(keys, event.record.actionTaken)
      ? keys[event.record.actionTaken]
      : "securityEvents.unknown",
  );
}
export function securityEventOutboundLabel(
  t: TFunction,
  event: SecurityEvent,
): string {
  if (event.eventType === "tool_risk")
    return t("securityEvents.toolObservation");
  if (event.record.actionTaken === "block")
    return t("security.outboundBlocked");
  if (
    ["audit", "redact", "redact_restore", "allow"].includes(
      event.record.actionTaken,
    )
  )
    return t("security.outboundSent");
  return t("securityEvents.unknown");
}
