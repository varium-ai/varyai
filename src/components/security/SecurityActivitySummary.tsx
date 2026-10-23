import { ChevronRight, Eye, ShieldAlert, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { resolveUsageRange } from "@/lib/usageRange";
import { useSecurityEventsSummary } from "@/hooks/useSecurityEvents";

export function SecurityActivitySummary({
  onOpenSecurity,
}: {
  onOpenSecurity: () => void;
}) {
  const { t, i18n } = useTranslation();
  const { startDate } = resolveUsageRange({ preset: "today" });
  const startTimestamp = startDate * 1000;
  const summary = useSecurityEventsSummary({
    view: "security_events",
    startTimestamp,
  });
  const data = summary.isError ? undefined : summary.data;
  const display = (value: number | undefined, loading: boolean) =>
    loading
      ? "…"
      : value === undefined
        ? "—"
        : value.toLocaleString(i18n.language);

  const cards = [
    {
      key: "protectiveBlocked",
      icon: ShieldAlert,
      value: data?.protectiveBlocked,
      color: "text-rose-600",
    },
    {
      key: "redacted",
      icon: ShieldCheck,
      value: data?.redacted,
      color: "text-blue-600",
    },
    {
      key: "audited",
      icon: Eye,
      value: data?.warnings,
      color: "text-amber-600",
    },
  ] as const;

  return (
    <section className="mb-6 space-y-3" aria-label={t("securityStats.today")}>
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-medium text-slate-500 dark:text-muted-foreground">
          {t("securityStats.today")}
        </h2>
        <button
          type="button"
          onClick={onOpenSecurity}
          className="flex items-center gap-1 text-xs font-semibold text-blue-600 hover:text-blue-700 dark:text-blue-400"
        >
          {t("securityStats.viewLogs")}
          <ChevronRight className="h-4 w-4" />
        </button>
      </div>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div className="rounded-2xl border border-blue-200 bg-white p-4 shadow-sm dark:border-blue-500/40 dark:bg-card">
          <span className="text-xs text-muted-foreground">
            {t("securityStats.events")}
          </span>
          <div className="mt-2 text-2xl font-semibold tabular-nums text-blue-600">
            {display(data?.total, summary.isLoading)}
          </div>
        </div>
        {cards.map(({ key, icon: Icon, value, color }) => (
          <div
            key={key}
            className="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm dark:border-border dark:bg-card"
          >
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Icon className={`h-3.5 w-3.5 ${color}`} />
              {t(`securityStats.${key}`)}
            </span>
            <div
              className={`mt-2 text-2xl font-semibold tabular-nums ${color}`}
            >
              {display(value, summary.isLoading)}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}
