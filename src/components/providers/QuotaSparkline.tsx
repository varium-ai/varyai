import { useEffect, useId } from "react";
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
  const part = (name.split(" · ").at(-1) ?? name).trim();
  if (known[part]) return known[part];
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
function quotaCycle(points: QuotaPoint[], current: QuotaPoint) {
  let start = current.resetsAt - current.span;
  let cycle: QuotaPoint[] = [];
  // The newest reading wins when storage and the current query overlap.
  const observed = new Map(
    [...points, current].map((point) => [point.at, point]),
  );
  for (const point of [...observed.values()].sort((a, b) => a.at - b.at)) {
    if (
      !Number.isFinite(point.at) ||
      !Number.isFinite(point.resetsAt) ||
      point.resetsAt <= point.at ||
      point.span !== current.span ||
      point.at < start ||
      point.at > current.at ||
      !Number.isFinite(point.remaining)
    )
      continue;
    const previous = cycle.at(-1);
    if (previous) {
      // Match Magpie's five-minute deadline tolerance and rolling windows.
      const moved = point.resetsAt - previous.resetsAt;
      const elapsed = point.at - previous.at;
      const rolling = moved > 0 && Math.abs(moved - elapsed) <= 300;
      if (
        previous.resetsAt <= point.at ||
        moved > elapsed + 300 ||
        moved < -300 ||
        (!rolling &&
          Math.abs(moved) <= 300 &&
          point.remaining > previous.remaining + 5)
      ) {
        cycle = [];
        const windowStart = point.resetsAt - point.span;
        start = Math.max(
          start,
          windowStart > previous.at ? windowStart : point.at,
        );
      }
    }
    cycle.push(point);
  }
  const x = (at: number) =>
    Math.max(0, Math.min(1, (at - start) / (current.resetsAt - start)));
  const position = (point: QuotaPoint) =>
    `${(x(point.at) * 200).toFixed(2)},${(30 * (1 - Math.max(0, Math.min(100, point.remaining)) / 100)).toFixed(2)}`;
  return { cycle, x, position };
}
export function quotaCurve(points: QuotaPoint[], current: QuotaPoint): string {
  const { cycle, position } = quotaCycle(points, current);
  return cycle
    .map((point, index) => `${index ? "L" : "M"}${position(point)}`)
    .join(" ");
}
export function quotaGap(
  points: QuotaPoint[],
  current: QuotaPoint,
): string | null {
  const { cycle, x, position } = quotaCycle(points, current);
  if (cycle.length < 2) return null;
  const even = (point: QuotaPoint) =>
    `${(x(point.at) * 200).toFixed(2)},${(x(point.at) * 30).toFixed(2)}`;
  return [
    ...cycle.map(position),
    even(cycle[cycle.length - 1]),
    even(cycle[0]),
  ].join(" ");
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
  const clipId = useId().replace(/:/g, "");
  const client = useQueryClient();
  const span = windowSeconds(tier.name);
  const reset = tier.resetsAt
    ? Math.floor(Date.parse(tier.resetsAt) / 1000)
    : NaN;
  const now = Math.floor(sampledAt / 1000);
  const enabled =
    !!span &&
    Number.isFinite(tier.utilization) &&
    Number.isFinite(reset) &&
    reset > now &&
    reset - span <= now;
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
      remaining: Math.max(0, Math.min(100, 100 - tier.utilization)),
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
  const curve = quotaCurve(history.data ?? [], point);
  const gap = quotaGap(history.data ?? [], point);
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
        {gap && (
          <>
            <defs>
              <clipPath id={`${clipId}-ahead`}>
                <polygon points="0,0 200,0 200,30" />
              </clipPath>
              <clipPath id={`${clipId}-behind`}>
                <polygon points="0,0 0,30 200,30" />
              </clipPath>
            </defs>
            <polygon
              className="tray-quota-gap-ahead"
              points={gap}
              clipPath={`url(#${clipId}-ahead)`}
            />
            <polygon
              className="tray-quota-gap-behind"
              points={gap}
              clipPath={`url(#${clipId}-behind)`}
            />
          </>
        )}
        <path className="tray-quota-even" d="M0,0 L200,30" />
        <path
          className="tray-quota-history"
          d={curve && !curve.includes("L") ? `${curve}h0.01` : curve}
        />
      </svg>
    </div>
  );
}
