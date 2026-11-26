import { useMemo, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { AccountLabel } from "@/components/AccountLabel";
import { SMART_RULE_DESCRIPTIONS } from "@/lib/routingGroupTransfer";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import {
  SharedModelPicker,
  type ModelChoice,
} from "@/components/models/SharedModelPicker";
import { Popover, PopoverTrigger } from "@/components/ui/popover";
import type {
  IntentRule,
  RoutingGroup,
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";
import {
  routingMemberKey,
  withSmartRuleMembers,
} from "@/lib/routingMemberSelection";

const templateRules = [
  ["Compaction", "routingPage.templateSmartCompactRule", undefined],
  [
    "Hard task",
    "routingPage.templateSmartHardRule",
    "routingPage.templateSmartHardDescription",
  ],
  [
    "Easy task",
    "routingPage.templateSmartEasyRule",
    "routingPage.templateSmartEasyDescription",
  ],
] as const;

export function RoutingTemplateRuleTargets({
  providers,
  group,
  disabled,
  onChange,
}: {
  providers: RoutingProviderOption[];
  group: RoutingGroup;
  disabled: boolean;
  onChange: (group: RoutingGroup) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState<string | null>(null);
  const { members, rules } = group;
  const catalog = useMemo(() => {
    const choices = new Map<
      string,
      ModelChoice & { providerId: string; modelId: string }
    >();
    for (const provider of providers) {
      if (!provider.taskEligible) continue;
      for (const model of provider.models ?? []) {
        if (!model.id.trim()) continue;
        const seat = { providerId: provider.id, modelId: model.id };
        const id = routingMemberKey(seat);
        if (!choices.has(id))
          choices.set(id, {
            ...seat,
            id,
            reference: model.id,
            name: model.name || model.id,
            group: provider.name,
            icon: provider.icon,
          });
      }
    }
    return choices;
  }, [providers]);
  const models = Array.from(catalog.values());
  const memberLabel = (member: RoutingMember) => {
    const provider = providers.find(({ id }) => id === member.providerId);
    const model = provider?.models?.find(({ id }) => id === member.modelId);
    return `${model?.name || member.modelId} · ${provider?.name || member.providerId}`;
  };
  const choose = (index: number, rule: IntentRule | undefined, id: string) => {
    const choice = catalog.get(id);
    if (!choice || disabled) return;
    const [name] = templateRules[index];
    const existing = members.find((member) => routingMemberKey(member) === id);
    const target = existing ?? {
      id: crypto.randomUUID(),
      providerId: choice.providerId,
      modelId: choice.modelId,
    };
    const nextRule: IntentRule = rule
      ? {
          ...rule,
          targetMemberId: target.id,
          fallbackMemberIds: (rule.fallbackMemberIds ?? []).filter(
            (fallback) => fallback !== target.id,
          ),
        }
      : {
          id: crypto.randomUUID(),
          name,
          enabled: true,
          targetMemberId: target.id,
          description:
            name === "Compaction" ? "" : SMART_RULE_DESCRIPTIONS[name],
          ...(name === "Compaction" ? { conditions: { compact: true } } : {}),
        };
    const nextRules = [...rules];
    if (rule) {
      nextRules[rules.indexOf(rule)] = nextRule;
    } else {
      const following = templateRules.slice(index + 1);
      const insertBefore = rules.findIndex((entry) =>
        following.some(([name]) => name === entry.name),
      );
      nextRules.splice(
        insertBefore < 0 ? rules.length : insertBefore,
        0,
        nextRule,
      );
    }
    onChange(
      withSmartRuleMembers({
        ...group,
        members: existing ? members : [...members, target],
        rules: nextRules,
      }),
    );
    setOpen(null);
  };
  return (
    <div className="rt-template-rule-targets">
      {templateRules.map(([name, labelKey, descriptionKey], index) => {
        const rule = rules.find((entry) => entry.name === name);
        const targetId =
          rule?.targetMemberId ??
          (name === "Hard task" ? group.defaultMemberId : undefined);
        const target = members.find(({ id }) => id === targetId);
        const label = t(labelKey);
        return (
          <div className="rt-template-rule-target" key={name}>
            <span className="rt-template-rule-number">{index + 1}</span>
            <span className="rt-template-rule-description">
              <strong>{label}</strong>
              <span>
                {descriptionKey &&
                (!rule || rule.description === SMART_RULE_DESCRIPTIONS[name])
                  ? t(descriptionKey)
                  : rule?.description || t("routingPage.templateRuleDefault")}
              </span>
            </span>
            <Popover
              modal
              open={open === name}
              onOpenChange={(next) => setOpen(next ? name : null)}
            >
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="rt-template-rule-model"
                  disabled={disabled}
                  aria-label={`${t("intentRouting.target")} ${label}`}
                >
                  <AccountLabel title>
                    {target
                      ? memberLabel(target)
                      : t("agentConnections.pickModel")}
                  </AccountLabel>
                  <ChevronDown size={12} aria-hidden />
                </button>
              </PopoverTrigger>
              <ModelPickerPopoverContent
                data-variant="agent"
                onDismiss={() => setOpen(null)}
                aria-label={label}
              >
                <SharedModelPicker
                  variant="agent"
                  models={models}
                  selected={rule && target ? [routingMemberKey(target)] : []}
                  disabled={disabled}
                  onPick={(id) => choose(index, rule, id)}
                  onDismiss={() => setOpen(null)}
                />
              </ModelPickerPopoverContent>
            </Popover>
          </div>
        );
      })}
    </div>
  );
}
