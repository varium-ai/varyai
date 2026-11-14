import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { quotaPanelApi, type QuotaPoint } from "@/lib/api/quotaPanel";
import type { QuotaTier } from "@/types/subscription";

export function windowSeconds(name: string): number | null {
  const known: Record<string, number> = {
    five_hour: 18000,
    seven_day: 604800,
    weekly: 604800,
    daily: 86400,
    thirty_day: 2592000,
  };
  if (known[name]) return known[name];
  const part = name.split(" · ").at(-1) ?? name;
  const match = /^(\d+)(?:_|\s)?(hour|day|h|d|w|m|mo)s?$/.exec(part);
  if (!match) return null;
  const seconds =
    Number(match[1]) *
    ({
      hour: 3600,
      day: 86400,
      h: 3600,
      d: 86400,
      w: 604800,
      m: 60,
      mo: 2592000,
    }[match[2]] ?? 0);
  return seconds >= 60 && seconds <= 366 * 86400 ? seconds : null;
}
export function quotaCurve(points: QuotaPoint[], current: QuotaPoint): string {
  const start = current.resetsAt - current.span;
  let cycle: QuotaPoint[] = [];
  for (const point of [...points, current].sort((a, b) => a.at - b.at)) {
    if (
      point.resetsAt !== current.resetsAt ||
      point.at < start ||
      point.at > current.at ||
      !Number.isFinite(point.remaining)
    )
      continue;
    if (cycle.length && point.remaining > cycle[cycle.length - 1].remaining + 5)
      cycle = [];
    if (cycle.at(-1)?.at === point.at) cycle.pop();
    cycle.push(point);
  }
  return cycle
    .map(
      (point, index) =>
        `${index ? "L" : "M"}${Math.max(0, Math.min(200, ((point.at - start) / current.span) * 200)).toFixed(2)},${(30 * (1 - Math.max(0, Math.min(100, point.remaining)) / 100)).toFixed(2)}`,
    )
    .join(" ");
}
export function QuotaSparkline({
  scope,
  tier,
  sampledAt,
  label,
}: {
  scope: string;
  tier: QuotaTier;
  sampledAt: number;
  label: string;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const span = windowSeconds(tier.name);
  const reset = tier.resetsAt
    ? Math.floor(Date.parse(tier.resetsAt) / 1000)
    : NaN;
  const now = Math.floor(sampledAt / 1000);
  const enabled =
    !!span && Number.isFinite(reset) && reset > now && reset - span! <= now;
  const scopeKey = `${scope}:${tier.name}`;
  const queryKey = ["quota-panel-history", scopeKey];
  const history = useQuery({
    queryKey,
    queryFn: () => quotaPanelApi.history(scopeKey),
    enabled,
    staleTime: 60_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  useEffect(() => {
    if (!enabled || history.isPending) return;
    let active = true;
    const point = {
      at: Math.floor(now),
      remaining: 100 - tier.utilization,
      resetsAt: Math.floor(reset),
      span: span!,
    };
    void quotaPanelApi
      .note(scopeKey, point)
      .then((points) => {
        if (active) client.setQueryData(queryKey, points);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [
    enabled,
    history.isPending,
    sampledAt,
    scopeKey,
    tier.utilization,
    reset,
    span,
    client,
  ]);
  if (!enabled) return null;
  const point = {
    at: now,
    remaining: 100 - tier.utilization,
    resetsAt: reset,
    span: span!,
  };
  return (
    <div
      className="tray-quota-spark"
      title={t("quotaPanel.curveHint", { name: label })}
    >
      <svg
        viewBox="0 0 200 30"
        preserveAspectRatio="none"
        role="img"
        aria-label={t("quotaPanel.curveLabel", { name: label })}
      >
        <path className="tray-quota-even" d="M0,0 L200,30" />
        <path
          className="tray-quota-history"
          d={quotaCurve(history.data ?? [], point)}
        />
        <circle
          className="tray-quota-now"
          cx={Math.max(
            0,
            Math.min(200, ((now - (reset - span!)) / span!) * 200),
          )}
          cy={30 * Math.max(0, Math.min(1, tier.utilization / 100))}
          r="0.9"
        />
      </svg>
    </div>
  );
}
