import { useTranslation } from "react-i18next";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import { Info, Loader2 } from "lucide-react";
import {
  fmtUsd,
  formatTokensShort,
  getResolvedLang,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import {
  getCacheWriteAvailability,
  KNOWN_APP_TYPES,
  type UsageRangeSelection,
  type UsageSummary,
  type UsageSummaryByApp,
} from "@/types/usage";

interface UsageHeroProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

/**
 * Combine per-app summaries into a single rolled-up summary.
 *
 * The backend's per-app rows already use fresh-input semantics (cache-inclusive
 * providers have been normalized in SQL), so plain addition is correct here.
 * `cacheHitRate` and `successRate` must be re-derived from the summed counts
 * rather than averaged across rows.
 */
function aggregateSummaries(items: UsageSummary[]): UsageSummary {
  let totalRequests = 0;
  let successCount = 0;
  let totalCostNum = 0;
  let input = 0;
  let output = 0;
  let cacheCreation = 0;
  let cacheRead = 0;

  for (const s of items) {
    totalRequests += s.totalRequests;
    successCount += Math.round((s.totalRequests * s.successRate) / 100);
    totalCostNum += parseFiniteNumber(s.totalCost) ?? 0;
    input += s.totalInputTokens;
    output += s.totalOutputTokens;
    cacheCreation += s.totalCacheCreationTokens;
    cacheRead += s.totalCacheReadTokens;
  }

  const cacheableInput = input + cacheCreation + cacheRead;
  return {
    totalRequests,
    totalCost: totalCostNum.toFixed(6),
    totalInputTokens: input,
    totalOutputTokens: output,
    totalCacheCreationTokens: cacheCreation,
    totalCacheReadTokens: cacheRead,
    successRate: totalRequests > 0 ? (successCount / totalRequests) * 100 : 0,
    realTotalTokens: input + output + cacheCreation + cacheRead,
    cacheHitRate: cacheableInput > 0 ? cacheRead / cacheableInput : 0,
  };
}

function pickSummary(
  apps: UsageSummaryByApp[],
  appType: string | undefined,
): UsageSummary | undefined {
  if (apps.length === 0) return undefined;
  if (appType) {
    return apps.find((a) => a.appType === appType)?.summary;
  }
  return aggregateSummaries(apps.map((a) => a.summary));
}

export function UsageHero({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageHeroProps) {
  const { t, i18n } = useTranslation();
  const lang = getResolvedLang(i18n);
  const locale = getLocaleFromLanguage(lang);

  const { data, isLoading } = useUsageSummaryByApp(
    range,
    { providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  // No client-side filtering: Hero's totals must match the Trend/Logs/Stats
  // below, which all go through the backend's full set of app_types. The
  // KNOWN_APP_TYPES list only governs which filter buttons appear, not which
  // rows participate in the "all" aggregate.
  const allApps = data ?? [];
  const summary = pickSummary(allApps, appType);

  const appLabel = KNOWN_APP_TYPES.some((type) => type === appType)
    ? t(`usage.appFilter.${appType}`)
    : null;

  const cacheWriteState = getCacheWriteAvailability(
    appType ? [appType] : allApps.map((a) => a.appType),
    summary?.totalCacheCreationTokens,
  );

  const input = summary?.totalInputTokens ?? 0;
  const output = summary?.totalOutputTokens ?? 0;
  const cacheWrite = summary?.totalCacheCreationTokens ?? 0;
  const cacheRead = summary?.totalCacheReadTokens ?? 0;
  const realTotal = summary?.realTotalTokens ?? 0;
  const hitRate = summary?.cacheHitRate ?? 0;
  const totalCost = parseFiniteNumber(summary?.totalCost);
  const requests = summary?.totalRequests ?? 0;

  const cacheWriteDisplay = {
    value:
      cacheWriteState === "na"
        ? t("usage.cacheCreationNotReported")
        : formatTokensShort(cacheWrite, lang),
    muted: cacheWriteState === "na",
    tooltip:
      cacheWriteState === "na"
        ? t("usage.cacheWriteNotReported")
        : cacheWriteState === "partial"
          ? t(
              "usage.cacheWritePartial",
              "部分协议（如 OpenAI）不上报缓存写入，数值可能偏低",
            )
          : undefined,
  };

  if (isLoading) {
    return (
      <div
        role="status"
        aria-label={t("common.loading")}
        className="stats-surface flex min-h-44 items-center justify-center"
      >
        <Loader2 className="size-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const hitPercent = Math.max(0, Math.min(100, hitRate * 100));
  const hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1);

  return (
    <section
      className="stats-surface stats-hero"
      aria-label={t("usage.realTotal", "真实消耗 Tokens")}
    >
      <dl className="stats-totals">
        <div className="stats-consumption">
          <dt>
            {appLabel && `${appLabel} · `}
            {t("usage.realTotal", "真实消耗 Tokens")}
          </dt>
          <dd>
            <strong>{realTotal.toLocaleString(locale)}</strong>
            <small>≈ {formatTokensShort(realTotal, lang, 2)}</small>
          </dd>
        </div>
        <div>
          <dt>{t("usage.totalRequests")}</dt>
          <dd>{requests.toLocaleString(locale)}</dd>
        </div>
        <div>
          <dt>{t("usage.totalCost")}</dt>
          <dd className="text-[var(--success)]">
            {totalCost == null ? "--" : fmtUsd(totalCost, 4)}
          </dd>
        </div>
      </dl>
      <dl className="stats-breakdown">
        <MiniStat
          label={t("usage.freshInput", "新增输入")}
          value={formatTokensShort(input, lang)}
        />
        <MiniStat
          label={t("usage.output")}
          value={formatTokensShort(output, lang)}
        />
        <MiniStat
          label={t("usage.cacheWrite", "缓存写入")}
          value={cacheWriteDisplay.value}
          tooltip={cacheWriteDisplay.tooltip}
          muted={cacheWriteDisplay.muted}
        />
        <MiniStat
          label={t("usage.cacheRead", "缓存命中")}
          value={formatTokensShort(cacheRead, lang)}
        />
        <div>
          <dt>{t("usage.cacheHitRate", "缓存命中率")}</dt>
          <dd className="stats-hit-rate">
            {hitPercentLabel}%
            <span aria-hidden="true">
              <i style={{ width: `${hitPercent}%` }} />
            </span>
          </dd>
        </div>
      </dl>
    </section>
  );
}

function MiniStat({
  label,
  value,
  tooltip,
  muted,
}: {
  label: string;
  value: string;
  tooltip?: string;
  muted?: boolean;
}) {
  return (
    <div title={tooltip}>
      <dt>
        {label}
        {tooltip && (
          <>
            <Info aria-hidden="true" size={12} />
            <span className="sr-only">{tooltip}</span>
          </>
        )}
      </dt>
      <dd className={muted ? "text-muted-foreground" : undefined}>{value}</dd>
    </div>
  );
}
