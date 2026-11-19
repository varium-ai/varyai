import { useTranslation } from "react-i18next";
import { resolveUsageRange } from "@/lib/usageRange";
import { useSecurityEventsSummary } from "@/hooks/useSecurityEvents";

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

  const cards = [
    ["events", data?.total, "text-primary"],
    ["protectiveBlocked", data?.protectiveBlocked, "text-destructive"],
    ["redacted", data?.redacted, "text-primary"],
    ["audited", data?.warnings, "text-[var(--warning)]"],
  ] as const;

  return (
    <section aria-label={t("securityStats.today")}>
      <h2 className="stats-eyebrow">{t("securityStats.today")}</h2>
      <dl className="stats-security-grid">
        {cards.map(([key, value, color]) => (
          <div key={key} className="stats-surface stats-event">
            <dt>{t(`securityStats.${key}`)}</dt>
            <dd className={color}>{display(value, summary.isLoading)}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
