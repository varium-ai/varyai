import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { BellOff, ChevronRight, Loader2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  notificationPermissionsApi,
  type NotificationPermissionStatus,
} from "@/lib/api/notificationPermissions";

function warning(status: NotificationPermissionStatus | null) {
  if (!status || status.authorization === "unknown") return null;
  if (["denied", "notDetermined"].includes(status.authorization))
    return "permission";
  return status.alertsEnabled === false ? "alerts" : null;
}

export function NotificationPermissionBanner() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<NotificationPermissionStatus | null>(
    null,
  );
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);

  const refresh = useCallback(async () => {
    const current = ++generation.current;
    try {
      const next = await notificationPermissionsApi.getStatus();
      if (current !== generation.current) return;
      setStatus(next);
      if (next.authorization !== "unknown" && !warning(next))
        setDismissed(null);
    } catch {
      // An unsupported environment or query failure is not proof of denial.
      if (current === generation.current) setStatus(null);
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

  const reason = warning(status);
  if (!status || !reason || dismissed === reason) return null;

  const enable = async () => {
    setBusy(true);
    try {
      if (status.authorization === "notDetermined" && status.canRequest)
        await notificationPermissionsApi.request();
      else await notificationPermissionsApi.openSettings();
      await refresh();
    } catch {
      toast.error(t("settings.permissions.actionFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      role="status"
      className="pointer-events-auto w-full max-w-2xl shrink-0 rounded-xl border border-amber-200/80 bg-amber-50/90 shadow-lg shadow-amber-900/10 backdrop-blur-md animate-slide-up dark:border-amber-900/70 dark:bg-amber-950/80 dark:shadow-black/20"
    >
      <div className="flex items-center gap-3 px-4 py-3 sm:px-5">
        <BellOff
          aria-hidden="true"
          className="h-5 w-5 shrink-0 text-amber-600 dark:text-amber-400"
        />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-amber-950 dark:text-amber-50">
            {t("settings.permissions.banner.title")}
          </h3>
          <p className="mt-0.5 text-xs text-amber-800/80 dark:text-amber-100/75">
            {t(
              `settings.permissions.banner.${reason === "alerts" ? "alertsOff" : "description"}`,
            )}
          </p>
        </div>
        {(status.canRequest || status.canOpenSettings) && (
          <Button
            variant="ghost"
            size="sm"
            disabled={busy}
            onClick={() => void enable()}
            className="h-8 shrink-0 px-2.5 text-amber-700 hover:bg-amber-50 hover:text-amber-800 dark:text-amber-300 dark:hover:bg-amber-950/60 dark:hover:text-amber-200"
          >
            {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            {t("settings.permissions.banner.enable")}
            <ChevronRight aria-hidden="true" className="h-4 w-4" />
          </Button>
        )}
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("common.close")}
          onClick={() => setDismissed(reason)}
          className="h-8 w-8 shrink-0 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:text-muted-foreground dark:hover:bg-muted dark:hover:text-foreground"
        >
          <X aria-hidden="true" className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
