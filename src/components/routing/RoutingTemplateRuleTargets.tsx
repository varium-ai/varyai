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
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";

const templateRules = ["Compaction", "Hard task", "Easy task"] as const;
const keyOf = (providerId: string, modelId: string) =>
  JSON.stringify([providerId, modelId]);

export function RoutingTemplateRuleTargets({
  providers,
  members,
  rules,
  disabled,
  onChange,
}: {
  providers: RoutingProviderOption[];
  members: RoutingMember[];
  rules: IntentRule[];
  disabled: boolean;
  onChange: (rules: IntentRule[]) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState<string | null>(null);
  const models = useMemo<ModelChoice[]>(
    () =>
      providers.flatMap((provider) =>
        (provider.models ?? []).map((model) => ({
          id: keyOf(provider.id, model.id),
          reference: model.id,
          modelId: model.id,
          name: model.name || model.id,
          group: provider.name,
          icon: provider.icon,
        })),
      ),
    [providers],
  );
  const memberLabel = (member: RoutingMember) => {
    const provider = providers.find(({ id }) => id === member.providerId);
    const model = provider?.models?.find(({ id }) => id === member.modelId);
    return `${model?.name || member.modelId} · ${provider?.name || member.providerId}`;
  };
  const selectedId = (memberId: string) => {
    const member = members.find(({ id }) => id === memberId);
    return member ? keyOf(member.providerId, member.modelId) : "";
  };
  const choose = (rule: IntentRule, id: string) => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(id);
    } catch {
      return;
    }
    if (!Array.isArray(parsed) || parsed.length !== 2) return;
    const [providerId, modelId] = parsed;
    const member = members.find(
      (entry) => entry.providerId === providerId && entry.modelId === modelId,
    );
    if (!member) return;
    onChange(
      rules.map((entry) =>
        entry.id === rule.id
          ? {
              ...entry,
              targetMemberId: member.id,
              fallbackMemberIds: (entry.fallbackMemberIds ?? []).filter(
                (id) => id !== member.id,
              ),
            }
          : entry,
      ),
    );
    setOpen(null);
  };
  return (
    <div className="rt-template-rule-targets">
      {templateRules.map((name, index) => {
        const rule = rules.find((entry) => entry.name === name);
        if (!rule) return null;
        const target = members.find(({ id }) => id === rule.targetMemberId);
        if (!target) return null;
        const label =
          name === "Compaction"
            ? t("routingPage.templateSmartCompactRule")
            : name === "Hard task"
              ? t("routingPage.templateSmartHardRule")
              : t("routingPage.templateSmartEasyRule");
        return (
          <div className="rt-template-rule-target" key={rule.id}>
            <span className="rt-template-rule-number">{index + 1}</span>
            <span className="rt-template-rule-description">
              <strong>{label}</strong>
              <span>
                {rule.description || t("routingPage.templateRuleDefault")}
              </span>
            </span>
            <Popover
              open={open === rule.id}
              onOpenChange={(next) => setOpen(next ? rule.id : null)}
            >
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="rt-template-rule-model"
                  disabled={disabled}
                  aria-label={`${t("intentRouting.target")} ${label}`}
                >
                  <AccountLabel title>{memberLabel(target)}</AccountLabel>
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
                  selected={[selectedId(target.id)]}
                  disabled={disabled}
                  onPick={(id) => choose(rule, id)}
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
