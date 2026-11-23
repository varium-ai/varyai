import { APP_ICON_NAME } from "@/config/appConfig";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { UsageHero } from "./UsageHero";
import {
  formatShortRoutingId,
  getUsageModelOptions,
  isReadableUsageModelName,
} from "./modelOptions";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import {
  KNOWN_APP_TYPES,
  getUsageRoutingModelId,
  type AppTypeFilter,
  type UsageRangeSelection,
} from "@/types/usage";
import { motion } from "framer-motion";
import { RefreshCw, LayoutGrid, Loader2, ScanSearch } from "lucide-react";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useQueryClient } from "@tanstack/react-query";
import {
  usageKeys,
  useModelStats,
  useProviderStats,
  useUsageModelLabels,
} from "@/lib/query/usage";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { cn } from "@/lib/utils";
import { getLocaleFromLanguage } from "./format";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import { UsageDateRangePicker } from "./UsageDateRangePicker";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { usageApi } from "@/lib/api/usage";
import { toast } from "sonner";
import {
  getSessionSourceApp,
  getUsageSourceLabel,
  type UsageSource,
} from "./source";

const APP_FILTER_OPTIONS: AppTypeFilter[] = ["all", ...KNOWN_APP_TYPES];
type UsageSourceOption = Required<UsageSource>;

const DEFAULT_REFRESH_INTERVAL_MS = 30000;
const REFRESH_INTERVAL_OPTIONS_MS = [0, 5000, 10000, 30000, 60000] as const;
type RefreshIntervalOption = (typeof REFRESH_INTERVAL_OPTIONS_MS)[number];

const isRefreshIntervalOption = (
  value: number | undefined,
): value is RefreshIntervalOption =>
  REFRESH_INTERVAL_OPTIONS_MS.includes(value as RefreshIntervalOption);

const normalizeRefreshInterval = (value: number | undefined) =>
  isRefreshIntervalOption(value) ? value : DEFAULT_REFRESH_INTERVAL_MS;

// Select 的 "all" 哨兵和用户自定义名称同处一个值域——真有来源/模型叫 "all"
// 就会撞名（重复 value、选中即清空筛选）。动态选项统一加前缀编码隔离值域。
const DYNAMIC_OPTION_PREFIX = "v:";
const encodeOptionValue = (name: string) => `${DYNAMIC_OPTION_PREFIX}${name}`;
const decodeOptionValue = (value: string) =>
  value === "all" ? undefined : value.slice(DYNAMIC_OPTION_PREFIX.length);

interface UsageDashboardProps {
  initialAppType?: string;
  refreshIntervalMs?: number;
  onRefreshIntervalChange?: (next: number) => Promise<boolean> | boolean | void;
  sessionAutoSyncEnabled?: boolean;
  onSessionAutoSyncEnabledChange?: (
    next: boolean,
  ) => Promise<boolean> | boolean | void;
}

export function UsageDashboard({
  initialAppType = "all",
  refreshIntervalMs: savedRefreshIntervalMs,
  onRefreshIntervalChange,
  sessionAutoSyncEnabled = true,
  onSessionAutoSyncEnabledChange,
}: UsageDashboardProps = {}) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<UsageRangeSelection>({ preset: "today" });
  const [appType, setAppType] = useState<string>(initialAppType);
  const [selectedSource, setSelectedSource] = useState<UsageSourceOption>();
  const providerName = selectedSource?.providerName;
  const [model, setModel] = useState<string | undefined>(undefined);
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(() =>
    normalizeRefreshInterval(savedRefreshIntervalMs),
  );
  const [syncingSession, setSyncingSession] = useState(false);

  useEffect(() => {
    setRefreshIntervalMs(normalizeRefreshInterval(savedRefreshIntervalMs));
  }, [savedRefreshIntervalMs]);

  // 切应用时清掉下游筛选，避免留下一个在新范围内查无数据的"幽灵"组合；
  // 切 Provider 同理清掉模型（模型选项随 Provider 级联）。
  const changeAppType = (next: AppTypeFilter) => {
    setAppType(next);
    if (next !== appType) {
      setSelectedSource(undefined);
      setModel(undefined);
    }
  };
  const changeProviderName = (next: string | undefined) => {
    setSelectedSource(
      providerOptions.find((source) => source.providerName === next),
    );
    if (next !== providerName) {
      setModel(undefined);
    }
  };

  // 后端写入新日志时 emit `usage-log-recorded`，本 hook 立刻 invalidate 所有
  // usage 查询，实现实时刷新（仅在 Dashboard 挂载时生效，离开页面自动取消监听）
  useUsageEventBridge();

  const changeRefreshInterval = async (next: number) => {
    const normalized = normalizeRefreshInterval(next);
    const previous = refreshIntervalMs;
    setRefreshIntervalMs(normalized);
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
    try {
      const saved = await onRefreshIntervalChange?.(normalized);
      if (saved === false) {
        setRefreshIntervalMs(previous);
      }
    } catch (error) {
      console.error(
        "[UsageDashboard] Failed to persist refresh interval",
        error,
      );
      setRefreshIntervalMs(previous);
    }
  };

  // 手动触发一次会话日志同步：手动模式下是唯一的直连用量补录途径，
  // 入口按钮仅在关闭自动扫描时展示（自动模式有后台定时扫描，无需手动触发）
  const runManualSessionSync = async () => {
    setSyncingSession(true);
    try {
      const result = await usageApi.syncSessionUsage();
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
      const message = t("usage.sessionSync.syncCompleted", {
        imported: result.imported,
        files: result.filesScanned,
        errors: result.errors.length,
      });
      if (result.errors.length > 0) {
        toast.warning(message);
      } else {
        toast.success(message);
      }
    } catch (error) {
      toast.error(
        t("usage.sessionSync.syncFailed", {
          error: String(error),
        }),
      );
    } finally {
      setSyncingSession(false);
    }
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  const resolvedRange = useMemo(() => resolveUsageRange(range), [range]);
  const rangeLabel = useMemo(() => {
    if (range.preset !== "custom") {
      return getUsageRangePresetLabel(range.preset, t);
    }

    const startStr = new Date(resolvedRange.startDate * 1000).toLocaleString(
      locale,
    );

    if (range.liveEndTime) {
      return `${startStr} → ${t("usage.liveEndTimeNow", "现在")}`;
    }

    const endStr = new Date(resolvedRange.endDate * 1000).toLocaleString(
      locale,
    );
    return `${startStr} - ${endStr}`;
  }, [locale, range, resolvedRange.endDate, resolvedRange.startDate, t]);

  // 顶栏下拉的选项池：Provider 列表只跟应用/时间范围走（不受自身选中值影响），
  // 模型列表随所选 Provider 级联。两者都只列当前范围内真实有数据的条目。
  // refetchInterval 必须跟随面板的刷新设置——未筛选时这两个查询与统计表共享
  // query key，落下的话会以默认 30s 拖着同 key 查询一起轮询，"--" 形同虚设。
  const optionsRefetch = {
    refetchInterval:
      refreshIntervalMs > 0 ? refreshIntervalMs : (false as const),
  };
  const { data: providerOptionsData } = useProviderStats(
    range,
    { appType },
    optionsRefetch,
  );
  const { data: modelOptionsData, isLoading: modelOptionsLoading } =
    useModelStats(range, { appType, providerName }, optionsRefetch);

  const {
    data: modelLabels,
    isPending: modelLabelsPending,
    isFetching: modelLabelsFetching,
  } = useUsageModelLabels(
    [
      ...(modelOptionsData ?? []).flatMap((stat) => [
        stat.model,
        ...(stat.aliases ?? []),
      ]),
      ...(model ? [model] : []),
    ],
    optionsRefetch,
  );

  const providerOptions = useMemo(() => {
    const sources = new Map<string, UsageSourceOption>();
    for (const stat of providerOptionsData ?? []) {
      const previous = sources.get(stat.providerName);
      if (!previous) {
        sources.set(stat.providerName, stat);
      } else if (getSessionSourceApp(previous) !== getSessionSourceApp(stat)) {
        // Name filters can include a saved provider and a session placeholder.
        // Keep the raw name when the filter does not identify only imports.
        sources.set(stat.providerName, {
          providerId: "",
          providerName: stat.providerName,
        });
      }
    }
    // Retain the selected label when it leaves the range.
    if (
      selectedSource?.providerName &&
      !sources.has(selectedSource.providerName)
    ) {
      sources.set(selectedSource.providerName, selectedSource);
    }
    return Array.from(sources.values());
  }, [providerOptionsData, selectedSource]);
  const selectedSourceOption = providerOptions.find(
    (source) => source.providerName === providerName,
  );

  const modelOptions = useMemo(() => {
    const options = getUsageModelOptions(
      modelOptionsData ?? [],
      model,
      modelLabels,
    ).map((option) => ({
      value: option.value,
      routingId: option.routingId
        ? formatShortRoutingId(option.routingId)
        : undefined,
      hidden: option.resolvedLabel
        ? !isReadableUsageModelName(option.resolvedLabel.model)
        : option.routingModels
          ? option.routingModels.length === 0
          : !isReadableUsageModelName(option.value),
      label: option.resolvedLabel
        ? option.resolvedLabel.providerName
          ? t("usage.providerModelOption", {
              model: option.resolvedLabel.displayName,
              provider: option.resolvedLabel.providerName,
            })
          : option.resolvedLabel.displayName
        : option.routingModels
          ? option.routingModels.length > 0
            ? t("usage.routingModelOption", {
                models: option.routingModels.join(", "),
              })
            : t("usage.unknownRoutingModelOption", {
                id: option.routingId
                  ? formatShortRoutingId(option.routingId)
                  : "",
              })
          : option.value,
    }));
    const labelCounts = new Map<string, number>();
    for (const option of options) {
      labelCounts.set(option.label, (labelCounts.get(option.label) ?? 0) + 1);
    }
    return options.map((option) => ({
      value: option.value,
      hidden: option.hidden,
      label:
        option.routingId && (labelCounts.get(option.label) ?? 0) > 1
          ? t("usage.routingModelOptionDisambiguated", {
              label: option.label,
              id: option.routingId,
            })
          : option.label,
    }));
  }, [modelOptionsData, model, modelLabels, t]);
  const visibleModelOptions = useMemo(
    () => modelOptions.filter((option) => !option.hidden),
    [modelOptions],
  );
  useEffect(() => {
    if (
      !model ||
      modelOptionsLoading ||
      modelLabelsPending ||
      modelLabelsFetching
    )
      return;
    const normalized = getUsageRoutingModelId(model) ?? model;
    if (modelOptions.find((option) => option.value === normalized)?.hidden) {
      setModel(undefined);
    } else if (normalized !== model) {
      setModel(normalized);
    }
  }, [
    model,
    modelOptions,
    modelOptionsLoading,
    modelLabelsPending,
    modelLabelsFetching,
  ]);

  const modelNames = useMemo(
    () =>
      Object.fromEntries(
        modelOptions.map((option) => [option.value, option.label]),
      ),
    [modelOptions],
  );
  const selectedModelLabel = modelOptions.find(
    (option) => option.value === model,
  )?.label;

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="stats-dashboard"
    >
      <div className="stats-toolbar">
        <div className="stats-agents">
          {APP_FILTER_OPTIONS.map((type) => {
            const label = t(`usage.appFilter.${type}`);
            return (
              <button
                key={type}
                type="button"
                onClick={() => changeAppType(type)}
                title={label}
                aria-label={label}
                aria-pressed={appType === type}
                className={cn(
                  "flex size-7 shrink-0 items-center justify-center rounded-md transition-colors focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                  appType === type
                    ? "bg-card text-primary shadow-sm"
                    : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
                )}
              >
                {type === "all" ? (
                  <LayoutGrid className="h-4 w-4" />
                ) : (
                  <ProviderIcon
                    icon={APP_ICON_NAME[type]}
                    name={label}
                    size={16}
                  />
                )}
              </button>
            );
          })}
        </div>

        <Select
          value={providerName != null ? encodeOptionValue(providerName) : "all"}
          onValueChange={(v) => changeProviderName(decodeOptionValue(v))}
        >
          <SelectTrigger
            className="h-[34px] w-full bg-card text-xs focus:border-border-default [&>span]:min-w-0 [&>span]:truncate"
            title={
              selectedSourceOption
                ? getUsageSourceLabel(selectedSourceOption, t)
                : t("usage.filterBySource")
            }
            aria-label={t("usage.filterBySource")}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="max-w-[280px]">
            <SelectItem value="all">{t("usage.allSources")}</SelectItem>
            {providerOptions.map((source) => (
              <SelectItem
                key={source.providerName}
                value={encodeOptionValue(source.providerName)}
                title={getUsageSourceLabel(source, t)}
                className="[&>span]:min-w-0 [&>span]:truncate"
              >
                {getUsageSourceLabel(source, t)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={model != null ? encodeOptionValue(model) : "all"}
          onValueChange={(v) => setModel(decodeOptionValue(v))}
        >
          <SelectTrigger
            className="h-[34px] w-full bg-card text-xs focus:border-border-default [&>span]:min-w-0 [&>span]:truncate"
            aria-label={t("usage.filterByModel")}
            title={selectedModelLabel ?? t("usage.filterByModel")}
            aria-describedby="usage-model-filter-scope"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent className="max-w-[280px]">
            <SelectItem value="all">{t("usage.allModels")}</SelectItem>
            {visibleModelOptions.map((option) => (
              <SelectItem
                key={option.value}
                value={encodeOptionValue(option.value)}
                title={option.label}
                className="[&>span]:min-w-0 [&>span]:truncate"
              >
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={String(refreshIntervalMs)}
          onValueChange={(v) => changeRefreshInterval(Number(v))}
        >
          <SelectTrigger
            className="h-[34px] w-full bg-card text-xs focus:border-border-default"
            title={t("usage.refreshInterval")}
            aria-label={t("usage.refreshInterval")}
          >
            <span className="flex items-center gap-2">
              <RefreshCw className="h-3.5 w-3.5 shrink-0" />
              <SelectValue />
            </span>
          </SelectTrigger>
          <SelectContent>
            {REFRESH_INTERVAL_OPTIONS_MS.map((ms) => (
              <SelectItem key={ms} value={String(ms)}>
                {ms > 0 ? `${ms / 1000}s` : t("usage.refreshOff")}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <UsageDateRangePicker
          selection={range}
          triggerLabel={rangeLabel}
          onApply={setRange}
          triggerClassName="h-[34px] w-full px-2"
        />
        <span id="usage-model-filter-scope" className="stats-filter-note">
          {t("usage.modelFilterScope")}
        </span>
      </div>

      <UsageHero
        range={range}
        appType={appType === "all" ? undefined : appType}
        providerName={providerName}
        model={model}
        refreshIntervalMs={refreshIntervalMs}
      />

      <UsageTrendChart
        range={range}
        rangeLabel={rangeLabel}
        appType={appType}
        providerName={providerName}
        model={model}
        refreshIntervalMs={refreshIntervalMs}
      />

      <div className="space-y-4">
        <Tabs defaultValue="logs" className="w-full">
          <div className="flex items-center justify-between mb-4">
            <TabsList
              slidingIndicator={false}
              className="h-auto max-w-full flex-wrap justify-start rounded-lg border border-border bg-muted"
            >
              <TabsTrigger
                value="logs"
                className="min-h-8 min-w-0 data-[state=active]:bg-card data-[state=active]:shadow-sm"
              >
                {t("usage.requestLogs")}
              </TabsTrigger>
              <TabsTrigger
                value="providers"
                className="min-h-8 min-w-0 data-[state=active]:bg-card data-[state=active]:shadow-sm"
              >
                {t("usage.providerStats")}
              </TabsTrigger>
              <TabsTrigger
                value="models"
                className="min-h-8 min-w-0 data-[state=active]:bg-card data-[state=active]:shadow-sm"
              >
                {t("usage.modelStats")}
              </TabsTrigger>
            </TabsList>
          </div>

          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.2 }}
          >
            <TabsContent value="logs" className="mt-0">
              <RequestLogTable
                modelNames={modelNames}
                range={range}
                rangeLabel={rangeLabel}
                appType={appType}
                providerName={providerName}
                model={model}
                refreshIntervalMs={refreshIntervalMs}
                onRangeChange={setRange}
              />
            </TabsContent>

            <TabsContent value="providers" className="mt-0">
              <ProviderStatsTable
                range={range}
                appType={appType}
                providerName={providerName}
                model={model}
                refreshIntervalMs={refreshIntervalMs}
              />
            </TabsContent>

            <TabsContent value="models" className="mt-0">
              <ModelStatsTable
                modelNames={modelNames}
                range={range}
                appType={appType}
                providerName={providerName}
                model={model}
                refreshIntervalMs={refreshIntervalMs}
              />
            </TabsContent>
          </motion.div>
        </Tabs>
      </div>

      <div className="space-y-4">
        <div className="stats-surface flex flex-wrap items-center justify-between gap-3 px-5 py-4">
          <div className="flex min-w-0 flex-[1_1_240px] items-center gap-3">
            <ScanSearch className="size-4 shrink-0 text-primary" />
            <div>
              <h3 className="text-sm font-medium">
                {t("usage.sessionSync.title")}
              </h3>
              <p className="text-xs text-muted-foreground">
                {t("usage.sessionSync.description")}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-3 shrink-0">
            {!sessionAutoSyncEnabled && (
              <Button
                variant="outline"
                size="sm"
                disabled={syncingSession}
                onClick={() => void runManualSessionSync()}
              >
                {syncingSession ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="mr-2 h-4 w-4" />
                )}
                {t("usage.sessionSync.syncNow")}
              </Button>
            )}
            <Switch
              checked={sessionAutoSyncEnabled}
              onCheckedChange={(value) =>
                void onSessionAutoSyncEnabledChange?.(value)
              }
              aria-label={t("usage.sessionSync.title")}
            />
          </div>
        </div>
      </div>
    </motion.div>
  );
}
