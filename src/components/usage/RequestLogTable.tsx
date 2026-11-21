import { getUsageModelDisplayName } from "./modelOptions";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useRequestLogs } from "@/lib/query/usage";
import {
  getFreshInputTokens,
  isUnpricedUsage,
  type LogFilters,
  type UsageRangeSelection,
} from "@/types/usage";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { UsageDateRangePicker } from "./UsageDateRangePicker";
import { UsageSourceLabel } from "./UsageSourceLabel";
import {
  formatOutputTokensPerSecond,
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";

interface RequestLogTableProps {
  modelNames?: Readonly<Record<string, string>>;
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  onRangeChange?: (range: UsageRangeSelection) => void;
}

export function RequestLogTable({
  modelNames = {},
  range,
  rangeLabel,
  appType: dashboardAppType,
  providerName,
  model,
  refreshIntervalMs,
  onRangeChange,
}: RequestLogTableProps) {
  const { t, i18n } = useTranslation();
  const modelName = (id: string) =>
    getUsageModelDisplayName(id, modelNames, (shortId) =>
      t("usage.unknownRoutingModelOption", { id: shortId }),
    );

  // 应用/Provider/模型筛选已上移到 Dashboard 顶栏（全局生效）；
  // 这里只保留日志特有的状态码筛选。
  const [statusCode, setStatusCode] = useState<number | undefined>(undefined);
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("");
  const tableRegion = useRef<HTMLDivElement>(null);
  const lastVisiblePage = useRef<number>();
  const pageSize = 20;

  const effectiveFilters: LogFilters = {
    appType:
      dashboardAppType && dashboardAppType !== "all"
        ? dashboardAppType
        : undefined,
    providerName,
    model,
    statusCode,
  };

  const {
    data: result,
    isLoading,
    isFetching,
    isPlaceholderData,
    isError,
    refetch,
  } = useRequestLogs({
    filters: effectiveFilters,
    range,
    page,
    pageSize,
    options: {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  });

  const logs = result?.data ?? [];
  const total = result?.total ?? 0;
  const totalPages = Math.ceil(total / pageSize);
  const displayedPage = result?.page ?? page;
  const changingPage = isPlaceholderData === true;

  useEffect(() => {
    if (!result || changingPage) return;
    const lastPage = Math.max(0, Math.ceil(result.total / pageSize) - 1);
    if (page > lastPage) setPage(lastPage);
  }, [result, changingPage, page]);

  useLayoutEffect(() => {
    if (!result || changingPage || result.page !== page) return;
    if (
      lastVisiblePage.current !== undefined &&
      lastVisiblePage.current !== result.page
    ) {
      // A short final page must stay visible in the workspace's scroll area.
      tableRegion.current?.scrollIntoView?.({ block: "start" });
    }
    lastVisiblePage.current = result.page;
  }, [result, changingPage, page]);

  useEffect(() => {
    setPage(0);
  }, [
    dashboardAppType,
    providerName,
    model,
    range.customEndDate,
    range.customStartDate,
    range.preset,
  ]);

  const handleGoToPage = () => {
    const trimmed = pageInput.trim();
    if (!/^\d+$/.test(trimmed)) return;
    const parsed = Number(trimmed);
    if (parsed < 1 || parsed > totalPages) return;
    setPage(parsed - 1);
    setPageInput("");
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  return (
    <div className="space-y-4" aria-busy={isFetching}>
      <div className="stats-surface stats-log-filters">
        <div>
          <span>{t("usage.status")}</span>
          <Select
            value={statusCode?.toString() || "all"}
            onValueChange={(v) => {
              const parsed = Number.parseInt(v, 10);
              setStatusCode(
                v === "all" || !Number.isFinite(parsed) ? undefined : parsed,
              );
              setPage(0);
            }}
          >
            <SelectTrigger
              aria-label={t("usage.status")}
              className="h-[34px] w-full bg-card text-xs"
            >
              <SelectValue placeholder={t("usage.statusCode")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("common.all")}</SelectItem>
              <SelectItem value="200">200 OK</SelectItem>
              <SelectItem value="400">400</SelectItem>
              <SelectItem value="401">401</SelectItem>
              <SelectItem value="429">429</SelectItem>
              <SelectItem value="500">500</SelectItem>
            </SelectContent>
          </Select>
        </div>
        {onRangeChange && (
          <div>
            <span>{t("usage.timeRange")}</span>
            <UsageDateRangePicker
              triggerClassName="h-[34px] w-full"
              selection={range}
              triggerLabel={rangeLabel}
              onApply={onRangeChange}
            />
          </div>
        )}
      </div>

      {isError && (
        <div role="alert" className="flex items-center gap-2 text-sm">
          <span>{t("usage.queryFailed")}</span>
          <Button variant="outline" size="sm" onClick={() => void refetch()}>
            {t("common.retry")}
          </Button>
        </div>
      )}
      {changingPage && (
        <div role="status" className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      )}
      {isLoading ? (
        <div
          role="status"
          aria-label={t("common.loading")}
          className="h-[400px] rounded-md bg-muted motion-safe:animate-pulse"
        />
      ) : (
        <>
          <div ref={tableRegion} className="stats-surface overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="text-left whitespace-nowrap">
                    {t("usage.time")}
                  </TableHead>
                  <TableHead className="text-left whitespace-nowrap">
                    {t("usage.providerOrSource")}
                  </TableHead>
                  <TableHead className="text-left whitespace-nowrap">
                    {t("usage.billingModel")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap px-1.5">
                    {t("usage.inputTokens")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap px-1.5">
                    {t("usage.outputTokens")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap px-1.5">
                    {t("usage.totalCost")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap">
                    {t("usage.timingInfo")}
                  </TableHead>
                  <TableHead className="text-left whitespace-nowrap">
                    {t("usage.status")}
                  </TableHead>
                  <TableHead className="text-left whitespace-nowrap">
                    {t("usage.source", { defaultValue: "Source" })}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {logs.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={9}
                      className="text-center text-muted-foreground"
                    >
                      {t("usage.noData")}
                    </TableCell>
                  </TableRow>
                ) : (
                  logs.map((log) => {
                    const unpriced = isUnpricedUsage(log);
                    return (
                      <TableRow key={log.requestId} className="h-14">
                        <TableCell className="text-left whitespace-nowrap text-xs px-1.5">
                          {new Date(log.createdAt * 1000).toLocaleString(
                            locale,
                            {
                              month: "2-digit",
                              day: "2-digit",
                              hour: "2-digit",
                              minute: "2-digit",
                            },
                          )}
                        </TableCell>
                        <TableCell className="text-left">
                          <UsageSourceLabel source={log} />
                        </TableCell>
                        <TableCell className="text-left font-mono text-xs min-w-[160px] max-w-[260px]">
                          <div
                            title={`${t("usage.requestModel")}: ${modelName(log.requestModel || log.model)} → ${t("usage.responseModel")}: ${modelName(log.model)}`}
                          >
                            <span className="block truncate">
                              {modelName(log.pricingModel || log.model)}
                            </span>
                            {((log.requestModel &&
                              log.requestModel !==
                                (log.pricingModel || log.model)) ||
                              log.model !==
                                (log.pricingModel || log.model)) && (
                              <div
                                className="truncate text-[10px] text-muted-foreground"
                                aria-label={t("usage.requestResponseModel")}
                              >
                                {log.requestModel &&
                                log.requestModel !== log.model
                                  ? `${modelName(log.requestModel)} → ${modelName(log.model)}`
                                  : modelName(log.model)}
                              </div>
                            )}
                          </div>
                        </TableCell>
                        <TableCell className="text-right px-1.5">
                          {(() => {
                            const freshInput = getFreshInputTokens(log);
                            const isCacheInclusive =
                              log.inputTokens !== freshInput;
                            return (
                              <div
                                className="tabular-nums"
                                title={
                                  isCacheInclusive
                                    ? `Raw: ${log.inputTokens.toLocaleString()}`
                                    : undefined
                                }
                              >
                                {fmtInt(freshInput, locale)}
                              </div>
                            );
                          })()}
                          {(log.cacheReadTokens > 0 ||
                            log.cacheCreationTokens > 0) && (
                            <div className="text-[10px] text-muted-foreground whitespace-nowrap">
                              {[
                                log.cacheReadTokens > 0 &&
                                  `R${fmtInt(log.cacheReadTokens, locale)}`,
                                log.cacheCreationTokens > 0 &&
                                  `W${fmtInt(log.cacheCreationTokens, locale)}`,
                              ]
                                .filter(Boolean)
                                .join("·")}
                            </div>
                          )}
                        </TableCell>
                        <TableCell className="text-right px-1.5">
                          <div className="tabular-nums">
                            {fmtInt(log.outputTokens, locale)}
                            {(() => {
                              const tpsStr = formatOutputTokensPerSecond(log);
                              if (tpsStr == null) return null;
                              return (
                                <span className="text-muted-foreground text-xs">
                                  /{tpsStr} tps
                                </span>
                              );
                            })()}
                          </div>
                        </TableCell>
                        <TableCell className="text-right px-1.5">
                          <div
                            className={`font-medium tabular-nums ${
                              unpriced ? "text-muted-foreground" : ""
                            }`}
                          >
                            {unpriced
                              ? t("usage.unpriced", "未定价")
                              : fmtUsd(log.totalCostUsd, 4)}
                          </div>
                          {parseFiniteNumber(log.costMultiplier) != null &&
                            parseFiniteNumber(log.costMultiplier) !== 1 && (
                              <div className="text-[11px] text-muted-foreground">
                                ×
                                {parseFiniteNumber(log.costMultiplier)?.toFixed(
                                  2,
                                )}
                              </div>
                            )}
                        </TableCell>
                        <TableCell className="text-right whitespace-nowrap text-xs tabular-nums">
                          {(log.latencyMs / 1000).toFixed(1)}s
                          {log.firstTokenMs != null && (
                            <span className="text-muted-foreground">
                              /{(log.firstTokenMs / 1000).toFixed(1)}s
                            </span>
                          )}
                        </TableCell>
                        <TableCell className="text-left">
                          <Badge
                            className="tabular-nums"
                            variant={
                              log.statusCode >= 200 && log.statusCode < 300
                                ? "success"
                                : "destructive"
                            }
                          >
                            {log.statusCode}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-left text-xs text-muted-foreground">
                          {log.dataSource || "proxy"}
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
          </div>

          <div className="flex items-center justify-between text-sm text-muted-foreground">
            <span>{t("usage.totalRecords", { total })}</span>
            <div className="flex items-center gap-1">
              <Button
                size="sm"
                variant="outline"
                disabled={changingPage || page === 0}
                onClick={() => setPage((p) => Math.max(0, p - 1))}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              {(() => {
                const pages: (number | string)[] = [];
                if (totalPages <= 9) {
                  for (let i = 0; i < totalPages; i++) pages.push(i);
                } else {
                  const pageSet = new Set<number>();
                  for (let i = 0; i < 3; i++) pageSet.add(i);
                  for (let i = totalPages - 3; i < totalPages; i++)
                    pageSet.add(i);
                  for (
                    let i = Math.max(0, page - 1);
                    i <= Math.min(totalPages - 1, page + 1);
                    i++
                  )
                    pageSet.add(i);
                  const sorted = Array.from(pageSet).sort((a, b) => a - b);
                  for (let i = 0; i < sorted.length; i++) {
                    if (i > 0 && sorted[i] - sorted[i - 1] > 1) {
                      pages.push(`ellipsis-${i}`);
                    }
                    pages.push(sorted[i]);
                  }
                }
                return pages.map((p) =>
                  typeof p === "string" ? (
                    <span key={p} className="px-2 text-muted-foreground">
                      ...
                    </span>
                  ) : (
                    <Button
                      key={p}
                      variant={p === displayedPage ? "default" : "outline"}
                      aria-current={p === displayedPage ? "page" : undefined}
                      disabled={changingPage}
                      size="sm"
                      className="h-8 w-8 p-0"
                      onClick={() => setPage(p)}
                    >
                      {p + 1}
                    </Button>
                  ),
                );
              })()}
              <Button
                size="sm"
                variant="outline"
                disabled={changingPage || page >= totalPages - 1}
                onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
              <div className="flex items-center gap-1 ml-2">
                <Input
                  type="text"
                  disabled={changingPage}
                  value={pageInput}
                  onChange={(e) => setPageInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleGoToPage();
                  }}
                  placeholder={t("usage.pageInputPlaceholder")}
                  className="h-8 w-16 text-center text-xs"
                />
                <Button
                  variant="outline"
                  size="sm"
                  disabled={changingPage}
                  onClick={handleGoToPage}
                >
                  {t("usage.goToPage")}
                </Button>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
