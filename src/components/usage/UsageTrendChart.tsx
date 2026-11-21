import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { useUsageTrends } from "@/lib/query/usage";
import { Loader2 } from "lucide-react";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export interface UsageTrendStatLike {
  date: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  totalCost: string | number;
}

export interface UsageTrendChartPoint {
  /** Unique category key for Recharts — must not collide across years. */
  xKey: string;
  rawDate: string;
  /** Short tick label shown on the X axis. */
  label: string;
  /** Fuller label used by the tooltip. */
  tooltipLabel: string;
  hour: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  cost: number | null;
}

export const usageTrendLegendItems = [
  {
    dataKey: "cost",
    labelKey: "usage.cost",
    fallbackLabel: "成本",
    color: "var(--chart-cost)",
    dashed: true,
  },
  {
    dataKey: "cacheCreationTokens",
    labelKey: "usage.cacheCreationTokens",
    fallbackLabel: "缓存创建",
    color: "var(--chart-cache-create)",
    dashed: false,
  },
  {
    dataKey: "cacheReadTokens",
    labelKey: "usage.cacheReadTokens",
    fallbackLabel: "缓存命中",
    color: "var(--chart-cache-hit)",
    dashed: false,
  },
  {
    dataKey: "inputTokens",
    labelKey: "usage.inputTokens",
    fallbackLabel: "输入 Tokens",
    color: "var(--chart-input)",
    dashed: false,
  },
  {
    dataKey: "outputTokens",
    labelKey: "usage.outputTokens",
    fallbackLabel: "输出 Tokens",
    color: "var(--chart-output)",
    dashed: false,
  },
] as const;

/** Build chart rows from backend trend stats. Exported for unit tests. */
export function buildUsageTrendChartData(
  trends: UsageTrendStatLike[] | undefined,
  options: {
    isHourly: boolean;
    dateLocale: string;
    /** Inclusive range endpoints (unix seconds). Used to decide year labels. */
    startDate: number;
    endDate: number;
  },
): UsageTrendChartPoint[] {
  const { isHourly, dateLocale, startDate, endDate } = options;
  const startYear = new Date(startDate * 1000).getFullYear();
  const endYear = new Date(endDate * 1000).getFullYear();
  const spansMultipleYears = startYear !== endYear;

  return (
    trends?.map((stat) => {
      const pointDate = new Date(stat.date);
      const cost = parseFiniteNumber(stat.totalCost);
      // Prefer a stable unique key from the source timestamp / date string.
      // Falling back to ISO keeps categories unique even if the backend
      // returns sparse points that share the same local MM/DD across years.
      const xKey = stat.date;
      const tooltipLabel = isHourly
        ? pointDate.toLocaleString(dateLocale, {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
          })
        : pointDate.toLocaleDateString(dateLocale, {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
          });
      const label = isHourly
        ? pointDate.toLocaleString(dateLocale, {
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
          })
        : spansMultipleYears
          ? pointDate.toLocaleDateString(dateLocale, {
              year: "2-digit",
              month: "2-digit",
              day: "2-digit",
            })
          : pointDate.toLocaleDateString(dateLocale, {
              month: "2-digit",
              day: "2-digit",
            });

      return {
        xKey,
        rawDate: stat.date,
        label,
        tooltipLabel,
        hour: pointDate.getHours(),
        inputTokens: stat.totalInputTokens,
        outputTokens: stat.totalOutputTokens,
        cacheCreationTokens: stat.totalCacheCreationTokens,
        cacheReadTokens: stat.totalCacheReadTokens,
        cost: cost ?? null,
      };
    }) || []
  );
}

/** Resolve a tick label by the unique category key (not by filtered tick index). */
export function formatUsageTrendTickLabel(
  xKey: string,
  chartData: UsageTrendChartPoint[],
): string {
  const point = chartData.find((row) => row.xKey === xKey);
  return point?.label ?? xKey;
}

export function createUsageTrendTokenTickFormatter(
  locale: string,
): Intl.NumberFormat {
  return new Intl.NumberFormat(locale, {
    notation: "compact",
    compactDisplay: "short",
    maximumFractionDigits: 1,
  });
}

export function formatUsageTrendTokenTickLabel(
  value: unknown,
  formatter: Intl.NumberFormat,
): string {
  const num = parseFiniteNumber(value);
  if (num == null) return "--";

  return formatter.format(num);
}

export function UsageTrendChart({
  range,
  rangeLabel,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const { startDate, endDate } = resolveUsageRange(range);
  const { data: trends, isLoading } = useUsageTrends(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const durationSeconds = Math.max(endDate - startDate, 0);
  const isHourly = durationSeconds <= 24 * 60 * 60;
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const dateLocale = getLocaleFromLanguage(language);
  const tokenTickFormatter = useMemo(
    () => createUsageTrendTokenTickFormatter(dateLocale),
    [dateLocale],
  );

  const chartData = useMemo(
    () =>
      buildUsageTrendChartData(trends, {
        isHourly,
        dateLocale,
        startDate,
        endDate,
      }),
    [trends, isHourly, dateLocale, startDate, endDate],
  );

  if (isLoading) {
    return (
      <div className="flex h-[350px] items-center justify-center rounded-xl bg-card/40 border border-border/50">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/30" />
      </div>
    );
  }

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      const point = payload[0]?.payload as UsageTrendChartPoint | undefined;
      const heading = point?.tooltipLabel ?? point?.label ?? "";
      return (
        <div className="rounded-lg border bg-background/95 p-3 shadow-lg backdrop-blur-md">
          <p className="mb-2 font-medium">{heading}</p>
          {payload.map((entry: any, index: number) => (
            <div key={index} className="flex items-center gap-2 text-sm">
              <div
                className="h-2 w-2 rounded-full"
                style={{ backgroundColor: entry.color }}
              />
              <span className="font-medium">{entry.name}:</span>
              <span>
                {entry.dataKey === "cost"
                  ? fmtUsd(entry.value, 6)
                  : fmtInt(entry.value, dateLocale)}
              </span>
            </div>
          ))}
        </div>
      );
    }
    return null;
  };

  return (
    <div className="stats-surface stats-trend">
      <div className="mb-5 flex flex-wrap items-center gap-x-4 gap-y-2">
        <h3 className="mr-auto text-sm font-medium">
          {t("usage.trends", "使用趋势")}
        </h3>
        <div
          className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs"
          aria-label={t("usage.legend", "图表图例")}
        >
          {usageTrendLegendItems.map((item) => (
            <span
              key={item.dataKey}
              className="inline-flex items-center gap-1.5 whitespace-nowrap"
            >
              <svg
                aria-hidden="true"
                className="h-3 w-3.5 shrink-0"
                style={{ color: item.color }}
                viewBox="0 0 20 12"
                fill="none"
              >
                <line
                  x1="1"
                  y1="6"
                  x2="19"
                  y2="6"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeDasharray={item.dashed ? "4 3" : undefined}
                />
                <circle
                  cx="10"
                  cy="6"
                  r="3.5"
                  fill="hsl(var(--card))"
                  stroke="currentColor"
                  strokeWidth="2"
                />
              </svg>
              <span>{t(item.labelKey, item.fallbackLabel)}</span>
            </span>
          ))}
        </div>
        <p className="text-xs text-muted-foreground">{rangeLabel}</p>
      </div>

      <div className="stats-plot">
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart
            data={chartData}
            margin={{ top: 10, right: 10, left: 0, bottom: 0 }}
          >
            <defs>
              <linearGradient id="colorInput" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="5%"
                  stopColor="var(--chart-input)"
                  stopOpacity={0.2}
                />
                <stop
                  offset="95%"
                  stopColor="var(--chart-input)"
                  stopOpacity={0}
                />
              </linearGradient>
              <linearGradient id="colorOutput" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="5%"
                  stopColor="var(--chart-output)"
                  stopOpacity={0.2}
                />
                <stop
                  offset="95%"
                  stopColor="var(--chart-output)"
                  stopOpacity={0}
                />
              </linearGradient>
              <linearGradient
                id="colorCacheCreation"
                x1="0"
                y1="0"
                x2="0"
                y2="1"
              >
                <stop
                  offset="5%"
                  stopColor="var(--chart-cache-create)"
                  stopOpacity={0.2}
                />
                <stop
                  offset="95%"
                  stopColor="var(--chart-cache-create)"
                  stopOpacity={0}
                />
              </linearGradient>
              <linearGradient id="colorCacheRead" x1="0" y1="0" x2="0" y2="1">
                <stop
                  offset="5%"
                  stopColor="var(--chart-cache-hit)"
                  stopOpacity={0.2}
                />
                <stop
                  offset="95%"
                  stopColor="var(--chart-cache-hit)"
                  stopOpacity={0}
                />
              </linearGradient>
            </defs>
            <CartesianGrid
              strokeDasharray="3 3"
              vertical={false}
              stroke="hsl(var(--border))"
              opacity={0.4}
            />
            <XAxis
              dataKey="xKey"
              axisLine={false}
              tickLine={false}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
              dy={10}
              tickFormatter={(value) =>
                formatUsageTrendTickLabel(String(value), chartData)
              }
              allowDuplicatedCategory={false}
            />
            <YAxis
              yAxisId="tokens"
              width={72}
              axisLine={false}
              tickLine={false}
              tickMargin={8}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
              tickFormatter={(value) =>
                formatUsageTrendTokenTickLabel(value, tokenTickFormatter)
              }
            />
            <YAxis
              yAxisId="cost"
              orientation="right"
              width={56}
              axisLine={false}
              tickLine={false}
              tickMargin={8}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
              tickFormatter={(value) => `$${value}`}
            />
            <Tooltip content={<CustomTooltip />} />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="inputTokens"
              name={t("usage.inputTokens", "输入 Tokens")}
              stroke="var(--chart-input)"
              fillOpacity={1}
              fill="url(#colorInput)"
              strokeWidth={2}
            />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="outputTokens"
              name={t("usage.outputTokens", "输出 Tokens")}
              stroke="var(--chart-output)"
              fillOpacity={1}
              fill="url(#colorOutput)"
              strokeWidth={2}
            />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="cacheCreationTokens"
              name={t("usage.cacheCreationTokens", "缓存创建")}
              stroke="var(--chart-cache-create)"
              fillOpacity={1}
              fill="url(#colorCacheCreation)"
              strokeWidth={2}
            />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="cacheReadTokens"
              name={t("usage.cacheReadTokens", "缓存命中")}
              stroke="var(--chart-cache-hit)"
              fillOpacity={1}
              fill="url(#colorCacheRead)"
              strokeWidth={2}
            />
            <Area
              yAxisId="cost"
              type="monotone"
              dataKey="cost"
              name={t("usage.cost", "成本")}
              stroke="var(--chart-cost)"
              fill="none"
              strokeWidth={2}
              strokeDasharray="4 4"
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
