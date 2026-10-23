import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CheckCircle2,
  ChevronRight,
  FileText,
  FlaskConical,
  Loader2,
  Plus,
  Pencil,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { APP_ICON_MAP, getAppLabel } from "@/config/appConfig";
import { Button } from "@/components/ui/button";
import { SlidingSegmentedControl } from "@/components/ui/sliding-segmented-control";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  useSecurityAuditLogs,
  useSecurityRules,
} from "@/hooks/useSecurityAudit";
import { securityApi } from "@/lib/api";
import {
  getBuiltinRuleDescription,
  getBuiltinRuleName,
} from "@/lib/securityRuleLocalization";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import type {
  AuditAction,
  AuditLogRecord,
  CustomSecurityRule,
  RiskLevel,
  RuleMatchResult,
  SecurityRuleItem,
  RuleType,
} from "@/types/security";
import { UsageDateRangePicker } from "@/components/usage/UsageDateRangePicker";
import type { UsageRangeSelection, UsageRangePreset } from "@/types/usage";
import { ToolRiskRulesPanel } from "@/components/security/ToolRiskRulesPanel";
import { toast } from "sonner";

type SecurityTab = "records" | "rules" | "toolRisk";

const MIN_REFRESH_LOADING_MS = 300;

type LogFilter = {
  sessionId?: string;
  appType?: string;
  riskLevel?: string;
  ruleCategory?: string;
  action?: string;
  keyword?: string;
};

type FilterOption = {
  value: string;
  label: string;
};

function createEmptyRule(): CustomSecurityRule {
  return {
    id: "",
    name: "",
    ruleType: "keyword",
    pattern: "",
    riskLevel: "HIGH",
    action: "redact",
    enabled: true,
    description: "",
    updatedAt: Date.now(),
  };
}

function MetricCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-2xl border border-border/90 bg-card p-5 shadow-sm transition-colors hover:border-border">
      <div className="text-3xl font-extrabold tracking-tight text-foreground sm:text-4xl">
        {value}
      </div>
      <div className="mt-1.5 text-xs font-medium text-muted-foreground sm:text-sm">
        {label}
      </div>
    </div>
  );
}

function FilterSelect({
  label,
  value,
  options,
  onChange,
  ariaLabel,
}: {
  label: string;
  value: string;
  options: FilterOption[];
  onChange: (value: string) => void;
  ariaLabel: string;
}) {
  const selected = options.find((option) => option.value === value);

  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger
        aria-label={ariaLabel}
        className="h-8 w-auto min-w-[108px] flex-none rounded-full border-border bg-card px-3.5 text-xs font-medium shadow-sm"
      >
        <SelectValue>
          {label}: {selected?.label ?? value}
        </SelectValue>
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {label}: {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function AppMark({ appType }: { appType: string }) {
  const app = APP_ICON_MAP[appType as keyof typeof APP_ICON_MAP];
  if (app?.icon) {
    return (
      <span className="flex h-5 w-5 shrink-0 items-center justify-center">
        {app.icon}
      </span>
    );
  }
  return (
    <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-muted text-[10px] font-bold text-muted-foreground">
      {getAppLabel(appType).slice(0, 1).toUpperCase()}
    </span>
  );
}

export function SecurityCenterPage({
  activeApp = "claude",
}: {
  activeApp?: string;
}) {
  const { t, i18n } = useTranslation();
  const [range, setRange] = useState<UsageRangeSelection>({ preset: "today" });
  const { startDate, endDate } = resolveUsageRange(range);
  const timeFilter = {
    startTimestamp: startDate * 1000,
    // Open-ended ranges keep new events visible on refresh; fixed custom ranges
    // include every millisecond of the selected final second.
    endTimestamp:
      range.preset === "custom" && !range.liveEndTime
        ? endDate * 1000 + 999
        : undefined,
  };
  const rangeLabels: Partial<Record<UsageRangePreset, string>> = {
    today: t("security.todayFilter"),
    "7d": t("security.last7Days"),
    "30d": t("security.last30Days"),
    custom: t("security.customDateRange"),
  };
  const rangeLabel =
    rangeLabels[range.preset] ?? getUsageRangePresetLabel(range.preset, t);

  const [activeTab, setActiveTab] = useState<SecurityTab>("records");
  const [logPage, setLogPage] = useState(1);
  const [logFilter, setLogFilter] = useState<LogFilter>({});
  const [isRefreshingLogs, setIsRefreshingLogs] = useState(false);
  const [isClearLogsDialogOpen, setIsClearLogsDialogOpen] = useState(false);
  const [selectedLogSnapshot, setSelectedLog] = useState<AuditLogRecord | null>(
    null,
  );
  const [isRuleModalOpen, setIsRuleModalOpen] = useState(false);
  const [ruleToDelete, setRuleToDelete] = useState<SecurityRuleItem | null>(
    null,
  );
  const [isDeletingRule, setIsDeletingRule] = useState(false);
  const [editingRule, setEditingRule] =
    useState<CustomSecurityRule>(createEmptyRule);
  const [sandboxText, setSandboxText] = useState("");
  const [sandboxResults, setSandboxResults] = useState<
    RuleMatchResult[] | null
  >(null);
  const [isTestingSandbox, setIsTestingSandbox] = useState(false);

  const {
    rules,
    isLoading: isRulesLoading,
    toggleRule,
    saveRule,
    deleteRule,
    updateBuiltinAction,
    updateCustomAction,
    isSaving,
  } = useSecurityRules();
  const {
    logsResult,
    isError: isLogsError,
    isLoading: isLogsLoading,
    refetch: refetchLogs,
    clearLogs,
    isClearing,
  } = useSecurityAuditLogs({ ...logFilter, ...timeFilter }, logPage, 20);

  const isTableLoading = isLogsLoading || isRefreshingLogs;

  const selectedLog =
    logsResult?.items.find((log) => log.id === selectedLogSnapshot?.id) ??
    selectedLogSnapshot;

  const rangeSummary = useSecurityAuditLogs(timeFilter, 1, 100);
  const redactedInRange = useSecurityAuditLogs(
    { action: "redact", ...timeFilter },
    1,
    1,
  );
  const blockedInRange = useSecurityAuditLogs(
    { action: "block", ...timeFilter },
    1,
    1,
  );
  const auditedInRange = useSecurityAuditLogs(
    { action: "audit", ...timeFilter },
    1,
    1,
  );

  const displayCount = (value: number | undefined, loading: boolean) => {
    if (loading) return "…";
    if (value === undefined) return "—";
    return value.toLocaleString(i18n.language);
  };

  const rangeSensitiveTypes = rangeSummary.logsResult
    ? new Set(
        rangeSummary.logsResult.items.map(
          (log) => log.ruleCategory || log.ruleName,
        ),
      ).size
    : undefined;
  const rangeTotal = rangeSummary.logsResult?.total;
  const filteredTotal = logsResult?.total;
  const statsLoading =
    rangeSummary.isLoading ||
    redactedInRange.isLoading ||
    blockedInRange.isLoading ||
    auditedInRange.isLoading;

  const getRiskLabel = (riskLevel: RiskLevel) => {
    if (riskLevel === "CRITICAL" || riskLevel === "HIGH") {
      return t("security.levelHighShort");
    }
    if (riskLevel === "MEDIUM") {
      return t("security.levelMediumShort");
    }
    return t("security.levelLowShort");
  };

  const getRiskClass = (riskLevel: RiskLevel) => {
    if (riskLevel === "CRITICAL" || riskLevel === "HIGH") {
      return "text-rose-600 dark:text-rose-400";
    }
    if (riskLevel === "MEDIUM") {
      return "text-amber-600 dark:text-amber-400";
    }
    return "text-muted-foreground";
  };

  const getActionLabel = (action: AuditAction) => {
    if (action === "block") return t("security.resultBlocked");
    if (action === "audit") return t("security.resultAlert");
    if (action === "redact") return t("security.resultRedacted");
    return t("security.actionAllow");
  };

  const getActionClass = (action: AuditAction) => {
    if (action === "block") return "text-rose-600 dark:text-rose-400";
    if (action === "audit") return "text-amber-600 dark:text-amber-400";
    if (action === "redact") return "text-blue-600 dark:text-blue-400";
    return "text-emerald-600 dark:text-emerald-400";
  };

  const getAppConfig = (appType: string) =>
    APP_ICON_MAP[appType as keyof typeof APP_ICON_MAP];

  const getCategoryLabel = (category: string) => {
    const normalized = category.toLowerCase();
    if (normalized === "custom") return t("security.categoryCustom");
    if (normalized === "password") return t("security.categorySecrets");
    if (
      normalized === "pii" ||
      normalized.includes("privacy") ||
      normalized.includes("personal")
    ) {
      return t("security.categoryPrivacy");
    }
    if (
      normalized.includes("secret") ||
      normalized.includes("credential") ||
      normalized.includes("key")
    ) {
      return t("security.categorySecrets");
    }
    if (
      normalized.includes("infra") ||
      normalized.includes("topology") ||
      normalized.includes("network")
    ) {
      return t("security.categoryInfrastructure");
    }
    return category;
  };

  const getRuleName = (ruleId: string, fallback: string) =>
    getBuiltinRuleName(t, ruleId, fallback);

  const getRuleDescription = (rule: SecurityRuleItem) =>
    rule.isBuiltin
      ? getBuiltinRuleDescription(
          t,
          rule.id,
          rule.description || getCategoryLabel(rule.category),
        )
      : rule.description || getCategoryLabel(rule.category);

  const handleLogFilterChange = (changes: Partial<LogFilter>) => {
    setLogFilter((current) => ({ ...current, ...changes }));
    setLogPage(1);
  };

  const resetLogFilters = () => {
    setLogFilter({});
    setLogPage(1);
  };

  const handleRefreshLogs = async () => {
    setIsRefreshingLogs(true);
    try {
      const results = await Promise.allSettled([
        refetchLogs({ throwOnError: true }),
        rangeSummary.refetch({ throwOnError: true }),
        redactedInRange.refetch({ throwOnError: true }),
        blockedInRange.refetch({ throwOnError: true }),
        auditedInRange.refetch({ throwOnError: true }),
        new Promise<void>((resolve) =>
          setTimeout(resolve, MIN_REFRESH_LOADING_MS),
        ),
      ]);
      if (results.some((result) => result.status === "rejected")) {
        toast.error(t("security.logsRefreshFailed"));
      }
    } finally {
      setIsRefreshingLogs(false);
    }
  };

  const handleConfirmClearLogs = async () => {
    if (isClearing) return;
    try {
      await clearLogs(undefined);
      setIsClearLogsDialogOpen(false);
      setSelectedLog(null);
      setLogPage(1);
    } catch {
      // The shared mutation reports the failure; keep the dialog open for retry.
    }
  };

  const handleOpenAddRule = () => {
    setEditingRule(createEmptyRule());
    setIsRuleModalOpen(true);
  };

  const handleOpenEditRule = (rule: SecurityRuleItem) => {
    if (rule.isBuiltin) return;
    setEditingRule({
      id: rule.id,
      name: rule.name,
      ruleType: rule.ruleType ?? "keyword",
      pattern: rule.pattern ?? "",
      riskLevel: rule.riskLevel,
      action: rule.action === "block" ? "redact" : rule.action,
      enabled: rule.enabled,
      description: rule.description ?? "",
      updatedAt: Date.now(),
    });
    setIsRuleModalOpen(true);
  };

  const handleRequestDeleteRule = (rule: SecurityRuleItem) => {
    if (!rule.isBuiltin) setRuleToDelete(rule);
  };

  const handleConfirmDeleteRule = async () => {
    if (!ruleToDelete) return;
    setIsDeletingRule(true);
    try {
      await deleteRule(ruleToDelete.id);
      setRuleToDelete(null);
    } catch {
      // The shared mutation reports the failure.
    } finally {
      setIsDeletingRule(false);
    }
  };

  const handleSaveRule = async () => {
    if (!editingRule.name.trim()) {
      toast.error(t("security.ruleNameRequired"));
      return;
    }
    if (!editingRule.pattern.trim()) {
      toast.error(t("security.patternRequired"));
      return;
    }
    try {
      await saveRule({
        ...editingRule,
        name: editingRule.name.trim(),
        pattern: editingRule.pattern.trim(),
        description: editingRule.description?.trim() || undefined,
        updatedAt: Date.now(),
      });
      setIsRuleModalOpen(false);
    } catch {
      // The shared mutation reports the failure.
    }
  };

  const handleRunSandbox = async () => {
    if (!sandboxText.trim()) {
      setSandboxResults([]);
      return;
    }
    setIsTestingSandbox(true);
    try {
      setSandboxResults(await securityApi.testRuleMatch(sandboxText));
    } catch (error) {
      toast.error(t("security.sandboxFailed") + String(error));
    } finally {
      setIsTestingSandbox(false);
    }
  };

  const formatLogTime = (log: AuditLogRecord) =>
    new Date(log.timestamp).toLocaleString(i18n.language);
  const formatLastSeen = (log: AuditLogRecord) =>
    new Date(log.lastSeenAt ?? log.timestamp).toLocaleString(i18n.language);

  const ruleCategories = Array.from(
    new Set(rules.map((rule) => rule.category).filter(Boolean)),
  );

  const serviceOptions: FilterOption[] = [
    { value: "all", label: t("security.allApps") },
    ...[
      "claude",
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
    ].map((appType) => ({ value: appType, label: getAppLabel(appType) })),
  ];
  const typeOptions: FilterOption[] = [
    { value: "all", label: t("security.allRules") },
    ...ruleCategories.map((category) => ({
      value: category,
      label: getCategoryLabel(category),
    })),
  ];
  const riskOptions: FilterOption[] = [
    { value: "all", label: t("security.allRisks") },
    { value: "HIGH", label: t("security.levelHighShort") },
    { value: "MEDIUM", label: t("security.levelMediumShort") },
    { value: "LOW", label: t("security.levelLowShort") },
  ];
  const actionOptions: FilterOption[] = [
    { value: "all", label: t("security.allActions") },
    { value: "block", label: t("security.resultBlocked") },
    { value: "audit", label: t("security.resultAlert") },
    { value: "redact", label: t("security.ruleActionRedact") },
    { value: "allow", label: t("security.ruleActionAllow") },
  ];
  const defaultActionOptions: Array<{
    value: AuditAction;
    label: string;
  }> = [
    { value: "redact", label: t("security.ruleActionRedact") },
    { value: "audit", label: t("security.ruleActionAlert") },
    { value: "allow", label: t("security.ruleActionAllow") },
  ];

  return (
    <div
      data-testid="security-center"
      data-active-app={activeApp}
      className="mx-auto flex h-full w-full max-w-6xl flex-col overflow-y-auto px-5 pb-12 pt-7 sm:px-8"
    >
      <div className="flex flex-col justify-between gap-4 sm:flex-row sm:items-center">
        <div className="space-y-1">
          <h1 className="text-xl font-bold tracking-tight text-foreground sm:text-2xl">
            {t("primaryNav.security")}
          </h1>
          <p className="text-xs text-muted-foreground sm:text-sm">
            {activeTab === "records"
              ? t("security.recordsDescription")
              : activeTab === "rules"
                ? t("security.rulesDescription")
                : t("toolRisk.enableDescription")}
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-3 self-start sm:self-center">
          {activeTab === "rules" && (
            <Button
              type="button"
              variant="ghost"
              className="h-8 rounded-lg px-2 text-xs font-semibold text-blue-600 hover:text-blue-700 dark:text-blue-400"
              onClick={handleOpenAddRule}
            >
              <Plus className="mr-1 h-3.5 w-3.5" />
              {t("security.addRule")}
            </Button>
          )}
          <SlidingSegmentedControl
            value={activeTab}
            onValueChange={(tab) => setActiveTab(tab)}
            options={[
              {
                value: "records",
                label: t("security.tabRecords"),
                testId: "security-tab-records",
              },
              {
                value: "rules",
                label: t("security.tabRules"),
                testId: "security-tab-rules",
              },
              {
                value: "toolRisk",
                label: t("toolRisk.tabLabel", { defaultValue: "工具调用风险" }),
                testId: "security-tab-tool-risk",
              },
            ]}
            ariaLabel={t("primaryNav.security")}
            role="tablist"
            itemRole="tab"
            indicatorVariant="surface"
            className="rounded-xl border-border/80 bg-muted/60 shadow-sm"
            buttonClassName="rounded-lg px-4 text-xs font-semibold"
            indicatorClassName="rounded-lg"
            indicatorTestId="security-tabs-indicator"
          />
        </div>
      </div>

      {activeTab === "records" ? (
        <section
          className="mt-6 space-y-5"
          role="tabpanel"
          aria-label={t("security.tabRecords")}
        >
          <div className="grid grid-cols-3 gap-4">
            <MetricCard
              label={t("securityStats.redacted")}
              value={displayCount(
                redactedInRange.logsResult?.total,
                statsLoading,
              )}
            />
            <MetricCard
              label={t("securityStats.warnings")}
              value={displayCount(
                auditedInRange.logsResult?.total,
                statsLoading,
              )}
            />
            <MetricCard
              label={t("securityStats.protectiveBlocked")}
              value={displayCount(
                blockedInRange.logsResult?.total,
                statsLoading,
              )}
            />
          </div>

          {/* Horizontal scrolling also clips vertical overflow; keep focus rings inside it. */}
          <div className="overflow-x-auto p-1">
            <div className="flex min-w-max items-center gap-2.5 text-xs">
              <UsageDateRangePicker
                selection={range}
                triggerLabel={rangeLabel}
                triggerClassName="h-8 w-auto flex-none rounded-full px-3 text-xs"
                popoverAlign="start"
                presets={["today", "7d", "30d"]}
                presetLabels={rangeLabels}
                onApply={(nextRange) => {
                  setRange(nextRange);
                  setLogPage(1);
                }}
              />
              <FilterSelect
                label={t("security.levelFilter")}
                value={logFilter.riskLevel || "all"}
                options={riskOptions}
                onChange={(value) =>
                  handleLogFilterChange({
                    riskLevel: value === "all" ? undefined : value,
                  })
                }
                ariaLabel={t("security.allRisks")}
              />
              <FilterSelect
                label={t("security.typeFilter")}
                value={logFilter.ruleCategory || "all"}
                options={typeOptions}
                onChange={(value) =>
                  handleLogFilterChange({
                    ruleCategory: value === "all" ? undefined : value,
                  })
                }
                ariaLabel={t("security.typeFilter")}
              />
              <FilterSelect
                label={t("security.serviceFilter")}
                value={logFilter.appType || "all"}
                options={serviceOptions}
                onChange={(value) =>
                  handleLogFilterChange({
                    appType: value === "all" ? undefined : value,
                  })
                }
                ariaLabel={t("security.allApps")}
              />
              <FilterSelect
                label={t("security.recordResult")}
                value={logFilter.action || "all"}
                options={actionOptions}
                onChange={(value) =>
                  handleLogFilterChange({
                    action: value === "all" ? undefined : value,
                  })
                }
                ariaLabel={t("security.allActions")}
              />
              <Input
                value={logFilter.sessionId ?? ""}
                onChange={(event) =>
                  handleLogFilterChange({ sessionId: event.target.value })
                }
                aria-label={t("security.sessionFilter")}
                placeholder={t("security.sessionFilterPlaceholder")}
                className="h-8 w-[220px] flex-none rounded-full bg-card text-xs focus:border-border-active"
              />
              <div className="relative w-[220px] flex-none">
                <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                <Input
                  value={logFilter.keyword ?? ""}
                  onChange={(event) =>
                    handleLogFilterChange({ keyword: event.target.value })
                  }
                  placeholder={t("security.searchPlaceholder")}
                  className="h-8 rounded-full bg-card pl-8 text-xs focus:border-border-active"
                />
              </div>
              {(logFilter.riskLevel ||
                logFilter.ruleCategory ||
                logFilter.appType ||
                logFilter.action ||
                logFilter.keyword ||
                logFilter.sessionId) && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-8 rounded-full px-3 text-xs text-muted-foreground"
                  onClick={resetLogFilters}
                >
                  <X className="mr-1 h-3.5 w-3.5" />
                  {t("security.resetFilters")}
                </Button>
              )}
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-8 w-8 rounded-full"
                onClick={() => void handleRefreshLogs()}
                disabled={isTableLoading}
                aria-busy={isTableLoading}
                aria-label={t("security.refreshLogs")}
                title={t(
                  isRefreshingLogs
                    ? "common.refreshing"
                    : "security.refreshLogs",
                )}
              >
                <RefreshCw
                  className={
                    "h-3.5 w-3.5" + (isTableLoading ? " animate-spin" : "")
                  }
                />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-8 w-8 rounded-full text-muted-foreground hover:text-destructive"
                onClick={() => setIsClearLogsDialogOpen(true)}
                disabled={isClearing}
                aria-label={t("security.clearLogs")}
                title={t("security.clearLogsTooltip")}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          <div className="flex items-center justify-between gap-4 rounded-xl border border-border/90 bg-card px-5 py-3.5 shadow-sm">
            <div className="flex items-center gap-2.5 text-sm text-foreground">
              <span className="inline-block h-2 w-2 shrink-0 rounded-full bg-blue-600" />
              <div>
                <p>
                  {t("security.recordsSummary", {
                    total: displayCount(rangeTotal, rangeSummary.isLoading),
                    filtered: displayCount(filteredTotal, isLogsLoading),
                  })}
                </p>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t(
                    rangeSummary.logsResult &&
                      rangeSummary.logsResult.total >
                        rangeSummary.logsResult.items.length
                      ? "securityStats.sensitiveTypesSample"
                      : "securityStats.sensitiveTypesSummary",
                    {
                      types: displayCount(
                        rangeSensitiveTypes,
                        rangeSummary.isLoading,
                      ),
                      sampleSize: displayCount(
                        rangeSummary.logsResult?.items.length,
                        rangeSummary.isLoading,
                      ),
                    },
                  )}
                </p>
              </div>
            </div>
            <span className="hidden shrink-0 font-mono text-xs text-muted-foreground sm:inline-block">
              {t("security.localOnly")}
            </span>
          </div>

          <div className="space-y-2.5 pt-1">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-medium text-muted-foreground">
                {t("security.recordsHeading")}
              </h2>
              <span className="hidden font-mono text-xs text-muted-foreground sm:inline-block">
                {t("security.localOnly")}
              </span>
            </div>

            <div className="overflow-hidden rounded-2xl border border-border/90 bg-card shadow-sm">
              <div className="overflow-x-auto">
                <table
                  aria-busy={isTableLoading}
                  className="w-full min-w-[1552px] table-fixed border-collapse text-left"
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
                    <col className="w-32" />
                    <col className="w-12" />
                  </colgroup>
                  <thead>
                    <tr className="border-b border-border/70 text-xs font-normal text-muted-foreground">
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.firstSeenAt")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.lastSeenAt")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.triggerCount")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.sessionId")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.recordType")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.riskLevel")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.recordResult")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.aiService")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 font-normal">
                        {t("security.outboundStatus")}
                      </th>
                      <th className="whitespace-nowrap px-4 py-3.5 text-right font-normal" />
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/60 text-sm">
                    {!isTableLoading &&
                      !isLogsError &&
                      logsResult?.items.map((log) => {
                        const appConfig = getAppConfig(log.appType);
                        const outboundBlocked = log.actionTaken === "block";
                        const ruleName = getRuleName(
                          log.ruleId,
                          log.ruleName || getCategoryLabel(log.ruleCategory),
                        );
                        return (
                          <tr
                            key={log.id}
                            tabIndex={0}
                            onClick={() => setSelectedLog(log)}
                            onKeyDown={(event) => {
                              if (event.key === "Enter" || event.key === " ") {
                                event.preventDefault();
                                setSelectedLog(log);
                              }
                            }}
                            className="group cursor-pointer transition-colors hover:bg-muted/50 focus:bg-muted/50 focus:outline-none"
                          >
                            <td className="whitespace-nowrap px-4 py-4 font-mono text-xs text-muted-foreground">
                              {formatLogTime(log)}
                            </td>
                            <td className="whitespace-nowrap px-4 py-4 font-mono text-xs text-muted-foreground">
                              {formatLastSeen(log)}
                            </td>
                            <td
                              className="truncate px-4 py-4 font-mono text-sm tabular-nums"
                              title={(log.triggerCount ?? 1).toLocaleString(
                                i18n.language,
                              )}
                            >
                              {(log.triggerCount ?? 1).toLocaleString(
                                i18n.language,
                              )}
                            </td>
                            <td className="px-4 py-4 font-mono text-xs text-muted-foreground">
                              <span
                                className="block truncate"
                                title={log.sessionId || undefined}
                              >
                                {log.sessionId || t("security.sessionUnknown")}
                              </span>
                            </td>
                            <td className="px-4 py-4 font-medium text-foreground">
                              <span className="block truncate" title={ruleName}>
                                {ruleName}
                              </span>
                            </td>
                            <td
                              className={
                                "whitespace-nowrap px-4 py-4 font-medium " +
                                getRiskClass(log.riskLevel)
                              }
                            >
                              {getRiskLabel(log.riskLevel)}
                            </td>
                            <td
                              className={
                                "whitespace-nowrap px-4 py-4 font-medium " +
                                getActionClass(log.actionTaken)
                              }
                            >
                              {getActionLabel(log.actionTaken)}
                            </td>
                            <td className="px-4 py-4">
                              <div className="flex items-center gap-2">
                                <AppMark appType={log.appType} />
                                <span
                                  className="min-w-0 truncate text-xs font-medium text-foreground sm:text-sm"
                                  title={
                                    appConfig?.label ?? getAppLabel(log.appType)
                                  }
                                >
                                  {appConfig?.label ?? getAppLabel(log.appType)}
                                </span>
                              </div>
                            </td>
                            <td className="whitespace-nowrap px-4 py-4">
                              <span
                                className={
                                  "text-xs sm:text-sm " +
                                  (outboundBlocked
                                    ? "font-medium text-foreground"
                                    : "text-muted-foreground")
                                }
                              >
                                {outboundBlocked
                                  ? t("security.outboundBlocked")
                                  : t("security.outboundSent")}
                              </span>
                            </td>
                            <td className="px-4 py-4 text-right">
                              <ChevronRight className="inline-block h-4 w-4 text-muted-foreground/40 transition-all group-hover:translate-x-0.5 group-hover:text-muted-foreground" />
                            </td>
                          </tr>
                        );
                      })}
                  </tbody>
                </table>
              </div>
              {/* Center status messages in the viewport, independently of table width. */}
              {(isTableLoading || isLogsError || !logsResult?.items.length) && (
                <div
                  role="status"
                  className={
                    "flex flex-col items-center justify-center px-4 py-14 text-center text-xs " +
                    (isLogsError && !isTableLoading
                      ? "text-destructive"
                      : "text-muted-foreground")
                  }
                >
                  {isTableLoading ? (
                    <>
                      <Loader2
                        aria-hidden="true"
                        className="mb-2 h-6 w-6 animate-spin"
                      />
                      {t("security.loadingLogs")}
                    </>
                  ) : isLogsError ? (
                    t("common.error")
                  ) : (
                    <>
                      <FileText className="mb-2 h-8 w-8 stroke-1 text-muted-foreground/50" />
                      {t("security.noFilteredRecords")}
                    </>
                  )}
                </div>
              )}
            </div>
          </div>

          {logsResult && logsResult.total > 0 && (
            <div className="flex flex-col gap-3 text-xs text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
              <span>
                {t("security.logPagination", {
                  total: logsResult.total,
                  page: logsResult.page,
                  pages: Math.max(
                    1,
                    Math.ceil(logsResult.total / logsResult.pageSize),
                  ),
                })}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="rounded-lg"
                  disabled={isTableLoading || logPage <= 1}
                  onClick={() => setLogPage((page) => Math.max(1, page - 1))}
                >
                  {t("security.prevPage")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="rounded-lg"
                  disabled={
                    isTableLoading ||
                    logPage * logsResult.pageSize >= logsResult.total
                  }
                  onClick={() => setLogPage((page) => page + 1)}
                >
                  {t("security.nextPage")}
                </Button>
              </div>
            </div>
          )}
        </section>
      ) : activeTab === "rules" ? (
        <section
          className="mt-6 space-y-5"
          role="tabpanel"
          aria-label={t("security.tabRules")}
        >
          <div className="space-y-2.5 pt-2">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <h2 className="text-sm font-medium text-muted-foreground">
                  {t("security.ruleHeading")}
                </h2>
                <span className="text-xs text-muted-foreground">
                  {t("security.ruleCount", { count: rules.length })}
                </span>
              </div>
            </div>

            <div className="overflow-x-auto rounded-2xl border border-border/90 bg-card shadow-sm">
              <div className="min-w-[760px] divide-y divide-border/70">
                {isRulesLoading ? (
                  <div className="p-10 text-center text-xs text-muted-foreground">
                    {t("security.loadingRules")}
                  </div>
                ) : rules.length === 0 ? (
                  <div className="p-10 text-center text-xs text-muted-foreground">
                    {t("security.noRules")}
                  </div>
                ) : (
                  rules.map((rule) => {
                    const isEnabled = rule.enabled;
                    const configuredAction = rule.actionOverride ?? rule.action;
                    const effectiveAction =
                      configuredAction === "block"
                        ? "redact"
                        : configuredAction;
                    const availableActionOptions =
                      rule.id === "catalog:generic-sk-key-notice"
                        ? defaultActionOptions.filter(
                            (option) =>
                              option.value === "audit" ||
                              option.value === "allow",
                          )
                        : defaultActionOptions;
                    return (
                      <div
                        key={rule.id}
                        data-testid={`security-rule-${rule.id}`}
                        className={
                          "group flex min-h-[84px] items-center justify-between gap-8 px-6 py-5 transition-colors hover:bg-muted/20 lg:px-10 " +
                          (!isEnabled ? "opacity-60" : "")
                        }
                      >
                        <div className="min-w-0 flex-1 space-y-1">
                          <div className="truncate text-base font-extrabold tracking-tight text-foreground">
                            {getRuleName(rule.id, rule.name)}
                          </div>
                          <p className="truncate text-xs text-muted-foreground">
                            {getRuleDescription(rule)}
                          </p>
                        </div>

                        <div className="flex shrink-0 items-center gap-2.5">
                          {availableActionOptions.map((option) => {
                            const selected = effectiveAction === option.value;
                            const selectedClass =
                              option.value === "audit"
                                ? "bg-amber-600 text-white shadow-sm hover:bg-amber-700"
                                : option.value === "redact"
                                  ? "bg-blue-600 text-white shadow-sm hover:bg-blue-700"
                                  : "bg-emerald-600 text-white shadow-sm hover:bg-emerald-700";
                            return (
                              <button
                                key={option.value}
                                type="button"
                                aria-pressed={selected}
                                onClick={() =>
                                  void (rule.isBuiltin
                                    ? updateBuiltinAction(rule.id, option.value)
                                    : updateCustomAction(rule.id, option.value))
                                }
                                className={
                                  "h-8 min-w-[76px] rounded-xl px-4 text-xs font-semibold transition-colors " +
                                  (selected
                                    ? selectedClass
                                    : "bg-muted/70 text-muted-foreground hover:bg-muted hover:text-foreground")
                                }
                              >
                                {option.label}
                              </button>
                            );
                          })}
                          {!rule.isBuiltin && (
                            <>
                              <button
                                type="button"
                                onClick={() =>
                                  void toggleRule(rule.id, !isEnabled)
                                }
                                className={
                                  "rounded-lg border px-3 py-1.5 text-xs font-semibold shadow-sm transition-all " +
                                  (isEnabled
                                    ? "border-blue-600 bg-card text-blue-600 hover:bg-blue-50/50 dark:text-blue-400"
                                    : "border-border bg-card text-muted-foreground hover:bg-muted")
                                }
                              >
                                {isEnabled
                                  ? t("security.ruleEnabled")
                                  : t("security.ruleDisabled")}
                              </button>
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                className="h-8 w-8 text-muted-foreground hover:bg-rose-50 hover:text-destructive dark:hover:bg-rose-500/10"
                                onClick={() => handleOpenEditRule(rule)}
                                aria-label={t("common.edit")}
                              >
                                <Pencil className="h-4 w-4" />
                              </Button>
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                className="h-8 w-8 text-muted-foreground hover:bg-rose-50 hover:text-destructive dark:hover:bg-rose-500/10"
                                onClick={() => handleRequestDeleteRule(rule)}
                                aria-label={t("common.delete")}
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            </>
                          )}
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </div>
          </div>

          <div className="rounded-2xl border border-border/90 bg-card p-5 shadow-sm">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex items-center gap-2">
                <FlaskConical className="h-4 w-4 text-blue-600 dark:text-blue-400" />
                <h2 className="text-sm font-semibold text-foreground">
                  {t("security.sandboxTitle")}
                </h2>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="rounded-lg"
                onClick={() => void handleRunSandbox()}
                disabled={isTestingSandbox}
              >
                <FlaskConical className="mr-1.5 h-3.5 w-3.5" />
                {isTestingSandbox
                  ? t("security.testing")
                  : t("security.runTest")}
              </Button>
            </div>
            <Textarea
              value={sandboxText}
              onChange={(event) => setSandboxText(event.target.value)}
              placeholder={t("security.sandboxPlaceholder")}
              className="mt-3 min-h-20 resize-y font-mono text-xs"
            />
            {sandboxResults && (
              <div className="mt-4 space-y-2 border-t border-border/70 pt-3">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-semibold">
                    {t("security.testResults")}
                  </span>
                  {sandboxResults.length > 0 ? (
                    <span className="text-amber-600 dark:text-amber-400">
                      {t("security.matchedCount", {
                        count: sandboxResults.length,
                      })}
                    </span>
                  ) : (
                    <span className="flex items-center gap-1 text-emerald-600 dark:text-emerald-400">
                      <CheckCircle2 className="h-3.5 w-3.5" />
                      {t("security.cleanPass")}
                    </span>
                  )}
                </div>
                {sandboxResults.map((result, index) => (
                  <div
                    key={result.ruleId + "-" + index}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-xl bg-muted/50 p-2.5 text-xs"
                  >
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <span className="font-medium">
                        {getRuleName(result.ruleId, result.ruleName)}
                      </span>
                      <span className={getRiskClass(result.riskLevel)}>
                        {getRiskLabel(result.riskLevel)}
                      </span>
                      <code className="max-w-full truncate text-muted-foreground">
                        {result.matchedSnippetMasked}
                      </code>
                    </div>
                    <span className={getActionClass(result.action)}>
                      {getActionLabel(result.action)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </section>
      ) : (
        <section
          className="mt-6 space-y-5"
          role="tabpanel"
          aria-label={t("toolRisk.tabLabel", { defaultValue: "工具调用风险" })}
        >
          <ToolRiskRulesPanel />
        </section>
      )}

      <Dialog
        open={isClearLogsDialogOpen}
        onOpenChange={(open) => {
          if (!isClearing) setIsClearLogsDialogOpen(open);
        }}
      >
        <DialogContent zIndex="alert" className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("security.clearLogsConfirmTitle")}</DialogTitle>
            <DialogDescription>
              {t("security.clearLogsConfirmDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={isClearing}
              onClick={() => setIsClearLogsDialogOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              variant="destructive"
              disabled={isClearing}
              onClick={() => void handleConfirmClearLogs()}
            >
              {isClearing && (
                <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
              )}
              {isClearing
                ? t("common.deleting")
                : t("security.clearLogsConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={selectedLog !== null}
        onOpenChange={(open) => {
          if (!open) setSelectedLog(null);
        }}
      >
        <DialogContent zIndex="top" className="max-w-lg overflow-hidden p-0">
          <DialogHeader className="border-b border-border/70 bg-muted/30 px-6 py-4">
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <DialogTitle>{t("security.detailTitle")}</DialogTitle>
                {selectedLog && (
                  <span className="rounded-md bg-muted px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
                    {getRiskLabel(selectedLog.riskLevel)}
                    {t("security.riskLevel")}
                  </span>
                )}
              </div>
            </div>
            <DialogDescription>{t("security.logsDesc")}</DialogDescription>
          </DialogHeader>
          {selectedLog && (
            <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 py-5">
              <div className="grid grid-cols-2 gap-3 text-xs">
                <div className="rounded-xl border border-border/70 bg-muted/30 p-3">
                  <div className="text-muted-foreground">
                    {t("security.detailType")}
                  </div>
                  <div className="mt-0.5 text-sm font-bold text-foreground">
                    {getRuleName(
                      selectedLog.ruleId,
                      selectedLog.ruleName ||
                        getCategoryLabel(selectedLog.ruleCategory),
                    )}
                  </div>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/30 p-3">
                  <div className="text-muted-foreground">
                    {t("security.detailService")}
                  </div>
                  <div className="mt-0.5 flex items-center gap-1.5 text-sm font-bold text-foreground">
                    <AppMark appType={selectedLog.appType} />
                    {getAppLabel(selectedLog.appType)}
                  </div>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/30 p-3">
                  <div className="text-muted-foreground">
                    {t("security.firstSeenAt")}
                  </div>
                  <div className="mt-0.5 font-mono font-medium text-foreground">
                    {formatLogTime(selectedLog)}
                  </div>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/30 p-3">
                  <div className="text-muted-foreground">
                    {t("security.lastSeenAt")}
                  </div>
                  <div className="mt-0.5 font-mono font-medium text-foreground">
                    {formatLastSeen(selectedLog)}
                  </div>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/30 p-3">
                  <div className="text-muted-foreground">
                    {t("security.triggerCount")}
                  </div>
                  <div className="mt-0.5 font-mono font-medium text-foreground">
                    {(selectedLog.triggerCount ?? 1).toLocaleString(
                      i18n.language,
                    )}
                  </div>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/30 p-3">
                  <div className="text-muted-foreground">
                    {t("security.detailOutbound")}
                  </div>
                  <div className="mt-0.5 font-medium text-foreground">
                    {selectedLog.actionTaken === "block"
                      ? t("security.outboundBlocked")
                      : t("security.outboundSent")}
                  </div>
                </div>
              </div>
              <div className="rounded-xl border border-border/70 bg-muted/30 p-3 text-xs">
                <div className="text-muted-foreground">
                  {t("security.sessionId")}
                </div>
                <div className="mt-0.5 break-all font-mono font-medium text-foreground">
                  {selectedLog.sessionId || t("security.sessionUnknown")}
                </div>
              </div>
              <div className="space-y-1.5">
                <div className="text-xs font-medium text-muted-foreground">
                  {t("security.detailSnippet")}
                </div>
                <div className="max-h-32 overflow-y-auto break-all rounded-xl border border-border bg-muted/80 p-3.5 font-mono text-xs leading-relaxed text-foreground">
                  {selectedLog.matchedSnippetMasked}
                </div>
              </div>
            </div>
          )}
          <DialogFooter className="flex items-center justify-between border-t border-border/70 bg-muted/30 px-6 py-4 sm:justify-between">
            <Button
              type="button"
              variant="ghost"
              className="px-0 text-xs text-blue-600 hover:text-blue-700 dark:text-blue-400"
              onClick={() => {
                setSelectedLog(null);
                setActiveTab("rules");
              }}
            >
              {t("security.goToRules")}
              <ChevronRight className="ml-1 h-3.5 w-3.5" />
            </Button>
            <Button type="button" onClick={() => setSelectedLog(null)}>
              {t("common.done")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={isRuleModalOpen} onOpenChange={setIsRuleModalOpen}>
        <DialogContent zIndex="alert" className="max-w-lg overflow-hidden">
          <DialogHeader>
            <DialogTitle>
              {editingRule.id
                ? t("security.dialogEditTitle")
                : t("security.dialogAddTitle")}
            </DialogTitle>
            <DialogDescription>
              {editingRule.id
                ? t("security.dialogEditDesc")
                : t("security.dialogAddDesc")}
            </DialogDescription>
          </DialogHeader>
          <div
            data-testid="security-rule-form"
            className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 py-5"
          >
            <div className="space-y-1.5">
              <label
                htmlFor="security-rule-name"
                className="text-sm font-medium"
              >
                {t("security.dialogRuleName")}
              </label>
              <Input
                id="security-rule-name"
                value={editingRule.name}
                onChange={(event) =>
                  setEditingRule((current) => ({
                    ...current,
                    name: event.target.value,
                  }))
                }
                placeholder={t("security.dialogRuleNamePlaceholder")}
              />
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="space-y-1.5">
                <label className="text-sm font-medium">
                  {t("security.dialogRuleType")}
                </label>
                <Select
                  value={editingRule.ruleType}
                  onValueChange={(value) =>
                    setEditingRule((current) => ({
                      ...current,
                      ruleType: value as RuleType,
                    }))
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="keyword">
                      {t("security.dialogTypeKeyword")}
                    </SelectItem>
                    <SelectItem value="regex">
                      {t("security.dialogTypeRegex")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <label className="text-sm font-medium">
                  {t("security.dialogRiskLevel")}
                </label>
                <Select
                  value={editingRule.riskLevel}
                  onValueChange={(value) =>
                    setEditingRule((current) => ({
                      ...current,
                      riskLevel: value as RiskLevel,
                    }))
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="CRITICAL">
                      {t("security.riskCritical")}
                    </SelectItem>
                    <SelectItem value="HIGH">
                      {t("security.riskHigh")}
                    </SelectItem>
                    <SelectItem value="MEDIUM">
                      {t("security.riskMedium")}
                    </SelectItem>
                    <SelectItem value="LOW">{t("security.riskLow")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="space-y-1.5">
              <label
                htmlFor="security-rule-pattern"
                className="text-sm font-medium"
              >
                {editingRule.ruleType === "regex"
                  ? t("security.dialogRegexPattern")
                  : t("security.dialogKeywordPattern")}
              </label>
              <Input
                id="security-rule-pattern"
                value={editingRule.pattern}
                onChange={(event) =>
                  setEditingRule((current) => ({
                    ...current,
                    pattern: event.target.value,
                  }))
                }
                placeholder={
                  editingRule.ruleType === "regex"
                    ? t("security.dialogRegexPlaceholder")
                    : t("security.dialogKeywordPlaceholder")
                }
                className="font-mono text-xs"
              />
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium">
                {t("security.dialogAction")}
              </label>
              <Select
                value={editingRule.action}
                onValueChange={(value) =>
                  setEditingRule((current) => ({
                    ...current,
                    action: value as AuditAction,
                  }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="redact">
                    {t("security.dialogActionRedact")}
                  </SelectItem>
                  <SelectItem value="audit">
                    {t("security.dialogActionAudit")}
                  </SelectItem>
                  <SelectItem value="allow">
                    {t("security.dialogActionAllow")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <label
                htmlFor="security-rule-description"
                className="text-sm font-medium"
              >
                {t("security.dialogDescription")}
              </label>
              <Textarea
                id="security-rule-description"
                value={editingRule.description ?? ""}
                onChange={(event) =>
                  setEditingRule((current) => ({
                    ...current,
                    description: event.target.value,
                  }))
                }
                placeholder={t("security.dialogDescriptionPlaceholder")}
                className="min-h-20 resize-y"
              />
            </div>
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setIsRuleModalOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              onClick={() => void handleSaveRule()}
              disabled={isSaving}
            >
              {isSaving ? t("security.savingRule") : t("security.saveRule")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={ruleToDelete !== null}
        onOpenChange={(open) => {
          if (!open && !isDeletingRule) setRuleToDelete(null);
        }}
      >
        <DialogContent zIndex="alert" className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("security.deleteRuleConfirmTitle")}</DialogTitle>
            <DialogDescription>
              {ruleToDelete
                ? t("security.deleteRuleConfirmDescription", {
                    name: getRuleName(ruleToDelete.id, ruleToDelete.name),
                  })
                : null}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              disabled={isDeletingRule}
              onClick={() => setRuleToDelete(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              variant="destructive"
              disabled={isDeletingRule}
              onClick={() => void handleConfirmDeleteRule()}
            >
              {isDeletingRule
                ? t("security.deletingRule")
                : t("security.deleteRuleConfirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
