import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { fetchModelsForConfig, type FetchedModel } from "@/lib/api/model-fetch";
import { toast } from "sonner";
import { Plus, ChevronRight, Copy, Trash2 } from "lucide-react";
import { isImageGenerationModel } from "@/lib/providerModelKind";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
  type ProviderCopySource,
} from "@/lib/api/routingUpstreams";
import { Button } from "@/components/ui/button";
import { CopyAgentSetupButton } from "@/components/CopyAgentSetupButton";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderAddSheet } from "./ProviderAddSheet";
import { ProviderImportDialog } from "./ProviderImportDialog";
import { ProviderEditorDialog, providerIdSlug } from "./ProviderEditorDialog";
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
import {
  showProviderError,
  type ProviderErrorHandler,
} from "./providerErrorToast";
import { ProviderListQuota } from "./ProviderListQuota";
import { ProviderListSkeleton } from "./ProviderLoading";
import { subscriptionProviders } from "@/config/subscriptionProviders";
import { randomBuiltinIcon } from "@/icons/builtin";

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
  const [discovering, setDiscovering] = useState(false);
  const discoveryRequest = useRef<object | null>(null);
  const latestEditorInput = useRef({ draft, apiKey });
  latestEditorInput.current = { draft, apiKey };
  useEffect(
    () => () => {
      discoveryRequest.current = null;
    },
    [],
  );
  const [deleting, setDeleting] = useState<RoutingUpstream | null>(null);
  const [selected, setSelected] = useState<Set<string> | null>(null);
  const [confirmBulk, setConfirmBulk] = useState(false);
  const [deletingBulk, setDeletingBulk] = useState(false);
  const [busy, setBusy] = useState(false);
  const report: ProviderErrorHandler = useCallback(
    (operation, cause) => showProviderError(t, operation, cause),
    [t],
  );
  useEffect(() => {
    if (records.isError) report("loadProviders", records.error);
  }, [records.isError, report]);
  const [adding, setAdding] = useState(startAdding);
  const [importing, setImporting] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [showOff, setShowOff] = useState(false);
  const [providerOrder, setProviderOrder] = useState(readProviderOrder);
  const [copyFrom, setCopyFrom] = useState<ProviderCopySource>();
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
  const editorKey = draft?.revision
    ? (loadedRecord?.id ?? draft.id)
    : `new-${copyFrom?.id ?? "custom"}`;
  const activeEditor = useRef({ key: editorKey, open: editorOpen });
  activeEditor.current = { key: editorKey, open: editorOpen };
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
  function returnFocus(closingEditorKey: string) {
    // Replaced dialogs finish focus cleanup after the new draft has opened.
    if (
      activeEditor.current.open ||
      activeEditor.current.key !== closingEditorKey
    )
      return;
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
    discoveryRequest.current = null;
    setDiscovering(false);
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
          ...(preset
            ? { presetId: preset.id, region: nextRegion }
            : { icon: randomBuiltinIcon() }),
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
  async function cacheSaved(saved: RoutingUpstream, originalId = saved.id) {
    await client.cancelQueries({ queryKey: ["routing-upstreams"] });
    client.setQueryData<RoutingUpstream[]>(["routing-upstreams"], (current) => {
      const rows = current ?? [];
      const previous = rows.find((row) => row.id === saved.id);
      if (previous && previous.revision > saved.revision) return current;
      const replaced = rows.findIndex(
        (row) => row.id === originalId || row.id === saved.id,
      );
      return replaced >= 0
        ? rows.flatMap((row, index) =>
            index === replaced
              ? [saved]
              : row.id === originalId || row.id === saved.id
                ? []
                : [row],
          )
        : [...rows, saved];
    });
    return saved;
  }
  async function saveRecord(
    ...args: Parameters<typeof routingUpstreamsApi.save>
  ) {
    const saved = await routingUpstreamsApi.save(...args);
    return cacheSaved(saved, args[2]);
  }
  function refresh() {
    void Promise.all(
      [
        "routing-upstreams",
        "intent-routing-providers",
        "intent-routing-groups",
        "intent-routing-client",
        "gateway-catalog",
        "agent-connections",
        "agent-model-catalog",
      ].map((key) => client.invalidateQueries({ queryKey: [key] })),
    ).catch((cause) => report("loadProviders", cause));
  }
  async function save(event: React.FormEvent, stagedModels?: unknown[]) {
    event.preventDefault();
    if (!draft) return;
    if (draft.source.kind === "codex") {
      setBusy(true);
      try {
        if (draft.codex)
          await cacheSaved(
            await routingUpstreamsApi.saveCodex(
              { ...draft, name: draft.name.trim() },
              draft.codex.settings,
            ),
          );
        else await saveRecord({ ...draft, name: draft.name.trim() });
        setEditorOpen(false);
        refresh();
        if (adding) closeAddPage();
      } catch (cause) {
        report("save", cause);
      } finally {
        setBusy(false);
      }
      return;
    }
    if (draft.source.kind === "subscription") {
      setBusy(true);
      try {
        await saveRecord({ ...draft, name: draft.name.trim() });
        setEditorOpen(false);
        refresh();
        if (adding) closeAddPage();
      } catch (cause) {
        report("save", cause);
      } finally {
        setBusy(false);
      }
      return;
    }
    const validKey = (value: string) =>
      !!value.trim() &&
      new TextEncoder().encode(value).length <= 16384 &&
      !/[\x00-\x1f\x7f-\x9f]/.test(value);
    if (
      (apiKey ? !validKey(apiKey) : !draft.hasApiKey) ||
      (draft.transport?.keys.length ?? 0) > 64 ||
      draft.transport?.keys.some((key) =>
        key.key !== undefined
          ? !validKey(key.key)
          : !loadedRecord?.transport?.keys.some((saved) => saved.id === key.id),
      )
    ) {
      report("save", "invalid");
      return;
    }
    let raw: unknown;
    try {
      raw = stagedModels ?? JSON.parse(models);
    } catch {
      report("save", "invalidModels");
      return;
    }
    if (!Array.isArray(raw)) {
      report("save", "invalidModels");
      return;
    }
    const parsed: unknown[] = raw;
    setBusy(true);
    try {
      const { apiKey: _storedKey, ...source } = draft.source;
      const value = {
        ...draft,
        id:
          !draft.revision || draft.id === loadedRecord?.id
            ? draft.id
            : providerIdSlug(draft.id) || loadedRecord?.id || draft.id,
        name: draft.name.trim(),
        source: {
          ...source,
          baseUrl: source.baseUrl.trim(),
          models: parsed,
          mode: !draft.revision
            ? parsed.some((row) => !isImageGenerationModel(row))
              ? "picked"
              : "vendor"
            : stagedModels?.some((row) => !isImageGenerationModel(row))
              ? "picked"
              : source.mode,
          catalog: source.catalog,
          ...(apiKey ? { apiKey } : {}),
        },
      };
      if (copyFrom) await saveRecord(value, copyFrom);
      else if (loadedRecord && value.id !== loadedRecord.id) {
        const saved = await saveRecord(value, undefined, loadedRecord.id);
        returnToProvider.current = saved.id;
        const nextOrder = providerOrder.map((id) =>
          id === loadedRecord.id ? saved.id : id,
        );
        setProviderOrder(nextOrder);
        try {
          localStorage.setItem(orderStorageKey, JSON.stringify(nextOrder));
        } catch {
          /* Sorting remains available for this session. */
        }
      } else await saveRecord(value);
      setEditorOpen(false);
      refresh();
      if (adding) closeAddPage();
    } catch (cause) {
      report("save", cause);
    } finally {
      setBusy(false);
    }
  }
  function modelRow(model: FetchedModel) {
    return {
      model: model.id,
      ...(model.displayName ? { displayName: model.displayName } : {}),
      ...(model.kind ? { kind: model.kind } : {}),
      ...(model.outputModalities
        ? { outputModalities: model.outputModalities }
        : {}),
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
    const originId = copyFrom?.id ?? loadedRecord?.id ?? draft.id;
    const saved =
      loadedRecord?.id === originId
        ? loadedRecord
        : records.data?.find((record) => record.id === originId);
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
  function stageDuplicate(saved: RoutingUpstream) {
    if (busy || draft || selected || saved.source.kind !== "api") return;
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
    if (!draft || draft.source.kind !== "api" || discoveryRequest.current)
      return [];
    const source = draft.source;
    const request = {};
    discoveryRequest.current = request;
    setDiscovering(true);
    const isCurrent = () => {
      const latest = latestEditorInput.current;
      return (
        discoveryRequest.current === request &&
        activeEditor.current.open &&
        latest.apiKey === apiKey &&
        latest.draft?.source.kind === "api" &&
        latest.draft.source.baseUrl === source.baseUrl &&
        latest.draft.source.apiFormat === source.apiFormat &&
        latest.draft.source.authType === source.authType
      );
    };
    try {
      const found = await fetchDraftModels();
      if (!isCurrent()) return [];
      setDiscovered(found);
      setDraft((current) =>
        current?.source.kind === "api"
          ? {
              ...current,
              source: { ...current.source, catalog: found.map(modelRow) },
            }
          : current,
      );
      return found;
    } catch (cause) {
      if (isCurrent()) report("fetchModels", cause);
      return [];
    } finally {
      if (discoveryRequest.current === request) {
        discoveryRequest.current = null;
        setDiscovering(false);
      }
    }
  }
  async function toggle(record: RoutingUpstream, enabled: boolean) {
    setBusy(true);
    try {
      await saveRecord({ ...record, enabled });
      refresh();
    } catch (cause) {
      report("toggle", cause);
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
    const saved = records.data?.find(
      (record) => record.id === (loadedRecord?.id ?? draft.id),
    );
    if (!saved || saved.revision !== draft.revision) {
      report("toggle", "routingUpstreams.conflict");
      return;
    }
    setBusy(true);
    try {
      const result = await saveRecord({ ...saved, enabled });
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
      refresh();
    } catch (cause) {
      report("toggle", cause);
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
  function searchableModels(record: RoutingUpstream) {
    if (record.source.kind === "codex") return [];
    return record.source.mode === "picked" || !record.source.catalog.length
      ? record.source.models
      : record.source.catalog;
  }
  function modelSearchText(row: unknown) {
    if (!row || typeof row !== "object") return modelId(row);
    const model = row as Record<string, unknown>;
    return [
      modelId(row),
      model.displayName,
      (model.modelPreferences as { name?: string } | undefined)?.name,
    ]
      .filter(Boolean)
      .join(" ");
  }
  const filtered =
    ordered.filter((record) => {
      const text = [
        record.name,
        endpointHost(record),
        searchableModels(record).map(modelSearchText).join(" "),
        record.source.kind === "subscription"
          ? subscriptionProviders.find(
              (p) =>
                record.source.kind === "subscription" &&
                p.id === record.source.provider,
            )?.aliases
          : record.source.kind === "codex"
            ? "ChatGPT Codex"
            : "",
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
    if (deletingBulk || !selected?.size || !records.data) return;
    setDeletingBulk(true);
    const removed = new Set<string>();
    try {
      for (const record of records.data.filter((record) =>
        selected.has(record.id),
      )) {
        await routingUpstreamsApi.remove(record.id, record.revision);
        removed.add(record.id);
        await client.cancelQueries({ queryKey: ["routing-upstreams"] });
        client.setQueryData<RoutingUpstream[]>(
          ["routing-upstreams"],
          (current) =>
            current?.filter(
              (row) => row.id !== record.id || row.revision > record.revision,
            ),
        );
      }
    } catch (cause) {
      report("remove", cause);
    } finally {
      setConfirmBulk(false);
      setSelected((previous) => {
        const remaining = new Set(
          [...(previous ?? [])].filter((id) => !removed.has(id)),
        );
        return remaining.size ? remaining : null;
      });
      setDeletingBulk(false);
      refresh();
    }
  }
  async function openRecord(record: RoutingUpstream, reauth = false) {
    if (busy || draft) return;
    if (record.source.kind !== "codex") {
      edit(record, undefined, reauth);
      return;
    }
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
      report("load", cause);
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
    const matchingModels = searchableModels(record).filter((row) =>
      terms.some((term) =>
        modelSearchText(row).toLocaleLowerCase().includes(term),
      ),
    );
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
            <ProviderListQuota record={record} />
            {record.source.kind === "api" && (
              <span className="pv-sub">
                {`${endpointHost(record) || t("providerFlow.custom")} · ${t("modelProviders.modelCount", { count: searchableModels(record).length })}`}
              </span>
            )}
            {!!matchingModels.length &&
              !terms.every((term) =>
                `${record.name} ${endpointHost(record)}`
                  .toLocaleLowerCase()
                  .includes(term),
              ) && (
                <span className="pv-sub pv-search-match">
                  {t("modelProviders.matchedModels")}:{" "}
                  {matchingModels.slice(0, 3).map(modelId).join(", ")}
                </span>
              )}
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
        <button
          type="button"
          className="pv-row-copy"
          aria-label={`${t("providerEditor.duplicate")} ${record.name}`}
          title={`${t("providerEditor.duplicate")} ${record.name}`}
          aria-haspopup="dialog"
          disabled={
            busy || !!draft || !!selected || record.source.kind !== "api"
          }
          onClick={(event) => {
            event.stopPropagation();
            stageDuplicate(record);
          }}
        >
          <Copy size={14} aria-hidden="true" />
        </button>
        <button
          type="button"
          className="pv-row-delete"
          aria-label={`${t("common.delete")} ${record.name}`}
          title={`${t("common.delete")} ${record.name}`}
          aria-haspopup="dialog"
          disabled={busy || !!draft || !!selected}
          onClick={(event) => {
            event.stopPropagation();
            setDeleting(record);
          }}
        >
          <Trash2 size={14} aria-hidden="true" />
        </button>
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
          <div className="pv-page-copy">
            <h1 className="mp-page-heading">{t("modelProviders.title")}</h1>
            <p className="mp-lead">{t("modelProviders.description")}</p>
          </div>
          <div className="pv-page-actions">
            <CopyAgentSetupButton
              labelKey="onboarding.copyForAgent"
              className="pv-import-trigger"
              size="sm"
              onCopied={() => toast.dismiss("provider-error-copy")}
              onCopyFailed={() => report("copy")}
            />
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
          </div>
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
            aria-describedby="provider-search-scope"
            title={t("modelProviders.searchScope")}
            placeholder={t("modelProviders.searchPlaceholder")}
            value={query}
            disabled={busy || !!draft}
            onChange={(event) => setQuery(event.target.value)}
          />
          <span id="provider-search-scope" className="pv-search-scope">
            {t("modelProviders.searchScope")}
          </span>
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
      {!adding &&
        (records.isPending ? (
          <ProviderListSkeleton />
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
            onClose={closeAddPage}
            onImport={() => setImporting(true)}
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
                const confirmed = created.codex.accounts.find(
                  (row) =>
                    row.id === accountId || row.aliases?.includes(accountId),
                );
                const sameAccount = new Set([
                  accountId,
                  ...(confirmed
                    ? [confirmed.id, ...(confirmed.aliases ?? [])]
                    : []),
                ]);
                created.codex.settings.accounts = [
                  ...new Set([
                    ...(existing ? created.codex.settings.accounts : []).map(
                      (id) => (sameAccount.has(id) ? accountId : id),
                    ),
                    accountId,
                  ]),
                ];
                edit(created);
                setAdding(false);
              } catch (cause) {
                report("create", cause);
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
        onImported={refresh}
      />
      {draft && (
        <ProviderEditorDialog
          key={editorKey}
          open={editorOpen}
          draft={draft}
          originalId={loadedRecord?.id ?? draft.id}
          copySourceName={copyFrom ? loadedRecord?.name : undefined}
          another={
            !!copyFrom ||
            (!!draft.presetId &&
              !!records.data?.some(
                (record) => record.presetId === draft.presetId,
              ))
          }
          models={models}
          discovered={discovered}
          apiKey={apiKey}
          busy={busy}
          discovering={discovering}
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
          onReturnFocus={() => returnFocus(editorKey)}
          onError={report}
          nestedOpen={discard || !!deleting}
          onProbingChange={setProbing}
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
        onCancel={() => setDiscard(false)}
        onConfirm={() => {
          setDiscard(false);
          setEditorOpen(false);
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
        pending={deletingBulk}
        onCancel={() => setConfirmBulk(false)}
        onConfirm={() => void removeSelected()}
      />
      <ConfirmDialog
        className="mp-dialog pv-confirm"
        isOpen={!!deleting}
        title={t("modelProviders.delete")}
        message={
          deleting
            ? `${deleting.name} · ${t("modelProviders.deleteHint")}`
            : t("modelProviders.deleteHint")
        }
        pending={busy}
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          if (!deleting) return;
          setBusy(true);
          void routingUpstreamsApi
            .remove(deleting.id, deleting.revision)
            .then(async () => {
              await client.cancelQueries({ queryKey: ["routing-upstreams"] });
              client.setQueryData<RoutingUpstream[]>(
                ["routing-upstreams"],
                (current) =>
                  current?.filter(
                    (row) =>
                      row.id !== deleting.id ||
                      row.revision > deleting.revision,
                  ),
              );
              if (draft?.id === deleting.id) {
                setEditorOpen(false);
              }
              refresh();
            })
            .catch((cause) => report("remove", cause))
            .finally(() => {
              setDeleting(null);
              setBusy(false);
            });
        }}
      />
    </section>
  );
}
