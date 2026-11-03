import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { getSecurityNotificationCopy } from "./securityNotificationCopy";

describe("security notification copy", () => {
  it.each([
    [
      "redact_restore",
      "Sensitive Information Tokenized",
      "Claude request detected “GitHub Access Token (HIGH)”. Replaced with local references before sending.",
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

  it.each([
    [
      "en",
      en,
      "{{app}} request detected “{{rule}}”. Replaced with local references before sending.",
    ],
    ["zh", zh, "{{app}} 请求命中“{{rule}}”，发送前已替换为本地引用。"],
    [
      "ja",
      ja,
      "{{app}} の要求で「{{rule}}」を検出し、送信前にローカル参照に置換しました。",
    ],
    ["zh-TW", zhTW, "{{app}} 請求命中「{{rule}}」，傳送前已替換為本機參照。"],
  ] as const)(
    "keeps the %s tokenization notice without a restoration disclaimer",
    (_language, locale, body) => {
      const copy = getSecurityNotificationCopy(
        "redact_restore",
        "Codex",
        "Synthetic rule (HIGH)",
        (key, options) => {
          const message =
            key === "security.notifyRedactRestoreTitle"
              ? locale.security.notifyRedactRestoreTitle
              : locale.security.notifyRedactRestoreBody;
          return message
            .replace("{{app}}", options?.app ?? "")
            .replace("{{rule}}", options?.rule ?? "");
        },
      );

      expect(copy).toEqual({
        title: locale.security.notifyRedactRestoreTitle,
        body: body
          .replace("{{app}}", "Codex")
          .replace("{{rule}}", "Synthetic rule (HIGH)"),
      });
    },
  );
});
