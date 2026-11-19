import { getSessionSourceApp } from "@/components/usage/source";
import type {
  TrayUsagePeriod,
  TrayUsageRow,
  TrayUsageTotals,
} from "@/lib/api/trayUsage";
import { normalizeUsageModelId } from "@/types/usage";

export const periods = ["today", "week", "month", "all"] as const;
export const metrics = ["tokens", "cost", "requests"] as const;
export const splits = ["provider", "model"] as const;
export type UsageMetric = (typeof metrics)[number];
export type UsageSplit = (typeof splits)[number];

export const localSessions = "sessions";
export const allProviders = "all";

export function providerKey(row: TrayUsageRow) {
  return getSessionSourceApp(row)
    ? localSessions
    : JSON.stringify([row.providerId, row.providerName]);
}

export function metricValue(totals: TrayUsageTotals, metric: UsageMetric) {
  return metric === "tokens"
    ? totals.input + totals.output + totals.reads + totals.writes
    : totals[metric];
}

export function sumUsage(rows: readonly TrayUsageTotals[]): TrayUsageTotals {
  const totals = {
    requests: 0,
    failed: 0,
    input: 0,
    output: 0,
    reads: 0,
    writes: 0,
    cost: 0,
    unpriced: 0,
    firstMs: 0,
    firstCount: 0,
    timedOutput: 0,
    generationMs: 0,
    timedCount: 0,
  };
  const keys = Object.keys(totals) as (keyof TrayUsageTotals)[];
  for (const row of rows) {
    for (const key of keys) totals[key] += row[key];
  }
  return totals;
}

export function groupUsage<Key>(
  rows: readonly TrayUsageRow[],
  key: (row: TrayUsageRow) => Key,
) {
  const groups = new Map<Key, TrayUsageRow[]>();
  for (const row of rows) {
    const id = key(row);
    const group = groups.get(id);
    if (group) group.push(row);
    else groups.set(id, [row]);
  }
  return groups;
}

export function seriesKey(split: UsageSplit) {
  return split === "provider"
    ? providerKey
    : (row: TrayUsageRow) => normalizeUsageModelId(row.model);
}

export function localDateKey(date: Date) {
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join("-");
}

export function calendarDays(count: number, now = new Date()) {
  return Array.from({ length: count }, (_, index) =>
    localDateKey(
      new Date(
        now.getFullYear(),
        now.getMonth(),
        now.getDate() + index - count + 1,
      ),
    ),
  );
}

export const activityWeeks = 53;

export function activityDates(now = new Date()) {
  const weekday = (now.getDay() + 6) % 7;
  return calendarDays((activityWeeks - 1) * 7 + weekday + 1, now);
}

export function activityLevels(values: number[]) {
  const positive = values.filter((value) => value > 0).sort((a, b) => a - b);
  const highest = positive.at(-1) ?? 0;
  const quartiles = [0.25, 0.5, 0.75].map(
    (fraction) => positive[Math.floor(fraction * positive.length)],
  );
  return values.map((value) => {
    if (value <= 0) return 0;
    if (value >= highest) return 4;
    if (positive.length < 8)
      return Math.max(1, Math.ceil((4 * value) / highest));
    const quartile = quartiles.findIndex((limit) => value <= limit);
    return quartile < 0 ? 4 : quartile + 1;
  });
}

export function trendBuckets(
  rows: TrayUsageRow[],
  period: TrayUsagePeriod,
  now = new Date(),
) {
  if (period === "today") {
    return {
      keys: Array.from(
        { length: 24 },
        (_, hour) => `${String(hour).padStart(2, "0")}:00`,
      ),
      grouped: groupUsage(rows, (row) => row.bucket),
      selectable: false,
    };
  }
  const grouped = groupUsage(rows, (row) => row.bucket);
  const days = calendarDays(period === "week" ? 7 : 30, now);
  const oldest = [...grouped.keys()].sort()[0];
  if (period !== "all" || !oldest || oldest >= days[0]) {
    return { keys: days, grouped, selectable: true };
  }
  const months = groupUsage(rows, (row) => row.bucket.slice(0, 7));
  const firstYear = Number(oldest.slice(0, 4));
  const monthCount =
    (now.getFullYear() - firstYear) * 12 +
    now.getMonth() -
    Number(oldest.slice(5, 7)) +
    2;
  if (monthCount <= 30) {
    const keys = Array.from({ length: monthCount }, (_, index) =>
      localDateKey(
        new Date(now.getFullYear(), now.getMonth() + index - monthCount + 1, 1),
      ).slice(0, 7),
    );
    return { keys, grouped: months, selectable: false };
  }
  const years = groupUsage(rows, (row) => row.bucket.slice(0, 4));
  return { keys: [...years.keys()].sort(), grouped: years, selectable: false };
}

export function axisTop(maximum: number) {
  if (maximum <= 4 && maximum >= 1) return 4;
  const step = maximum / 4;
  const magnitude = 10 ** Math.floor(Math.log10(step));
  const rounded =
    [1, 2, 2.5, 5, 10].find((factor) => factor * magnitude >= step) ?? 10;
  return 4 * rounded * magnitude;
}
