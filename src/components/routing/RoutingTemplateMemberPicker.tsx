import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, GripVertical, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { AccountLabel } from "@/components/AccountLabel";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import {
  SharedModelPicker,
  type ModelChoice,
} from "@/components/models/SharedModelPicker";
import { Button } from "@/components/ui/button";
import { Popover, PopoverTrigger } from "@/components/ui/popover";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import type {
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";
import {
  ROUTING_MEMBER_LIMIT,
  routingMemberKey,
  routingMemberUnavailableReason,
} from "@/lib/routingMemberSelection";
import { RoutingMemberQuotaDates } from "./RoutingMemberQuotaDates";

type TemplateMemberPickerProps = {
  providers: RoutingProviderOption[];
  members: RoutingMember[];
  showExpiry?: boolean;
  disabled: boolean;
  onChange: (members: RoutingMember[]) => void;
  onRecommendOrder?: () => void;
};

export function RoutingTemplateMemberPicker({
  providers,
  members,
  showExpiry,
  disabled,
  onChange,
  onRecommendOrder,
}: TemplateMemberPickerProps) {
  const { t } = useTranslation();
  const upstreams = useQuery({
    queryKey: ["routing-upstreams"],
    queryFn: routingUpstreamsApi.list,
    enabled: !!showExpiry,
    staleTime: 30_000,
    retry: false,
  });
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const catalog = useMemo(() => {
    const choices = new Map<
      string,
      ModelChoice & { providerId: string; modelId: string }
    >();
    for (const provider of providers) {
      if (!provider.taskEligible) continue;
      for (const model of provider.models ?? []) {
        if (!model.id.trim()) continue;
        const target = { providerId: provider.id, modelId: model.id };
        const id = routingMemberKey(target);
        if (!choices.has(id))
          choices.set(id, {
            ...target,
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
  const memberKeys = new Set(members.map(routingMemberKey));
  const selection = openIndex === null ? undefined : members[openIndex];
  const selected = selection ? routingMemberKey(selection) : undefined;
  const models = Array.from(catalog.values(), (model) => ({
    ...model,
    disabled: model.id !== selected && memberKeys.has(model.id),
  }));
  const canAdd =
    members.length < ROUTING_MEMBER_LIMIT &&
    models.some((model) => !memberKeys.has(model.id));

  const summary = (member: RoutingMember) => {
    const provider = providers.find(({ id }) => id === member.providerId);
    const model = provider?.models?.find(({ id }) => id === member.modelId);
    return {
      provider,
      modelName: model?.name || member.modelId,
      unavailable: routingMemberUnavailableReason(providers, member),
    };
  };

  function choose(index: number, id: string) {
    const choice = catalog.get(id);
    if (!choice || disabled || index > members.length) return;
    const existing = members[index];
    if (!existing && members.length >= ROUTING_MEMBER_LIMIT) return;
    if (memberKeys.has(id) && (!existing || routingMemberKey(existing) !== id))
      return;
    const next = members.slice();
    next[index] = {
      ...(existing ?? { id: crypto.randomUUID() }),
      providerId: choice.providerId,
      modelId: choice.modelId,
    };
    onChange(next);
    setOpenIndex(null);
  }

  function move(from: number, to: number) {
    if (disabled || from === to || from < 0 || to < 0 || to >= members.length)
      return;
    const next = members.slice();
    next.splice(to, 0, ...next.splice(from, 1));
    setOpenIndex(null);
    onChange(next);
  }
  const drag = useRoutingRowDrag("data-routing-member", (from, to) => {
    move(
      members.findIndex((member) => member.id === from),
      members.findIndex((member) => member.id === to),
    );
  });

  return (
    <div className="rt-template-member-picker">
      {members.length > 0 && (
        <ol
          className="rt-template-member-list"
          role="list"
          data-routing-list
          aria-label={t("routingPage.modelOrder")}
        >
          {members.map((member, index) => {
            const { provider, modelName, unavailable } = summary(member);
            const label = `${modelName} · ${provider?.name ?? member.providerId}`;
            return (
              <li
                className="rt-template-member-row"
                data-routing-member={member.id}
                key={member.id}
              >
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="rt-template-member-grip size-7 shrink-0"
                  disabled={disabled || members.length < 2}
                  aria-label={`${t("agentConnections.dragModel")} ${modelName}`}
                  title={t("routingPage.dragHint")}
                  onPointerDown={(event) => {
                    if (!disabled) drag.start(event, member.id);
                  }}
                  onClick={() => drag.consumeClick(member.id)}
                  onKeyDown={(event) => {
                    if (
                      !event.altKey ||
                      !["ArrowUp", "ArrowDown"].includes(event.key)
                    )
                      return;
                    event.preventDefault();
                    event.stopPropagation();
                    move(index, index + (event.key === "ArrowUp" ? -1 : 1));
                  }}
                >
                  <GripVertical size={15} aria-hidden />
                </Button>
                <Popover
                  modal
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
                      <span className="rt-template-member-index" aria-hidden>
                        {index + 1}
                      </span>
                      <ProviderIcon
                        icon={provider?.icon ?? "generic"}
                        name={provider?.name ?? member.providerId}
                        size={16}
                      />
                      <span className="rt-template-member-name">
                        <AccountLabel title>{label}</AccountLabel>
                        {unavailable && (
                          <small>
                            {t(
                              `intentRouting.memberUnavailable.${unavailable}`,
                            )}
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
                      selected={[routingMemberKey(member)]}
                      disabled={disabled}
                      onPick={(id) => choose(index, id)}
                      onDismiss={() => setOpenIndex(null)}
                    />
                  </ModelPickerPopoverContent>
                </Popover>
                {showExpiry && (
                  <RoutingMemberQuotaDates
                    upstream={upstreams.data?.find(
                      ({ id }) => id === member.providerId,
                    )}
                    relayExpiry={provider?.relayExpiry}
                    modelId={member.modelId}
                    loadingUpstreams={upstreams.isPending}
                  />
                )}
                {members.length > 1 &&
                  ([-1, 1] as const).map((offset) => {
                    const action = t(
                      offset < 0
                        ? "intentRouting.moveUp"
                        : "intentRouting.moveDown",
                    );
                    const Icon = offset < 0 ? ChevronUp : ChevronDown;
                    return (
                      <Button
                        key={offset}
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="size-7 shrink-0"
                        aria-label={`${action} ${modelName}`}
                        title={action}
                        disabled={
                          disabled ||
                          index + offset < 0 ||
                          index + offset >= members.length
                        }
                        onClick={() => move(index, index + offset)}
                      >
                        <Icon size={13} aria-hidden />
                      </Button>
                    );
                  })}
              </li>
            );
          })}
        </ol>
      )}
      {(canAdd || (onRecommendOrder && members.length > 1)) && (
        <div className="rt-template-member-actions">
          {canAdd && (
            <Popover
              modal
              open={openIndex === members.length}
              onOpenChange={(open) =>
                setOpenIndex(open ? members.length : null)
              }
            >
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="rt-template-member-add"
                  disabled={disabled}
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
                  onPick={(id) => choose(members.length, id)}
                  onDismiss={() => setOpenIndex(null)}
                />
              </ModelPickerPopoverContent>
            </Popover>
          )}
          {onRecommendOrder && members.length > 1 && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={disabled || !providers.length}
              onClick={onRecommendOrder}
            >
              {t("routingPage.applyRecommendedModelOrder")}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
