import type { AuditAction } from "@/types/security";

type NotificationAction = Exclude<AuditAction, "allow">;

type TranslateSecurityCopy = (
  key: string,
  options?: { defaultValue?: string; app?: string; rule?: string },
) => string;

const copyByAction: Record<
  NotificationAction,
  {
    titleKey: string;
    bodyKey: string;
    defaultTitle: string;
    defaultBody: string;
  }
> = {
  block: {
    titleKey: "security.notifyBlockTitle",
    bodyKey: "security.notifyBlockBody",
    defaultTitle: "Request Blocked",
    defaultBody: "{{app}} request detected “{{rule}}”. Request not sent.",
  },
  redact: {
    titleKey: "security.notifyRedactTitle",
    bodyKey: "security.notifyRedactBody",
    defaultTitle: "Sensitive Information Hidden",
    defaultBody: "{{app}} request detected “{{rule}}”. Hidden before sending.",
  },
  audit: {
    titleKey: "security.notifyAuditTitle",
    bodyKey: "security.notifyAuditBody",
    defaultTitle: "Sensitive Information Detected",
    defaultBody: "{{app}} request detected “{{rule}}”. Request sent unchanged.",
  },
};

export function getSecurityNotificationCopy(
  action: NotificationAction,
  app: string,
  rule: string,
  translate: TranslateSecurityCopy,
) {
  const copy = copyByAction[action];
  return {
    title: translate(copy.titleKey, { defaultValue: copy.defaultTitle }),
    body: translate(copy.bodyKey, {
      defaultValue: copy.defaultBody,
      app,
      rule,
    }),
  };
}
