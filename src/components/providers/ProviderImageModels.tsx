import { useTranslation } from "react-i18next";
import { SubscriptionModelChip } from "./SubscriptionModelChip";

export function ProviderImageModels({
  models,
  selected,
  disabled,
  onToggle,
}: {
  models: Array<{ id: string; name: string }>;
  selected: Set<string>;
  disabled: boolean;
  onToggle: (id: string) => void;
}) {
  const { t } = useTranslation();
  if (!models.length) return null;
  return (
    <div className="pv-field">
      <span className="pv-field-label">{t("providerEditor.imageModels")}</span>
      <div className="pv-models">
        <div
          className="pv-model-chips"
          role="group"
          aria-label={t("providerEditor.imageModels")}
        >
          {models.map(({ id, name }) => (
            <SubscriptionModelChip
              key={id}
              aria-label={id}
              title={id}
              selected={selected.has(id)}
              disabled={disabled || (!selected.has(id) && selected.size >= 256)}
              onClick={() => onToggle(id)}
            >
              {name}
            </SubscriptionModelChip>
          ))}
        </div>
        <p className="pv-hint">{t("providerEditor.imageModelsHint")}</p>
      </div>
    </div>
  );
}
