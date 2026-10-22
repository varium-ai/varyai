import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { sendNotification } from "@/lib/api/notifications";
import { ExternalLink, Loader2, RefreshCw, Send } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  notificationPermissionsApi,
  type NotificationPermissionStatus,
} from "@/lib/api/notificationPermissions";

export function NotificationPermissionSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<NotificationPermissionStatus | null>(
    null,
  );
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);

  const refresh = useCallback(async () => {
    const current = ++generation.current;
    setLoading(true);
    try {
      const next = await notificationPermissionsApi.getStatus();
      if (current === generation.current) {
        setStatus(next);
        setFailed(false);
      }
    } catch {
      if (current === generation.current) {
        setStatus(null);
        setFailed(true);
      }
    } finally {
      if (current === generation.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    const unlisten = isTauri()
      ? getCurrentWindow()
          .onFocusChanged(({ payload }) => {
            if (payload) void refresh();
          })
          .catch(() => undefined)
      : undefined;
    return () => {
      generation.current++;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
      void unlisten?.then((dispose) => dispose?.());
    };
  }, [refresh]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch {
      toast.error(t("settings.permissions.actionFailed"));
    } finally {
      setBusy(false);
    }
  };

  const settingLabel = (value: boolean | null) =>
    t(
      `settings.permissions.${value === null ? "unavailable" : value ? "enabled" : "disabled"}`,
    );

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1 space-y-3">
          {status?.isDev && status.platform === "macos" ? (
            <p className="text-sm text-muted-foreground">
              {t("settings.permissions.devHint")}
            </p>
          ) : status?.authorization === "unknown" ? (
            <p className="text-sm text-muted-foreground">
              {t("settings.permissions.unsupportedHint")}
            </p>
          ) : null}
          {status &&
            (status.alertsEnabled !== null || status.soundEnabled !== null) && (
              <dl className="flex flex-wrap gap-x-6 gap-y-2 text-sm">
                <div className="flex gap-2">
                  <dt className="text-muted-foreground">
                    {t("settings.permissions.alerts")}
                  </dt>
                  <dd>{settingLabel(status.alertsEnabled)}</dd>
                </div>
                <div className="flex gap-2">
                  <dt className="text-muted-foreground">
                    {t("settings.permissions.sound")}
                  </dt>
                  <dd>{settingLabel(status.soundEnabled)}</dd>
                </div>
              </dl>
            )}
        </div>
        <span
          role="status"
          className="shrink-0 rounded-md bg-muted px-2 py-1 text-xs font-medium"
        >
          {loading
            ? t("settings.permissions.checking")
            : failed
              ? t("settings.permissions.loadFailed")
              : t(
                  `settings.permissions.status.${status?.authorization ?? "unknown"}`,
                )}
        </span>
      </div>
      <div className="flex flex-wrap gap-2">
        {status?.canRequest && (
          <Button
            size="sm"
            disabled={busy || loading}
            onClick={() =>
              void run(async () => {
                await notificationPermissionsApi.request();
                await refresh();
              })
            }
          >
            {t("settings.permissions.request")}
          </Button>
        )}
        {status?.canOpenSettings && (
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => void run(notificationPermissionsApi.openSettings)}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            {t("settings.permissions.openSettings")}
          </Button>
        )}
        <Button
          variant="outline"
          size="sm"
          disabled={busy || loading}
          onClick={() =>
            void run(async () => {
              await sendNotification({
                title: t("settings.permissions.testTitle"),
                body: t("settings.permissions.testBody"),
              });
              toast.success(t("settings.permissions.testSubmitted"));
            })
          }
        >
          <Send className="h-3.5 w-3.5" />
          {t("settings.permissions.test")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={busy || loading}
          onClick={() => void refresh()}
        >
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {t("settings.permissions.refresh")}
        </Button>
      </div>
    </div>
  );
}
