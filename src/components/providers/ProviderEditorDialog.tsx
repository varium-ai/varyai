import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowUpRight, Check } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { UpstreamIcon } from "./UpstreamIcon";
import { ProviderIconField } from "./ProviderIconField";
import { settingsApi } from "@/lib/api/settings";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import type { FetchedModel } from "@/lib/api/model-fetch";
import type { ProtocolDetection } from "@/lib/api/provider-protocol-detection";
import type { GlobalProviderPreset } from "@/config/globalProviderPresets";
import { ProviderModelPreferences } from "./ProviderModelPreferences";
import { CodexProviderFields } from "./CodexProviderFields";
import { ProviderKeysField } from "./ProviderKeysField";
import { SubscriptionProviderFields } from "./SubscriptionProviderFields";

type Format = GlobalProviderPreset["apiFormat"];
export function providerModelId(value: unknown): string {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object") return "";
  const row = value as Record<string, unknown>;
  return String(row.model ?? row.id ?? "");
}
function modelLabel(value: unknown) {
  if (!value || typeof value !== "object") return providerModelId(value);
  const row = value as Record<string, unknown>;
  const prefs = row.modelPreferences as { name?: string } | undefined;
  return (
    prefs?.name ||
    String(row.displayName ?? row.display_name ?? providerModelId(value))
  );
}
function contextLabel(value: unknown) {
  if (!value || typeof value !== "object") return "";
  const row = value as Record<string, unknown>;
  const n = Number(
    row.contextWindow ?? row.context_window ?? row.context_length,
  );
  return n >= 1e6
    ? `${+(n / 1e6).toFixed(1)}M`
    : n >= 1000
      ? `${+(n / 1000).toFixed(1)}K`
      : "";
}
const formats: Array<[Format, string, string]> = [
  ["openai_chat", "openai", "Chat Completions"],
  ["openai_responses", "responses", "Responses"],
  ["anthropic", "anthropic", "Anthropic"],
  ["gemini_native", "gemini", "Gemini"],
];

export function ProviderEditorDialog({
  open = true,
  draft,
  models,
  discovered,
  apiKey,
  busy,
  error,
  icon,
  region,
  regions,
  allowedFormats,
  onDraftChange,
  onEnabledChange,
  onModelsChange,
  onKeyChange,
  onProtocolChange,
  onRegionChange,
  onDiscover,
  onFetchProbeModels,
  onProbe,
  onSave,
  onClose,
  onDelete,
  onDuplicate,
  onReturnFocus,
  onError,
  nestedOpen,
  onProbingChange,
  canSave = true,
  canDuplicate = true,
  loading = false,
}: {
  open?: boolean;
  draft: RoutingUpstream;
  models: string;
  discovered: FetchedModel[];
  apiKey: string;
  busy: boolean;
  error: string | null;
  icon?: string;
  region: string;
  regions: Array<{ id: string; name: string }>;
  allowedFormats?: Partial<Record<Format, string>>;
  onDraftChange: (value: RoutingUpstream) => void;
  onEnabledChange: (enabled: boolean) => void;
  onModelsChange: (value: string) => void;
  onKeyChange: (value: string) => void;
  onProtocolChange: (value: Format) => void;
  onRegionChange: (value: string) => void;
  onDiscover: () => Promise<FetchedModel[]>;
  onFetchProbeModels: () => Promise<FetchedModel[]>;
  onProbe: (model: string) => Promise<ProtocolDetection[]>;
  onSave: (event: React.FormEvent) => Promise<void>;
  onClose: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onReturnFocus: () => void;
  onError: () => void;
  nestedOpen: boolean;
  onProbingChange: (value: boolean) => void;
  canSave?: boolean;
  canDuplicate?: boolean;
  loading?: boolean;
}) {
  const { t } = useTranslation();
  const id = useId();
  const name = useRef<HTMLInputElement>(null);
  const key = useRef<HTMLInputElement>(null);
  const modelSearchInput = useRef<HTMLInputElement>(null);
  const wasLoading = useRef(loading);
  useEffect(() => {
    if (wasLoading.current && !loading)
      name.current?.focus({ preventScroll: true });
    wasLoading.current = loading;
  }, [loading]);
  const [showAll, setShowAll] = useState(false);
  const [typedModel, setTypedModel] = useState(() => {
    if (draft.revision) return "";
    try {
      return (JSON.parse(models) as unknown[]).map(providerModelId).join(", ");
    } catch {
      return "";
    }
  });
  const [modelSearch, setModelSearch] = useState("");
  const [probeModel, setProbeModel] = useState("");
  const [probing, setProbing] = useState(false);
  const [accountBusy, setAccountBusy] = useState(false);
  const [iconBusy, setIconBusy] = useState(false);
  const [keyBusy, setKeyBusy] = useState(false);
  useEffect(() => {
    onProbingChange(probing || accountBusy || iconBusy || keyBusy);
  }, [probing, accountBusy, iconBusy, keyBusy, onProbingChange]);
  const [results, setResults] = useState<
    Array<{ model: string; results: ProtocolDetection[] }>
  >([]);
  const [retained, setRetained] = useState<unknown[]>(() => {
    try {
      const value: unknown = JSON.parse(models);
      return Array.isArray(value) ? value : [];
    } catch {
      return [];
    }
  });
  let selected: unknown[] = [];
  try {
    const value: unknown = JSON.parse(models);
    if (Array.isArray(value)) selected = value;
  } catch {
    /* Invalid drafts are rejected by the save boundary. */
  }
  const available = new Map<string, unknown>();
  for (const value of [
    ...discovered.map((value) => ({
      model: value.id,
      ...(value.reasoningLevels
        ? { reasoningLevels: value.reasoningLevels }
        : {}),
      ...(value.reasoning !== undefined ? { reasoning: value.reasoning } : {}),
      ...(value.inputModalities
        ? { inputModalities: value.inputModalities }
        : {}),
      ...(value.context_window
        ? { contextWindow: value.context_window, contextSource: "vendor" }
        : {}),
      ...(value.max_context_window
        ? { max_context_window: value.max_context_window }
        : {}),
      ...(value.effective_context_window_percent
        ? {
            effective_context_window_percent:
              value.effective_context_window_percent,
          }
        : {}),
      ...(value.auto_compact_token_limit
        ? { auto_compact_token_limit: value.auto_compact_token_limit }
        : {}),
    })),
    ...(draft.source.kind === "api" ? draft.source.catalog : []),
    ...retained,
    ...selected,
  ])
    if (providerModelId(value)) available.set(providerModelId(value), value);
  function choose(rows: unknown[]) {
    setRetained([...available.values()]);
    onModelsChange(JSON.stringify(rows, null, 2));
    if (!draft.revision) setTypedModel(rows.map(providerModelId).join(", "));
  }
  function toggle(model: string) {
    if (draft.source.kind === "api" && draft.revision)
      onDraftChange({ ...draft, source: { ...draft.source, mode: "picked" } });
    choose(
      selected.some((value) => providerModelId(value) === model)
        ? selected.filter((value) => providerModelId(value) !== model)
        : selected.length < 256
          ? [...selected, available.get(model)]
          : selected,
    );
  }
  function addModel() {
    const ids = typedModel
      .split(",")
      .map((value) => value.trim())
      .filter(Boolean);
    const next = [...selected];
    for (const model of ids) {
      if (
        !model ||
        selected.some((value) => providerModelId(value) === model) ||
        selected.length >= 256
      )
        continue;
      if (!next.some((row) => providerModelId(row) === model))
        next.push(available.get(model) ?? { model });
    }
    choose(next.slice(0, 256));
    if (draft.source.kind === "api" && draft.revision)
      onDraftChange({ ...draft, source: { ...draft.source, mode: "picked" } });
    setTypedModel("");
  }
  async function probe(all: boolean) {
    if (probing || busy) return;
    setProbing(true);
    onProbingChange(true);
    setResults([]);
    try {
      let ids = all
        ? selected.map(providerModelId)
        : [probeModel.trim() || selected.map(providerModelId)[0] || ""];
      if (!ids[0]) {
        const found = await onFetchProbeModels();
        ids = found[0] ? [found[0].id] : [];
      }
      ids = ids.filter(Boolean).slice(0, 30);
      for (let at = 0; at < ids.length; at += 3) {
        const outcomes = await Promise.allSettled(
          ids.slice(at, at + 3).map(async (model) => ({
            model,
            results: await onProbe(model),
          })),
        );
        setResults((old) => [
          ...old,
          ...outcomes.flatMap((outcome) =>
            outcome.status === "fulfilled" ? [outcome.value] : [],
          ),
        ]);
        if (outcomes.some((outcome) => outcome.status === "rejected"))
          onError();
      }
    } catch {
      onError();
    } finally {
      setProbing(false);
      onProbingChange(false);
    }
  }
  const source = draft.source.kind === "api" ? draft.source : null;
  const disabled = busy || probing || accountBusy || iconBusy || keyBusy;
  let site = "";
  try {
    const url = new URL(
      draft.codex
        ? "https://chatgpt.com"
        : draft.source.kind === "subscription"
          ? draft.source.provider === "github_copilot"
            ? "https://github.com"
            : "https://grok.com"
          : (source?.baseUrl ?? ""),
    );
    if (["https:", "http:"].includes(url.protocol)) site = url.origin;
  } catch {
    /* An incomplete address is kept in the draft. */
  }
  const modelTerm = modelSearch.trim().toLocaleLowerCase();
  const rows = [...available.values()].filter((value) =>
    `${providerModelId(value)} ${modelLabel(value)} ${draft.name}`
      .toLocaleLowerCase()
      .includes(modelTerm),
  );
  return (
    <Dialog
      open={open}
      onOpenChange={(open) => {
        if (!open && !disabled && !nestedOpen) onClose();
      }}
    >
      <DialogContent
        className={`mp-dialog pv-provider-dialog${draft.codex ? " pv-codex-dialog" : ""}`}
        aria-busy={loading}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          const target =
            (draft.revision || !draft.presetId ? name.current : key.current) ??
            (event.currentTarget instanceof HTMLElement
              ? event.currentTarget
              : null);
          target?.focus({
            preventScroll: true,
          });
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          onReturnFocus();
        }}
        onEscapeKeyDown={(event) => {
          if (disabled || nestedOpen) event.preventDefault();
          if (modelSearch && event.target === modelSearchInput.current) {
            event.preventDefault();
            setModelSearch("");
          }
        }}
        onInteractOutside={(event) => {
          if (disabled || nestedOpen) event.preventDefault();
        }}
      >
        <section
          className="pv-editor"
          role="region"
          aria-label={draft.name || t("providerFlow.custom")}
        >
          <header className="pv-editor-head">
            <span className="pv-editor-logo" aria-hidden>
              <UpstreamIcon icon={icon} name={draft.name} size={20} />
            </span>
            <DialogTitle>{draft.name || t("providerFlow.custom")}</DialogTitle>
            {site && (
              <button
                type="button"
                className="pv-site"
                onClick={() =>
                  void settingsApi.openExternal(site).catch(onError)
                }
              >
                {new URL(site).host}
                <ArrowUpRight size={12} />
              </button>
            )}
            {!!draft.revision && (
              <Switch
                className="mp-switch"
                aria-label={t("providerEditor.enabled")}
                checked={draft.enabled !== false}
                disabled={disabled}
                onCheckedChange={onEnabledChange}
              />
            )}
            <DialogDescription className="sr-only">
              {t("providerEditor.description")}
            </DialogDescription>
          </header>
          <form
            onSubmit={(event) => {
              if (disabled) event.preventDefault();
              else void onSave(event);
            }}
            className="pv-editor-form"
          >
            <div className="pv-editor-body">
              {draft.enabled === false && (
                <p className="pv-off-note">{t("providerEditor.offHint")}</p>
              )}
              {regions.length > 1 && (
                <div className="pv-field">
                  <span className="pv-field-label" id={`${id}-regions`}>
                    {t("providerFlow.region")}
                  </span>
                  <div
                    className="pv-protocols"
                    role="radiogroup"
                    aria-labelledby={`${id}-regions`}
                  >
                    {regions.map((value) => (
                      <button
                        key={value.id}
                        type="button"
                        role="radio"
                        aria-checked={region === value.id}
                        disabled={disabled}
                        onClick={() => onRegionChange(value.id)}
                      >
                        {value.name}
                      </button>
                    ))}
                  </div>
                </div>
              )}
              {!draft.codex && (!draft.presetId || !!draft.revision) && (
                <div className="pv-field">
                  <label htmlFor={`${id}-name`}>
                    {t("modelProviders.name")}
                  </label>
                  <Input
                    ref={name}
                    id={`${id}-name`}
                    required
                    maxLength={120}
                    disabled={disabled}
                    value={draft.name}
                    onChange={(event) =>
                      onDraftChange({ ...draft, name: event.target.value })
                    }
                  />
                </div>
              )}
              {loading && (
                <p role="status" className="pv-hint">
                  {t("common.loading")}
                </p>
              )}
              {!source &&
                draft.source.kind !== "subscription" &&
                !draft.codex &&
                !loading &&
                !error && (
                  <p className="pv-hint" role="status">
                    {t("providerEditor.importedAccountHint")}
                  </p>
                )}
              {draft.source.kind === "subscription" && (
                <SubscriptionProviderFields
                  draft={draft}
                  disabled={busy || probing}
                  onChange={onDraftChange}
                  onError={onError}
                  onBusyChange={setAccountBusy}
                />
              )}
              {draft.codex && (
                <CodexProviderFields
                  draft={draft}
                  disabled={busy || probing}
                  onChange={onDraftChange}
                  onError={onError}
                  onBusyChange={setAccountBusy}
                />
              )}
              {draft.codex && (
                <details className="pv-advanced">
                  <summary>{t("codexProvider.advanced")}</summary>
                  <div className="pv-field">
                    <label htmlFor={`${id}-codex-name`}>
                      {t("modelProviders.name")}
                    </label>
                    <Input
                      id={`${id}-codex-name`}
                      value={draft.name}
                      disabled={disabled}
                      onChange={(event) =>
                        onDraftChange({ ...draft, name: event.target.value })
                      }
                    />
                  </div>
                  <p className="pv-hint">
                    ID <code>{draft.id}</code>
                  </p>
                </details>
              )}
              {source && (
                <>
                  <div className="pv-field">
                    <label htmlFor={`${id}-url`}>Base URL</label>
                    <div className="pv-stack">
                      <div
                        className="pv-protocols"
                        role="radiogroup"
                        aria-label={t("modelProviders.protocol")}
                      >
                        {formats
                          .filter(
                            ([format]) =>
                              !allowedFormats || !!allowedFormats[format],
                          )
                          .map(([format, label, accessible]) => (
                            <button
                              key={format}
                              type="button"
                              role="radio"
                              aria-label={accessible}
                              aria-checked={source.apiFormat === format}
                              disabled={disabled}
                              onClick={() => {
                                setResults([]);
                                onProtocolChange(format);
                              }}
                            >
                              {t(`providerEditor.${label}`)}
                            </button>
                          ))}
                      </div>
                      <Input
                        id={`${id}-url`}
                        aria-label={t("modelProviders.baseUrl")}
                        required
                        type="url"
                        disabled={disabled}
                        value={source.baseUrl}
                        onChange={(event) => {
                          setResults([]);
                          onDraftChange({
                            ...draft,
                            source: { ...source, baseUrl: event.target.value },
                          });
                        }}
                      />
                      <div className="pv-probe-actions">
                        <Button
                          type="button"
                          variant="outline"
                          disabled={
                            disabled ||
                            !source.baseUrl ||
                            (!apiKey && !draft.hasApiKey)
                          }
                          onClick={() => void probe(false)}
                        >
                          {t(
                            probing
                              ? "providerEditor.probing"
                              : "providerEditor.detect",
                          )}
                        </Button>
                        <Input
                          aria-label={t("providerEditor.probeModel")}
                          placeholder={t("providerEditor.probeModelHint")}
                          value={probeModel}
                          disabled={disabled}
                          onChange={(event) =>
                            setProbeModel(event.target.value)
                          }
                        />
                        <Button
                          type="button"
                          variant="outline"
                          title={t("providerEditor.testHint", { count: 30 })}
                          disabled={
                            disabled ||
                            !selected.length ||
                            (!apiKey && !draft.hasApiKey)
                          }
                          onClick={() => void probe(true)}
                        >
                          {t("providerEditor.testSelected")}
                        </Button>
                      </div>
                      {!!results.length && (
                        <div className="pv-probe-results" role="status">
                          {results.map((value) => (
                            <div key={value.model}>
                              <code>{value.model}</code>
                              {value.results.map((result) => (
                                <span
                                  key={result.format}
                                  className={
                                    result.text.status === "verified"
                                      ? "ok"
                                      : ""
                                  }
                                >
                                  {
                                    formats.find(
                                      ([format]) => format === result.format,
                                    )?.[2]
                                  }{" "}
                                  ·{" "}
                                  {t(
                                    `providerEditor.${result.text.status === "verified" ? "verified" : "unverified"}`,
                                  )}
                                  {result.stream &&
                                    ` / ${t(result.stream.status === "verified" ? "providerEditor.streamVerified" : "providerEditor.streamUnverified")}`}
                                </span>
                              ))}
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>
                  <ProviderKeysField
                    draft={draft}
                    apiKey={apiKey}
                    inputRef={key}
                    disabled={disabled}
                    onDraftChange={onDraftChange}
                    onKeyChange={(value) => {
                      setResults([]);
                      onKeyChange(value);
                    }}
                    onBusyChange={setKeyBusy}
                  />
                  <ProviderIconField
                    draft={draft}
                    icon={icon}
                    disabled={disabled}
                    onChange={onDraftChange}
                    onBusyChange={setIconBusy}
                  />
                  <div className="pv-field">
                    <label htmlFor={`${id}-model-ids`}>
                      {t("modelPicker.models")}
                    </label>
                    <div className="pv-models">
                      {!!draft.revision && (
                        <>
                          <div
                            className="pv-protocols"
                            role="radiogroup"
                            aria-label={t("providerEditor.modelView")}
                          >
                            {(
                              ["vendor", "picked", "best", "usage"] as const
                            ).map((mode) => (
                              <button
                                key={mode}
                                type="button"
                                role="radio"
                                aria-checked={source.mode === mode}
                                disabled={disabled}
                                onClick={() =>
                                  onDraftChange({
                                    ...draft,
                                    source: { ...source, mode },
                                  })
                                }
                              >
                                {t(`providerParity.mode_${mode}`)}
                              </button>
                            ))}
                          </div>
                          <p className="pv-hint">
                            {t(`providerParity.hint_${source.mode}`)}
                          </p>
                        </>
                      )}
                      <div className="pv-add-model">
                        <Input
                          id={`${id}-model-ids`}
                          aria-label={t("providerEditor.addModelId")}
                          placeholder={t("providerParity.optionalModels")}
                          disabled={disabled}
                          value={typedModel}
                          onChange={(event) => {
                            if (draft.revision)
                              setTypedModel(event.target.value);
                            else {
                              const ids = [
                                ...new Set(
                                  event.target.value
                                    .split(",")
                                    .map((id) => id.trim())
                                    .filter(Boolean),
                                ),
                              ];
                              setTypedModel(event.target.value);
                              onModelsChange(
                                JSON.stringify(
                                  ids.map(
                                    (model) =>
                                      available.get(model) ?? { model },
                                  ),
                                ),
                              );
                            }
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") {
                              event.preventDefault();
                              if (draft.revision) addModel();
                            }
                          }}
                        />
                        {!!draft.revision && (
                          <Button
                            type="button"
                            variant="ghost"
                            disabled={disabled || !typedModel.trim()}
                            onClick={addModel}
                          >
                            {t("providerEditor.addModel")}
                          </Button>
                        )}
                        <Button
                          type="button"
                          variant="ghost"
                          disabled={
                            disabled ||
                            !source.baseUrl ||
                            (!apiKey && !draft.hasApiKey)
                          }
                          onClick={() => void onDiscover()}
                        >
                          {t("providerForm.fetchModels")}
                        </Button>
                      </div>
                      {(!!draft.revision || discovered.length > 0) && (
                        <>
                          {available.size > 24 && (
                            <Input
                              ref={modelSearchInput}
                              type="search"
                              className="pv-model-search"
                              aria-label={t("modelPicker.search")}
                              placeholder={t("modelPicker.search")}
                              disabled={disabled}
                              value={modelSearch}
                              onChange={(event) =>
                                setModelSearch(event.target.value)
                              }
                            />
                          )}
                          {!!available.size && (
                            <div className="pv-model-actions">
                              <span>
                                {t("providerParity.pickedCount", {
                                  count: selected.length,
                                  total: available.size,
                                })}
                              </span>
                              <Button
                                type="button"
                                variant="ghost"
                                disabled={disabled}
                                onClick={() => {
                                  choose(
                                    [
                                      ...new Map(
                                        [...selected, ...rows].map((row) => [
                                          providerModelId(row),
                                          row,
                                        ]),
                                      ).values(),
                                    ].slice(0, 256),
                                  );
                                  if (draft.revision)
                                    onDraftChange({
                                      ...draft,
                                      source: { ...source, mode: "picked" },
                                    });
                                }}
                              >
                                {t("providerEditor.selectAll")}
                              </Button>
                              <Button
                                type="button"
                                variant="ghost"
                                disabled={disabled || !selected.length}
                                onClick={() => {
                                  choose([]);
                                  if (draft.revision)
                                    onDraftChange({
                                      ...draft,
                                      source: { ...source, mode: "picked" },
                                    });
                                }}
                              >
                                {t("providerEditor.selectNone")}
                              </Button>
                            </div>
                          )}
                          <div
                            className="pv-model-chips"
                            role="group"
                            aria-label={t("modelPicker.models")}
                          >
                            {rows
                              .filter(
                                (row, index) =>
                                  showAll ||
                                  !!modelTerm ||
                                  index < 120 ||
                                  selected.some(
                                    (p) =>
                                      providerModelId(p) ===
                                      providerModelId(row),
                                  ),
                              )
                              .map((value) => {
                                const model = providerModelId(value);
                                const checked = selected.some(
                                  (item) => providerModelId(item) === model,
                                );
                                return (
                                  <button
                                    key={model}
                                    type="button"
                                    className={`pv-model-chip ${checked ? "selected" : draft.revision && source.mode !== "picked" && source.mode !== "usage" ? "served" : ""}`}
                                    aria-pressed={checked}
                                    aria-label={model}
                                    title={model}
                                    disabled={
                                      disabled ||
                                      (!checked && selected.length >= 256)
                                    }
                                    onClick={() => toggle(model)}
                                  >
                                    {checked && <Check size={11} aria-hidden />}
                                    <span>{modelLabel(value)}</span>
                                    {contextLabel(value) && (
                                      <small>{contextLabel(value)}</small>
                                    )}
                                  </button>
                                );
                              })}
                            {!showAll && !modelTerm && rows.length > 120 && (
                              <button
                                type="button"
                                onClick={() => setShowAll(true)}
                              >
                                {t("providerParity.showOther", {
                                  count: rows.length - 120,
                                })}
                              </button>
                            )}
                          </div>
                          {!rows.length && (
                            <p className="pv-hint">
                              {t("providerEditor.noModels")}
                            </p>
                          )}
                          {!!selected.length && (
                            <ProviderModelPreferences
                              models={selected}
                              disabled={disabled}
                              onChange={choose}
                            />
                          )}
                        </>
                      )}
                      {!draft.revision && (
                        <p className="pv-hint">
                          {t("providerParity.optionalHint")}
                        </p>
                      )}
                      {!!draft.revision && (
                        <label className="cx-check pv-groups-only">
                          <input
                            type="checkbox"
                            checked={source.groupsOnly}
                            disabled={disabled}
                            onChange={(event) =>
                              onDraftChange({
                                ...draft,
                                source: {
                                  ...source,
                                  groupsOnly: event.target.checked,
                                },
                              })
                            }
                          />
                          {t("codexProvider.groupsOnly")}
                        </label>
                      )}
                    </div>
                  </div>
                  {source.apiFormat === "anthropic" && (
                    <details className="pv-advanced">
                      <summary>{t("providerParity.more")}</summary>
                      <div className="pv-field">
                        <label htmlFor={`${id}-auth`}>
                          {t("modelProviders.authentication")}
                        </label>
                        <select
                          id={`${id}-auth`}
                          disabled={disabled}
                          value={source.authType ?? "bearer"}
                          onChange={(event) =>
                            onDraftChange({
                              ...draft,
                              source: {
                                ...source,
                                authType: event.target.value as
                                  | "api-key"
                                  | "bearer"
                                  | "both",
                              },
                            })
                          }
                        >
                          <option value="bearer">Authorization: Bearer</option>
                          <option value="api-key">x-api-key</option>
                          <option value="both">
                            Authorization + x-api-key
                          </option>
                        </select>
                      </div>
                    </details>
                  )}
                </>
              )}
              {error && (
                <p role="alert" className="pv-error">
                  {t(error)}
                </p>
              )}
            </div>
            <footer className="pv-editor-foot">
              {!!draft.revision && (
                <>
                  <Button
                    type="button"
                    variant="ghost"
                    className="pv-delete"
                    disabled={disabled}
                    onClick={onDelete}
                  >
                    {t("common.delete")}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={onDuplicate}
                    disabled={disabled || !canDuplicate}
                  >
                    {t("providerEditor.duplicate")}
                  </Button>
                </>
              )}
              <span className="pv-foot-space" />
              <Button
                type="button"
                variant="ghost"
                disabled={disabled}
                onClick={onClose}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="submit"
                className="mp-primary"
                disabled={disabled || !canSave}
              >
                {t(
                  loading
                    ? "common.loading"
                    : busy
                      ? "modelPicker.saving"
                      : "common.save",
                )}
              </Button>
            </footer>
          </form>
        </section>
      </DialogContent>
    </Dialog>
  );
}
