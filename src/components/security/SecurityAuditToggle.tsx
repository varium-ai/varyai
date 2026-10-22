import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useSecurityProtection } from "@/hooks/useSecurityProtection";

export function SecurityAuditToggle({ appType }: { appType: string }) {
  const { t } = useTranslation();
  const { settings, connected, isLoading, isUpdating, setEnabled } =
    useSecurityProtection(appType);
  const enabled = settings?.enabled ?? false;

  const detailKey = !settings
    ? "homeAudit.scope"
    : !enabled
      ? "homeAudit.disabledDetail"
      : connected
        ? appType === "pi"
          ? "piRouting.description"
          : "homeAudit.protectionDetail"
        : "homeAudit.disconnectedDetail";

  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            data-testid="security-audit-toggle"
            className="flex shrink-0 items-center gap-2.5 rounded-xl border border-slate-200/80 bg-slate-50/60 px-3 py-1.5 transition-colors hover:bg-slate-100/70 dark:border-border dark:bg-muted/40 dark:hover:bg-muted"
          >
            <span className="text-xs font-semibold text-slate-700 dark:text-foreground">
              {t("homeAudit.label")}
            </span>
            <Switch
              aria-label={t("homeAudit.label")}
              checked={enabled}
              disabled={isLoading || isUpdating || !settings}
              onCheckedChange={async (checked) => {
                if (!settings) return;
                try {
                  await setEnabled(checked);
                } catch {
                  // The shared mutation reports the error; retain the persisted state.
                }
              }}
            />
          </div>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          align="center"
          sideOffset={8}
          className="w-[min(560px,calc(100vw-2rem))] max-w-[calc(100vw-2rem)] whitespace-normal border border-slate-200 bg-white px-4 py-3 text-left text-sm leading-6 text-slate-700 shadow-lg dark:border-border dark:bg-card dark:text-foreground"
        >
          {t(detailKey)}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
