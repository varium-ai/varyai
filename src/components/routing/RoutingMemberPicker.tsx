import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Check, ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { AccountLabel, useAccountText } from "@/components/AccountLabel";
import { Checkbox } from "@/components/ui/checkbox";
import { ProviderIcon } from "@/components/ProviderIcon";
import type {
  RoutingGroup,
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";
import {
  catalogRoutingMembers,
  routingMemberKey,
  routingMemberAvailable,
  ROUTING_MEMBER_LIMIT,
} from "@/lib/routingMemberSelection";

type Choice = Pick<RoutingMember, "providerId" | "modelId">;

function ModelDisclosure({
  expanded,
  id,
  children,
}: {
  expanded: boolean;
  id: string;
  children: ReactNode;
}) {
  const content = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (expanded) content.current?.removeAttribute("inert");
    else content.current?.setAttribute("inert", "");
  }, [expanded]);
  return (
    <div
      id={id}
      className="rt-provider-collapse"
      data-expanded={expanded}
      aria-hidden={!expanded}
      ref={content}
    >
      <div className="rt-provider-collapse-inner">{children}</div>
    </div>
  );
}

export function RoutingMemberPicker({
  providers,
  members,
  disabled,
  onSelect,
}: {
  providers: RoutingProviderOption[];
  members: RoutingGroup["members"];
  disabled: boolean;
  onSelect: (choices: Choice[], selected: boolean) => void;
}) {
  const { t } = useTranslation();
  const pickerId = useId();
  const [folded, setFolded] = useState<Set<string>>(() => new Set());
  const accountText = useAccountText();
  const selected = new Set(members.map(routingMemberKey));
  const available = catalogRoutingMembers(providers);
  const sections = providers.map((provider) => ({
    ...provider,
    models: [...(provider.models ?? [])],
  }));
  for (const member of members) {
    let provider = sections.find((entry) => entry.id === member.providerId);
    if (!provider) {
      provider = {
        id: member.providerId,
        name: member.providerId,
        taskEligible: false,
        classifierEligible: false,
        models: [],
      };
      sections.push(provider);
    }
    if (!provider.models.some((model) => model.id === member.modelId)) {
      provider.models.push({ id: member.modelId, name: member.modelId });
    }
  }
  const providerNameCounts = new Map<string, number>();
  for (const provider of sections) {
    const key = provider.name.trim().toLocaleLowerCase();
    providerNameCounts.set(key, (providerNameCounts.get(key) ?? 0) + 1);
  }
  return (
    <div className="rt-member-picker">
      <div className="rt-member-picker-actions">
        <button
          type="button"
          disabled={
            disabled ||
            !available.some(
              (choice) => !selected.has(routingMemberKey(choice)),
            ) ||
            members.length >= ROUTING_MEMBER_LIMIT
          }
          onClick={() => onSelect(available, true)}
        >
          {t("providerEditor.selectAll")}
        </button>
        <button
          type="button"
          disabled={disabled || !members.length}
          onClick={() => onSelect(members, false)}
        >
          {t("providerEditor.selectNone")}
        </button>
      </div>
      {sections
        .filter((provider) => provider.models.length)
        .map((provider) => {
          const seen = new Set<string>();
          const models = provider.models.filter((model) => {
            if (!model.id.trim() || seen.has(model.id)) return false;
            seen.add(model.id);
            return true;
          });
          const choices = models.map((model) => ({
            providerId: provider.id,
            modelId: model.id,
          }));
          const providerNameKey = provider.name.trim().toLocaleLowerCase();
          const duplicateName =
            (providerNameCounts.get(providerNameKey) ?? 0) > 1;
          const providerLabel = duplicateName
            ? `${provider.name} · ${provider.id}`
            : provider.name;
          const count = choices.filter((choice) =>
            selected.has(routingMemberKey(choice)),
          ).length;
          const all = choices.length > 0 && count === choices.length;
          const expanded = !folded.has(provider.id);
          const modelsId = `${pickerId}-${encodeURIComponent(provider.id)}`;
          return (
            <section
              key={provider.id}
              className="rt-provider-models"
              role="group"
              aria-label={accountText(provider.name)}
            >
              <div className="rt-provider-models-heading">
                <div className="rt-provider-models-title">
                  <Checkbox
                    checked={all ? true : count ? "indeterminate" : false}
                    disabled={
                      disabled ||
                      (!provider.taskEligible && !count) ||
                      (!count && members.length >= ROUTING_MEMBER_LIMIT)
                    }
                    onCheckedChange={() =>
                      onSelect(
                        all
                          ? choices
                          : choices.filter(
                              (choice) =>
                                routingMemberAvailable(providers, choice) ||
                                selected.has(routingMemberKey(choice)),
                            ),
                        provider.taskEligible && !all,
                      )
                    }
                    aria-label={accountText(provider.name)}
                  />
                  <button
                    type="button"
                    className="rt-provider-models-toggle"
                    aria-label={accountText(provider.name)}
                    aria-expanded={expanded}
                    aria-controls={modelsId}
                    title={accountText(providerLabel)}
                    onClick={() =>
                      setFolded((previous) => {
                        const next = new Set(previous);
                        if (next.has(provider.id)) next.delete(provider.id);
                        else next.add(provider.id);
                        return next;
                      })
                    }
                  >
                    <ProviderIcon
                      icon={provider.icon ?? "generic"}
                      name={accountText(provider.name)}
                      size={14}
                    />
                    <AccountLabel>{provider.name}</AccountLabel>
                    {duplicateName && (
                      <span className="rt-provider-models-id">
                        · {provider.id}
                      </span>
                    )}
                    {provider.appType && (
                      <span className="rt-provider-models-agent">
                        {provider.appType}
                      </span>
                    )}
                    <ChevronDown
                      className="rt-provider-chevron"
                      size={13}
                      aria-hidden
                    />
                  </button>
                </div>
                <span className="rt-provider-models-count">
                  {count} / {models.length}
                </span>
              </div>
              {!provider.taskEligible && (
                <p className="rt-field-hint">
                  {t("intentRouting.memberNotReady")}
                </p>
              )}
              <ModelDisclosure id={modelsId} expanded={expanded}>
                <div className="rt-model-chips">
                  {models.map((model) => {
                    const choice = {
                      providerId: provider.id,
                      modelId: model.id,
                    };
                    const checked = selected.has(routingMemberKey(choice));
                    const ready = routingMemberAvailable(providers, choice);
                    return (
                      <label
                        key={model.id}
                        className={`rt-model-chip${checked ? " is-selected" : ""}`}
                        title={model.id}
                      >
                        <input
                          type="checkbox"
                          aria-label={model.name || model.id}
                          checked={checked}
                          disabled={
                            disabled ||
                            (!checked &&
                              (!ready ||
                                members.length >= ROUTING_MEMBER_LIMIT))
                          }
                          onChange={(event) =>
                            onSelect([choice], event.target.checked)
                          }
                        />
                        <Check size={13} aria-hidden />
                        <span>{model.name || model.id}</span>
                        {!ready && provider.taskEligible && (
                          <small>{t("intentRouting.memberNotReady")}</small>
                        )}
                      </label>
                    );
                  })}
                </div>
              </ModelDisclosure>
            </section>
          );
        })}
      {!sections.some((provider) => provider.models.length) && (
        <p className="rt-field-hint">{t("modelPicker.empty")}</p>
      )}
      {members.length >= ROUTING_MEMBER_LIMIT && (
        <p className="rt-field-hint" role="status">
          {t("intentRouting.memberSelectionLimit", {
            count: ROUTING_MEMBER_LIMIT,
          })}
        </p>
      )}
    </div>
  );
}
