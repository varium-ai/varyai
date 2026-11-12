import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Check, Copy, Pencil, Plus } from "lucide-react";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { copyText } from "@/lib/clipboard";

function mask(value: string) {
  const chars = Array.from(value);
  return chars.length < 12
    ? "••••••••"
    : `${chars.slice(0, 4).join("")}…${chars.slice(-4).join("")}`;
}

export function ProviderKeysField({
  draft,
  apiKey,
  disabled,
  onDraftChange,
  onKeyChange,
  onBusyChange,
  inputRef,
}: {
  draft: RoutingUpstream;
  apiKey: string;
  disabled: boolean;
  onDraftChange: (draft: RoutingUpstream) => void;
  onKeyChange: (value: string) => void;
  onBusyChange: (value: boolean) => void;
  inputRef?: RefObject<HTMLInputElement>;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [editing, setEditing] = useState<string | null>(
    draft.hasApiKey && draft.revision ? null : "",
  );
  const replacementInput = useRef<HTMLInputElement | null>(null);
  const previousEditing = useRef(editing);
  useEffect(() => {
    if (editing !== null && previousEditing.current !== editing)
      (inputRef ?? replacementInput).current?.focus();
    previousEditing.current = editing;
  }, [editing, inputRef]);
  const [show, setShow] = useState(false);
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const [modelPicker, setModelPicker] = useState<{
    keyId: string;
    models: string[];
  } | null>(null);
  useEffect(() => {
    onBusyChange(copying);
  }, [copying, onBusyChange]);
  const summaries = useQuery({
    queryKey: ["routing-upstream-keys", draft.id, draft.revision],
    queryFn: () => routingUpstreamsApi.keys(draft.id, draft.revision),
    enabled: !!draft.revision && typeof routingUpstreamsApi.keys === "function",
    refetchInterval: 3000,
    retry: false,
  });
  if (draft.source.kind !== "api") return null;
  const transport = draft.transport ?? {
    endpoints: {},
    fullUrl: false,
    keys: [],
  };
  const keys = [
    {
      id: "",
      name: transport.primaryName || t("providerEditor.primaryKey"),
      enabled: !transport.primaryOff,
      key: apiKey,
      models: transport.primaryModels,
    },
    ...transport.keys,
  ];
  const preferred = transport.preferredKey ?? "";
  const enabledKeys = keys.filter((key) => key.enabled);
  const active =
    enabledKeys.find((key) => key.id === preferred)?.id ?? enabledKeys[0]?.id;
  const several = enabledKeys.length > 1;
  const modelIds = [
    ...new Set(
      [...draft.source.models, ...draft.source.catalog]
        .map((row) =>
          typeof row === "string"
            ? row
            : row && typeof row === "object"
              ? String(
                  (row as Record<string, unknown>).model ??
                    (row as Record<string, unknown>).id ??
                    "",
                )
              : "",
        )
        .filter(Boolean),
    ),
  ];
  const blocked = disabled || copying;
  function updateTransport(next: typeof transport) {
    onDraftChange({ ...draft, transport: next });
  }
  function toggle(keyId: string, enabled: boolean) {
    updateTransport(
      keyId === ""
        ? { ...transport, primaryOff: !enabled }
        : {
            ...transport,
            keys: transport.keys.map((key) =>
              key.id === keyId ? { ...key, enabled } : key,
            ),
          },
    );
  }
  function prefer(keyId: string) {
    updateTransport({
      ...transport,
      preferredKey: keyId || null,
      ...(keyId === ""
        ? { primaryOff: false }
        : {
            keys: transport.keys.map((key) =>
              key.id === keyId ? { ...key, enabled: true } : key,
            ),
          }),
    });
  }
  function edit(keyId: string) {
    setEditing(keyId);
    setShow(false);
    setError(false);
  }
  function add() {
    const keyId = crypto.randomUUID();
    updateTransport({
      ...transport,
      keys: [
        ...transport.keys,
        { id: keyId, name: "", enabled: true, key: "" },
      ],
    });
    edit(keyId);
  }
  function changeKey(value: string) {
    if (editing === "") onKeyChange(value);
    else
      updateTransport({
        ...transport,
        keys: transport.keys.map((key) =>
          key.id === editing ? { ...key, key: value || undefined } : key,
        ),
      });
  }
  function remove(keyId = editing) {
    updateTransport({
      ...transport,
      preferredKey: preferred === keyId ? null : transport.preferredKey,
      keys: transport.keys.filter((key) => key.id !== keyId),
    });
    setEditing(null);
  }
  function saveModels(models?: string[]) {
    if (!modelPicker) return;
    updateTransport(
      modelPicker.keyId === ""
        ? { ...transport, primaryModels: models }
        : {
            ...transport,
            keys: transport.keys.map((key) =>
              key.id === modelPicker.keyId ? { ...key, models } : key,
            ),
          },
    );
    setModelPicker(null);
  }
  async function copy(keyId: string) {
    setCopying(true);
    setError(false);
    setCopied(null);
    try {
      const value =
        keys.find((key) => key.id === keyId)?.key ||
        (await routingUpstreamsApi.revealKey(draft.id, draft.revision, keyId));
      await copyText(value);
      setCopied(keyId);
    } catch {
      setError(true);
    } finally {
      setCopying(false);
    }
  }
  if (!draft.revision)
    return (
      <div className="pv-field">
        <label htmlFor={`${id}-new-key`}>{t("modelProviders.apiKey")}</label>
        <div className="pv-field-content">
          <div className="pv-new-key">
            <Input
              id={`${id}-new-key`}
              ref={inputRef}
              type={show ? "text" : "password"}
              value={apiKey}
              placeholder={t("providerEditor.keyPlaceholder")}
              autoComplete="off"
              required={!draft.hasApiKey}
              maxLength={16384}
              spellCheck={false}
              disabled={disabled}
              onChange={(event) => onKeyChange(event.target.value)}
            />
            <Button
              type="button"
              variant="ghost"
              disabled={disabled}
              onClick={() => setShow(!show)}
            >
              {t(show ? "providerEditor.hideKey" : "providerEditor.showKey")}
            </Button>
          </div>
          <p className="pv-hint">{t("providerEditor.newKeyHint")}</p>
        </div>
      </div>
    );
  return (
    <div className="pv-field">
      <span className="pv-field-label" id={`${id}-label`}>
        {t("providerEditor.accounts")}
      </span>
      <div className="pv-stack">
        <div
          className="pv-account-list pv-key-pool"
          role="group"
          aria-label={t("providerEditor.savedKeys", { count: keys.length })}
        >
          <ul className="pv-saved-keys">
            {keys.map((key, index) => {
              const summary = summaries.data?.find(
                (entry) => entry.id === key.id,
              );
              const name =
                key.name || t("providerEditor.additionalKey", { n: index });
              return (
                <li
                  className={`pv-account pv-key-row ${key.enabled ? "" : "off"} ${editing === key.id ? "editing" : ""}`}
                  key={key.id}
                >
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={key.enabled}
                    aria-label={t("providerEditor.useKey", { name })}
                    className="pv-key-check"
                    disabled={blocked}
                    onClick={() => toggle(key.id, !key.enabled)}
                  >
                    <Check size={13} aria-hidden="true" />
                  </button>
                  {editing === key.id ? (
                    <div className="pv-key-edit">
                      <div className="pv-key-input">
                        <label htmlFor={`${id}-key`} className="sr-only">
                          {t("modelProviders.apiKey")}
                        </label>
                        <Input
                          ref={inputRef ?? replacementInput}
                          id={`${id}-key`}
                          type={show ? "text" : "password"}
                          autoComplete="new-password"
                          maxLength={16384}
                          value={key.key ?? ""}
                          required={
                            editing === ""
                              ? !draft.hasApiKey
                              : !summaries.data?.some(
                                  (entry) => entry.id === editing,
                                )
                          }
                          disabled={blocked}
                          onChange={(event) => changeKey(event.target.value)}
                          placeholder={
                            summary
                              ? t("modelProviders.keepKey")
                              : t("providerEditor.keyPlaceholder")
                          }
                        />
                        {!!key.key && (
                          <Button
                            type="button"
                            variant="ghost"
                            disabled={blocked}
                            onClick={() => setShow(!show)}
                          >
                            {t(
                              show
                                ? "providerEditor.hideKey"
                                : "providerEditor.showKey",
                            )}
                          </Button>
                        )}
                        <Button
                          type="button"
                          variant="ghost"
                          disabled={blocked}
                          onClick={() => setEditing(null)}
                        >
                          {t("common.done")}
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <span className="pv-key-label" title={name}>
                      {summary?.label && !key.key
                        ? summary.label
                        : key.key
                          ? mask(key.key)
                          : name}
                    </span>
                  )}
                  <button
                    type="button"
                    className="pv-key-action"
                    aria-label={t("providerEditor.copyKey")}
                    title={t("providerEditor.copyKey")}
                    disabled={blocked || (!key.key && !summary)}
                    onClick={() => void copy(key.id)}
                  >
                    {copied === key.id ? (
                      <Check size={12} />
                    ) : (
                      <Copy size={12} />
                    )}
                  </button>
                  <button
                    type="button"
                    className="pv-key-action"
                    aria-label={t("providerEditor.editKey")}
                    title={t("providerEditor.editKey")}
                    disabled={blocked}
                    onClick={() => edit(key.id)}
                  >
                    <Pencil size={12} />
                  </button>
                  {(keys.length > 1 || key.models != null) && (
                    <button
                      type="button"
                      className={`pv-key-models${key.models != null ? " set" : ""}`}
                      disabled={blocked}
                      aria-expanded={modelPicker?.keyId === key.id}
                      onClick={() =>
                        setModelPicker(
                          modelPicker?.keyId === key.id
                            ? null
                            : {
                                keyId: key.id,
                                models: [...(key.models ?? [])],
                              },
                        )
                      }
                    >
                      {t(
                        key.models != null
                          ? "providerEditor.keyModels"
                          : "providerEditor.keyAllModels",
                        { count: key.models?.length ?? 0 },
                      )}
                    </button>
                  )}
                  <span className="pv-key-limit">
                    {summary?.running
                      ? t("providerEditor.keyRunning", {
                          count: summary.running,
                        })
                      : t("providerEditor.keyUnlimitedConcurrency")}
                  </span>
                  <span className="pv-key-limit">
                    {t("providerEditor.keyNoSpendCap")}
                  </span>
                  <span className="pv-key-acts">
                    {(!key.enabled || several) && (
                      <button
                        type="button"
                        disabled={blocked}
                        onClick={() => toggle(key.id, !key.enabled)}
                      >
                        {t(
                          key.enabled
                            ? "providerEditor.keySwitchOff"
                            : "common.enable",
                        )}
                      </button>
                    )}
                    {key.id !== "" && (
                      <button
                        type="button"
                        aria-label={t("providerEditor.removeKey")}
                        disabled={blocked}
                        onClick={() => remove(key.id)}
                      >
                        {t("providerEditor.keyRemove")}
                      </button>
                    )}
                    {active === key.id ? (
                      <small>
                        {t(
                          several
                            ? "providerEditor.firstKey"
                            : "providerEditor.inUse",
                        )}
                      </small>
                    ) : (
                      key.enabled && (
                        <button
                          type="button"
                          disabled={blocked}
                          onClick={() => prefer(key.id)}
                        >
                          {t("providerEditor.makePreferred")}
                        </button>
                      )
                    )}
                  </span>
                  {modelPicker?.keyId === key.id && (
                    <div
                      className="pv-key-model-picker"
                      role="group"
                      aria-label={t("providerEditor.keyModelPicker")}
                    >
                      <div className="pv-model-chips">
                        {[...new Set([...modelIds, ...modelPicker.models])].map(
                          (model) => (
                            <button
                              key={model}
                              type="button"
                              className={`pv-model-chip${modelPicker.models.includes(model) ? " selected" : ""}`}
                              aria-pressed={modelPicker.models.includes(model)}
                              disabled={blocked}
                              onClick={() =>
                                setModelPicker({
                                  ...modelPicker,
                                  models: modelPicker.models.includes(model)
                                    ? modelPicker.models.filter(
                                        (id) => id !== model,
                                      )
                                    : [...modelPicker.models, model],
                                })
                              }
                            >
                              {model}
                            </button>
                          ),
                        )}
                      </div>
                      <p className="pv-hint">
                        {t("providerEditor.keyModelsHint")}
                      </p>
                      <div className="pv-key-model-foot">
                        <Button
                          type="button"
                          variant="outline"
                          disabled={blocked}
                          onClick={() =>
                            setModelPicker({
                              ...modelPicker,
                              models: [
                                ...new Set([
                                  ...modelIds,
                                  ...modelPicker.models,
                                ]),
                              ],
                            })
                          }
                        >
                          {t("providerEditor.keySelectAvailable")}
                        </Button>
                        <Button
                          type="button"
                          variant="outline"
                          disabled={blocked}
                          onClick={() => saveModels()}
                        >
                          {t("providerEditor.keyAllModels")}
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          disabled={blocked}
                          onClick={() => setModelPicker(null)}
                        >
                          {t("common.cancel")}
                        </Button>
                        <Button
                          type="button"
                          className="mp-primary"
                          disabled={blocked || !modelPicker.models.length}
                          onClick={() => saveModels(modelPicker.models)}
                        >
                          {t("common.save")}
                        </Button>
                      </div>
                    </div>
                  )}
                  {editing === key.id && (
                    <div className="pv-key-details pv-stack">
                      {editing !== "" && (
                        <Input
                          aria-label={t("providerEditor.keyName")}
                          value={key.name}
                          placeholder={t("providerEditor.keyName")}
                          disabled={blocked}
                          onChange={(event) =>
                            updateTransport({
                              ...transport,
                              keys: transport.keys.map((key) =>
                                key.id === editing
                                  ? { ...key, name: event.target.value }
                                  : key,
                              ),
                            })
                          }
                        />
                      )}
                      {summary && (
                        <p className="pv-hint">
                          {t("providerEditor.savedKeyHint")}
                        </p>
                      )}
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
          <button
            type="button"
            className="pv-key-add"
            disabled={blocked || transport.keys.length >= 64}
            onClick={add}
          >
            <Plus size={16} />
            {t("providerEditor.addKey")}
          </button>
        </div>
        <p className="pv-hint">{t("providerEditor.keyPoolHint")}</p>
        {error && (
          <p role="alert" className="pv-error">
            {t("providerEditor.copyKeyFailed")}
          </p>
        )}
      </div>
    </div>
  );
}
