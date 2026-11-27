import type { RoutingTrace } from "@/lib/api/routingTrace";

export type ClassificationBadgeTone =
  | "easy"
  | "hard"
  | "matched"
  | "neutral"
  | "failed";

export interface RoutingClassificationDisplay {
  tone: ClassificationBadgeTone;
  badgeKey: string;
  detailKey: string;
}

const KNOWN_CLASSIFICATION_OUTCOMES = new Set([
  "matched",
  "no_match",
  "no_rules",
  "privacy",
  "input_limit",
  "output_limit",
  "invalid_output",
  "request_failed",
  "unavailable",
  "timeout",
  "no_user_text",
]);

/** Maps recorded trace classification outcome and difficulty to localized stage badge and footer keys. */
export function resolveRoutingClassification(
  trace?: Pick<RoutingTrace, "classificationOutcome" | "classificationResult">,
): RoutingClassificationDisplay | undefined {
  const outcome = trace?.classificationOutcome;
  if (!outcome) return undefined;

  if (outcome === "matched") {
    if (trace.classificationResult === "easy") {
      return {
        tone: "easy",
        badgeKey: "routingPage.classificationBadge.easy",
        detailKey: "routingPage.classificationResult.easy",
      };
    }
    if (trace.classificationResult === "hard") {
      return {
        tone: "hard",
        badgeKey: "routingPage.classificationBadge.hard",
        detailKey: "routingPage.classificationResult.hard",
      };
    }
    return {
      tone: "matched",
      badgeKey: "routingPage.classificationBadge.matched",
      detailKey: "routingPage.classificationOutcome.matched",
    };
  }

  if (outcome === "no_match") {
    return {
      tone: "neutral",
      badgeKey: "routingPage.classificationBadge.no_match",
      detailKey: "routingPage.classificationOutcome.no_match",
    };
  }

  return {
    tone: "failed",
    badgeKey: "routingPage.classificationBadge.failed",
    detailKey: KNOWN_CLASSIFICATION_OUTCOMES.has(outcome)
      ? `routingPage.classificationOutcome.${outcome}`
      : "routingPage.classificationBadge.failed",
  };
}
