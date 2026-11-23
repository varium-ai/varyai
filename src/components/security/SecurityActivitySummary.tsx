import { useTranslation } from "react-i18next";
import { resolveUsageRange } from "@/lib/usageRange";
import { useSecurityEventsSummary } from "@/hooks/useSecurityEvents";
import { SecurityMetric, securityMetricOrder } from "./SecurityMetric";

export function SecurityActivitySummary() {
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

  return (
    <section aria-label={t("securityStats.today")}>
      <h2 className="stats-eyebrow">{t("securityStats.today")}</h2>
      <dl className="stats-security-grid">
        {(["events", ...securityMetricOrder] as const).map((metric) => (
          <SecurityMetric
            key={metric}
            metric={metric}
            value={display(
              metric === "events" ? data?.total : data?.[metric],
              summary.isLoading,
            )}
            className="stats-surface"
          />
        ))}
      </dl>
    </section>
  );
}
