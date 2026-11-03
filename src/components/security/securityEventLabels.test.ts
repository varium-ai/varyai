import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import type { SecurityEvent } from "@/types/securityEvents";
import {
  securityEventOutboundLabel,
  securityEventResultLabel,
} from "./securityEventLabels";

const t = ((key: string) => key) as TFunction;
const event: SecurityEvent = {
  key: "dlp:1",
  eventType: "dlp",
  timestamp: 1,
  lastSeenAt: 1,
  triggerCount: 1,
  appType: "codex",
  sessionId: null,
  riskLevel: "HIGH",
  result: "redacted",
  record: {
    id: 1,
    timestamp: 1,
    appType: "codex",
    providerId: "synthetic",
    ruleId: "synthetic",
    ruleName: "Synthetic rule",
    ruleCategory: "custom",
    riskLevel: "HIGH",
    actionTaken: "redact_restore",
    matchedSnippetMasked: "***",
  },
};

describe("redact_restore audit labels", () => {
  it("labels request tokenization rather than unknown or restoration success", () => {
    expect(securityEventResultLabel(t, event)).toBe(
      "security.resultRedactRestore",
    );
    expect(securityEventOutboundLabel(t, event)).toBe("security.outboundSent");
  });
});
