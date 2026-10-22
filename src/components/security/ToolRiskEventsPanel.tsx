import { Fragment, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eraser, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useToolRiskEvents } from "@/hooks/useToolRisk";
import { ShellCommandListDialog } from "./ShellCommandListsPanel";
import { groupToolRiskEvents } from "./toolRiskEventGroups";
import {
  COMMAND_CLASSES,
  EVENT_REASON_CODES,
  KNOWN_REASON_CODES,
  OPERATION_TYPES,
  RISK_CATEGORIES,
  type CommandListType,
  type ToolRiskEventRecord,
  type ToolRiskRuleSeed,
  type ToolRiskVerdict,
} from "@/types/toolRisk";

function ruleSeed(record: ToolRiskEventRecord): ToolRiskRuleSeed {
  const commandClass = COMMAND_CLASSES.includes(record.commandClass as never)
    ? record.commandClass!
    : record.reasonCode === "unclassified_command"
      ? "unknown"
      : COMMAND_CLASSES.includes(record.reasonCode as never)
        ? (record.reasonCode as NonNullable<ToolRiskRuleSeed["commandClass"]>)
        : undefined;
  return {
    ...(OPERATION_TYPES.includes(record.operationCategory as never)
      ? {
          operationCategory:
            record.operationCategory as ToolRiskRuleSeed["operationCategory"],
        }
      : {}),
    ...(commandClass ? { commandClass } : {}),
    ...(RISK_CATEGORIES.includes(record.riskCategory as never)
      ? {
          riskCategory: record.riskCategory as ToolRiskRuleSeed["riskCategory"],
        }
      : {}),
    ...(record.severity === "medium" || record.severity === "high"
      ? { severity: record.severity }
      : {}),
    ...(KNOWN_REASON_CODES.includes(record.reasonCode as never)
      ? { reasonCode: record.reasonCode as ToolRiskRuleSeed["reasonCode"] }
      : {}),
  };
}

const PAGE_SIZE = 50;

function verdictBadgeVariant(
  verdict?: string | null,
): "destructive" | "secondary" | "outline" {
  if (verdict === "risky") return "destructive";
  if (verdict === "unchecked" || verdict === "uncertain") return "secondary";
  return "outline";
}

function formatTimestamp(ts: number): string {
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return String(ts);
  }
}

export function ToolRiskEventsPanel({
  onCreateRule,
}: { onCreateRule?: (seed: ToolRiskRuleSeed) => void } = {}) {
  const { t } = useTranslation();
  const [offset, setOffset] = useState(0);
  const [verdict, setVerdict] = useState<ToolRiskVerdict | "all">("risky");
  const [expandedGroups, setExpandedGroups] = useState<ReadonlySet<number>>(
    new Set(),
  );
  const [listDraft, setListDraft] = useState<{
    listType: CommandListType;
    summary: string;
  } | null>(null);
  const { events, isLoading, refetch, clearEvents, isClearing } =
    useToolRiskEvents(
      PAGE_SIZE,
      offset,
      verdict === "all" ? undefined : verdict,
    );
  const groups = groupToolRiskEvents(events);

  const hasPrev = offset > 0;
  const hasNext = events.length >= PAGE_SIZE;

  const renderEvent = (
    record: ToolRiskEventRecord,
    showId = false,
    groupCount?: number,
  ) => (
    <TableRow key={record.id} data-testid={`tool-risk-event-${record.id}`}>
      <TableCell className="text-xs text-muted-foreground">
        {formatTimestamp(record.timestamp)}
        {showId && <div>{t("toolRisk.eventRecordId", { id: record.id })}</div>}
        {groupCount != null && (
          <Button
            size="sm"
            variant="ghost"
            className="mt-1 h-auto whitespace-normal text-left text-xs"
            data-testid={`tool-risk-event-group-toggle-${record.id}`}
            aria-expanded={expandedGroups.has(record.id)}
            onClick={() =>
              setExpandedGroups((previous) => {
                const next = new Set(previous);
                if (next.has(record.id)) next.delete(record.id);
                else next.add(record.id);
                return next;
              })
            }
          >
            {t("toolRisk.eventSimilarRecords", { count: groupCount })}
          </Button>
        )}
      </TableCell>
      <TableCell className="text-xs">{record.clientCategory ?? "-"}</TableCell>
      <TableCell className="text-xs">
        {record.operationCategory ?? "-"}
      </TableCell>
      <TableCell
        className={`max-w-[280px] text-xs text-muted-foreground ${
          record.commandSummary?.trim()
            ? "truncate font-mono"
            : "whitespace-normal"
        }`}
        title={
          record.commandSummary?.trim() ? record.commandSummary : undefined
        }
      >
        {record.commandSummary?.trim()
          ? record.commandSummary
          : t("toolRisk.commandSummaryUnavailable", {
              defaultValue: "未采集；历史摘要无法恢复",
            })}
      </TableCell>
      <TableCell>
        <Badge
          variant={verdictBadgeVariant(record.verdict)}
          className="text-[10px]"
        >
          {record.verdict
            ? t(`toolRisk.verdict.${record.verdict}`, {
                defaultValue: record.verdict,
              })
            : "-"}
        </Badge>
      </TableCell>
      <TableCell className="text-xs">
        {record.riskCategory
          ? t(`toolRisk.category.${record.riskCategory}`, {
              defaultValue: record.riskCategory,
            })
          : "-"}
      </TableCell>
      <TableCell className="text-xs">
        {record.severity
          ? t(`toolRisk.severity.${record.severity}`, {
              defaultValue: record.severity,
            })
          : "-"}
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">
        {EVENT_REASON_CODES.find((code) => code === record.reasonCode)
          ? t(`toolRisk.reason.${record.reasonCode}`)
          : t("toolRisk.reasonUnknown")}
      </TableCell>
      <TableCell>
        <div className="flex flex-wrap gap-1">
          {(["whitelist", "blacklist"] as const).map((listType) => (
            <Button
              key={listType}
              size="sm"
              variant="ghost"
              data-testid={`tool-risk-${listType}-${record.id}`}
              disabled={!record.commandSummary?.trim()}
              onClick={() =>
                setListDraft({
                  listType,
                  summary: record.commandSummary!,
                })
              }
            >
              {t(`toolRisk.commandLists.addTo.${listType}`)}
            </Button>
          ))}
          {onCreateRule && (
            <Button
              size="sm"
              variant="ghost"
              data-testid={`tool-risk-create-rule-${record.id}`}
              onClick={() => onCreateRule(ruleSeed(record))}
            >
              {t("toolRisk.createRuleFromEvent")}
            </Button>
          )}
        </div>
      </TableCell>
    </TableRow>
  );

  return (
    <div className="rounded-2xl border border-border/90 bg-card shadow-sm">
      <div className="flex items-center justify-between border-b border-border/80 px-5 py-4">
        <div>
          <div className="text-sm font-semibold text-foreground">
            {t("toolRisk.eventsHeading", { defaultValue: "调用记录" })}
          </div>
          <div className="mt-0.5 text-xs text-muted-foreground">
            {t("toolRisk.eventsDescription", {
              defaultValue:
                "展示后端处理后的命令摘要：保留普通命令、路径和 URL，仅尽力替换可识别的隐私与敏感值；可能遗漏，请勿视为完整脱敏保证。",
            })}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => void refetch()}
          >
            <RefreshCw className="h-4 w-4" />
            {t("common.refresh", { defaultValue: "刷新" })}
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            data-testid="tool-risk-events-clear"
            disabled={isClearing}
            onClick={() =>
              void clearEvents()
                .then(() => {
                  setOffset(0);
                  setExpandedGroups(new Set());
                })
                .catch(() => {
                  /* The hook already reports the mutation error. */
                })
            }
          >
            <Eraser className="h-4 w-4" />
            {t("toolRisk.clearAllEvents")}
          </Button>
        </div>
      </div>

      <div className="space-y-2 border-b border-border/80 px-5 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <label
            htmlFor="tool-risk-events-verdict"
            className="text-xs font-medium"
          >
            {t("toolRisk.eventVerdictFilter")}
          </label>
          <Select
            value={verdict}
            onValueChange={(value) => {
              if (
                ![
                  "all",
                  "risky",
                  "unchecked",
                  "uncertain",
                  "low_risk",
                ].includes(value)
              )
                return;
              setVerdict(value as ToolRiskVerdict | "all");
              setOffset(0);
              setExpandedGroups(new Set());
            }}
          >
            <SelectTrigger
              id="tool-risk-events-verdict"
              data-testid="tool-risk-events-verdict-filter"
              className="h-8 w-auto min-w-36 text-xs"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="risky">
                {t("toolRisk.verdict.risky")}
              </SelectItem>
              <SelectItem value="all">
                {t("toolRisk.eventFilterAll")}
              </SelectItem>
              <SelectItem value="unchecked">
                {t("toolRisk.verdict.unchecked")}
              </SelectItem>
              <SelectItem value="uncertain">
                {t("toolRisk.verdict.uncertain")}
              </SelectItem>
              <SelectItem value="low_risk">
                {t("toolRisk.verdict.low_risk")}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("toolRisk.eventRetentionHint")}
        </p>
        <p className="text-xs text-muted-foreground">
          {t("toolRisk.eventPageGroupingHint")}
        </p>
      </div>

      <Table data-testid="tool-risk-events-table">
        <TableHeader>
          <TableRow>
            <TableHead>
              {t("toolRisk.colTime", { defaultValue: "时间" })}
            </TableHead>
            <TableHead>
              {t("toolRisk.colClient", { defaultValue: "客户端" })}
            </TableHead>
            <TableHead>
              {t("toolRisk.colOperation", { defaultValue: "操作" })}
            </TableHead>
            <TableHead>
              {t("toolRisk.colCommandSummary", { defaultValue: "命令摘要" })}
            </TableHead>
            <TableHead>
              {t("toolRisk.colVerdict", { defaultValue: "判定" })}
            </TableHead>
            <TableHead>
              {t("toolRisk.colCategory", { defaultValue: "风险类别" })}
            </TableHead>
            <TableHead>
              {t("toolRisk.colSeverity", { defaultValue: "严重度" })}
            </TableHead>
            <TableHead>{t("toolRisk.colReason")}</TableHead>
            <TableHead>{t("toolRisk.colActions")}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {isLoading ? (
            <TableRow>
              <TableCell
                colSpan={9}
                className="py-8 text-center text-sm text-muted-foreground"
              >
                {t("common.loading", { defaultValue: "加载中…" })}
              </TableCell>
            </TableRow>
          ) : events.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={9}
                className="py-8 text-center text-sm text-muted-foreground"
              >
                {t("toolRisk.noEvents", { defaultValue: "暂无调用记录" })}
              </TableCell>
            </TableRow>
          ) : (
            groups.map(({ representative, members }) => (
              <Fragment key={representative.id}>
                {renderEvent(
                  representative,
                  members.length > 1 && expandedGroups.has(representative.id),
                  members.length > 1 ? members.length : undefined,
                )}
                {members.length > 1 &&
                  expandedGroups.has(representative.id) &&
                  members.slice(1).map((member) => renderEvent(member, true))}
              </Fragment>
            ))
          )}
        </TableBody>
      </Table>

      {(hasPrev || hasNext) && (
        <div className="flex items-center justify-end gap-2 border-t border-border/80 px-5 py-3">
          <Button
            size="sm"
            variant="outline"
            disabled={!hasPrev}
            onClick={() => {
              setOffset((o) => Math.max(0, o - PAGE_SIZE));
              setExpandedGroups(new Set());
            }}
          >
            {t("common.prev", { defaultValue: "上一页" })}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={!hasNext}
            onClick={() => {
              setOffset((o) => o + PAGE_SIZE);
              setExpandedGroups(new Set());
            }}
          >
            {t("common.next", { defaultValue: "下一页" })}
          </Button>
        </div>
      )}
      <ShellCommandListDialog
        open={listDraft != null}
        initialListType={listDraft?.listType}
        commandSummary={listDraft?.summary}
        onOpenChange={(open) => !open && setListDraft(null)}
      />
    </div>
  );
}
