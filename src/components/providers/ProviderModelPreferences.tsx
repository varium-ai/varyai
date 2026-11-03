import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

const levels = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
type Row = Record<string, unknown>;
function object(value: unknown): Row {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Row)
    : {};
}
function strings(value: unknown): string[] {
  return Array.isArray(value)
    ? value.flatMap((item) => {
        const text = typeof item === "string" ? item : object(item).effort;
        return typeof text === "string" ? [text.toLowerCase()] : [];
      })
    : [];
}

export function ProviderModelPreferences({
  models,
  disabled,
  onChange,
}: {
  models: unknown[];
  disabled: boolean;
  onChange: (models: unknown[]) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [open, setOpen] = useState(false);
  const toggle = useRef<HTMLButtonElement>(null);
  function update(index: number, next: Row) {
    onChange(models.map((value, at) => (at === index ? next : value)));
  }
  return (
    <div className="pv-model-preferences">
      <button
        type="button"
        ref={toggle}
        className={`pv-names-toggle${open ? " on" : ""}`}
        aria-expanded={open}
        aria-controls={id}
        disabled={disabled || !models.length}
        onClick={() => setOpen(!open)}
      >
        {t("modelProviders.namesAndLevels")}
      </button>
      {open && (
        <div
          className="mnames"
          id={id}
          role="region"
          aria-label={t("modelProviders.namesAndLevels")}
          onKeyDown={(event) => {
            if (
              event.key === "Enter" &&
              event.target instanceof HTMLInputElement
            ) {
              event.preventDefault();
              event.target.blur();
            }
          }}
        >
          <div className="mnhead">
            <b>{t("modelProviders.namesAndLevels")}</b>
            <button
              type="button"
              disabled={disabled}
              onClick={() => {
                setOpen(false);
                toggle.current?.focus({ preventScroll: true });
              }}
            >
              {t("modelProviders.foldNames")}
            </button>
          </div>
          <p className="hint">{t("modelProviders.namesDraftHint")}</p>
          {models.map((value, index) => {
            const row =
              typeof value === "string" ? { model: value } : object(value);
            const model = String(row.model ?? row.id ?? "");
            if (!model) return null;
            const prefs = object(row.modelPreferences);
            const vendorLevels = strings(
              row.reasoningLevels ??
                row.reasoning_levels ??
                row.supported_reasoning_levels,
            );
            const quiet =
              row.reasoning === false || row.supports_reasoning === false;
            const kept = Array.isArray(prefs.reasoningLevels)
              ? strings(prefs.reasoningLevels)
              : quiet
                ? []
                : vendorLevels;
            const modalities = strings(
              row.inputModalities ?? row.input_modalities,
            );
            const vendorImages = modalities.length
              ? modalities.includes("image")
              : undefined;
            const images =
              typeof prefs.images === "boolean" ? prefs.images : vendorImages;
            function change(key: string, next?: unknown) {
              const updated = { ...prefs };
              if (next === undefined) delete updated[key];
              else updated[key] = next;
              const updatedRow = { ...row };
              if (Object.keys(updated).length)
                updatedRow.modelPreferences = updated;
              else delete updatedRow.modelPreferences;
              update(index, updatedRow);
            }
            return (
              <div className="mname" key={index}>
                <div className="mwho">
                  <input
                    type="text"
                    maxLength={80}
                    disabled={disabled}
                    aria-label={`${t("modelProviders.modelName")} ${model}`}
                    placeholder={String(
                      row.displayName ?? row.display_name ?? model,
                    )}
                    value={String(prefs.name ?? "")}
                    onChange={(event) =>
                      change("name", event.target.value || undefined)
                    }
                    onBlur={(event) =>
                      change("name", event.target.value.trim() || undefined)
                    }
                  />
                  <code title={model}>{model}</code>
                </div>
                <label className="msame">
                  <span>{t("modelProviders.sameAs")}</span>
                  <input
                    type="text"
                    disabled={disabled}
                    maxLength={200}
                    aria-label={`${t("modelProviders.sameAs")} ${model}`}
                    title={t("modelProviders.sameAsHint")}
                    placeholder={model}
                    value={String(prefs.sameAs ?? "")}
                    onChange={(event) =>
                      change("sameAs", event.target.value || undefined)
                    }
                    onBlur={(event) =>
                      change("sameAs", event.target.value.trim() || undefined)
                    }
                  />
                </label>
                <label className="tick">
                  <input
                    type="checkbox"
                    disabled={disabled}
                    checked={images ?? false}
                    onChange={(event) =>
                      change(
                        "images",
                        event.target.checked === vendorImages
                          ? undefined
                          : event.target.checked,
                      )
                    }
                  />
                  {t("modelProviders.acceptsImages")}
                  {images === undefined && (
                    <small>{t("modelProviders.capabilityUnknown")}</small>
                  )}
                </label>
                <div
                  className="mlevels"
                  role="group"
                  aria-label={`${t("modelProviders.reasoningLevels")} ${model}`}
                >
                  {(vendorLevels.length
                    ? levels.filter((level) => vendorLevels.includes(level))
                    : levels
                  ).map((level) => (
                    <label className="tick" key={level}>
                      <input
                        type="checkbox"
                        disabled={disabled}
                        checked={kept.includes(level)}
                        onChange={(event) => {
                          const next = levels.filter((known) =>
                            known === level
                              ? event.target.checked
                              : kept.includes(known),
                          );
                          change(
                            "reasoningLevels",
                            !quiet &&
                              vendorLevels.length === next.length &&
                              vendorLevels.every((known) =>
                                next.includes(known),
                              )
                              ? undefined
                              : next,
                          );
                        }}
                      />
                      {level}
                    </label>
                  ))}
                  {Array.isArray(prefs.reasoningLevels) && !kept.length && (
                    <span className="hint">
                      {t("modelProviders.noReasoning")}
                    </span>
                  )}
                </div>
                <button
                  type="button"
                  disabled={disabled || !Object.keys(prefs).length}
                  aria-label={`${t("modelProviders.restoreModelDefaults")} ${model}`}
                  onClick={() => {
                    const next = { ...row };
                    delete next.modelPreferences;
                    update(index, next);
                  }}
                >
                  {t("modelProviders.restoreModelDefaults")}
                </button>
                {!!Object.keys(prefs).length && (
                  <span className="hint">
                    {t("modelProviders.modelOverride")}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
