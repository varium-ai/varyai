import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { fetchModelsForConfig, type FetchedModel } from "@/lib/api/model-fetch";
import { Plus, ChevronRight } from "lucide-react";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
  type ProviderCopySource,
} from "@/lib/api/routingUpstreams";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderAddSheet } from "./ProviderAddSheet";
import { ProviderImportDialog } from "./ProviderImportDialog";
import { ProviderEditorDialog } from "./ProviderEditorDialog";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";
import {
  globalProviderPresets,
  type GlobalProviderPreset,
} from "@/config/globalProviderPresets";
import { upstreamIcon, UpstreamIcon } from "./UpstreamIcon";
import { SortableWorkspaceRow } from "@/components/SortableWorkspaceRow";
import { SortableProviderList } from "./SortableProviderList";
import {
  useWorkspaceEditing,
  type EditingState,
} from "@/hooks/useWorkspaceEditing";
import "./providers.css";
import { subscriptionProviders } from "@/config/subscriptionProviders";

const orderStorageKey = "vary-provider-order";
function readProviderOrder(): string[] {
  try {
    const value: unknown = JSON.parse(
      localStorage.getItem(orderStorageKey) ?? "null",
    );
    return Array.isArray(value)
      ? [...new Set(value.filter((id): id is string => typeof id === "string"))]
      : [];
  } catch {
    return [];
  }
}

function endpointHost(record: RoutingUpstream) {
  if (record.source.kind !== "api") return "";
  try {
    return new URL(record.source.baseUrl).host;
  } catch {
    return "";
  }
}
function modelId(entry: unknown) {
  if (typeof entry === "string") return entry;
  if (!entry || typeof entry !== "object") return "";
  const row = entry as Record<string, unknown>;
  const value = row.model ?? row.id;
  return typeof value === "string" ? value : "";
}

export function RoutingUpstreamsPanel({
  startAdding = false,
  onEditingChange,
}: {
  startAdding?: boolean;
  onEditingChange?: (state: EditingState) => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const records = useQuery({
    queryKey: ["routing-upstreams"],
    queryFn: routingUpstreamsApi.list,
    refetchInterval: 15_000,
  });
  const [draft, setDraft] = useState<RoutingUpstream | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [loadedRecord, setLoadedRecord] = useState<RoutingUpstream | null>(
    null,
  );
  const [models, setModels] = useState("");
  const [discovered, setDiscovered] = useState<FetchedModel[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [deleting, setDeleting] = useState<RoutingUpstream | null>(null);
  const [selected, setSelected] = useState<Set<string> | null>(null);
  const [confirmBulk, setConfirmBulk] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(startAdding);
  const [importing, setImporting] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [showOff, setShowOff] = useState(false);
  const [providerOrder, setProviderOrder] = useState(readProviderOrder);
  const [copyFrom, setCopyFrom] = useState<ProviderCopySource>();
  const [duplicating, setDuplicating] = useState(false);
  const [probing, setProbing] = useState(false);
  const [startReauth, setStartReauth] = useState(false);
  const [editorPreset, setEditorPreset] = useState<GlobalProviderPreset | null>(
    null,
  );
  const [region, setRegion] = useState("global");
  const presetRegions =
    editorPreset?.regions ??
    (editorPreset?.china
      ? [
          {
            id: "global",
            name: t("providerFlow.global"),
            endpoints: editorPreset.endpoints ?? {},
          },
          {
            id: "china",
            name: t("providerFlow.china"),
            endpoints: editorPreset.china.endpoints,
          },
        ]
      : []);
  const activeEndpoints =
    presetRegions.find((entry) => entry.id === region)?.endpoints ??
    editorPreset?.endpoints;
  const protocolUrls = useRef<
    Partial<Record<GlobalProviderPreset["apiFormat"], string>>
  >({});
  const [discard, setDiscard] = useState(false);
  const initial = useRef("");
  const page = useRef<HTMLElement>(null);
  const listScroll = useRef(0);
  const sheet = useRef<HTMLDivElement>(null);
  const addTrigger = useRef<HTMLButtonElement>(null);
  const listFocusPending = useRef(false);
  const returnToProvider = useRef<string | null>(null);
  const focusPending = useRef(false);
  function openAddPage() {
    listScroll.current = page.current?.closest("main")?.scrollTop ?? 0;
    setSelected(null);
    setAdding(true);
  }
  function closeAddPage() {
    listFocusPending.current = true;
    setAdding(false);
  }
  useEffect(() => {
    if (!adding) return;
    const scroller = page.current?.closest("main");
    if (scroller) scroller.scrollTop = 0;
  }, [adding]);
  useEffect(() => {
    if (!adding && !draft && !busy && !importBusy && listFocusPending.current) {
      listFocusPending.current = false;
      const scroller = page.current?.closest("main");
      if (scroller) scroller.scrollTop = listScroll.current;
      addTrigger.current?.focus({ preventScroll: true });
    }
  }, [adding, draft, busy, importBusy]);
  function returnFocus() {
    focusPending.current = true;
    setStartReauth(false);
    setDraft(null);
    setApiKey("");
  }
  function focusEditorTrigger() {
    if (document.querySelector(".pv-provider-dialog[role=dialog]")) return;
    if (returnToProvider.current) {
      Array.from(document.querySelectorAll<HTMLElement>("[data-provider-id]"))
        .find(
          (element) => element.dataset.providerId === returnToProvider.current,
        )
        ?.querySelector<HTMLButtonElement>(".pv-open")
        ?.focus({ preventScroll: true });
    } else {
      (
        sheet.current?.querySelector<HTMLInputElement>("input[type=search]") ??
        addTrigger.current
      )?.focus({ preventScroll: true });
    }
  }
  useEffect(() => {
    if (!draft && focusPending.current) {
      focusPending.current = false;
      focusEditorTrigger();
    }
  }, [draft]);
  useWorkspaceEditing(
    editorOpen &&
      !!draft &&
      JSON.stringify({ draft, models, apiKey }) !== initial.current,
    busy || probing || importBusy,
    onEditingChange,
  );
  function closeEditor() {
    if (busy || probing) return;
    if (
      draft &&
      JSON.stringify({ draft, models, apiKey }) !== initial.current
    ) {
      setDiscard(true);
      return;
    }
    setStartReauth(false);
    setEditorOpen(false);
  }
  function edit(
    record?: RoutingUpstream,
    preset?: GlobalProviderPreset,
    reauth = false,
  ) {
    setStartReauth(reauth);
    setEditorOpen(true);
    setLoadedRecord(record ?? null);
    returnToProvider.current = record?.id ?? null;
    preset =
      preset ??
      globalProviderPresets.find((entry) => entry.id === record?.presetId);
    setEditorPreset(preset ?? null);
    const nextRegion = preset?.regions?.length
      ? (preset.regions.find((entry) => entry.id === record?.region)?.id ??
        (record?.source.kind === "api"
          ? preset.regions.find((entry) =>
              Object.values(entry.endpoints).includes(
                record.source.kind === "api" ? record.source.baseUrl : "",
              ),
            )?.id
          : undefined) ??
        preset.regions[0].id)
      : (record?.region ?? "global");
    setRegion(nextRegion);
    protocolUrls.current = preset?.endpoints
      ? {
          ...(preset.regions?.find((entry) => entry.id === nextRegion)
            ?.endpoints ??
            (nextRegion === "china"
              ? preset.china?.endpoints
              : preset.endpoints)),
          ...(record?.source.kind === "api"
            ? { [record.source.apiFormat]: record.source.baseUrl }
            : {}),
        }
      : record?.source.kind === "api"
        ? {
            ...record.transport?.endpoints,
            [record.source.apiFormat]: record.source.baseUrl,
          }
        : {};
    const value: RoutingUpstream = record
      ? structuredClone(record)
      : {
          schemaVersion: 1,
          id: "",
          name: preset?.name ?? "",
          revision: 0,
          hasApiKey: false,
          ...(preset ? { presetId: preset.id, region: nextRegion } : {}),
          source: {
            kind: "api",
            baseUrl: preset?.baseUrl ?? "",
            apiFormat: preset?.apiFormat ?? "openai_chat",
            ...(preset?.authType ? { authType: preset.authType } : {}),
            models: [],
            catalog: [],
            mode: "vendor",
            groupsOnly: false,
          },
        };
    setDiscovered([]);
    setCopyFrom(undefined);
    setDraft(value);
    const nextModels =
      value.source.kind !== "codex"
        ? JSON.stringify(value.source.models, null, 2)
        : "";
    setModels(nextModels);
    setApiKey("");
    setError(null);
    initial.current = JSON.stringify({
      draft: value,
      models: nextModels,
      apiKey: "",
    });
  }
  function changeProtocol(format: GlobalProviderPreset["apiFormat"]) {
    if (draft?.source.kind !== "api") return;
    const source = draft.source;
    protocolUrls.current[source.apiFormat] = source.baseUrl;
    const baseUrl = protocolUrls.current[format] ?? source.baseUrl;
    protocolUrls.current[format] = baseUrl;
    setDraft({
      ...draft,
      source: { ...source, apiFormat: format, baseUrl, catalog: [] },
    });
    setDiscovered([]);
  }
  function changeRegion(nextRegion: string) {
    if (!editorPreset || draft?.source.kind !== "api") return;
    if (nextRegion === region) return;
    const endpoints = presetRegions.find(
      (entry) => entry.id === nextRegion,
    )?.endpoints;
    if (!endpoints) return;
    const format = endpoints?.[draft.source.apiFormat]
      ? draft.source.apiFormat
      : (Object.keys(endpoints)[0] as GlobalProviderPreset["apiFormat"]);
    protocolUrls.current = { ...endpoints };
    setRegion(nextRegion);
    const wasDefaultName = [
      editorPreset.name,
      editorPreset.china?.name,
    ].includes(draft.name);
    setDraft({
      ...draft,
      region: nextRegion,
      name:
        wasDefaultName && editorPreset.china
          ? nextRegion === "china"
            ? editorPreset.china.name
            : editorPreset.name
          : draft.name,
      source: {
        ...draft.source,
        apiFormat: format,
        baseUrl: endpoints?.[format] ?? editorPreset.baseUrl,
        models: [],
        catalog: [],
      },
    });
    setModels("[]");
    setDiscovered([]);
  }
  async function refresh() {
    await Promise.all(
      [
        "routing-upstreams",
        "intent-routing-providers",
        "intent-routing-groups",
        "intent-routing-client",
        "gateway-catalog",
        "agent-connections",
        "agent-model-catalog",
      ].map((key) => client.invalidateQueries({ queryKey: [key] })),
    );
  }
  function report(cause: unknown) {
    const code =
      typeof cause === "string" ? cause.replace(/^routingUpstreams\./, "") : "";
    setError(
      [
        "invalid",
        "conflict",
        "limit",
        "referenced",
        "sourceUnavailable",
      ].includes(code)
        ? `modelProviders.${code}`
        : "modelProviders.failed",
    );
  }
  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!draft) return;
    if (draft.source.kind === "codex") {
      setBusy(true);
      setError(null);
      try {
        if (draft.codex)
          await routingUpstreamsApi.saveCodex(
            { ...draft, name: draft.name.trim() },
            draft.codex.settings,
          );
        else
          await routingUpstreamsApi.save({ ...draft, name: draft.name.trim() });
        setEditorOpen(false);
        await refresh();
        if (adding) closeAddPage();
      } catch (cause) {
        report(cause);
      } finally {
        setBusy(false);
      }
      return;
    }
    if (draft.source.kind === "subscription") {
      setBusy(true);
      setError(null);
      try {
        await routingUpstreamsApi.save({ ...draft, name: draft.name.trim() });
        setEditorOpen(false);
        await refresh();
        if (adding) closeAddPage();
      } catch (cause) {
        report(cause);
      } finally {
        setBusy(false);
      }
      return;
    }
    let raw: unknown;
    try {
      raw = JSON.parse(models);
    } catch {
      setError("modelProviders.invalidModels");
      return;
    }
    if (!Array.isArray(raw)) {
      setError("modelProviders.invalidModels");
      return;
    }
    const parsed: unknown[] = raw;
    setBusy(true);
    setError(null);
    try {
      const { apiKey: _storedKey, ...source } = draft.source;
      const value = {
        ...draft,
        name: draft.name.trim(),
        source: {
          ...source,
          baseUrl: source.baseUrl.trim(),
          models: parsed,
          mode: !draft.revision
            ? parsed.length
              ? "picked"
              : "vendor"
            : source.mode,
          catalog: source.catalog,
          ...(apiKey ? { apiKey } : {}),
        },
      };
      if (copyFrom) await routingUpstreamsApi.save(value, copyFrom);
      else await routingUpstreamsApi.save(value);
      setEditorOpen(false);
      await refresh();
      if (adding) closeAddPage();
    } catch (cause) {
      report(cause);
    } finally {
      setBusy(false);
    }
  }
  function modelRow(model: FetchedModel) {
    return {
      model: model.id,
      ...(model.reasoningLevels
        ? { reasoningLevels: model.reasoningLevels }
        : {}),
      ...(model.reasoning !== undefined ? { reasoning: model.reasoning } : {}),
      ...(model.inputModalities
        ? { inputModalities: model.inputModalities }
        : {}),
      ...(model.context_window
        ? { contextWindow: model.context_window, contextSource: "vendor" }
        : {}),
      ...(model.max_context_window
        ? { max_context_window: model.max_context_window }
        : {}),
      ...(model.effective_context_window_percent
        ? {
            effective_context_window_percent:
              model.effective_context_window_percent,
          }
        : {}),
      ...(model.auto_compact_token_limit
        ? { auto_compact_token_limit: model.auto_compact_token_limit }
        : {}),
    };
  }
  function savedCredentialSource() {
    if (!draft || draft.source.kind !== "api") throw new Error("unavailable");
    const saved =
      loadedRecord?.id === (copyFrom?.id ?? draft.id)
        ? loadedRecord
        : records.data?.find(
            (record) => record.id === (copyFrom?.id ?? draft.id),
          );
    if (
      saved?.source.kind !== "api" ||
      saved.revision !== (copyFrom?.revision ?? draft.revision) ||
      saved.source.baseUrl !== draft.source.baseUrl ||
      saved.source.apiFormat !== draft.source.apiFormat ||
      saved.source.authType !== draft.source.authType
    )
      throw new Error("unavailable");
    return saved;
  }
  async function probe(model: string) {
    if (!draft || draft.source.kind !== "api") throw new Error("unavailable");
    setError(null);
    if (!apiKey) {
      const saved = savedCredentialSource();
      return routingUpstreamsApi.probe(saved.id, saved.revision, model);
    }
    return detectProviderProtocols({
      baseUrl: draft.source.baseUrl,
      apiKey,
      model,
      anthropicAuthField:
        draft.source.authType === "api-key"
          ? "ANTHROPIC_API_KEY"
          : "ANTHROPIC_AUTH_TOKEN",
    });
  }
  function duplicate() {
    if (!draft) return;
    const saved = records.data?.find((record) => record.id === draft.id);
    if (!saved) return;
    if (JSON.stringify({ draft, models, apiKey }) !== initial.current) {
      setDuplicating(true);
      setDiscard(true);
      return;
    }
    stageDuplicate(saved);
  }
  function stageDuplicate(saved: RoutingUpstream) {
    edit(saved);
    const next = {
      ...structuredClone(saved),
      id: "",
      revision: 0,
      name: t("providerEditor.copyName", { name: saved.name }),
    };
    const nextModels =
      saved.source.kind === "api"
        ? JSON.stringify(saved.source.models, null, 2)
        : "[]";
    setDraft(next);
    setCopyFrom({ id: saved.id, revision: saved.revision });
    initial.current = JSON.stringify({
      draft: next,
      models: nextModels,
      apiKey: "",
    });
    setDuplicating(false);
  }
  async function fetchDraftModels(): Promise<FetchedModel[]> {
    if (!draft || draft.source.kind !== "api") return [];
    if (!apiKey) {
      const saved = savedCredentialSource();
      return routingUpstreamsApi.discover(saved.id, saved.revision);
    }
    const source = draft.source;
    const header =
      source.apiFormat === "gemini_native"
        ? "x-goog-api-key"
        : source.apiFormat === "anthropic" && source.authType === "api-key"
          ? "x-api-key"
          : "Authorization";
    return fetchModelsForConfig(
      source.baseUrl,
      "",
      false,
      undefined,
      undefined,
      {
        apiFormat: source.apiFormat,
        requestHeaders: {
          [header]: header === "Authorization" ? `Bearer ${apiKey}` : apiKey,
        },
      },
    );
  }
  async function discover() {
    if (!draft || draft.source.kind !== "api") return [];
    setBusy(true);
    setError(null);
    try {
      const found = await fetchDraftModels();
      setDiscovered(found);
      setDraft({
        ...draft,
        source: { ...draft.source, catalog: found.map(modelRow) },
      });
      return found;
    } catch {
      setError("modelProviders.failed");
      return [];
    } finally {
      setBusy(false);
    }
  }
  async function toggle(record: RoutingUpstream, enabled: boolean) {
    setBusy(true);
    setError(null);
    try {
      await routingUpstreamsApi.save({ ...record, enabled });
      await refresh();
    } catch (cause) {
      report(cause);
    } finally {
      setBusy(false);
    }
  }
  async function toggleEditor(enabled: boolean) {
    if (!draft) return;
    if (!draft.revision) {
      setDraft({ ...draft, enabled });
      return;
    }
    const saved = records.data?.find((record) => record.id === draft.id);
    if (!saved || saved.revision !== draft.revision) {
      report("routingUpstreams.conflict");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await routingUpstreamsApi.save({ ...saved, enabled });
      setLoadedRecord((current) =>
        current ? { ...current, enabled, revision: result.revision } : null,
      );
      setDraft((current) =>
        current ? { ...current, enabled, revision: result.revision } : null,
      );
      const baseline = JSON.parse(initial.current) as {
        draft: RoutingUpstream;
      };
      baseline.draft = {
        ...baseline.draft,
        enabled,
        revision: result.revision,
      };
      initial.current = JSON.stringify(baseline);
      await refresh();
    } catch (cause) {
      report(cause);
    } finally {
      setBusy(false);
    }
  }
  const positions = new Map(providerOrder.map((id, index) => [id, index]));
  const ordered = [...(records.data ?? [])].sort(
    (a, b) =>
      (positions.get(a.id) ?? Infinity) - (positions.get(b.id) ?? Infinity),
  );
  function reorder(from: string, to: string) {
    if (busy || draft) return;
    const source = ordered.find((row) => row.id === from);
    const target = ordered.find((row) => row.id === to);
    if (
      !source ||
      !target ||
      (source.enabled === false) !== (target.enabled === false)
    )
      return;
    // Replace only visible slots; filtered and switched-off providers keep their positions.
    const visible = (source.enabled === false ? off : on).map((row) => row.id);
    const sourceIndex = visible.indexOf(from),
      targetIndex = visible.indexOf(to);
    if (sourceIndex < 0 || targetIndex < 0 || sourceIndex === targetIndex)
      return;
    visible.splice(targetIndex, 0, ...visible.splice(sourceIndex, 1));
    const visibleIds = new Set(visible);
    let index = 0;
    const next = ordered.map((row) =>
      visibleIds.has(row.id) ? visible[index++] : row.id,
    );
    setProviderOrder(next);
    try {
      localStorage.setItem(orderStorageKey, JSON.stringify(next));
    } catch {
      // Keep the new order available in this session when storage is unavailable.
    }
  }
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const filtered =
    ordered.filter((record) => {
      const text = [
        record.name,
        endpointHost(record),
        record.source.kind === "api"
          ? record.source.models.map(modelId).join(" ")
          : record.source.kind === "subscription"
            ? subscriptionProviders.find(
                (p) =>
                  record.source.kind === "subscription" &&
                  p.id === record.source.provider,
              )?.aliases
            : "ChatGPT Codex",
      ]
        .join(" ")
        .toLocaleLowerCase();
      return terms.every((term) => text.includes(term));
    }) ?? [];
  const on = filtered.filter((record) => record.enabled !== false);
  const off = filtered.filter((record) => record.enabled === false);
  const visible = showOff || !!terms.length ? filtered : on;
  function toggleSelected(id: string, checked: boolean) {
    setSelected((previous) => {
      const next = new Set(previous);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  }
  async function removeSelected() {
    if (!selected?.size || !records.data) return;
    setBusy(true);
    setError(null);
    try {
      for (const record of records.data.filter((record) =>
        selected.has(record.id),
      )) {
        await routingUpstreamsApi.remove(record.id, record.revision);
        setSelected((previous) => {
          if (!previous) return null;
          const next = new Set(previous);
          next.delete(record.id);
          return next;
        });
      }
      setSelected(null);
    } catch (cause) {
      report(cause);
    } finally {
      setConfirmBulk(false);
      await refresh();
      setBusy(false);
    }
  }
  async function openRecord(record: RoutingUpstream, reauth = false) {
    if (busy || draft) return;
    if (record.source.kind !== "codex") {
      edit(record, undefined, reauth);
      return;
    }
    setError(null);
    setBusy(true);
    try {
      const value = await routingUpstreamsApi.editor(
        record.id,
        record.revision,
      );
      if (value.id !== record.id || value.revision !== record.revision)
        throw new Error("unavailable");
      edit(value, undefined, reauth);
    } catch (cause) {
      report(cause);
    } finally {
      setBusy(false);
    }
  }
  function providerRow(record: RoutingUpstream) {
    const plans = [...new Set(record.subscriptionPlans ?? [])];
    const subscriptionLabel =
      record.source.kind !== "api" && record.reauthRequired === false
        ? record.source.kind === "codex"
          ? "ChatGPT"
          : {
              github_copilot: "GitHub Copilot",
              xai_oauth: "Grok",
              claude: "Claude",
              chatgpt_api: "ChatGPT",
              gemini_cli: "Google",
              antigravity: "Google",
            }[record.source.provider]
        : t("providerParity.subscription");
    const subscription = plans
      .map((plan) => plan || subscriptionLabel)
      .join(" · ");
    const hasPlan =
      plans.some((plan) => !!plan) ||
      (record.source.kind !== "api" && record.reauthRequired === false);
    const needsLogin = record.source.kind !== "api" && record.reauthRequired;
    return (
      <SortableWorkspaceRow
        key={record.id}
        id={record.id}
        disabled={busy || !!draft || !!selected}
        handleLabel={t("modelProviders.sorting.handle", {
          provider: record.name,
        })}
        handleRole={t("modelProviders.sorting.role")}
        handleHint={t("modelProviders.sorting.hint")}
        handleClassName="pv-drag-handle"
        aria-label={record.name}
        data-provider-id={record.id}
        className={`pv-row ${record.enabled === false ? "off" : ""}`}
        onClick={(event) => {
          if ((event.target as Element).closest("button, input")) return;
          if (selected) {
            toggleSelected(record.id, !selected.has(record.id));
            return;
          }
          void openRecord(record);
        }}
      >
        {selected && (
          <input
            type="checkbox"
            className="pv-select-check"
            disabled={busy}
            aria-label={record.name}
            checked={selected.has(record.id)}
            onChange={(event) =>
              toggleSelected(record.id, event.target.checked)
            }
          />
        )}
        <button
          className="pv-open"
          aria-label={`${t("common.edit")} ${record.name}`}
          disabled={busy || !!draft}
          onClick={() => {
            if (selected) {
              toggleSelected(record.id, !selected.has(record.id));
              return;
            }
            void openRecord(record);
          }}
        >
          <span className="pv-logo" aria-hidden>
            <UpstreamIcon
              icon={upstreamIcon(record)}
              name={record.name}
              size={32}
            />
          </span>
          <span className="pv-who">
            <span className="pv-name">
              <span className="pv-title">{record.name}</span>
              {record.enabled === false && (
                <span className="pv-off-badge">
                  {t("modelProviders.switchedOff")}
                </span>
              )}
            </span>
            <span className="pv-sub">
              {record.source.kind === "api"
                ? `${endpointHost(record) || t("providerFlow.custom")} \u00b7 ${t("modelProviders.modelCount", { count: record.source.models.length })}`
                : record.source.kind === "subscription"
                  ? subscriptionProviders.find(
                      (p) =>
                        record.source.kind === "subscription" &&
                        p.id === record.source.provider,
                    )?.name
                  : "ChatGPT / Codex"}
            </span>
          </span>
        </button>
        <span
          className={`pv-key ${needsLogin ? "missing" : record.source.kind !== "api" ? (hasPlan ? "" : "imported") : record.hasApiKey ? "" : "missing"}`}
          title={
            record.source.kind !== "api"
              ? subscription || t("providerParity.subscription")
              : undefined
          }
        >
          <span>
            {needsLogin
              ? t("codexProvider.loggedOut")
              : record.source.kind !== "api"
                ? subscription || t("providerParity.subscription")
                : t(
                    record.hasApiKey
                      ? "modelProviders.keySaved"
                      : "modelProviders.keyMissing",
                  )}
          </span>
        </span>
        {needsLogin && (
          <button
            type="button"
            className="pv-reauth"
            disabled={busy || !!draft || !!selected}
            aria-label={`${t("codexOauth.reauthLogin")} ${record.name}`}
            onClick={() => void openRecord(record, true)}
          >
            {t("codexOauth.reauthLogin")}
          </button>
        )}
        <Switch
          className="mp-switch"
          aria-label={`${t("modelPicker.enableProvider")} ${record.name}`}
          checked={record.enabled !== false}
          disabled={busy || !!draft || !!selected}
          onCheckedChange={(enabled) => void toggle(record, enabled)}
        />
        <ChevronRight className="pv-chevron" size={11} aria-hidden />
      </SortableWorkspaceRow>
    );
  }
  return (
    <section
      ref={page}
      className="vary-providers"
      data-testid="global-providers"
    >
      {!adding && (
        <header className="pv-page-head">
          <div>
            <h1 className="mp-page-heading">{t("modelProviders.title")}</h1>
            <p className="mp-lead">{t("modelProviders.description")}</p>
          </div>
          <Button
            ref={addTrigger}
            data-provider-add-trigger
            className="mp-primary pv-add-trigger"
            size="sm"
            disabled={busy || !!draft || records.isPending || records.isError}
            onClick={openAddPage}
          >
            <Plus size={13} />
            {t("modelProviders.add")}
          </Button>
        </header>
      )}
      {!adding && !!records.data?.length && (
        <div className="pv-head">
          {selected && (
            <label className="pv-select-all">
              <input
                type="checkbox"
                disabled={busy || !visible.length}
                checked={
                  !!visible.length &&
                  visible.every((record) => selected.has(record.id))
                }
                onChange={(event) =>
                  setSelected(
                    event.target.checked
                      ? new Set(visible.map((record) => record.id))
                      : new Set(),
                  )
                }
              />
              <span>{t("modelProviders.selectAll")}</span>
            </label>
          )}
          <Input
            className="pv-query"
            type="search"
            aria-label={t("modelProviders.find")}
            placeholder={t("modelProviders.find")}
            value={query}
            disabled={busy || !!draft}
            onChange={(event) => setQuery(event.target.value)}
          />
          {selected && (
            <button
              type="button"
              className="pv-batch-delete"
              disabled={!selected.size || busy}
              onClick={() => setConfirmBulk(true)}
            >
              {t("modelProviders.removeSelected", { count: selected.size })}
            </button>
          )}
          <button
            type="button"
            className="pv-manage"
            aria-pressed={!!selected}
            disabled={busy || !!draft}
            onClick={() => setSelected(selected ? null : new Set())}
          >
            {t(selected ? "common.cancel" : "modelProviders.manage")}
          </button>
        </div>
      )}
      {records.isError && <p role="alert">{t("modelProviders.failed")}</p>}
      {error && !draft && (
        <p role="alert" className="mb-4 text-sm text-destructive">
          {t(error)}
        </p>
      )}
      {!adding &&
        (records.isPending ? (
          <p role="status">{t("common.loading")}</p>
        ) : (
          <>
            {records.data?.length === 0 && (
              <p className="pv-empty">{t("modelProviders.empty")}</p>
            )}
            {!!on.length && (
              <SortableProviderList records={on} onReorder={reorder}>
                {on.map(providerRow)}
              </SortableProviderList>
            )}
            {!!terms.length && !filtered.length && (
              <p className="pv-empty" role="status">
                {t("modelProviders.noMatches")}
              </p>
            )}
            {!!off.length && (
              <div className="pv-off-section">
                <button
                  className="pv-off-head"
                  aria-label={t("modelProviders.turnedOff")}
                  aria-expanded={showOff || !!terms.length}
                  aria-controls="pv-off-list"
                  disabled={busy || !!draft || !!terms.length}
                  onClick={() => setShowOff(!showOff)}
                >
                  <ChevronRight size={11} />
                  <span>{t("modelProviders.turnedOff")}</span>
                  <small>{off.length}</small>
                </button>
                <SortableProviderList
                  records={off}
                  onReorder={reorder}
                  id="pv-off-list"
                  hidden={!showOff && !terms.length}
                >
                  {off.map(providerRow)}
                </SortableProviderList>
              </div>
            )}
          </>
        ))}
      {adding && (
        <div ref={sheet}>
          <ProviderAddSheet
            onImport={() => setImporting(true)}
            onClose={closeAddPage}
            onChoose={(preset) => edit(undefined, preset)}
            busy={busy || !!draft || importBusy}
            addedSubscriptions={subscriptionProviders
              .filter((preset) =>
                records.data?.some((record) =>
                  preset.id === "codex"
                    ? record.source.kind === "codex"
                    : record.source.kind === "subscription" &&
                      record.source.provider === preset.id,
                ),
              )
              .map((preset) => preset.id)}
            onAccount={async (provider, accountId) => {
              const existing = records.data?.find((record) =>
                provider === "codex"
                  ? record.source.kind === "codex"
                  : record.source.kind === "subscription" &&
                    record.source.provider === provider,
              );
              if (provider !== "codex") {
                const preset = subscriptionProviders.find(
                  (p) => p.id === provider,
                )!;
                const next: RoutingUpstream =
                  existing?.source.kind === "subscription"
                    ? {
                        ...existing,
                        source:
                          existing.source.accountId === accountId
                            ? existing.source
                            : {
                                ...existing.source,
                                accountId,
                                models: [],
                                catalog: [],
                                mode: "vendor",
                              },
                      }
                    : {
                        schemaVersion: 1,
                        id: "",
                        revision: 0,
                        name: preset.name,
                        icon: preset.icon,
                        hasApiKey: false,
                        source: {
                          kind: "subscription",
                          provider,
                          accountId,
                          models: [],
                          catalog: [],
                          mode: "vendor",
                          groupsOnly: false,
                        },
                      };
                edit(next);
                setAdding(false);
                return;
              }
              setBusy(true);
              try {
                const created = existing
                  ? await routingUpstreamsApi.editor(
                      existing.id,
                      existing.revision,
                    )
                  : await routingUpstreamsApi.createCodex();
                if (!created.codex) throw new Error("unavailable");
                created.codex.settings.accounts = [
                  ...new Set([
                    ...(existing ? created.codex.settings.accounts : []),
                    accountId,
                  ]),
                ];
                edit(created);
                setAdding(false);
              } catch (cause) {
                report(cause);
              } finally {
                setBusy(false);
              }
            }}
          />
        </div>
      )}
      <ProviderImportDialog
        open={importing}
        onBusyChange={setImportBusy}
        onClose={() => setImporting(false)}
        onImported={async () => {
          await refresh();
          closeAddPage();
        }}
      />
      {draft && (
        <ProviderEditorDialog
          key={draft.revision ? draft.id : `new-${copyFrom?.id ?? "custom"}`}
          open={editorOpen}
          draft={draft}
          models={models}
          discovered={discovered}
          apiKey={apiKey}
          busy={busy}
          error={error}
          icon={upstreamIcon(draft)}
          region={region}
          regions={presetRegions}
          allowedFormats={activeEndpoints}
          startReauth={startReauth}
          onReauthStarted={() => setStartReauth(false)}
          onDraftChange={setDraft}
          onEnabledChange={(enabled) => void toggleEditor(enabled)}
          onModelsChange={setModels}
          onKeyChange={setApiKey}
          onProtocolChange={changeProtocol}
          onRegionChange={changeRegion}
          onDiscover={discover}
          onFetchProbeModels={fetchDraftModels}
          onProbe={probe}
          onSave={save}
          onClose={closeEditor}
          onDelete={() => {
            setError(null);
            setDeleting(draft);
          }}
          onDuplicate={duplicate}
          onReturnFocus={returnFocus}
          onError={() => setError("modelProviders.failed")}
          nestedOpen={discard || !!deleting}
          onProbingChange={setProbing}
          canDuplicate={draft.source.kind === "api"}
          canSave={
            draft.source.kind !== "subscription" || !!draft.source.accountId
          }
        />
      )}
      <ConfirmDialog
        className="mp-dialog pv-confirm"
        isOpen={discard}
        title={t("providerFlow.discardTitle")}
        message={t("providerFlow.discardHint")}
        confirmText={t("providerFlow.discard")}
        onCancel={() => {
          setDiscard(false);
          setDuplicating(false);
        }}
        onConfirm={() => {
          setDiscard(false);
          if (duplicating && draft) {
            const saved = records.data?.find(
              (record) => record.id === draft.id,
            );
            if (saved) stageDuplicate(saved);
          } else {
            setEditorOpen(false);
          }
          setDuplicating(false);
        }}
      />
      <ConfirmDialog
        className="mp-dialog pv-confirm"
        isOpen={confirmBulk}
        title={t("modelProviders.removeSelected", {
          count: selected?.size ?? 0,
        })}
        message={t("modelProviders.removeSelectedHint", {
          count: selected?.size ?? 0,
        })}
        pending={busy}
        onCancel={() => setConfirmBulk(false)}
        onConfirm={() => void removeSelected()}
      />
      <ConfirmDialog
        className="mp-dialog pv-confirm"
        isOpen={!!deleting}
        title={t("modelProviders.delete")}
        message={t("modelProviders.deleteHint")}
        pending={busy}
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          if (!deleting) return;
          setBusy(true);
          void routingUpstreamsApi
            .remove(deleting.id, deleting.revision)
            .then(async () => {
              if (draft?.id === deleting.id) {
                setEditorOpen(false);
              }
              await refresh();
            })
            .catch(report)
            .finally(() => {
              setDeleting(null);
              setBusy(false);
            });
        }}
      />
    </section>
  );
}
