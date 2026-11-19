import { useMemo, useState } from "react";
import { ChevronDown, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { AccountLabel } from "@/components/AccountLabel";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import {
  SharedModelPicker,
  type ModelChoice,
} from "@/components/models/SharedModelPicker";
import { Popover, PopoverTrigger } from "@/components/ui/popover";
import type {
  RoutingGroup,
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";
import {
  routingMemberKey,
  routingMemberUnavailableReason,
} from "@/lib/routingMemberSelection";

type TemplateMemberPickerProps = {
  providers: RoutingProviderOption[];
  members: RoutingGroup["members"];
  disabled: boolean;
  onChange: (members: RoutingMember[]) => void;
};

const choiceId = (providerId: string, modelId: string) =>
  JSON.stringify([providerId, modelId]);

export function RoutingTemplateMemberPicker({
  providers,
  members,
  disabled,
  onChange,
}: TemplateMemberPickerProps) {
  const { t } = useTranslation();
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const models = useMemo<ModelChoice[]>(() => {
    const choices: ModelChoice[] = [];
    const seen = new Set<string>();
    for (const provider of providers) {
      for (const model of provider.models ?? []) {
        if (!model.id.trim()) continue;
        const id = choiceId(provider.id, model.id);
        if (seen.has(id)) continue;
        seen.add(id);
        choices.push({
          id,
          reference: model.id,
          modelId: model.id,
          name: model.name || model.id,
          group: provider.name,
          icon: provider.icon,
        });
      }
    }
    // Keep saved members visible even when a provider has temporarily lost its
    // catalog entry. They remain selectable until explicitly replaced.
    for (const member of members) {
      const id = choiceId(member.providerId, member.modelId);
      if (seen.has(id)) continue;
      seen.add(id);
      const provider = providers.find(({ id }) => id === member.providerId);
      choices.push({
        id,
        reference: member.modelId,
        modelId: member.modelId,
        name: member.modelId,
        group: provider?.name ?? member.providerId,
        icon: provider?.icon,
      });
    }
    return choices;
  }, [members, providers]);

  const summary = (member: RoutingMember) => {
    const provider = providers.find(({ id }) => id === member.providerId);
    const model = provider?.models?.find(({ id }) => id === member.modelId);
    return {
      provider,
      modelName: model?.name || member.modelId,
      unavailable: routingMemberUnavailableReason(providers, member),
    };
  };
  const canAddProvider = providers.some(
    (provider) =>
      (provider.models?.length ?? 0) > 0 &&
      !members.some((member) => member.providerId === provider.id),
  );

  function replace(index: number, id: string) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(id);
    } catch {
      return;
    }
    if (!Array.isArray(parsed) || parsed.length !== 2) return;
    const [providerId, modelId] = parsed;
    if (typeof providerId !== "string" || typeof modelId !== "string") return;
    if (
      members.some(
        (member, memberIndex) =>
          memberIndex !== index &&
          (routingMemberKey(member) === id || member.providerId === providerId),
      )
    )
      return;
    const next = members.slice();
    next[index] = next[index]
      ? { ...next[index], providerId, modelId }
      : { id: crypto.randomUUID(), providerId, modelId };
    onChange(next);
    setOpenIndex(null);
  }

  return (
    <div className="rt-template-member-picker">
      {members.slice(0, 3).map((member, index) => {
        const { provider, modelName, unavailable } = summary(member);
        const label = `${modelName} · ${provider?.name ?? member.providerId}`;
        const selected = choiceId(member.providerId, member.modelId);
        return (
          <Popover
            key={member.id}
            open={openIndex === index}
            onOpenChange={(open) => setOpenIndex(open ? index : null)}
          >
            <PopoverTrigger asChild>
              <button
                type="button"
                className="rt-template-member"
                disabled={disabled}
                aria-label={label}
              >
                <span className="rt-template-member-index">{index + 1}</span>
                <ProviderIcon
                  icon={provider?.icon ?? "generic"}
                  name={provider?.name ?? member.providerId}
                  size={16}
                />
                <span className="rt-template-member-name">
                  <AccountLabel title>{label}</AccountLabel>
                  {unavailable && (
                    <small>
                      {t(`intentRouting.memberUnavailable.${unavailable}`)}
                    </small>
                  )}
                </span>
                <ChevronDown size={13} aria-hidden />
              </button>
            </PopoverTrigger>
            <ModelPickerPopoverContent
              data-variant="agent"
              onDismiss={() => setOpenIndex(null)}
              aria-label={t("agentConnections.modelListTitle", {
                agent: label,
              })}
            >
              <SharedModelPicker
                variant="agent"
                models={models}
                selected={[selected]}
                disabled={disabled}
                onPick={(next) => replace(index, next)}
                onDismiss={() => setOpenIndex(null)}
              />
            </ModelPickerPopoverContent>
          </Popover>
        );
      })}
      {members.length < 3 && canAddProvider && (
        <Popover
          open={openIndex === members.length}
          onOpenChange={(open) => setOpenIndex(open ? members.length : null)}
        >
          <PopoverTrigger asChild>
            <button
              type="button"
              className="rt-template-member-add"
              disabled={disabled}
              aria-label={t("intentRouting.templateMemberAdd")}
            >
              <Plus size={13} aria-hidden />
              {t("intentRouting.templateMemberAdd")}
            </button>
          </PopoverTrigger>
          <ModelPickerPopoverContent
            data-variant="agent"
            onDismiss={() => setOpenIndex(null)}
            aria-label={t("intentRouting.templateMemberAdd")}
          >
            <SharedModelPicker
              variant="agent"
              models={models}
              selected={[]}
              disabled={disabled}
              onPick={(next) => replace(members.length, next)}
              onDismiss={() => setOpenIndex(null)}
            />
          </ModelPickerPopoverContent>
        </Popover>
      )}
    </div>
  );
}
