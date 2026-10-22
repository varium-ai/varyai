import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@/lib/api/notifications";
import { useSecurityEventBridge } from "@/hooks/useSecurityEventBridge";
import { securityApi } from "@/lib/api/security";
import { getBuiltinRuleName } from "@/lib/securityRuleLocalization";
import { getSecurityNotificationCopy } from "@/lib/securityNotificationCopy";
import { getAppLabel } from "@/config/appConfig";
import { reportFrontendError } from "@/lib/frontendLogger";
import type { AuditLogRecord } from "@/types/security";

const PAGE_SIZE = 100;
const NOTIFICATION_WINDOW_MS = 60_000;
const MAX_NOTIFICATION_GROUPS = 1_000;

function createSession() {
  return {
    mountedAt: Date.now(),
    lastSeenId: 0,
    running: false,
    dirty: false,
    disposed: false,
    notificationWindows: new Map<string, { expiresAt: number }>(),
  };
}

/** Notification catch-up cursor for this mounted window only. */
export function SecuritySystemNotifier() {
  const { t } = useTranslation();
  const translate = useRef(t);
  translate.current = t;
  const session = useRef(createSession());
  const permission = useRef<Promise<boolean>>(Promise.resolve(false));

  useEffect(() => {
    if (session.current.disposed) {
      session.current = createSession();
    }
    const state = session.current;
    permission.current = (async () => {
      try {
        const granted =
          (await isPermissionGranted()) ||
          (await requestPermission()) === "granted";
        if (!granted && !state.disposed)
          reportFrontendError("security_audit_permission_denied", "denied");
        return granted;
      } catch {
        reportFrontendError("security_audit_permission", "failed");
        return false;
      }
    })();
    return () => {
      state.disposed = true;
    };
  }, []);

  const onRecorded = useCallback(() => {
    const state = session.current;
    if (state.disposed) return;
    if (state.running) {
      state.dirty = true;
      return;
    }
    state.running = true;
    void (async () => {
      try {
        do {
          state.dirty = false;
          while (!state.disposed) {
            let records: AuditLogRecord[];
            try {
              records = await securityApi.getAuditNotificationBatch(
                state.lastSeenId,
                state.mountedAt,
                PAGE_SIZE,
              );
            } catch {
              reportFrontendError(
                "security_audit_notification_fetch",
                "failed",
                `sinceId=${state.lastSeenId} limit=${PAGE_SIZE}`,
              );

              state.dirty = false;
              return;
            }
            if (state.disposed) return;
            for (const record of records) {
              if (state.disposed) return;
              if (record.id <= state.lastSeenId) continue;
              if (record.actionTaken === "allow") {
                state.lastSeenId = record.id;
                continue;
              }
              // A single request may contain several different secrets matching
              // the same rule. Keep all audit rows, but rate-limit both delivery
              // channels together; action/severity changes still notify at once.
              const notificationKey = JSON.stringify([
                record.appType,
                record.ruleId,
                record.actionTaken,
                record.riskLevel,
              ]);
              const now = performance.now();
              for (const [key, window] of state.notificationWindows) {
                if (window.expiresAt <= now)
                  state.notificationWindows.delete(key);
              }
              if (state.notificationWindows.has(notificationKey)) {
                state.lastSeenId = record.id;
                continue;
              }
              if (state.notificationWindows.size >= MAX_NOTIFICATION_GROUPS) {
                const oldest = state.notificationWindows.keys().next().value;
                if (oldest !== undefined)
                  state.notificationWindows.delete(oldest);
              }
              const notificationWindow = {
                expiresAt: now + NOTIFICATION_WINDOW_MS,
              };
              state.notificationWindows.set(
                notificationKey,
                notificationWindow,
              );
              const rule = `${getBuiltinRuleName(translate.current, record.ruleId, record.ruleName)} (${record.riskLevel})`;
              const { title, body } = getSecurityNotificationCopy(
                record.actionTaken === "block" ||
                  record.actionTaken === "redact"
                  ? record.actionTaken
                  : "audit",
                getAppLabel(record.appType),
                rule,
                translate.current,
              );
              try {
                const showToast =
                  record.actionTaken === "block" ? toast.error : toast.warning;
                showToast(title, {
                  description: body,
                  id: `security-audit-${notificationKey}`,
                  closeButton: true,
                });
              } catch {
                reportFrontendError(
                  "security_audit_toast",
                  "failed",
                  `id=${record.id}`,
                );
              }
              state.lastSeenId = record.id;

              void (async () => {
                try {
                  const granted = await permission.current;
                  if (
                    granted &&
                    !state.disposed &&
                    state.notificationWindows.get(notificationKey) ===
                      notificationWindow
                  ) {
                    await sendNotification({ title, body });
                  }
                } catch {
                  reportFrontendError(
                    "security_audit_notification_send",
                    "failed",
                    `id=${record.id}`,
                  );
                }
              })();
            }
            if (records.length < PAGE_SIZE) break;
          }
        } while (!state.disposed && state.dirty);
      } finally {
        state.running = false;
      }
    })();
  }, []);

  useSecurityEventBridge(onRecorded);
  return null;
}
