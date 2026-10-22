import { Loader2, Radio } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
import { TAKEOVER_APP_IDS } from "@/config/appConfig";

interface ClaudeDesktopRouteToggleProps {
  className?: string;
  variant?: "toolbar" | "settings";
  disabled?: boolean;
}

export function ClaudeDesktopRouteToggle({
  className,
  variant = "toolbar",
  disabled = false,
}: ClaudeDesktopRouteToggleProps) {
  const { t } = useTranslation();
  const {
    isRunning,
    status,
    takeoverStatus,
    startProxyServer,
    stopProxyServer,
    isPending,
    isInitialStatusPending,
  } = useProxyStatus();

  const isBusy = disabled || isPending || isInitialStatusPending;
  const otherTakeoverActive =
    takeoverStatus?.pi || TAKEOVER_APP_IDS.some((app) => takeoverStatus?.[app]);
  const routeAddress = status?.address ?? "127.0.0.1";
  const routePort = status?.port ?? 15721;

  const handleToggle = async (checked: boolean) => {
    try {
      if (checked) {
        await startProxyServer();
        return;
      }

      if (otherTakeoverActive) {
        toast.warning(
          t("claudeDesktop.route.stopBlockedByTakeover", {
            defaultValue:
              "其它应用正在使用代理接管。请先在设置中关闭对应应用接管，再停止本地路由。",
          }),
          { duration: 5000 },
        );
        return;
      }

      await stopProxyServer();
    } catch (error) {
      console.error("[ClaudeDesktopRouteToggle] Toggle route failed:", error);
    }
  };

  const tooltipText = isRunning
    ? t("claudeDesktop.route.tooltip.active", {
        address: routeAddress,
        port: routePort,
        defaultValue: `Claude Desktop 本地路由已开启 - ${routeAddress}:${routePort}`,
      })
    : t("claudeDesktop.route.tooltip.inactive", {
        address: routeAddress,
        port: routePort,
        defaultValue: `开启 Claude Desktop 本地路由，用于需要模型映射或格式转换的供应商。当前配置地址：${routeAddress}:${routePort}`,
      });

  const control = (
    <Switch
      checked={isRunning}
      onCheckedChange={handleToggle}
      disabled={isBusy}
      aria-label={t("claudeDesktop.route.ariaLabel", {
        defaultValue: "Claude Desktop 本地路由",
      })}
      title={tooltipText}
      className={variant === "settings" ? className : undefined}
    />
  );

  if (variant === "settings") return control;

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 h-8 rounded-lg bg-muted/50 transition-all",
        className,
      )}
      title={tooltipText}
    >
      {isBusy ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            isRunning
              ? "text-emerald-500 status-heartbeat"
              : "text-muted-foreground",
          )}
        />
      )}
      {control}
    </div>
  );
}
