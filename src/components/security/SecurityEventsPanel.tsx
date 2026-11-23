import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronLeft,
  ChevronRight,
  FileText,
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
import { SecurityEventsLoading } from "./SecurityEventsLoading";
import { SecurityMetric, securityMetricOrder } from "./SecurityMetric";
import { ToolRiskEventsPanel } from "./ToolRiskEventsPanel";
import {
  securityEventName,
  securityEventOutboundLabel,
  securityEventResultLabel,
  securityEventRiskLabel,
  securityEventSessionId,
} from "./securityEventLabels";

const overviewColumns = [
  ["security.firstSeenAt", 176],
  ["security.aiService", 110],
  ["security.recordType", 148],
  ["security.riskLevel", 104],
  ["security.recordResult", 120],
] as const;
const auditColumns = [
  ...overviewColumns,
  ["security.lastSeenAt", 176],
  ["security.triggerCount", 112],
  ["security.sessionId", 150],
  ["security.outboundStatus", 148],
  ["securityEvents.source", 104],
  ["", 48],
] as const;
const auditTableMinWidth = auditColumns.reduce(
  (total, [, width]) => total + width,
  0,
);

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
      <SelectTrigger aria-label={label} className="min-w-0">
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
  const toolCallsCount = useSecurityEvents(
    { view: "tool_calls", ...timeFilter },
    1,
    1,
  );
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
        toolCallsCount.refetch(),
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
  const totalRecordCount =
    summary.data?.dlpCount !== undefined &&
    toolCallsCount.eventsResult?.total !== undefined
      ? summary.data.dlpCount + toolCallsCount.eventsResult.total
      : undefined;
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
    "workbuddy-ai",
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
  const clearDisabled =
    tableLoading || isError || isClearBusy || !eventsResult?.total;
  const hasEventRows = !isError && Boolean(eventsResult?.items.length);
  const columns = hasEventRows ? auditColumns : overviewColumns;
  const status = !hasEventRows;
  return (
    <section
      className="sc-events-panel"
      role="tabpanel"
      aria-label={t("security.tabRecords")}
    >
      <div className="sc-surface sc-overview">
        <dl className="sc-metrics">
          {securityMetricOrder.map((metric) => (
            <SecurityMetric
              key={metric}
              metric={metric}
              value={summaryCount(summary.data?.[metric])}
            />
          ))}
        </dl>
        <div className="sc-summary">
          <p className="text-xs text-muted-foreground">
            {t("securityEvents.summaryScope")}
          </p>
          <div className="flex flex-wrap gap-x-4 gap-y-1">
            <span data-testid="security-events-summary-total">
              {t("securityEvents.totalRecords")}:{" "}
              {count(
                totalRecordCount,
                summary.isLoading || toolCallsCount.isLoading,
                summary.isError || toolCallsCount.isError,
              )}
            </span>
            <span data-testid="security-events-summary-dlp">
              {t("securityEvents.dlpCount")}:{" "}
              {summaryCount(summary.data?.dlpCount)}
            </span>
            <span data-testid="security-events-summary-tool-risk">
              {t("securityEvents.toolRiskCount")}:{" "}
              {count(
                toolCallsCount.eventsResult?.total,
                toolCallsCount.isLoading,
                toolCallsCount.isError,
              )}
            </span>
          </div>
        </div>
      </div>
      <div className="sc-surface sc-events-directory">
        <div className="sc-record-controls">
          <div className="sc-record-toolbar">
            <div
              className="sc-view-picker"
              role="tablist"
              aria-label={t("security.tabRecords")}
            >
              {(["security_events", "tool_calls"] as const).map((view) => (
                <Button
                  key={view}
                  role="tab"
                  aria-selected={filter.view === view}
                  size="sm"
                  className="aria-selected:bg-card aria-selected:text-primary"
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
            <div className="sc-filter-actions">
              <Button
                variant="outline"
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
              <span className="inline-flex" title={t("security.refreshLogs")}>
                <Button
                  variant="outline"
                  size="icon"
                  aria-label={t("security.refreshLogs")}
                  title={t("security.refreshLogs")}
                  aria-busy={tableLoading}
                  onClick={refresh}
                  disabled={isLoading || isRefreshing}
                >
                  <RefreshCw
                    className={
                      "h-4 w-4 " +
                      (isLoading || isRefreshing ? "animate-spin" : "")
                    }
                  />
                </Button>
              </span>
              <span
                className="inline-flex"
                title={t("securityEvents.clearFiltered")}
              >
                <Button
                  variant="outline"
                  size="icon"
                  aria-label={t("securityEvents.clearFiltered")}
                  className="text-destructive"
                  title={t("securityEvents.clearFiltered")}
                  disabled={clearDisabled}
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
              </span>
            </div>
          </div>
          <div className="sc-filters">
            <UsageDateRangePicker
              selection={range}
              triggerLabel={
                rangeLabels[range.preset] ??
                getUsageRangePresetLabel(range.preset, t)
              }
              presets={["today", "7d", "30d"]}
              presetLabels={rangeLabels}
              triggerClassName="w-full min-w-0"
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
                  {
                    value: "tool_risk",
                    label: t("securityEvents.toolRisk"),
                  },
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
                        eventType: value.startsWith("dlp:")
                          ? "dlp"
                          : "tool_risk",
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
                changeFilter({
                  appType: value === "all" ? undefined : value,
                })
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
                    (value) => ({
                      value,
                      label: t(`toolRisk.verdict.${value}`),
                    }),
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
            {filter.view === "security_events" &&
              filter.eventType === "dlp" && (
                <>
                  <Input
                    aria-label={t("security.sessionFilter")}
                    value={filter.sessionId ?? ""}
                    onChange={(event) =>
                      changeFilter({ sessionId: event.target.value })
                    }
                    placeholder={t("security.sessionFilterPlaceholder")}
                    className="h-[var(--filter-height)] w-full min-w-0 text-[13px] focus:ring-0"
                  />
                  <Input
                    aria-label={t("securityEvents.keywordHint")}
                    title={t("securityEvents.keywordHint")}
                    value={filter.keyword ?? ""}
                    onChange={(event) =>
                      changeFilter({ keyword: event.target.value })
                    }
                    placeholder={t("security.searchPlaceholder")}
                    className="h-[var(--filter-height)] w-full min-w-0 text-[13px] focus:ring-0"
                  />
                </>
              )}
          </div>
          {filter.view === "security_events" && filter.eventType === "dlp" && (
            <p className="text-xs text-muted-foreground">
              {t("securityEvents.keywordHint")}
            </p>
          )}
          {hasUpdates && (
            <p role="status" className="text-xs text-[var(--warning)]">
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
        </div>
        <div aria-busy={tableLoading}>
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
            <div>
              <div className="overflow-x-auto">
                <table
                  aria-busy={tableLoading}
                  className="w-full table-fixed border-collapse text-left"
                  style={
                    hasEventRows ? { minWidth: auditTableMinWidth } : undefined
                  }
                >
                  {hasEventRows && (
                    <colgroup>
                      {columns.map(([key, width]) => (
                        <col key={key} style={{ width }} />
                      ))}
                    </colgroup>
                  )}
                  <thead>
                    <tr className="border-b border-border text-xs text-muted-foreground">
                      {columns.map(([key], index) => (
                        <th
                          key={key || index}
                          className={
                            hasEventRows ? "whitespace-nowrap" : undefined
                          }
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
                          <td className="sc-event-time">
                            {new Date(event.timestamp).toLocaleString(
                              i18n.language,
                            )}
                          </td>
                          <td className="px-3 py-3">
                            <span className="block truncate">
                              {event.appType
                                ? getAppLabel(event.appType)
                                : t("securityEvents.notRecorded")}
                            </span>
                          </td>
                          <td className="px-3 py-3">
                            <span
                              className="block truncate"
                              title={securityEventName(t, event)}
                            >
                              {securityEventName(t, event)}
                            </span>
                          </td>
                          <td className="px-3 py-3">
                            {securityEventRiskLabel(t, event.riskLevel)}
                          </td>
                          <td className="px-3 py-3">
                            {securityEventResultLabel(t, event)}
                          </td>
                          <td className="sc-event-time">
                            {new Date(event.lastSeenAt).toLocaleString(
                              i18n.language,
                            )}
                          </td>
                          <td className="truncate px-3 py-3 font-mono">
                            {event.triggerCount.toLocaleString(i18n.language)}
                          </td>
                          <td className="px-3 py-3 font-mono text-xs">
                            <span
                              className="block truncate"
                              title={securityEventSessionId(event) || undefined}
                            >
                              {securityEventSessionId(event) ||
                                t(
                                  event.eventType === "dlp"
                                    ? "security.sessionUnknown"
                                    : "securityEvents.notRecorded",
                                )}
                            </span>
                          </td>
                          <td className="px-3 py-3 text-xs">
                            {securityEventOutboundLabel(t, event)}
                          </td>
                          <td className="px-3 py-3">
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
              {tableLoading ? (
                <SecurityEventsLoading />
              ) : status ? (
                <div
                  role="status"
                  className="flex flex-col items-center justify-center px-4 py-16 text-center text-sm"
                >
                  {isError ? (
                    t("securityEvents.queryError")
                  ) : (
                    <>
                      <FileText className="mb-2 h-6 w-6 text-muted-foreground" />
                      {t("security.noFilteredRecords")}
                    </>
                  )}
                </div>
              ) : null}
            </div>
          )}
        </div>
        {(isError || summary.isError || toolCallsCount.isError) && (
          <Button variant="outline" disabled={tableLoading} onClick={refresh}>
            {t("securityEvents.retry")}
          </Button>
        )}
        {!isLoading && !isError && eventsResult && eventsResult.total > 0 && (
          <div className="sc-pagination">
            <span>
              {t("security.logPagination", {
                total: eventsResult.total,
                page,
                pages: Math.max(1, Math.ceil(eventsResult.total / pageSize)),
              })}
            </span>
            <div className="sc-event-page-buttons">
              <Button
                variant="outline"
                size="icon"
                aria-label={t("security.prevPage")}
                title={t("security.prevPage")}
                disabled={isRefreshing || page <= 1}
                onClick={() => onPageChange(page - 1)}
              >
                <ChevronLeft aria-hidden="true" />
              </Button>
              <Button
                variant="outline"
                size="icon"
                aria-label={t("security.nextPage")}
                title={t("security.nextPage")}
                disabled={isRefreshing || page * pageSize >= eventsResult.total}
                onClick={() => onPageChange(page + 1)}
              >
                <ChevronRight aria-hidden="true" />
              </Button>
            </div>
          </div>
        )}
      </div>
      <SecurityEventDetailsDialog
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
