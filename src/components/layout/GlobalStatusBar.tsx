import { useTranslation } from "react-i18next";
import { APP_DISPLAY_NAME } from "@/config/brand";
import { useSecurityProtection } from "@/hooks/useSecurityProtection";
import type { ProxyStatus } from "@/types/proxy";

const DEFAULT_PROXY_PORT = 15721;

type AuditState = "running" | "disabled" | "notConnected" | "unavailable";
type GatewayState = "running" | "stopped" | "unavailable";

interface GlobalStatusBarProps {
  activeApp: string;
  isProxyRunning: boolean;
  proxyStatus?: ProxyStatus;
}

export function GlobalStatusBar({
  activeApp,
  isProxyRunning,
  proxyStatus,
}: GlobalStatusBarProps) {
  const { t } = useTranslation();
  const protection = useSecurityProtection(activeApp);
  const auditState: AuditState =
    protection.isLoading && !protection.settings
      ? "unavailable"
      : !protection.settings
        ? "unavailable"
        : !protection.connected
          ? "notConnected"
          : protection.settings.enabled
            ? "running"
            : "disabled";
  const gatewayState: GatewayState = !proxyStatus
    ? "unavailable"
    : isProxyRunning
      ? "running"
      : "stopped";
  const gatewayAddress = `${proxyStatus?.address ?? "127.0.0.1"}:${proxyStatus?.port ?? DEFAULT_PROXY_PORT}`;

  return (
    <footer
      aria-label={t("globalStatus.label")}
      data-testid="global-status-bar"
      className="flex shrink-0 flex-col items-center justify-between gap-2 border-t border-slate-200 bg-white px-6 py-3 text-xs text-slate-500 select-none dark:border-border dark:bg-card dark:text-muted-foreground sm:flex-row"
    >
      <div className="flex min-w-0 items-center gap-2">
        <span className="shrink-0 font-bold text-slate-800 dark:text-foreground">
          {APP_DISPLAY_NAME}
        </span>
        <span aria-hidden="true">·</span>
        <span className="truncate">{t("globalStatus.tagline")}</span>
      </div>

      <div className="flex items-center gap-4 font-mono text-[11px] text-slate-400 dark:text-muted-foreground">
        <span aria-label={t(`globalStatus.gatewayStates.${gatewayState}`)}>
          {t("globalStatus.gateway")}: {gatewayAddress}
        </span>
        <span aria-hidden="true">·</span>
        <span aria-live="polite">
          {t("globalStatus.audit")}:{" "}
          <span
            className={
              auditState === "running"
                ? "font-semibold text-emerald-600 dark:text-emerald-400"
                : auditState === "disabled"
                  ? "font-semibold text-rose-600 dark:text-rose-400"
                  : "font-semibold text-amber-600 dark:text-amber-400"
            }
          >
            {t(`globalStatus.auditStates.${auditState}`)}
          </span>
        </span>
      </div>
    </footer>
  );
}
