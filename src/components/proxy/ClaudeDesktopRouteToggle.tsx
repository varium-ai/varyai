import { Loader2, Radio } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
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
    takeoverStatus,
    setTakeoverForApp,
    isPending,
    isInitialStatusPending,
  } = useProxyStatus();
  const active = isRunning && takeoverStatus?.["claude-desktop"] === true;
  const busy = disabled || isPending || isInitialStatusPending;
  const toggle = (enabled: boolean) => {
    void setTakeoverForApp({ appType: "claude-desktop", enabled }).catch(
      () => {},
    );
  };
  const control = (
    <Switch
      checked={active}
      onCheckedChange={toggle}
      disabled={busy}
      aria-label={t("claudeDesktop.route.ariaLabel", {
        defaultValue: "Claude Desktop 本地路由",
      })}
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
    >
      {busy ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            active
              ? "text-emerald-500 status-heartbeat"
              : "text-muted-foreground",
          )}
        />
      )}
      {control}
    </div>
  );
}
