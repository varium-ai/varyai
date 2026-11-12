import { useEffect, useId, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getModelContextWindows } from "@/lib/api/model-fetch";
import {
  mergeModelMetadata,
  modelContextWindow,
  modelContextBadge,
} from "@/lib/providerModelContext";
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
import { isImageGenerationModel } from "@/lib/providerModelKind";
import { subscriptionProviders } from "@/config/subscriptionProviders";
import { settingsApi } from "@/lib/api/settings";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import type { FetchedModel } from "@/lib/api/model-fetch";
import type { ProtocolDetection } from "@/lib/api/provider-protocol-detection";
import {
  globalProviderPresets,
  type GlobalProviderPreset,
} from "@/config/globalProviderPresets";
import { ProviderImageModels } from "./ProviderImageModels";
import type { ProviderErrorHandler } from "./providerErrorToast";
import { CodexProviderFields } from "./CodexProviderFields";
import { ProviderKeysField } from "./ProviderKeysField";
import { SubscriptionProviderFields } from "./SubscriptionProviderFields";

type Format = GlobalProviderPreset["apiFormat"];
export function providerIdSlug(value: string) {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}
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
const formats: Array<[Format, string, string]> = [
  ["openai_chat", "openai", "Chat Completions"],
  ["openai_responses", "responses", "Responses"],
  ["anthropic", "anthropic", "Anthropic"],
  ["gemini_native", "gemini", "Gemini"],
];

export function ProviderEditorDialog({
  open = true,
  draft,
  originalId = draft.id,
  another = false,
  copySourceName,
  models,
  discovered,
  apiKey,
  busy,
  icon,
  region,
  regions,
  allowedFormats,
  startReauth = false,
  onReauthStarted,
  onDraftChange,
  onEnabledChange,
  onModelsChange,
  onKeyChange,
  onProtocolChange,
  onRegionChange,
  onDiscover: _onDiscover,
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
  originalId?: string;
  another?: boolean;
  copySourceName?: string;
  models: string;
  discovered: FetchedModel[];
  apiKey: string;
  busy: boolean;
  icon?: string;
  region: string;
  regions: Array<{ id: string; name: string }>;
  allowedFormats?: Partial<Record<Format, string>>;
  startReauth?: boolean;
  onReauthStarted?: () => void;
  onDraftChange: (value: RoutingUpstream) => void;
  onEnabledChange: (enabled: boolean) => void;
  onModelsChange: (value: string) => void;
  onKeyChange: (value: string) => void;
  onProtocolChange: (value: Format) => void;
  onRegionChange: (value: string) => void;
  onDiscover: () => Promise<FetchedModel[]>;
  onFetchProbeModels: () => Promise<FetchedModel[]>;
  onProbe: (model: string) => Promise<ProtocolDetection[]>;
  onSave: (event: React.FormEvent, stagedModels?: unknown[]) => Promise<void>;
  onClose: () => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onReturnFocus: () => void;
  onError: ProviderErrorHandler;
  nestedOpen: boolean;
  onProbingChange: (value: boolean) => void;
  canSave?: boolean;
  canDuplicate?: boolean;
  loading?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const id = useId();
  const name = useRef<HTMLInputElement>(null);
  const openedName = useRef(draft.name);
  const key = useRef<HTMLInputElement>(null);
  const modelSearchInput = useRef<HTMLInputElement>(null);
  const wasLoading = useRef(loading);
  useEffect(() => {
    if (wasLoading.current && !loading)
      name.current?.focus({ preventScroll: true });
    wasLoading.current = loading;
  }, [loading]);
  const [showAll, setShowAll] = useState(false);
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
      ...(value.displayName ? { displayName: value.displayName } : {}),
      ...(value.kind ? { kind: value.kind } : {}),
      ...(value.outputModalities
        ? { outputModalities: value.outputModalities }
        : {}),
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
    if (providerModelId(value)) {
      const model = providerModelId(value);
      available.set(model, {
        ...mergeModelMetadata(available.get(model), value),
        model,
      });
    }
  const contextIds = [
    ...new Set([
      ...available.keys(),
      ...selected.flatMap((value) => {
        const prefs = (
          value && typeof value === "object"
            ? (value as Record<string, unknown>).modelPreferences
            : undefined
        ) as { sameAs?: string } | undefined;
        return prefs?.sameAs ? [prefs.sameAs] : [];
      }),
    ]),
  ]
    .filter((model) => model.length <= 256 && !/[\x00-\x1f\x7f]/.test(model))
    .sort()
    .slice(0, 4096);
  const publicWindows = useQuery({
    queryKey: ["model-context-windows", contextIds],
    queryFn: () => getModelContextWindows(contextIds),
    enabled: open && draft.source.kind === "api" && contextIds.length > 0,
    staleTime: 60 * 60_000,
    retry: false,
  });
  function contextWindow(value: unknown) {
    const row = value as Record<string, unknown> | undefined;
    const prefs = row?.modelPreferences as { sameAs?: string } | undefined;
    return (
      modelContextWindow(value) ??
      publicWindows.data?.[prefs?.sameAs ?? providerModelId(value)]
    );
  }
  function contextLabel(value: unknown) {
    return modelContextBadge(contextWindow(value), modelLabel(value));
  }
  function choose(rows: unknown[]) {
    setRetained([...available.values()]);
    onModelsChange(JSON.stringify(rows, null, 2));
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
  const chatSelected = selected.filter((row) => !isImageGenerationModel(row));
  async function probe(all: boolean) {
    if (probing || busy) return;
    setProbing(true);
    onProbingChange(true);
    setResults([]);
    try {
      let ids = all
        ? chatSelected.map(providerModelId)
        : [probeModel.trim() || chatSelected.map(providerModelId)[0] || ""];
      if (!ids[0]) {
        const found = await onFetchProbeModels();
        const chat = found.find((row) => !isImageGenerationModel(row));
        ids = chat ? [chat.id] : [];
      }
      ids = ids.filter(Boolean).slice(0, 30);
      let failed = false;
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
          failed = true;
      }
      if (failed) onError("testModels");
    } catch (cause) {
      onError("testModels", cause);
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
          ? (subscriptionProviders.find(
              (provider) =>
                draft.source.kind === "subscription" &&
                provider.id === draft.source.provider,
            )?.website ?? "")
          : (source?.baseUrl ?? ""),
    );
    if (["https:", "http:"].includes(url.protocol)) site = url.origin;
  } catch {
    /* An incomplete address is kept in the draft. */
  }
  const modelTerm = modelSearch.trim().toLocaleLowerCase();
  const chatModels = [...available.values()].filter(
    (value) => !isImageGenerationModel(value),
  );
  const imageModels = [...available.values()].filter(isImageGenerationModel);
  const rows = chatModels.filter((value) =>
    `${providerModelId(value)} ${modelLabel(value)}`
      .toLocaleLowerCase()
      .includes(modelTerm),
  );
  const preset = globalProviderPresets.find(
    (value) => value.id === draft.presetId,
  );
  const regionAtTop =
    regions.length > 1 &&
    regions.every((value) => ["global", "china"].includes(value.id));
  const regionControls = regions.length > 1 && (
    <div className="pv-field">
      <span className="pv-field-label" id={`${id}-regions`}>
        {t(
          preset?.regionLabel === "plan"
            ? "providerFlow.plan"
            : "providerFlow.region",
        )}
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
  );
  const fetchModelsControl = source && (
    <Button
      type="button"
      variant="ghost"
      disabled={disabled || !source.baseUrl || (!apiKey && !draft.hasApiKey)}
      onClick={() => void _onDiscover()}
    >
      {t("providerForm.fetchModels")}
    </Button>
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
            <DialogTitle>
              {draft.revision
                ? openedName.current || t("providerFlow.custom")
                : copySourceName
                  ? t("providerEditor.copyTitle", { name: copySourceName })
                  : ((region === "china"
                      ? (preset?.china?.name ?? preset?.name)
                      : preset?.name) ??
                    (source ? t("providerFlow.custom") : draft.name))}
            </DialogTitle>
            {site && (
              <button
                type="button"
                className="pv-site"
                onClick={() =>
                  void settingsApi
                    .openExternal(site)
                    .catch((cause) => onError("openWebsite", cause))
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
            onInvalidCapture={() => onError("save", "invalid")}
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
              {regionAtTop && regionControls}
              {source && !draft.presetId && (
                <ProviderIconField
                  draft={draft}
                  icon={icon}
                  disabled={disabled}
                  onChange={onDraftChange}
                  onBusyChange={setIconBusy}
                />
              )}
              {source && (!draft.presetId || !!draft.revision || another) && (
                <div className="pv-field">
                  <label htmlFor={`${id}-name`}>
                    {t("modelProviders.name")}
                  </label>
                  <Input
                    ref={name}
                    id={`${id}-name`}
                    required={!draft.presetId}
                    maxLength={120}
                    disabled={disabled}
                    placeholder={
                      another && preset
                        ? t("providerEditor.anotherNamePlaceholder", {
                            name: preset.name,
                          })
                        : (preset?.name ??
                          t("providerEditor.customNamePlaceholder"))
                    }
                    value={
                      preset && draft.name === preset.name ? "" : draft.name
                    }
                    onChange={(event) =>
                      onDraftChange({
                        ...draft,
                        name: event.target.value || preset?.name || "",
                      })
                    }
                  />
                </div>
              )}
              {source && !!draft.revision && (
                <div className="pv-field">
                  <label htmlFor={`${id}-identity`}>ID</label>
                  <div className="pv-field-content">
                    <Input
                      id={`${id}-identity`}
                      placeholder={originalId}
                      value={draft.id}
                      disabled={disabled}
                      maxLength={120}
                      onChange={(event) =>
                        onDraftChange({ ...draft, id: event.target.value })
                      }
                      onBlur={() =>
                        onDraftChange({
                          ...draft,
                          id: providerIdSlug(draft.id) || originalId,
                        })
                      }
                    />
                    <p className="pv-hint">
                      {t("providerEditor.idReference", {
                        id: draft.id || originalId,
                      })}
                      {draft.id !== originalId &&
                        ` · ${t("providerEditor.idMovesReferences", { id: originalId })}`}
                    </p>
                  </div>
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
                !loading && (
                  <p className="pv-hint" role="status">
                    {t("providerEditor.importedAccountHint")}
                  </p>
                )}
              {draft.source.kind === "subscription" && (
                <SubscriptionProviderFields
                  editorOpen={open}
                  draft={draft}
                  disabled={busy || probing}
                  startReauth={startReauth}
                  onReauthStarted={onReauthStarted}
                  onChange={onDraftChange}
                  onError={onError}
                  onBusyChange={setAccountBusy}
                />
              )}
              {draft.codex && (
                <CodexProviderFields
                  editorOpen={open}
                  draft={draft}
                  disabled={busy || probing}
                  startReauth={startReauth}
                  onReauthStarted={onReauthStarted}
                  onChange={onDraftChange}
                  onError={onError}
                  onBusyChange={setAccountBusy}
                />
              )}
              {source && (
                <>
                  {!draft.presetId && (
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
                          placeholder={
                            source.apiFormat === "anthropic"
                              ? "https://…"
                              : source.apiFormat === "gemini_native"
                                ? "https://…/v1beta"
                                : "https://…/v1"
                          }
                          disabled={disabled}
                          value={source.baseUrl}
                          onChange={(event) => {
                            setResults([]);
                            onDraftChange({
                              ...draft,
                              source: {
                                ...source,
                                baseUrl: event.target.value,
                              },
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
                  )}
                  <ProviderKeysField
                    draft={{ ...draft, id: originalId }}
                    apiKey={apiKey}
                    inputRef={key}
                    disabled={disabled}
                    onDraftChange={(next) =>
                      onDraftChange({ ...next, id: draft.id })
                    }
                    onKeyChange={(value) => {
                      setResults([]);
                      onKeyChange(value);
                    }}
                    onBusyChange={setKeyBusy}
                  />
                  {!regionAtTop && regionControls}
                  {(!!draft.revision || !draft.presetId) && (
                    <div className="pv-field">
                      <label htmlFor={`${id}-model-ids`}>
                        {t("modelPicker.models")}
                      </label>
                      <div className="pv-models">
                        {!draft.revision && (
                          <div className="pv-model-footer pv-model-create">
                            <div className="pv-add-model">
                              {fetchModelsControl}
                            </div>
                          </div>
                        )}
                        <>
                          {!!chatModels.length && (
                            <div
                              className={`pv-model-actions${draft.revision ? "" : " pv-create-model-actions"}`}
                            >
                              {!draft.revision && (
                                <span>
                                  {t("providerParity.pickedCount", {
                                    count: chatSelected.length,
                                    total: chatModels.length,
                                  })}
                                </span>
                              )}
                              {chatModels.length > 24 && (
                                <Input
                                  ref={modelSearchInput}
                                  type="search"
                                  className="pv-model-search"
                                  aria-label={t("providerEditor.filterModels", {
                                    count: chatModels.length,
                                  })}
                                  placeholder={t(
                                    "providerEditor.filterModels",
                                    {
                                      count: chatModels.length,
                                    },
                                  )}
                                  disabled={disabled}
                                  value={modelSearch}
                                  onChange={(event) =>
                                    setModelSearch(event.target.value)
                                  }
                                />
                              )}
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
                                disabled={
                                  disabled ||
                                  (!chatSelected.length &&
                                    source.mode === "picked")
                                }
                                onClick={() => {
                                  choose(
                                    selected.filter(isImageGenerationModel),
                                  );
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
                                    <Check
                                      size={11}
                                      aria-hidden
                                      style={{
                                        visibility: checked
                                          ? "visible"
                                          : "hidden",
                                      }}
                                    />
                                    <span>{modelLabel(value)}</span>
                                    {contextLabel(value) && (
                                      <small
                                        title={t("providerEditor.holdsTokens", {
                                          n: contextWindow(
                                            value,
                                          )?.toLocaleString(i18n.language),
                                        })}
                                      >
                                        {contextLabel(value)}
                                      </small>
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
                          {!!draft.revision && !rows.length && (
                            <p className="pv-hint">
                              {t("providerEditor.noModels")}
                            </p>
                          )}
                        </>
                        {!!draft.revision && (
                          <div className="pv-model-footer">
                            <div className="pv-add-model">
                              {fetchModelsControl}
                            </div>
                          </div>
                        )}
                        {!draft.revision && (
                          <p className="pv-hint">
                            {t("providerParity.optionalHint")}
                          </p>
                        )}
                        {!!draft.revision && (
                          <p className="pv-hint">
                            {t(`providerParity.hint_${source.mode}`)}
                          </p>
                        )}
                      </div>
                    </div>
                  )}
                  {(!!draft.revision || !draft.presetId) && (
                    <ProviderImageModels
                      models={imageModels.map((value) => ({
                        id: providerModelId(value),
                        name: modelLabel(value),
                      }))}
                      selected={new Set(selected.map(providerModelId))}
                      disabled={disabled}
                      onToggle={(model) =>
                        choose(
                          selected.some((row) => providerModelId(row) === model)
                            ? selected.filter(
                                (row) => providerModelId(row) !== model,
                              )
                            : [...selected, available.get(model)],
                        )
                      }
                    />
                  )}
                </>
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
                      : draft.revision
                        ? "common.save"
                        : "common.add",
                )}
              </Button>
            </footer>
          </form>
        </section>
      </DialogContent>
    </Dialog>
  );
}
