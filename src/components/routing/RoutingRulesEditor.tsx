import { useLayoutEffect, useRef, useState, type ReactElement } from "react";
import {
  ChevronDown,
  GripVertical,
  Info,
  MoreHorizontal,
  Plus,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { AccountLabel, useAccountText } from "@/components/AccountLabel";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import {
  SharedModelPicker,
  type ModelChoice,
} from "@/components/models/SharedModelPicker";
import {
  RoutingRuleConditions,
  pauseRoutingRule,
} from "./RoutingRuleConditions";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import type {
  IntentRule,
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";

function RuleModelPicker({
  children,
  label,
  models,
  selected,
  disabled,
  onPick,
}: {
  children: ReactElement;
  label: string;
  models: ModelChoice[];
  selected?: string;
  disabled: boolean;
  onPick: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const dismiss = () => setOpen(false);
  return (
    <Popover modal open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <ModelPickerPopoverContent
        data-variant="agent"
        aria-label={label}
        onDismiss={dismiss}
      >
        <SharedModelPicker
          variant="agent"
          models={models}
          selected={selected ? [selected] : []}
          disabled={disabled}
          onDismiss={dismiss}
          onPick={(id) => {
            onPick(id);
            dismiss();
          }}
        />
      </ModelPickerPopoverContent>
    </Popover>
  );
}

export function RoutingRulesEditor({
  rules,
  members,
  providers,
  disabled,
  onChange,
}: {
  rules: IntentRule[];
  members: RoutingMember[];
  providers: RoutingProviderOption[];
  disabled: boolean;
  onChange: (rules: IntentRule[]) => void;
}) {
  const { t } = useTranslation();
  const accountText = useAccountText();
  const list = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<string | null>(null);
  const [reasoning, setReasoning] = useState(false);
  const [focusRule, setFocusRule] = useState<string | null>(null);
  useLayoutEffect(() => {
    if (!focusRule) return;
    const frame = requestAnimationFrame(() => {
      Array.from(
        list.current?.querySelectorAll<HTMLElement>("[data-routing-rule]") ??
          [],
      )
        .find((row) => row.getAttribute("data-routing-rule") === focusRule)
        ?.querySelector<HTMLTextAreaElement>("textarea")
        ?.focus({ preventScroll: false });
      setFocusRule(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [focusRule]);
  function update(rule: IntentRule) {
    onChange(rules.map((old) => (old.id === rule.id ? rule : old)));
  }
  function move(from: number, to: number, keep?: "handle" | "menu") {
    if (disabled || to < 0 || to >= rules.length || from === to) return;
    const next = [...rules];
    next.splice(to, 0, ...next.splice(from, 1));
    onChange(next);
    if (keep)
      requestAnimationFrame(() =>
        list.current
          ?.querySelectorAll<HTMLElement>(
            keep === "handle" ? ".rt-rhandle" : ".rt-rmore",
          )
          [to]?.focus({ preventScroll: true }),
      );
  }
  const drag = useRoutingRowDrag("data-routing-rule", (from, to) =>
    move(
      rules.findIndex((rule) => rule.id === from),
      rules.findIndex((rule) => rule.id === to),
    ),
  );
  const models = new Map<string, ModelChoice>(
    members.map(({ id, providerId, modelId, effort }) => {
      const provider = providers.find((entry) => entry.id === providerId);
      const catalogModel = provider?.models?.find(
        (entry) => entry.id === modelId,
      );
      return [
        id,
        {
          id,
          modelId,
          reference: modelId,
          name: `${catalogModel?.name || modelId}${effort ? ` :${effort}` : ""}`,
          group: provider?.name || providerId,
          icon: provider?.icon,
        },
      ];
    }),
  );
  const memberChoices = [...models.values()];
  return (
    <div className="rt-rules rt-setting-row">
      <h4>{t("intentRouting.rules")}</h4>
      <div className="rt-rules-heading">
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="rt-info"
              aria-label={t("routingPage.ruleHelp")}
            >
              <Info size={14} aria-hidden />
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="top"
            align="start"
            className="w-80 max-w-[calc(100vw-2rem)] px-3 py-2 text-xs leading-relaxed"
            aria-label={t("routingPage.ruleHelp")}
          >
            {t("intentRouting.conditionsHelp")}
          </PopoverContent>
        </Popover>
        <button
          type="button"
          className="rt-rule-add"
          disabled={disabled || members.length < 2 || rules.length >= 16}
          onClick={() => {
            const id = crypto.randomUUID();
            onChange([
              ...rules,
              {
                id,
                name: t("intentRouting.ruleNumber", { n: rules.length + 1 }),
                description: "",
                enabled: true,
                targetMemberId: members[members.length - 1]?.id ?? "",
              },
            ]);
            setFocusRule(id);
          }}
        >
          <Plus size={14} />
          {t("intentRouting.addRule")}
        </button>
      </div>
      {members.length < 2 ? (
        <p className="rt-rule-hint">{t("routingPage.ruleNeedsModels")}</p>
      ) : !rules.length ? (
        <p className="rt-rule-hint">{t("routingPage.ruleEmpty")}</p>
      ) : null}
      <div className="rt-rule-list" data-routing-list ref={list}>
        {rules.map((rule, index) => {
          const uses = [
            rule.targetMemberId,
            ...(rule.pause ? [] : (rule.fallbackMemberIds ?? [])),
          ].filter(Boolean);
          const setUses = (ids: string[]) =>
            update({
              ...rule,
              targetMemberId: ids[0],
              fallbackMemberIds: ids.slice(1),
            });
          const available = memberChoices.filter(
            ({ id }) => !uses.includes(id),
          );
          const ruleNumber = t("intentRouting.ruleNumber", { n: index + 1 });
          const targetLabel = t(
            rule.pause ? "intentRouting.pauseMember" : "intentRouting.target",
          );
          const fallbackLabel = t("routingPage.ruleFallbackModels");
          const model = (id: string, at: number, label: string) => {
            const selected = models.get(id) ?? { id, name: id };
            const name = selected.group
              ? `${selected.name} · ${selected.group}`
              : selected.name;
            return (
              <span className="rt-c" key={at}>
                <RuleModelPicker
                  label={label}
                  models={[selected, ...available]}
                  selected={id}
                  disabled={disabled}
                  onPick={(next) =>
                    setUses(uses.map((old, i) => (i === at ? next : old)))
                  }
                >
                  <button
                    type="button"
                    className="rt-cond on rt-model"
                    disabled={disabled}
                    aria-label={accountText(`${label} ${name}`)}
                  >
                    <AccountLabel title>{name}</AccountLabel>
                    <ChevronDown size={12} aria-hidden />
                  </button>
                </RuleModelPicker>
                {uses.length > 1 && (
                  <button
                    className="rt-cx"
                    type="button"
                    disabled={disabled}
                    aria-label={accountText(
                      `${t("routingPage.removeRuleModel")} ${name}`,
                    )}
                    onClick={() => setUses(uses.filter((_, i) => i !== at))}
                  >
                    <X size={12} />
                  </button>
                )}
              </span>
            );
          };
          return (
            <div
              key={rule.id}
              className="rt-rule"
              data-routing-rule={rule.id}
              role="group"
              aria-label={ruleNumber}
            >
              <div className="rt-rule-heading">
                <button
                  className="rt-rhandle"
                  type="button"
                  disabled={disabled}
                  aria-label={t("routingPage.moveRule", { n: index + 1 })}
                  title={t("routingPage.dragHint")}
                  onPointerDown={(event) => {
                    if (!disabled) drag.start(event, rule.id);
                  }}
                  onClick={() => drag.consumeClick(rule.id)}
                  onKeyDown={(event) => {
                    if (
                      event.altKey &&
                      ["ArrowUp", "ArrowDown"].includes(event.key)
                    ) {
                      event.preventDefault();
                      event.stopPropagation();
                      move(
                        index,
                        index + (event.key === "ArrowUp" ? -1 : 1),
                        "handle",
                      );
                    }
                  }}
                >
                  <GripVertical size={14} />
                </button>
                <h5>{ruleNumber}</h5>
                {!rule.enabled && (
                  <span className="rt-rule-status">
                    {t("routingPage.ruleDisabled")}
                  </span>
                )}
                <DropdownMenu
                  open={menu === rule.id}
                  onOpenChange={(open) => {
                    setMenu(open ? rule.id : null);
                    setReasoning(false);
                  }}
                >
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      className="rt-rmore mp-icon-button"
                      disabled={disabled}
                      aria-label={t("routingPage.ruleActions", {
                        n: index + 1,
                      })}
                    >
                      <MoreHorizontal size={14} />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent className="mp-row-menu" align="end">
                    {reasoning ? (
                      ["", "low", "medium", "high", "xhigh", "max"].map(
                        (level) => (
                          <DropdownMenuItem
                            key={level}
                            onSelect={() => {
                              update({ ...rule, useEffort: level });
                              setMenu(null);
                            }}
                          >
                            {level || t("intentRouting.memberEffortAgent")}
                          </DropdownMenuItem>
                        ),
                      )
                    ) : (
                      <>
                        <DropdownMenuItem
                          disabled={index === 0}
                          onSelect={() => move(index, index - 1, "menu")}
                        >
                          {t("intentRouting.moveUp")}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          disabled={index === rules.length - 1}
                          onSelect={() => move(index, index + 1, "menu")}
                        >
                          {t("intentRouting.moveDown")}
                        </DropdownMenuItem>
                        {!rule.pause && (
                          <>
                            <DropdownMenuSeparator />
                            <DropdownMenuItem
                              onSelect={(event) => {
                                event.preventDefault();
                                setReasoning(true);
                              }}
                            >
                              {t("intentRouting.ruleUseEffort")}
                            </DropdownMenuItem>
                          </>
                        )}
                        <DropdownMenuSeparator />
                        <DropdownMenuItem
                          onSelect={() =>
                            update(pauseRoutingRule(rule, !rule.pause))
                          }
                        >
                          {t(
                            rule.pause
                              ? "routingPage.sendRule"
                              : "routingPage.pauseRule",
                          )}
                        </DropdownMenuItem>
                        {!rule.enabled && (
                          <DropdownMenuItem
                            onSelect={() => update({ ...rule, enabled: true })}
                          >
                            {t("intentRouting.ruleEnabled")}
                          </DropdownMenuItem>
                        )}
                        <DropdownMenuSeparator />
                        <DropdownMenuItem
                          onSelect={() =>
                            onChange(rules.filter((old) => old.id !== rule.id))
                          }
                        >
                          {t("routingPage.removeRule")}
                        </DropdownMenuItem>
                      </>
                    )}
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
              <div className="rt-rule-fields">
                <div className="rt-rule-field">
                  <span>{t("routingPage.ruleConditions")}</span>
                  <RoutingRuleConditions
                    rule={rule}
                    disabled={disabled}
                    onChange={update}
                  />
                </div>
                <div className="rt-rule-field">
                  <span>{targetLabel}</span>
                  <div className="rt-line">
                    {uses[0] && model(uses[0], 0, targetLabel)}
                    {!rule.pause && rule.useEffort && (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <button
                            type="button"
                            disabled={disabled}
                            className="rt-cond on"
                            aria-label={t("intentRouting.ruleUseEffort")}
                          >
                            <span>
                              {t("intentRouting.ruleUseEffort")} ·{" "}
                              {rule.useEffort}
                            </span>
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent className="mp-row-menu">
                          {["", "low", "medium", "high", "xhigh", "max"].map(
                            (level) => (
                              <DropdownMenuItem
                                key={level}
                                onSelect={() =>
                                  update({ ...rule, useEffort: level })
                                }
                              >
                                {level || t("intentRouting.memberEffortAgent")}
                              </DropdownMenuItem>
                            ),
                          )}
                        </DropdownMenuContent>
                      </DropdownMenu>
                    )}
                  </div>
                </div>
                {!rule.pause && (uses.length > 1 || available.length > 0) && (
                  <div className="rt-rule-field">
                    <span>{fallbackLabel}</span>
                    <div className="rt-line">
                      {uses
                        .slice(1)
                        .map((id, at) => model(id, at + 1, fallbackLabel))}
                      {!!available.length && (
                        <RuleModelPicker
                          label={t("routingPage.addRuleModel")}
                          models={available}
                          disabled={disabled}
                          onPick={(id) => setUses([...uses, id])}
                        >
                          <button
                            type="button"
                            className="rt-rule-add"
                            disabled={disabled}
                          >
                            <Plus size={12} />
                            {t("routingPage.addRuleModel")}
                          </button>
                        </RuleModelPicker>
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
