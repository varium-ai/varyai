import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import "./security-metric.css";

export const securityMetricOrder = [
  "protectiveBlocked",
  "redacted",
  "warnings",
] as const;

type Metric = "events" | (typeof securityMetricOrder)[number];

export function SecurityMetric({
  metric,
  value,
  className,
}: {
  metric: Metric;
  value: string;
  className?: string;
}) {
  const { t } = useTranslation();
  return (
    <div className={cn("security-metric", className)} data-metric={metric}>
      <dt>{t(`securityStats.${metric}`)}</dt>
      <dd>{value}</dd>
    </div>
  );
}
