import { useMemo, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { AccountLabel } from "@/components/AccountLabel";
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
  ["Compaction", "routingPage.templateSmartCompactRule"],
  ["Hard task", "routingPage.templateSmartHardRule"],
  ["Easy task", "routingPage.templateSmartEasyRule"],
] as const;
const defaultTarget = [[undefined, "intentRouting.defaultTarget"]] as const;

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
  const choose = (rule: IntentRule | undefined, id: string) => {
    const choice = catalog.get(id);
    if (!choice || disabled) return;
    const existing = members.find((member) => routingMemberKey(member) === id);
    const target = existing ?? {
      id: crypto.randomUUID(),
      providerId: choice.providerId,
      modelId: choice.modelId,
    };
    const nextRules = rules.map((entry) =>
      entry.id === rule?.id
        ? {
            ...entry,
            targetMemberId: target.id,
            fallbackMemberIds: (entry.fallbackMemberIds ?? []).filter(
              (fallback) => fallback !== target.id,
            ),
          }
        : entry,
    );
    onChange(
      withSmartRuleMembers({
        ...group,
        members: existing ? members : [...members, target],
        defaultMemberId: rule ? group.defaultMemberId : target.id,
        rules: nextRules,
      }),
    );
    setOpen(null);
  };
  return (
    <div className="rt-template-rule-targets">
      {(rules.length ? templateRules : defaultTarget).map(
        ([name, labelKey], index) => {
          const rule = rules.find((entry) => entry.name === name);
          if (name && !rule) return null;
          const rowId = rule?.id ?? "default";
          const targetId = rule?.targetMemberId ?? group.defaultMemberId;
          const target = members.find(({ id }) => id === targetId);
          const label = t(labelKey);
          return (
            <div className="rt-template-rule-target" key={rowId}>
              <span className="rt-template-rule-number">{index + 1}</span>
              <span className="rt-template-rule-description">
                <strong>{label}</strong>
                {rule && (
                  <span>
                    {rule.description || t("routingPage.templateRuleDefault")}
                  </span>
                )}
              </span>
              <Popover
                modal
                open={open === rowId}
                onOpenChange={(next) => setOpen(next ? rowId : null)}
              >
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    className="rt-template-rule-model"
                    disabled={disabled}
                    aria-label={
                      rule ? `${t("intentRouting.target")} ${label}` : label
                    }
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
                    selected={target ? [routingMemberKey(target)] : []}
                    disabled={disabled}
                    onPick={(id) => choose(rule, id)}
                    onDismiss={() => setOpen(null)}
                  />
                </ModelPickerPopoverContent>
              </Popover>
            </div>
          );
        },
      )}
    </div>
  );
}
