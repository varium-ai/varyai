import { useEffect, useMemo, useRef, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  FlaskConical,
  History,
  Pencil,
  Plus,
  RefreshCw,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
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
import {
  useToolRiskRules,
  useToolRiskSandbox,
  useToolRiskSettings,
} from "@/hooks/useToolRisk";
import { ToolRiskPolicyChangesPanel } from "./ToolRiskPolicyChangesPanel";
import { ShellCommandListsPanel } from "./ShellCommandListsPanel";
import { toolRiskApi } from "@/lib/api";
import { getToolRiskRuleName } from "@/lib/toolRiskRuleLocalization";
import { toast } from "sonner";
import {
  DETECTOR_FIELDS,
  OPERATION_FIELDS,
  OPERATION_FIELD_SLUGS,
  toolRiskConditionIssue,
  toolRiskConditionOps,
  toolRiskScopeTerms,
  KNOWN_DETECTORS,
  KNOWN_REASON_CODES,
  OPERATION_TYPES,
  RISK_CATEGORIES,
  RISK_SEVERITIES,
  COMMAND_CLASSES,
  type ToolRiskRuleSeed,
  parseToolRiskToolRefs,
  type SaveCustomRuleInput,
  type ToolRiskCategory,
  type ToolRiskConditionOp,
  type ToolRiskOperationType,
  type ToolRiskRule,
  type ToolRiskRuleCondition,
  type ToolRiskRuleItem,
  type ToolRiskSeverity,
} from "@/types/toolRisk";

function emptyDraft(): SaveCustomRuleInput {
  return {
    id: null,
    name: "",
    description: "",
    enabled: false,
    selector: { target: { kind: "operation", types: ["shell_exec"] } },
    conditions: [{ field: "/command", op: "keyword", value: "" }],
    riskCategory: "remote_code_execution",
    severity: "high",
    reasonCode: "remote_exec_chain",
    localReview: "off",
    expectedRevision: null,
  };
}

function severityBadgeVariant(
  severity: string,
): "destructive" | "secondary" | "outline" {
  if (severity === "high") return "destructive";
  if (severity === "medium") return "secondary";
  return "outline";
}

/** Semantic field label; preserve unknown JSON Pointers without guessing. */
function conditionFieldLabel(t: TFunction, field: string) {
  const slug = OPERATION_FIELD_SLUGS[field];
  if (!slug) return field;
  return t(`toolRisk.conditionField.${slug}`, { defaultValue: field });
}

/** Condition value label; display unknown values literally. */
function conditionValueLabel(t: TFunction, condition: ToolRiskRuleCondition) {
  const values =
    condition.values ?? (condition.value != null ? [condition.value] : []);
  const label = (value: string) => {
    if (condition.op === "detector")
      return t(`toolRisk.detector.${value}`, { defaultValue: value });
    if (condition.field === "/command_class")
      return t(`toolRisk.commandClass.${value}`, { defaultValue: value });
    if (condition.field === "/op_type")
      return t(`toolRisk.operation.${value}`, { defaultValue: value });
    if (condition.field === "/recursive" || condition.field === "/overwrite")
      return t(`toolRisk.conditionBool.${value}`, { defaultValue: value });
    return value;
  };
  return values.map(label).join(t("toolRisk.scopeSeparator"));
}

/** Keep an existing value only when it belongs to the newly selected field domain. */
function conditionValueFitsField(condition: ToolRiskRuleCondition) {
  if (condition.op === "detector") {
    return (
      !!condition.value &&
      (DETECTOR_FIELDS as Record<string, string>)[condition.value] ===
        condition.field
    );
  }
  const values =
    condition.values ?? (condition.value != null ? [condition.value] : []);
  if (condition.field === "/command_class") {
    return values.every((value) =>
      COMMAND_CLASSES.some((known) => known === value),
    );
  }
  if (condition.field === "/op_type") {
    return values.every((value) =>
      OPERATION_TYPES.some((known) => known === value),
    );
  }
  if (condition.field === "/recursive" || condition.field === "/overwrite") {
    return values.every((value) => value === "true" || value === "false");
  }
  return true;
}

/** Semantic values for `in`; null retains the legacy text editor. */
function conditionSetOptions(field: string): readonly string[] | null {
  if (field === "/command_class") return COMMAND_CLASSES;
  if (field === "/op_type") return OPERATION_TYPES;
  return null;
}

/** Display unknown set values literally instead of presenting them as known enum values. */
function conditionSetOptionLabel(t: TFunction, field: string, value: string) {
  if (field === "/command_class")
    return t(`toolRisk.commandClass.${value}`, { defaultValue: value });
  if (field === "/op_type")
    return t(`toolRisk.operation.${value}`, { defaultValue: value });
  return value;
}

/** Limit visible conditions and summarize overflow to keep the table within its bounds. */
const MAX_LIST_CONDITION_TERMS = 3;

export function ToolRiskRulesPanel({
  pendingRuleSeed,
  onRuleSeedConsumed,
}: {
  pendingRuleSeed?: ToolRiskRuleSeed | null;
  onRuleSeedConsumed?: () => void;
} = {}) {
  const { t } = useTranslation();
  const {
    settings,
    isLoading: isSettingsLoading,
    updateSettings,
    isUpdating,
  } = useToolRiskSettings();
  const {
    rules,
    isLoading: isRulesLoading,
    toggleRule,
    resetBuiltinRule,
    saveRule,
    deleteRule,
    isSaving,
  } = useToolRiskRules();
  const sandbox = useToolRiskSandbox();

  const [showPolicyChanges, setShowPolicyChanges] = useState(false);
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [draft, setDraft] = useState<SaveCustomRuleInput>(emptyDraft());
  const [ruleToDelete, setRuleToDelete] = useState<ToolRiskRuleItem | null>(
    null,
  );

  const [saveError, setSaveError] = useState(false);
  const conditionIssues = draft.conditions.map((condition) =>
    toolRiskConditionIssue(condition, draft.selector.target.kind),
  );
  const invalidConditions =
    draft.conditions.length === 0 ||
    conditionIssues.some((issue) => issue !== null);
  const [toolRefsText, setToolRefsText] = useState("[]");
  const toolRefs = useMemo(
    () => parseToolRiskToolRefs(toolRefsText),
    [toolRefsText],
  );
  const invalidTarget =
    draft.selector.target.kind === "tool"
      ? toolRefs === null
      : draft.selector.target.types.length === 0;
  const [sandboxTool, setSandboxTool] = useState("Bash");
  const [sandboxInput, setSandboxInput] = useState('{"command": "ls"}');
  const [sandboxClient, setSandboxClient] = useState("");
  const [sandboxDefinitionRef, setSandboxDefinitionRef] = useState("");
  const [sandboxInputError, setSandboxInputError] = useState(false);

  const visibleRules = rules;
  const enabledCount = useMemo(
    () => visibleRules.filter((r) => r.enabled).length,
    [visibleRules],
  );

  const openCreate = (seed?: ToolRiskRuleSeed) => {
    const next = emptyDraft();
    if (seed) {
      next.name = t("toolRisk.eventRuleName");
      next.selector = {
        target: {
          kind: "operation",
          types: [seed.operationCategory ?? "shell_exec"],
        },
      };
      next.conditions = seed.commandClass
        ? [{ field: "/command_class", op: "eq", value: seed.commandClass }]
        : [{ field: "", op: "keyword", value: "" }];
      if (seed.riskCategory) next.riskCategory = seed.riskCategory;
      if (seed.severity) next.severity = seed.severity;
      if (seed.reasonCode) next.reasonCode = seed.reasonCode;
    }
    setSaveError(false);
    setDraft(next);
    setToolRefsText("[]");
    setIsDialogOpen(true);
  };

  const consumedSeed = useRef<ToolRiskRuleSeed | null>(null);
  useEffect(() => {
    if (!pendingRuleSeed || consumedSeed.current === pendingRuleSeed) return;
    consumedSeed.current = pendingRuleSeed;
    openCreate(pendingRuleSeed);
    onRuleSeedConsumed?.();
  }, [pendingRuleSeed, onRuleSeedConsumed]);

  const openEdit = async (rule: ToolRiskRuleItem) => {
    let full: ToolRiskRule | null = null;
    try {
      full = await toolRiskApi.getRule(rule.id);
    } catch {
      toast.error(t("toolRisk.ruleLoadFailed"));
      return;
    }
    if (!full) {
      toast.error(t("toolRisk.ruleNotFound", { defaultValue: "规则不存在" }));
      return;
    }
    setSaveError(false);
    setDraft({
      id: full.id,
      name: full.name,
      description: full.description ?? "",
      enabled: full.enabled,
      selector: full.selector,
      conditions: full.conditions,
      riskCategory: full.risk_category,
      severity: full.severity,
      reasonCode: full.reason_code,
      localReview: full.local_review,
      expectedRevision: full.revision,
    });
    setToolRefsText(
      full.selector.target.kind === "tool"
        ? JSON.stringify(full.selector.target.tool_refs ?? [], null, 2)
        : "[]",
    );
    setIsDialogOpen(true);
  };

  const handleSave = async () => {
    if (
      isSaving ||
      !draft.name.trim() ||
      invalidTarget ||
      invalidConditions ||
      draft.severity === "none"
    )
      return;
    setSaveError(false);
    try {
      await saveRule(
        draft.selector.target.kind === "tool" && toolRefs
          ? {
              ...draft,
              selector: {
                ...draft.selector,
                target: { kind: "tool", tool_refs: toolRefs },
              },
            }
          : draft,
      );
      setIsDialogOpen(false);
    } catch {
      setSaveError(true);
    }
  };

  const handleRunSandbox = async () => {
    setSandboxInputError(false);
    sandbox.reset();
    let parsed: unknown;
    try {
      parsed = JSON.parse(sandboxInput);
    } catch {
      // Preserve free-form inputs such as apply_patch, but never reinterpret broken JSON.
      if (/^\s*[{[\"]/.test(sandboxInput)) {
        setSandboxInputError(true);
        return;
      }
      parsed = sandboxInput;
    }
    await sandbox.test({
      toolName: sandboxTool,
      input: parsed,
      ...(sandboxClient.trim() ? { client: sandboxClient.trim() } : {}),
      ...(sandboxDefinitionRef.trim()
        ? { definitionRef: sandboxDefinitionRef.trim() }
        : {}),
    });
  };

  const updateCondition = (
    index: number,
    patch: Partial<ToolRiskRuleCondition>,
  ) => {
    setDraft((d) => ({
      ...d,
      conditions: d.conditions.map((condition, i) => {
        if (i !== index) return condition;
        const targetKind = d.selector.target.kind;
        const merged: ToolRiskRuleCondition = { ...condition, ...patch };
        const fieldChanged =
          patch.field !== undefined && patch.field !== condition.field;
        const opChanged = patch.op !== undefined && patch.op !== condition.op;

        if (opChanged && !fieldChanged) {
          // Changing operators changes the value shape; clear the previous value explicitly.
          const { value: _value, values: _values, ...rest } = merged;
          return merged.op === "in"
            ? { ...rest, values: [] }
            : { ...rest, value: "" };
        }
        if (fieldChanged) {
          // Fall back to the first supported operator when the selected field is incompatible.
          const ops = toolRiskConditionOps(merged.field, targetKind);
          const op = ops.includes(merged.op) ? merged.op : ops[0];
          const {
            value: previousValue,
            values: previousValues,
            ...rest
          } = merged;
          const candidate: ToolRiskRuleCondition = { ...rest, op };
          const keepsValue = conditionValueFitsField({
            ...candidate,
            value: previousValue ?? null,
            values: previousValues ?? null,
          });
          if (!keepsValue || opChanged) {
            return op === "in"
              ? { ...candidate, values: [] }
              : { ...candidate, value: "" };
          }
          // Preserve a value that remains valid for the selected field.
          return op === "in"
            ? { ...candidate, values: previousValues ?? [] }
            : { ...candidate, value: previousValue ?? null };
        }
        return merged;
      }),
    }));
  };

  const addCondition = () => {
    setDraft((d) => ({
      ...d,
      conditions: [
        ...d.conditions,
        { field: "/command", op: "keyword", value: "" },
      ],
    }));
  };

  /** Update set membership from the latest draft so rapid clicks do not lose selections. */
  const toggleConditionSetValue = (
    index: number,
    value: string,
    checked: boolean,
  ) => {
    setDraft((d) => ({
      ...d,
      conditions: d.conditions.map((condition, i) => {
        if (i !== index) return condition;
        const current = condition.values ?? [];
        return {
          ...condition,
          values: checked
            ? [...current, value]
            : current.filter((item) => item !== value),
        };
      }),
    }));
  };

  const removeCondition = (index: number) => {
    setDraft((d) => ({
      ...d,
      conditions: d.conditions.filter((_, i) => i !== index),
    }));
  };

  const toggleOperationType = (op: ToolRiskOperationType) => {
    setDraft((d) => {
      const target = d.selector.target;
      if (target.kind !== "operation") return d;
      const has = target.types.includes(op);
      const types = has
        ? target.types.filter((x) => x !== op)
        : [...target.types, op];
      return {
        ...d,
        selector: { ...d.selector, target: { kind: "operation", types } },
      };
    });
  };

  return (
    <div className="space-y-6">
      {}
      <div className="flex items-center justify-between rounded-2xl border border-border/90 bg-card p-5 shadow-sm">
        <div className="flex items-start gap-3">
          <ShieldAlert className="mt-0.5 h-5 w-5 text-amber-500" />
          <div>
            <div className="text-sm font-semibold text-foreground">
              {t("toolRisk.enableTitle", { defaultValue: "工具调用风险预警" })}
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              {t("toolRisk.enableDescription", {
                defaultValue:
                  "仅检测启用规则覆盖的调用，其他调用默认不检测。只预警，不阻断。",
              })}
            </div>
            {settings && (
              <div className="mt-1 text-[11px] text-muted-foreground">
                {t("toolRisk.policyVersion", {
                  defaultValue: "已保存策略版本",
                })}
                : {settings.policyVersion}
              </div>
            )}
          </div>
        </div>
        <Switch
          checked={settings?.enabled ?? false}
          disabled={isSettingsLoading || isUpdating}
          onCheckedChange={(checked) =>
            updateSettings({
              enabled: checked,
              policyVersion: settings?.policyVersion ?? 0,
            })
          }
        />
      </div>

      <ShellCommandListsPanel />

      {}
      <div className="rounded-2xl border border-border/90 bg-card shadow-sm">
        <div className="flex items-center justify-between border-b border-border/80 px-5 py-4">
          <div>
            <div className="text-sm font-semibold text-foreground">
              {t("toolRisk.rulesHeading", { defaultValue: "检测规则" })}
            </div>
            <div className="mt-0.5 text-xs text-muted-foreground">
              {t("toolRisk.rulesSummary", {
                defaultValue: "共 {{total}} 条，启用 {{enabled}} 条",
                total: visibleRules.length,
                enabled: enabledCount,
              })}
            </div>
          </div>
          <div className="flex gap-2">
            <Dialog
              open={showPolicyChanges}
              onOpenChange={setShowPolicyChanges}
            >
              <DialogTrigger asChild>
                <Button size="sm" variant="outline" className="gap-1.5">
                  <History className="h-4 w-4" />
                  {t("toolRisk.policyChanges.entry")}
                </Button>
              </DialogTrigger>
              <DialogContent
                zIndex="top"
                className="w-[calc(100vw-2rem)] max-w-6xl overflow-hidden"
              >
                <ToolRiskPolicyChangesPanel
                  onClose={() => setShowPolicyChanges(false)}
                />
              </DialogContent>
            </Dialog>
            <Button size="sm" onClick={() => openCreate()} className="gap-1.5">
              <Plus className="h-4 w-4" />
              {t("toolRisk.addRule", { defaultValue: "新增规则" })}
            </Button>
          </div>
        </div>

        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>
                {t("toolRisk.colName", { defaultValue: "名称" })}
              </TableHead>
              <TableHead>
                {t("toolRisk.colScope", { defaultValue: "范围" })}
              </TableHead>
              <TableHead>
                {t("toolRisk.colCategory", { defaultValue: "风险类别" })}
              </TableHead>
              <TableHead>
                {t("toolRisk.colSeverity", { defaultValue: "严重度" })}
              </TableHead>
              <TableHead className="text-right">
                {t("toolRisk.colActions", { defaultValue: "操作" })}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {isRulesLoading ? (
              <TableRow>
                <TableCell
                  colSpan={5}
                  className="py-8 text-center text-sm text-muted-foreground"
                >
                  {t("common.loading", { defaultValue: "加载中…" })}
                </TableCell>
              </TableRow>
            ) : visibleRules.length === 0 ? (
              <TableRow>
                <TableCell
                  colSpan={5}
                  className="py-8 text-center text-sm text-muted-foreground"
                >
                  {t("toolRisk.noRules", { defaultValue: "暂无规则" })}
                </TableCell>
              </TableRow>
            ) : (
              visibleRules.map((rule) => {
                const separator = t("toolRisk.scopeSeparator");
                const scopeText =
                  rule.targetKind === "tool"
                    ? t("toolRisk.scopeToolRefs", {
                        count: rule.toolRefCount,
                      })
                    : toolRiskScopeTerms(rule)
                        .map((operation) =>
                          t(`toolRisk.operation.${operation}`, {
                            defaultValue: operation,
                          }),
                        )
                        .join(separator);
                const conditionTerms = rule.conditions.map((condition) => {
                  const field = conditionFieldLabel(t, condition.field);
                  const op = t(`toolRisk.op.${condition.op}`, {
                    defaultValue: condition.op,
                  });
                  const value = conditionValueLabel(t, condition);
                  return value
                    ? t("toolRisk.conditionSummary.patternWithValue", {
                        field,
                        op,
                        value,
                      })
                    : t("toolRisk.conditionSummary.patternWithoutValue", {
                        field,
                        op,
                      });
                });
                const hiddenConditionCount =
                  conditionTerms.length - MAX_LIST_CONDITION_TERMS;
                const conditionText = conditionTerms
                  .slice(0, MAX_LIST_CONDITION_TERMS)
                  .join(separator);
                return (
                  <TableRow key={rule.id}>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-foreground">
                          {getToolRiskRuleName(t, rule)}
                        </span>
                        {rule.origin === "builtin" && (
                          <Badge variant="outline" className="text-[10px]">
                            {t("toolRisk.builtin", { defaultValue: "内置" })}
                          </Badge>
                        )}
                        {rule.origin === "builtin" &&
                          rule.id === "builtin.destructive_database_change" && (
                            <Badge variant="secondary" className="text-[10px]">
                              {t("toolRisk.capabilityNotReady")}
                            </Badge>
                          )}
                      </div>
                      {rule.origin === "builtin" &&
                        rule.id === "builtin.destructive_database_change" && (
                          <p className="mt-1 text-[11px] text-muted-foreground">
                            {t("toolRisk.databaseCapabilityHint")}
                          </p>
                        )}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      <div
                        data-testid={`tool-risk-scope-${rule.id}`}
                        className="text-foreground/80"
                      >
                        {scopeText}
                      </div>
                      <div
                        data-testid={`tool-risk-conditions-${rule.id}`}
                        className="mt-0.5"
                      >
                        {conditionText}
                        {hiddenConditionCount > 0 &&
                          `${separator}${t("toolRisk.conditionSummary.more", {
                            count: hiddenConditionCount,
                          })}`}
                      </div>
                    </TableCell>
                    <TableCell>
                      <span className="text-xs">
                        {t(`toolRisk.category.${rule.riskCategory}`, {
                          defaultValue: rule.riskCategory,
                        })}
                      </span>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={severityBadgeVariant(rule.severity)}
                        className="text-[10px]"
                      >
                        {t(`toolRisk.severity.${rule.severity}`, {
                          defaultValue: rule.severity,
                        })}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center justify-end gap-2">
                        <Switch
                          checked={rule.enabled}
                          onCheckedChange={(checked) =>
                            toggleRule(rule.id, checked, rule.revision)
                          }
                        />
                        {rule.origin === "builtin" ? (
                          <Button
                            variant="ghost"
                            size="icon"
                            title={t("toolRisk.resetBuiltin", {
                              defaultValue: "恢复默认",
                            })}
                            onClick={() =>
                              resetBuiltinRule(rule.id, rule.revision)
                            }
                          >
                            <RefreshCw className="h-4 w-4" />
                          </Button>
                        ) : (
                          <>
                            <Button
                              variant="ghost"
                              size="icon"
                              data-testid={`tool-risk-edit-${rule.id}`}
                              onClick={() => openEdit(rule)}
                            >
                              <Pencil className="h-4 w-4" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              onClick={() => setRuleToDelete(rule)}
                            >
                              <Trash2 className="h-4 w-4 text-destructive" />
                            </Button>
                          </>
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                );
              })
            )}
          </TableBody>
        </Table>
      </div>

      {}
      <div className="rounded-2xl border border-border/90 bg-card p-5 shadow-sm">
        <div className="flex items-center gap-2">
          <FlaskConical className="h-4 w-4 text-blue-500" />
          <div className="text-sm font-semibold text-foreground">
            {t("toolRisk.sandboxTitle", { defaultValue: "规则沙箱测试" })}
          </div>
        </div>
        <div className="mt-1 text-xs text-muted-foreground">
          {t("toolRisk.sandboxDescription", {
            defaultValue:
              "手动预览规则，不受实时观测总开关影响；不执行工具、不产生风险记录、不保存或回显原文。",
          })}
        </div>
        {settings && !settings.enabled && (
          <p
            role="status"
            className="mt-2 text-xs text-amber-700 dark:text-amber-400"
          >
            {t("toolRisk.sandboxObservationDisabled")}
          </p>
        )}
        <div className="mt-4 grid gap-3 sm:grid-cols-2">
          <div className="space-y-1.5">
            <Label htmlFor="tool-risk-sandbox-client">
              {t("toolRisk.sandboxClient")}
            </Label>
            <Input
              id="tool-risk-sandbox-client"
              value={sandboxClient}
              onChange={(e) => setSandboxClient(e.target.value)}
              placeholder="claude / codex / gemini / openai"
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="tool-risk-sandbox-definition">
              {t("toolRisk.sandboxDefinitionRef")}
            </Label>
            <Input
              id="tool-risk-sandbox-definition"
              value={sandboxDefinitionRef}
              onChange={(e) => setSandboxDefinitionRef(e.target.value)}
              placeholder="builtin:claude:tools:v1"
            />
          </div>
        </div>
        <p className="mt-2 text-xs text-muted-foreground">
          {t("toolRisk.sandboxTrustHint")}
        </p>
        {sandboxClient.trim() === "openai" &&
          sandboxTool.trim() === "write_file" && (
            <p className="mt-2 text-xs text-muted-foreground">
              {t("toolRisk.sandboxOpenaiWriteHint")}
            </p>
          )}
        <div className="mt-4 grid gap-3 sm:grid-cols-[160px_1fr_auto]">
          <Input
            value={sandboxTool}
            onChange={(e) => setSandboxTool(e.target.value)}
            placeholder={t("toolRisk.sandboxTool", { defaultValue: "工具名" })}
          />
          <Textarea
            value={sandboxInput}
            onChange={(e) => setSandboxInput(e.target.value)}
            rows={2}
            placeholder='{"command": "ls"}'
          />
          <Button
            onClick={handleRunSandbox}
            disabled={sandbox.isTesting}
            className="self-start"
          >
            {t("toolRisk.sandboxRun", { defaultValue: "测试" })}
          </Button>
        </div>
        {sandboxInputError && (
          <p role="alert" className="mt-2 text-xs text-destructive">
            {t("toolRisk.sandboxInvalidJson")}
          </p>
        )}
        {sandbox.result && (
          <div className="mt-3 rounded-xl border border-border/80 bg-muted/40 p-3 text-xs">
            <div className="flex flex-wrap gap-3">
              <span>
                {t("toolRisk.sandboxScope", { defaultValue: "范围" })}:{" "}
                <strong>{sandbox.result.scope}</strong>
              </span>
              <span>
                {t("toolRisk.sandboxVerdict", { defaultValue: "裁决" })}:{" "}
                <strong>{sandbox.result.verdict ?? "-"}</strong>
              </span>
              <span>
                {t("toolRisk.sandboxApplicable", { defaultValue: "适用规则" })}:{" "}
                <strong>{sandbox.result.applicableRules}</strong>
              </span>
            </div>
            {sandbox.result.matchedRuleIds.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1">
                {sandbox.result.matchedRuleIds.map((id) => (
                  <Badge key={id} variant="destructive" className="text-[10px]">
                    {id}
                  </Badge>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Rule editor */}
      <Dialog open={isDialogOpen} onOpenChange={setIsDialogOpen}>
        <DialogContent
          zIndex="top"
          className="max-h-[85vh] w-[calc(100%-2rem)] overflow-hidden sm:max-w-2xl"
        >
          <DialogHeader>
            <DialogTitle>
              {draft.id
                ? t("toolRisk.editRule", { defaultValue: "编辑规则" })
                : t("toolRisk.newRule", { defaultValue: "新增规则" })}
            </DialogTitle>
            <DialogDescription>
              {t("toolRisk.dialogDescription", {
                defaultValue: "新规则默认保存为禁用草稿，预览后主动启用。",
              })}
            </DialogDescription>
          </DialogHeader>

          <div
            role="region"
            aria-label={t(draft.id ? "toolRisk.editRule" : "toolRisk.newRule")}
            className="min-h-0 flex-1 space-y-4 overflow-y-auto overscroll-contain px-6 py-4"
          >
            <p className="text-xs text-muted-foreground">
              {t("toolRisk.customRuleBoundaryHint")}
            </p>
            <div className="space-y-1.5">
              <Label htmlFor="tool-risk-rule-name">
                {t("toolRisk.fieldName", { defaultValue: "名称" })}
              </Label>
              <Input
                id="tool-risk-rule-name"
                placeholder={t("toolRisk.ruleNamePlaceholder")}
                value={draft.name}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              />
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="tool-risk-rule-description">
                {t("toolRisk.fieldDescription", { defaultValue: "描述" })}
              </Label>
              <Input
                id="tool-risk-rule-description"
                placeholder={t("toolRisk.ruleDescriptionPlaceholder")}
                value={draft.description ?? ""}
                onChange={(e) =>
                  setDraft({ ...draft, description: e.target.value })
                }
              />
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="tool-risk-target-kind">
                {t("toolRisk.fieldTarget")}
              </Label>
              <Select
                value={draft.selector.target.kind}
                onValueChange={(value) =>
                  setDraft({
                    ...draft,
                    selector: {
                      ...draft.selector,
                      target:
                        value === "tool"
                          ? { kind: "tool", tool_refs: toolRefs ?? [] }
                          : { kind: "operation", types: ["shell_exec"] },
                    },
                  })
                }
              >
                <SelectTrigger id="tool-risk-target-kind">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="operation">
                    {t("toolRisk.fieldOperations")}
                  </SelectItem>
                  <SelectItem value="tool">
                    {t("toolRisk.fieldToolRefs")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            {draft.selector.target.kind === "tool" ? (
              <div className="space-y-1.5">
                <Label htmlFor="tool-risk-tool-refs">
                  {t("toolRisk.fieldToolRefs")}
                </Label>
                <Textarea
                  id="tool-risk-tool-refs"
                  placeholder={t("toolRisk.toolRefsPlaceholder")}
                  value={toolRefsText}
                  onChange={(e) => setToolRefsText(e.target.value)}
                  aria-invalid={toolRefs === null}
                  aria-describedby="tool-risk-tool-refs-hint"
                  rows={5}
                />
                <p
                  id="tool-risk-tool-refs-hint"
                  className="text-xs text-muted-foreground"
                >
                  {t("toolRisk.toolRefsHint")}
                </p>
                {toolRefs === null && (
                  <p role="alert" className="text-xs text-destructive">
                    {t("toolRisk.invalidToolRefs")}
                  </p>
                )}
              </div>
            ) : (
              <div className="space-y-1.5">
                <Label>
                  {t("toolRisk.fieldOperations", { defaultValue: "操作类别" })}
                </Label>
                <div className="flex flex-wrap gap-2">
                  {OPERATION_TYPES.map((op) => {
                    const target = draft.selector.target;
                    const active =
                      target.kind === "operation" && target.types.includes(op);
                    return (
                      <button
                        key={op}
                        type="button"
                        onClick={() => toggleOperationType(op)}
                        className={`rounded-full border px-3 py-1 text-xs transition-colors ${
                          active
                            ? "border-primary bg-primary/10 text-primary"
                            : "border-border bg-card text-muted-foreground"
                        }`}
                      >
                        {t(`toolRisk.operation.${op}`, { defaultValue: op })}
                      </button>
                    );
                  })}
                </div>
              </div>
            )}

            <div className="grid gap-3 sm:grid-cols-3">
              <div className="space-y-1.5">
                <Label>
                  {t("toolRisk.fieldCategory", { defaultValue: "风险类别" })}
                </Label>
                <Select
                  value={draft.riskCategory}
                  onValueChange={(v) =>
                    setDraft({ ...draft, riskCategory: v as ToolRiskCategory })
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {RISK_CATEGORIES.map((c) => (
                      <SelectItem key={c} value={c}>
                        {t(`toolRisk.category.${c}`, { defaultValue: c })}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label>
                  {t("toolRisk.fieldSeverity", { defaultValue: "严重度" })}
                </Label>
                <Select
                  value={draft.severity}
                  onValueChange={(v) =>
                    setDraft({ ...draft, severity: v as ToolRiskSeverity })
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {RISK_SEVERITIES.map((s) => (
                      <SelectItem key={s} value={s}>
                        {t(`toolRisk.severity.${s}`, { defaultValue: s })}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label>
                  {t("toolRisk.fieldReason", { defaultValue: "理由代码" })}
                </Label>
                <Select
                  value={draft.reasonCode}
                  onValueChange={(v) => setDraft({ ...draft, reasonCode: v })}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {KNOWN_REASON_CODES.map((r) => (
                      <SelectItem key={r} value={r}>
                        {t(`toolRisk.reason.${r}`, { defaultValue: r })}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            {draft.severity === "none" && (
              <p role="alert" className="text-xs text-destructive">
                {t("toolRisk.riskSeverityRequired")}
              </p>
            )}
            <div className="space-y-2">
              <p className="text-xs text-muted-foreground">
                {t("toolRisk.commandClassConditionHint")}
              </p>
              <div className="flex items-center justify-between">
                <Label>
                  {t("toolRisk.fieldConditions", { defaultValue: "条件" })}
                </Label>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={addCondition}
                  className="gap-1"
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("toolRisk.addCondition", { defaultValue: "添加条件" })}
                </Button>
              </div>
              {draft.conditions.length === 0 && (
                <p role="alert" className="text-xs text-destructive">
                  {t("toolRisk.conditionValueRequired")}
                </p>
              )}
              {draft.conditions.map((cond, index) => {
                const targetKind = draft.selector.target.kind;
                const detectors =
                  targetKind === "operation"
                    ? KNOWN_DETECTORS.filter(
                        (id) => DETECTOR_FIELDS[id] === cond.field,
                      )
                    : [];
                const ops = toolRiskConditionOps(cond.field, targetKind);
                const legacyDetector =
                  cond.op === "detector" &&
                  !!cond.value &&
                  !detectors.some((id) => id === cond.value);
                // Use semantic selection for empty fields; retain free text for unknown or legacy fields.
                const semanticField =
                  targetKind === "operation" &&
                  (cond.field === "" ||
                    OPERATION_FIELDS.some((field) => field === cond.field));
                const setOptions =
                  cond.op === "in" ? conditionSetOptions(cond.field) : null;
                const values = cond.values ?? [];
                const semanticSet =
                  !!setOptions &&
                  values.every((value) =>
                    setOptions.some((known) => known === value),
                  );
                const boolField =
                  cond.field === "/recursive" || cond.field === "/overwrite";
                return (
                  <div key={index} className="space-y-1.5">
                    <div className="grid items-start gap-2 sm:grid-cols-[minmax(0,1fr)_140px_minmax(0,1fr)_auto]">
                      {semanticField ? (
                        <Select
                          value={cond.field}
                          onValueChange={(value) =>
                            updateCondition(index, { field: value })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("toolRisk.fieldConditionField")}
                          >
                            <SelectValue
                              placeholder={t("toolRisk.pickConditionField")}
                            >
                              {cond.field ? (
                                <>
                                  {conditionFieldLabel(t, cond.field)}
                                  <span className="ml-1 font-mono text-[10px] text-muted-foreground">
                                    {cond.field}
                                  </span>
                                </>
                              ) : undefined}
                            </SelectValue>
                          </SelectTrigger>
                          <SelectContent>
                            {OPERATION_FIELDS.map((field) => (
                              <SelectItem key={field} value={field}>
                                {conditionFieldLabel(t, field)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : (
                        <Input
                          aria-label={t("toolRisk.fieldConditionField")}
                          value={cond.field}
                          onChange={(e) =>
                            updateCondition(index, { field: e.target.value })
                          }
                          placeholder="/command"
                        />
                      )}
                      <Select
                        value={cond.op}
                        onValueChange={(v) =>
                          updateCondition(index, {
                            op: v as ToolRiskConditionOp,
                          })
                        }
                      >
                        <SelectTrigger
                          aria-label={t("toolRisk.fieldConditionOp")}
                        >
                          <SelectValue>
                            {t(`toolRisk.op.${cond.op}`, {
                              defaultValue: cond.op,
                            })}
                          </SelectValue>
                        </SelectTrigger>
                        <SelectContent>
                          {ops.map((op) => (
                            <SelectItem key={op} value={op}>
                              {t(`toolRisk.op.${op}`, { defaultValue: op })}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      {cond.op === "in" ? (
                        semanticSet && setOptions ? (
                          <div
                            role="group"
                            aria-label={t("toolRisk.fieldConditionValues")}
                            className="flex flex-wrap gap-x-3 gap-y-1.5 rounded-md border border-border/80 px-2 py-1.5"
                          >
                            {setOptions.map((value) => (
                              <label
                                key={value}
                                className="flex items-center gap-1.5 text-xs"
                              >
                                <Checkbox
                                  checked={values.includes(value)}
                                  aria-label={conditionSetOptionLabel(
                                    t,
                                    cond.field,
                                    value,
                                  )}
                                  onCheckedChange={(checked) =>
                                    toggleConditionSetValue(
                                      index,
                                      value,
                                      checked === true,
                                    )
                                  }
                                />
                                <span>
                                  {conditionSetOptionLabel(
                                    t,
                                    cond.field,
                                    value,
                                  )}
                                </span>
                              </label>
                            ))}
                          </div>
                        ) : (
                          <div className="space-y-1">
                            <Textarea
                              aria-label={t("toolRisk.fieldConditionValues")}
                              placeholder={t(
                                "toolRisk.conditionValuesPlaceholder",
                              )}
                              value={values.join("\n")}
                              onChange={(e) =>
                                updateCondition(index, {
                                  values: e.target.value.split("\n"),
                                })
                              }
                              rows={3}
                            />
                            <p className="text-xs text-muted-foreground">
                              {t("toolRisk.conditionValuesHint")}
                            </p>
                          </div>
                        )
                      ) : cond.op === "detector" ? (
                        <Select
                          value={cond.value ?? ""}
                          onValueChange={(v) =>
                            updateCondition(index, { value: v })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("toolRisk.pickDetector")}
                            disabled={detectors.length === 0}
                          >
                            <SelectValue
                              placeholder={t("toolRisk.pickDetector")}
                            >
                              {cond.value
                                ? t(`toolRisk.detector.${cond.value}`, {
                                    defaultValue: cond.value,
                                  })
                                : undefined}
                            </SelectValue>
                          </SelectTrigger>
                          <SelectContent>
                            {legacyDetector && (
                              <SelectItem disabled value={cond.value!}>
                                {t(`toolRisk.detector.${cond.value}`, {
                                  defaultValue: cond.value!,
                                })}
                              </SelectItem>
                            )}
                            {detectors.map((id) => (
                              <SelectItem key={id} value={id}>
                                {t(`toolRisk.detector.${id}`, {
                                  defaultValue: id,
                                })}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : cond.op === "eq" &&
                        cond.field === "/command_class" ? (
                        <Select
                          value={JSON.stringify(cond.value ?? "")}
                          onValueChange={(value) =>
                            updateCondition(index, {
                              value: JSON.parse(value) as string,
                            })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("toolRisk.fieldCommandClass")}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value={JSON.stringify("")}>
                              {t("toolRisk.pickCommandClass")}
                            </SelectItem>
                            {cond.value &&
                              !COMMAND_CLASSES.some(
                                (value) => value === cond.value,
                              ) && (
                                <SelectItem value={JSON.stringify(cond.value)}>
                                  {cond.value}
                                </SelectItem>
                              )}
                            {COMMAND_CLASSES.map((value) => (
                              <SelectItem
                                key={value}
                                value={JSON.stringify(value)}
                              >
                                {t(`toolRisk.commandClass.${value}`)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : cond.op === "eq" && cond.field === "/op_type" ? (
                        <Select
                          value={JSON.stringify(cond.value ?? "")}
                          onValueChange={(value) =>
                            updateCondition(index, {
                              value: JSON.parse(value) as string,
                            })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("toolRisk.fieldConditionValue")}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value={JSON.stringify("")}>
                              {t("toolRisk.pickConditionValue")}
                            </SelectItem>
                            {cond.value &&
                              !OPERATION_TYPES.some(
                                (value) => value === cond.value,
                              ) && (
                                <SelectItem value={JSON.stringify(cond.value)}>
                                  {cond.value}
                                </SelectItem>
                              )}
                            {OPERATION_TYPES.map((value) => (
                              <SelectItem
                                key={value}
                                value={JSON.stringify(value)}
                              >
                                {t(`toolRisk.operation.${value}`)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : cond.op === "eq" && boolField ? (
                        <Select
                          value={cond.value ?? ""}
                          onValueChange={(value) =>
                            updateCondition(index, { value })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("toolRisk.fieldConditionValue")}
                          >
                            <SelectValue
                              placeholder={t("toolRisk.pickConditionValue")}
                            >
                              {cond.value
                                ? t(`toolRisk.conditionBool.${cond.value}`, {
                                    defaultValue: cond.value,
                                  })
                                : undefined}
                            </SelectValue>
                          </SelectTrigger>
                          <SelectContent>
                            {(["true", "false"] as const).map((value) => (
                              <SelectItem key={value} value={value}>
                                {t(`toolRisk.conditionBool.${value}`)}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      ) : (
                        <Input
                          aria-label={t("toolRisk.fieldConditionValue")}
                          placeholder={t("toolRisk.conditionValuePlaceholder")}
                          value={cond.value ?? ""}
                          onChange={(e) =>
                            updateCondition(index, { value: e.target.value })
                          }
                        />
                      )}
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => removeCondition(index)}
                        disabled={draft.conditions.length <= 1}
                      >
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    </div>
                    {conditionIssues[index] && (
                      <p role="alert" className="text-xs text-destructive">
                        {t(`toolRisk.${conditionIssues[index]}`)}
                      </p>
                    )}
                  </div>
                );
              })}
            </div>

            <div className="flex items-center justify-between rounded-xl border border-border/80 bg-muted/30 px-4 py-3">
              <div className="text-xs text-muted-foreground">
                {t("toolRisk.fieldEnabled", { defaultValue: "启用规则" })}
              </div>
              <Switch
                checked={draft.enabled}
                onCheckedChange={(checked) =>
                  setDraft({ ...draft, enabled: checked })
                }
              />
            </div>
            {saveError && (
              <p role="alert" className="text-xs text-destructive">
                {t("toolRisk.ruleSaveErrorHint")}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setIsDialogOpen(false)}>
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
            <Button
              data-testid="tool-risk-save"
              onClick={handleSave}
              disabled={
                isSaving ||
                !draft.name.trim() ||
                invalidTarget ||
                invalidConditions ||
                draft.severity === "none"
              }
            >
              {t("common.save", { defaultValue: "保存" })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {}
      <Dialog
        open={!!ruleToDelete}
        onOpenChange={(open) => !open && setRuleToDelete(null)}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("toolRisk.confirmDelete", { defaultValue: "确认删除规则" })}
            </DialogTitle>
            <DialogDescription>
              {t("toolRisk.confirmDeleteDescription", {
                defaultValue: "删除后不可恢复，历史记录保留原本地规则 ID。",
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRuleToDelete(null)}>
              {t("common.cancel", { defaultValue: "取消" })}
            </Button>
            <Button
              variant="destructive"
              onClick={async () => {
                if (ruleToDelete)
                  await deleteRule(ruleToDelete.id, ruleToDelete.revision);
                setRuleToDelete(null);
              }}
            >
              {t("common.delete", { defaultValue: "删除" })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
