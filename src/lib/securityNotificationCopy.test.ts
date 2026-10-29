import { describe, expect, it } from "vitest";
import { getSecurityNotificationCopy } from "./securityNotificationCopy";

describe("security notification copy", () => {
  it.each([
    [
      "redact_restore",
      "Sensitive Information Tokenized",
      "Claude request detected “GitHub Access Token (HIGH)”. Replaced with local references before sending; response restoration is not confirmed.",
    ],
    [
      "block",
      "Request Blocked",
      "Claude request detected “GitHub Access Token (HIGH)”. Request not sent.",
    ],
    [
      "redact",
      "Sensitive Information Hidden",
      "Claude request detected “GitHub Access Token (HIGH)”. Hidden before sending.",
    ],
    [
      "audit",
      "Sensitive Information Detected",
      "Claude request detected “GitHub Access Token (HIGH)”. Request sent unchanged.",
    ],
  ] as const)("uses the documented copy for %s", (action, title, body) => {
    const copy = getSecurityNotificationCopy(
      action,
      "Claude",
      "GitHub Access Token (HIGH)",
      (key, options) => {
        const messages: Record<string, string> = {
          "security.notifyBlockTitle": "Request Blocked",
          "security.notifyBlockBody":
            "{{app}} request detected “{{rule}}”. Request not sent.",
          "security.notifyRedactTitle": "Sensitive Information Hidden",
          "security.notifyRedactBody":
            "{{app}} request detected “{{rule}}”. Hidden before sending.",
          "security.notifyAuditTitle": "Sensitive Information Detected",
          "security.notifyAuditBody":
            "{{app}} request detected “{{rule}}”. Request sent unchanged.",
        };
        return (messages[key] ?? options?.defaultValue ?? key)
          .replace("{{app}}", options?.app ?? "")
          .replace("{{rule}}", options?.rule ?? "");
      },
    );

    expect(copy).toEqual({ title, body });
  });
});
