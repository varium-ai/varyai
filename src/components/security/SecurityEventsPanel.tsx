import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronRight,
  FileText,
  Loader2,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { UsageDateRangePicker } from "@/components/usage/UsageDateRangePicker";
import { getAppLabel } from "@/config/appConfig";
import {
  useSecurityEvents,
  useSecurityEventsSummary,
  useSecurityEventsLiveRefresh,
} from "@/hooks/useSecurityEvents";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import type {
  SecurityEvent,
  SecurityEventFilter,
} from "@/types/securityEvents";
import {
  RISK_CATEGORIES,
  type ToolRiskEventRecord,
  type ToolRiskRuleSeed,
} from "@/types/toolRisk";
import type { UsageRangeSelection, UsageRangePreset } from "@/types/usage";
import { SecurityEventDetailsDialog } from "./SecurityEventDetailsDialog";
import { ToolRiskEventsPanel } from "./ToolRiskEventsPanel";
import {
  securityEventName,
  securityEventOutboundLabel,
  securityEventResultLabel,
  securityEventRiskLabel,
} from "./securityEventLabels";

type Option = { value: string; label: string };
function FilterSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value?: string;
  options: Option[];
  onChange: (value: string) => void;
}) {
  return (
    <Select value={value || "all"} onValueChange={onChange}>
      <SelectTrigger
        aria-label={label}
        className="h-8 w-auto min-w-28 flex-none rounded-full bg-card text-xs"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export interface SecurityEventsPanelProps {
  filter: SecurityEventFilter;
  onFilterChange: (filter: SecurityEventFilter) => void;
  page: number;
  onPageChange: (page: number) => void;
  range: UsageRangeSelection;
  onRangeChange: (range: UsageRangeSelection) => void;
  onCreateRule?: (seed: ToolRiskRuleSeed) => void;
  onOpenDlpRules?: () => void;
  dlpCategories?: Option[];
}

export function SecurityEventsPanel({
  filter,
  onFilterChange,
  page,
  onPageChange,
  range,
  onRangeChange,
  onCreateRule,
  onOpenDlpRules,
  dlpCategories = [],
}: SecurityEventsPanelProps) {
  const { t, i18n } = useTranslation();
  const [refreshRequest, setRefreshRequest] = useState(0);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const manualRefresh = useRef<{ started: boolean } | null>(null);
  const [isSubmittingClear, setIsSubmittingClear] = useState(false);
  const cancelClearRef = useRef<HTMLButtonElement>(null);
  // Keep the reading range stable across unrelated renders (including deep-page signals).
  const { startDate, endDate } = useMemo(
    () => resolveUsageRange(range),
    [
      range.preset,
      range.customStartDate,
      range.customEndDate,
      range.liveEndTime,
      refreshRequest,
    ],
  );
  const timeFilter = {
    startTimestamp: startDate * 1000,
    endTimestamp:
      range.preset === "custom" && !range.liveEndTime
        ? endDate * 1000 + 999
        : undefined,
  };
  const queryFilter = { ...filter, ...timeFilter };
  const pageSize = filter.view === "tool_calls" ? 50 : 20;
  const { eventsResult, isLoading, isError, refetch, clearEvents, isClearing } =
    useSecurityEvents(queryFilter, page, pageSize);
  const summary = useSecurityEventsSummary({
    view: "security_events",
    ...timeFilter,
  });
  const [selectedSnapshot, setSelected] = useState<SecurityEvent | null>(null);
  const selected =
    page === 1
      ? (eventsResult?.items.find(
          (event) => event.key === selectedSnapshot?.key,
        ) ?? selectedSnapshot)
      : selectedSnapshot;
  const [clearPreview, setClearPreview] = useState<{
    filter: SecurityEventFilter;
    total?: number;
  } | null>(null);
  const [clearFailed, setClearFailed] = useState(false);
  const { hasUpdates, resetUpdates } = useSecurityEventsLiveRefresh(page, () =>
    setRefreshRequest((request) => request + 1),
  );
  // Apply new page/range state first, then force a request even if page one was cached.
  useEffect(() => {
    if (refreshRequest === 0 || manualRefresh.current?.started) return;
    const manual = manualRefresh.current;
    if (manual) manual.started = true;
    const refreshQueries = async () => {
      const results = await Promise.allSettled([
        refetch(),
        summary.refetch(),
        ...(manual
          ? [new Promise<void>((resolve) => setTimeout(resolve, 300))]
          : []),
      ]);
      if (manual) {
        if (
          results.some(
            (result) =>
              result.status === "rejected" ||
              (result.status === "fulfilled" && result.value?.isError),
          )
        ) {
          toast.error(t("security.refreshFailed"));
        }
        manualRefresh.current = null;
        setIsRefreshing(false);
      }
    };
    void refreshQueries();
  }, [refreshRequest]);
  const refresh = () => {
    if (isLoading || manualRefresh.current) return;
    manualRefresh.current = { started: false };
    setIsRefreshing(true);
    onPageChange(1);
    onRangeChange({ ...range });
    resetUpdates();
    setRefreshRequest((request) => request + 1);
  };
  const changeFilter = (changes: Partial<SecurityEventFilter>) => {
    onFilterChange({ ...filter, ...changes });
    onPageChange(1);
  };
  const switchView = (view: SecurityEventFilter["view"]) =>
    changeFilter({
      view,
      eventType: view === "tool_calls" ? "tool_risk" : undefined,
      sessionId: undefined,
      keyword: undefined,
      category: undefined,
      toolVerdict: undefined,
    });
  const rangeLabels: Partial<Record<UsageRangePreset, string>> = {
    today: t("security.todayFilter"),
    "7d": t("security.last7Days"),
    "30d": t("security.last30Days"),
    custom: t("security.customDateRange"),
  };
  const count = (value?: number, loading = false, error = false) =>
    loading
      ? "…"
      : error || value === undefined
        ? "—"
        : value.toLocaleString(i18n.language);
  const summaryCount = (value?: number) =>
    count(value, summary.isLoading, summary.isError);
  const dlpVisible =
    filter.view === "security_events" && filter.eventType !== "tool_risk";
  const all = (label: string): Option => ({ value: "all", label: t(label) });
  const appIds = [
    "claude",
    "claude_code",
    "pi",
    "claude-desktop",
    "codex",
    "gemini",
    "grokbuild",
    "opencode",
    "openclaw",
    "workbuddy",
    "qoder",
    "doubaowork",
    "trae",
    "kimi",
    "qwen",
    "zcode",
    "openai",
  ];
  const categoryOptions = [
    all("security.allRules"),
    ...(dlpVisible
      ? dlpCategories.map((category) => ({
          ...category,
          value: `dlp:${category.value}`,
        }))
      : []),
    ...(filter.eventType !== "dlp"
      ? RISK_CATEGORIES.map((category) => ({
          value: `tool_risk:${category}`,
          label: t(`toolRisk.category.${category}`),
        }))
      : []),
  ];
  const selectTool = (record: ToolRiskEventRecord) =>
    setSelected(
      eventsResult?.items.find(
        (event) =>
          event.eventType === "tool_risk" && event.record.id === record.id,
      ) ?? null,
    );
  const createRule = onCreateRule
    ? (seed: ToolRiskRuleSeed) => {
        setSelected(null);
        onCreateRule(seed);
      }
    : undefined;
  const isClearBusy = isClearing || isSubmittingClear;
  const clear = async () => {
    if (!clearPreview || isClearBusy) return;
    setIsSubmittingClear(true);
    const frozen = {
      ...clearPreview.filter,
      endTimestamp: Math.min(
        clearPreview.filter.endTimestamp ?? Infinity,
        Date.now(),
      ),
    };
    setClearFailed(false);
    try {
      const receipt = await clearEvents(frozen);
      setSelected(null);
      setClearPreview(null);
      onPageChange(1);
      resetUpdates();
      setRefreshRequest((request) => request + 1);
      toast.success(
        t("securityEvents.clearSuccess", {
          total: receipt.deletedTotal,
          dlp: receipt.deletedDlp,
          toolRisk: receipt.deletedToolRisk,
        }),
      );
    } catch {
      setClearFailed(true);
    } finally {
      setIsSubmittingClear(false);
    }
  };
  const tableLoading = isLoading || isRefreshing;
  const status = isLoading || isError || !eventsResult?.items.length;
  return (
    <section
      className="mt-6 space-y-5"
      role="tabpanel"
      aria-label={t("security.tabRecords")}
    >
      <div className="grid grid-cols-3 gap-4">
        {(["redacted", "warnings", "protectiveBlocked"] as const).map(
          (metric) => (
            <div
              key={metric}
              className="rounded-2xl border border-border/90 bg-card p-5 shadow-sm"
            >
              <div className="text-3xl font-extrabold">
                {summaryCount(summary.data?.[metric])}
              </div>
              <div className="mt-1.5 text-xs text-muted-foreground">
                {t(`securityStats.${metric}`)}
              </div>
            </div>
          ),
        )}
      </div>
      <div className="rounded-xl border bg-card p-4 text-sm">
        <p className="text-xs text-muted-foreground">
          {t("securityEvents.summaryScope")}
        </p>
        <div className="mt-2 flex flex-wrap gap-4">
          <span data-testid="security-events-summary-total">
            {t("securityEvents.totalRecords")}:{" "}
            {summaryCount(summary.data?.total)}
          </span>
          <span data-testid="security-events-summary-dlp">
            {t("securityEvents.dlpCount")}:{" "}
            {summaryCount(summary.data?.dlpCount)}
          </span>
          <span data-testid="security-events-summary-tool-risk">
            {t("securityEvents.toolRiskCount")}:{" "}
            {summaryCount(summary.data?.toolRiskCount)}
          </span>
        </div>
      </div>
      <div
        className="flex gap-2"
        role="tablist"
        aria-label={t("security.tabRecords")}
      >
        {(["security_events", "tool_calls"] as const).map((view) => (
          <Button
            key={view}
            role="tab"
            aria-selected={filter.view === view}
            variant={filter.view === view ? "secondary" : "ghost"}
            onClick={() => switchView(view)}
          >
            {t(
              view === "security_events"
                ? "securityEvents.detailsView"
                : "securityEvents.toolCallsView",
            )}
          </Button>
        ))}
      </div>
      <div className="overflow-x-auto p-1">
        <div className="flex min-w-max items-center gap-2.5 text-xs">
          <UsageDateRangePicker
            selection={range}
            triggerLabel={
              rangeLabels[range.preset] ??
              getUsageRangePresetLabel(range.preset, t)
            }
            presets={["today", "7d", "30d"]}
            presetLabels={rangeLabels}
            triggerClassName="h-8 w-auto rounded-full text-xs"
            popoverAlign="start"
            onApply={(next) => {
              onRangeChange(next);
              onPageChange(1);
            }}
          />
          {filter.view === "security_events" && (
            <FilterSelect
              label={t("securityEvents.source")}
              value={filter.eventType}
              options={[
                all("securityEvents.allSources"),
                { value: "dlp", label: t("securityEvents.dlp") },
                { value: "tool_risk", label: t("securityEvents.toolRisk") },
              ]}
              onChange={(value) =>
                changeFilter({
                  eventType:
                    value === "all"
                      ? undefined
                      : (value as "dlp" | "tool_risk"),
                  category: undefined,
                  ...(value !== "dlp"
                    ? { sessionId: undefined, keyword: undefined }
                    : {}),
                })
              }
            />
          )}
          <FilterSelect
            label={t("security.allRisks")}
            value={filter.riskLevel}
            options={[
              all("security.allRisks"),
              ...["HIGH", "MEDIUM", "LOW"].map((value) => ({
                value,
                label: securityEventRiskLabel(t, value),
              })),
            ]}
            onChange={(value) =>
              changeFilter({
                riskLevel:
                  value === "all"
                    ? undefined
                    : (value as SecurityEventFilter["riskLevel"]),
              })
            }
          />
          <FilterSelect
            label={t("security.typeFilter")}
            value={filter.category}
            options={categoryOptions}
            onChange={(value) =>
              changeFilter({
                category: value === "all" ? undefined : value,
                ...(value === "all"
                  ? {}
                  : {
                      eventType: value.startsWith("dlp:") ? "dlp" : "tool_risk",
                      ...(value.startsWith("tool_risk:")
                        ? { sessionId: undefined, keyword: undefined }
                        : {}),
                    }),
              })
            }
          />
          <FilterSelect
            label={t("security.allApps")}
            value={filter.appType}
            options={[
              all("security.allApps"),
              ...appIds.map((value) => ({
                value,
                label: value === "openai" ? "OpenAI" : getAppLabel(value),
              })),
            ]}
            onChange={(value) =>
              changeFilter({ appType: value === "all" ? undefined : value })
            }
          />
          <FilterSelect
            label={t("security.allActions")}
            value={filter.result}
            options={[
              all("security.allActions"),
              ...(
                [
                  "redacted",
                  "warning",
                  "blocked",
                  "allowed",
                  "unchecked",
                  "uncertain",
                  "low_risk",
                  "unknown",
                ] as const
              ).map((value) => ({
                value,
                label: t(
                  {
                    redacted: "security.resultRedacted",
                    warning: "security.resultAlert",
                    blocked: "security.resultBlocked",
                    allowed: "security.actionAllow",
                    unchecked: "toolRisk.verdict.unchecked",
                    uncertain: "toolRisk.verdict.uncertain",
                    low_risk: "toolRisk.verdict.low_risk",
                    unknown: "securityEvents.unknown",
                  }[value],
                ),
              })),
            ]}
            onChange={(value) =>
              changeFilter({
                result:
                  value === "all"
                    ? undefined
                    : (value as SecurityEventFilter["result"]),
              })
            }
          />
          {filter.view === "tool_calls" && (
            <FilterSelect
              label={t("toolRisk.eventVerdictFilter")}
              value={filter.toolVerdict}
              options={[
                all("toolRisk.eventFilterAll"),
                ...["risky", "unchecked", "uncertain", "low_risk"].map(
                  (value) => ({ value, label: t(`toolRisk.verdict.${value}`) }),
                ),
              ]}
              onChange={(value) =>
                changeFilter({
                  toolVerdict:
                    value === "all"
                      ? undefined
                      : (value as SecurityEventFilter["toolVerdict"]),
                })
              }
            />
          )}
          {filter.view === "security_events" && filter.eventType === "dlp" && (
            <>
              <Input
                aria-label={t("security.sessionFilter")}
                value={filter.sessionId ?? ""}
                onChange={(event) =>
                  changeFilter({ sessionId: event.target.value })
                }
                placeholder={t("security.sessionFilterPlaceholder")}
                className="h-8 w-[220px] flex-none rounded-full bg-card text-xs focus:border-border-active"
              />
              <Input
                aria-label={t("securityEvents.keywordHint")}
                title={t("securityEvents.keywordHint")}
                value={filter.keyword ?? ""}
                onChange={(event) =>
                  changeFilter({ keyword: event.target.value })
                }
                placeholder={t("security.searchPlaceholder")}
                className="h-8 w-[220px] flex-none rounded-full bg-card text-xs focus:border-border-active"
              />
            </>
          )}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              onFilterChange({
                view: filter.view,
                eventType:
                  filter.view === "tool_calls" ? "tool_risk" : undefined,
              });
              onPageChange(1);
            }}
          >
            {t("security.resetFilters")}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("security.refreshLogs")}
            title={t("security.refreshLogs")}
            aria-busy={tableLoading}
            onClick={refresh}
            disabled={isLoading || isRefreshing}
          >
            <RefreshCw
              className={
                "h-4 w-4 " + (isLoading || isRefreshing ? "animate-spin" : "")
              }
            />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("securityEvents.clearFiltered")}
            title={t("securityEvents.clearFiltered")}
            disabled={
              tableLoading || isError || isClearBusy || !eventsResult?.total
            }
            onClick={() => {
              setClearFailed(false);
              setClearPreview({
                filter: { ...queryFilter },
                total: eventsResult?.total,
              });
            }}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </div>
      {filter.view === "security_events" && filter.eventType === "dlp" && (
        <p className="text-xs text-muted-foreground">
          {t("securityEvents.keywordHint")}
        </p>
      )}
      {hasUpdates && (
        <p role="status" className="text-xs text-amber-600">
          {t("securityEvents.updatesAvailable")}
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        {t(
          filter.view === "tool_calls"
            ? "securityEvents.toolCallsTotal"
            : "securityEvents.listTotal",
        )}
        : {count(eventsResult?.total, isLoading, isError)}
      </p>
      <div
        className={isRefreshing ? "relative min-h-48" : "relative"}
        aria-busy={tableLoading}
      >
        {filter.view === "tool_calls" ? (
          <ToolRiskEventsPanel
            controlled={{
              events:
                tableLoading || isError
                  ? []
                  : (eventsResult?.items
                      .filter(
                        (
                          event,
                        ): event is SecurityEvent & {
                          eventType: "tool_risk";
                          record: ToolRiskEventRecord;
                        } => event.eventType === "tool_risk",
                      )
                      .map((event) => event.record) ?? []),
              isLoading: tableLoading,
              isError: !tableLoading && isError,
              onSelectEvent: selectTool,
            }}
            onCreateRule={createRule}
          />
        ) : (
          <div className="overflow-hidden rounded-2xl border border-border/90 bg-card shadow-sm">
            <div className="overflow-x-auto">
              <table
                aria-busy={tableLoading}
                className="w-full min-w-[1900px] table-fixed border-collapse text-left"
              >
                <colgroup>
                  <col className="w-56" />
                  <col className="w-56" />
                  <col className="w-32" />
                  <col className="w-56" />
                  <col className="w-48" />
                  <col className="w-28" />
                  <col className="w-32" />
                  <col className="w-36" />
                  <col className="w-64" />
                  <col className="w-32" />
                  <col className="w-12" />
                </colgroup>
                <thead>
                  <tr className="border-b text-xs text-muted-foreground">
                    {[
                      "security.firstSeenAt",
                      "security.lastSeenAt",
                      "security.triggerCount",
                      "security.sessionId",
                      "security.recordType",
                      "security.riskLevel",
                      "security.recordResult",
                      "security.aiService",
                      "security.outboundStatus",
                      "securityEvents.source",
                      "",
                    ].map((key, index) => (
                      <th
                        key={key || index}
                        className="whitespace-nowrap px-4 py-3.5 font-normal"
                      >
                        {key ? t(key) : null}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/60 text-sm">
                  {!tableLoading &&
                    !isError &&
                    eventsResult?.items.map((event) => (
                      <tr
                        key={event.key}
                        data-testid={`security-event-${event.key}`}
                        tabIndex={0}
                        className="cursor-pointer hover:bg-muted/50"
                        onClick={() => setSelected(event)}
                        onKeyDown={(key) => {
                          if (key.key === "Enter" || key.key === " ") {
                            key.preventDefault();
                            setSelected(event);
                          }
                        }}
                      >
                        <td className="whitespace-nowrap px-4 py-4 font-mono text-xs">
                          {new Date(event.timestamp).toLocaleString(
                            i18n.language,
                          )}
                        </td>
                        <td className="whitespace-nowrap px-4 py-4 font-mono text-xs">
                          {new Date(event.lastSeenAt).toLocaleString(
                            i18n.language,
                          )}
                        </td>
                        <td className="truncate px-4 py-4 font-mono">
                          {event.triggerCount.toLocaleString(i18n.language)}
                        </td>
                        <td className="px-4 py-4 font-mono text-xs">
                          <span
                            className="block truncate"
                            title={event.sessionId || undefined}
                          >
                            {event.sessionId ||
                              t(
                                event.eventType === "dlp"
                                  ? "security.sessionUnknown"
                                  : "securityEvents.notRecorded",
                              )}
                          </span>
                        </td>
                        <td className="px-4 py-4">
                          <span
                            className="block truncate"
                            title={securityEventName(t, event)}
                          >
                            {securityEventName(t, event)}
                          </span>
                        </td>
                        <td className="px-4 py-4">
                          {securityEventRiskLabel(t, event.riskLevel)}
                        </td>
                        <td className="px-4 py-4">
                          {securityEventResultLabel(t, event)}
                        </td>
                        <td className="px-4 py-4">
                          <span className="block truncate">
                            {event.appType
                              ? getAppLabel(event.appType)
                              : t("securityEvents.notRecorded")}
                          </span>
                        </td>
                        <td className="px-4 py-4 text-xs">
                          {securityEventOutboundLabel(t, event)}
                        </td>
                        <td className="px-4 py-4">
                          {t(
                            event.eventType === "dlp"
                              ? "securityEvents.dlp"
                              : "securityEvents.toolRisk",
                          )}
                        </td>
                        <td>
                          <ChevronRight className="h-4 w-4" />
                        </td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
            {!isRefreshing && status && (
              <div
                role="status"
                className="flex flex-col items-center justify-center px-4 py-14 text-center text-xs"
              >
                {isLoading ? (
                  t("security.loadingLogs")
                ) : isError ? (
                  t("securityEvents.queryError")
                ) : (
                  <>
                    <FileText className="mb-2 h-8 w-8" />
                    {t("security.noFilteredRecords")}
                  </>
                )}
              </div>
            )}
          </div>
        )}
        {isRefreshing && (
          <div
            role="status"
            data-testid="security-events-loading"
            className="absolute inset-0 flex min-h-32 flex-col items-center justify-center gap-2 rounded-2xl bg-card/95 text-xs text-muted-foreground"
          >
            <Loader2 className="h-5 w-5 animate-spin" aria-hidden="true" />
            {t("security.loadingLogs")}
          </div>
        )}
      </div>
      {(isError || summary.isError) && (
        <Button variant="outline" disabled={tableLoading} onClick={refresh}>
          {t("securityEvents.retry")}
        </Button>
      )}
      {!isLoading && !isError && eventsResult && eventsResult.total > 0 && (
        <div className="flex items-center justify-between text-xs text-muted-foreground">
          <span>
            {t("security.logPagination", {
              total: eventsResult.total,
              page,
              pages: Math.max(1, Math.ceil(eventsResult.total / pageSize)),
            })}
          </span>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={isRefreshing || page <= 1}
              onClick={() => onPageChange(page - 1)}
            >
              {t("security.prevPage")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={isRefreshing || page * pageSize >= eventsResult.total}
              onClick={() => onPageChange(page + 1)}
            >
              {t("security.nextPage")}
            </Button>
          </div>
        </div>
      )}
      <SecurityEventDetailsDialog
        key={selected?.key ?? "closed"}
        event={selected}
        onClose={() => setSelected(null)}
        onCreateRule={createRule}
        onOpenDlpRules={
          onOpenDlpRules
            ? () => {
                setSelected(null);
                onOpenDlpRules();
              }
            : undefined
        }
      />
      <Dialog
        open={clearPreview !== null}
        onOpenChange={(open) => {
          if (!open && !isClearBusy) setClearPreview(null);
        }}
      >
        <DialogContent
          zIndex="alert"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            cancelClearRef.current?.focus();
          }}
          onEscapeKeyDown={(event) => {
            if (isClearBusy) event.preventDefault();
          }}
          onPointerDownOutside={(event) => {
            if (isClearBusy) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("securityEvents.clearTitle")}</DialogTitle>
            <DialogDescription>
              {t("securityEvents.clearDescription", {
                count: clearPreview?.total,
              })}{" "}
              <span>{t("securityEvents.clearIrreversible")}</span>
            </DialogDescription>
          </DialogHeader>
          {clearPreview && (
            <div className="space-y-2 px-6 py-4 text-sm">
              <p>
                {t(
                  clearPreview.filter.view === "tool_calls"
                    ? "securityEvents.toolCallsView"
                    : "securityEvents.detailsView",
                )}{" "}
                ·{" "}
                {t(
                  clearPreview.filter.eventType === "dlp"
                    ? "securityEvents.dlp"
                    : clearPreview.filter.eventType === "tool_risk"
                      ? "securityEvents.toolRisk"
                      : "securityEvents.allSources",
                )}
              </p>
              <p>
                {t("securityEvents.timestampRange")}:{" "}
                {new Date(
                  clearPreview.filter.startTimestamp ?? 0,
                ).toLocaleString(i18n.language)}{" "}
                —{" "}
                {new Date(
                  clearPreview.filter.endTimestamp ?? Date.now(),
                ).toLocaleString(i18n.language)}
              </p>
              <p>
                {t(
                  clearPreview.filter.view === "tool_calls" &&
                    clearPreview.filter.toolVerdict !== "risky"
                    ? "securityEvents.clearIncludesNonRisk"
                    : "securityEvents.clearRiskOnly",
                )}
              </p>
              {clearFailed && (
                <p role="alert" className="text-destructive">
                  {t("securityEvents.clearFailed")}
                </p>
              )}
            </div>
          )}
          <DialogFooter>
            <Button
              variant="outline"
              ref={cancelClearRef}
              disabled={isClearBusy}
              onClick={() => setClearPreview(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button disabled={isClearBusy} onClick={() => void clear()}>
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
