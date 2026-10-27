import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCircle2, FlaskConical, Plus, Pencil, Trash2 } from "lucide-react";
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
import { useSecurityRules } from "@/hooks/useSecurityAudit";
import { securityApi } from "@/lib/api";
import {
  getBuiltinRuleDescription,
  getBuiltinRuleName,
} from "@/lib/securityRuleLocalization";
import type {
  AuditAction,
  CustomSecurityRule,
  RiskLevel,
  RuleMatchResult,
  SecurityRuleItem,
  RuleType,
} from "@/types/security";
import type { UsageRangeSelection } from "@/types/usage";
import { ToolRiskRulesPanel } from "@/components/security/ToolRiskRulesPanel";
import { toast } from "sonner";
import { SecurityEventsPanel } from "./SecurityEventsPanel";
import type { SecurityEventFilter } from "@/types/securityEvents";
import type { ToolRiskRuleSeed } from "@/types/toolRisk";

type SecurityTab = "records" | "rules" | "toolRisk";

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

export function SecurityCenterPage({
  activeApp = "claude",
}: {
  activeApp?: string;
}) {
  const { t } = useTranslation();
  const [range, setRange] = useState<UsageRangeSelection>({ preset: "today" });
  const [activeTab, setActiveTab] = useState<SecurityTab>("records");
  const [logPage, setLogPage] = useState(1);
  const [eventFilter, setEventFilter] = useState<SecurityEventFilter>({
    view: "security_events",
  });
  const [pendingRuleSeed, setPendingRuleSeed] =
    useState<ToolRiskRuleSeed | null>(null);
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
        <SecurityEventsPanel
          filter={eventFilter}
          onFilterChange={setEventFilter}
          page={logPage}
          onPageChange={setLogPage}
          range={range}
          onRangeChange={setRange}
          dlpCategories={Array.from(
            new Set(rules.map((rule) => rule.category).filter(Boolean)),
          ).map((value) => ({ value, label: getCategoryLabel(value) }))}
          onOpenDlpRules={() => setActiveTab("rules")}
          onCreateRule={(seed) => {
            setPendingRuleSeed(seed);
            setActiveTab("toolRisk");
          }}
        />
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
                          <p
                            className="break-words text-xs text-muted-foreground"
                            title={getRuleDescription(rule)}
                          >
                            {getRuleDescription(rule)}
                          </p>
                        </div>

                        <div className="flex shrink-0 items-center gap-2.5">
                          {defaultActionOptions.map((option) => {
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
          <ToolRiskRulesPanel
            pendingRuleSeed={pendingRuleSeed}
            onRuleSeedConsumed={() => setPendingRuleSeed(null)}
          />
        </section>
      )}

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
