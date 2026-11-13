import {
  Search,
  Info,
  ChevronDown,
  Download,
  Settings,
  Pencil,
  Trash2,
  Ellipsis,
} from "lucide-react";
import {
  blankRoutingGroup,
  importRoutingGroup,
  importSharedRoutingGroup,
  shareRoutingGroup,
  type RoutingImportResult,
  routingTemplates,
  routingLevels,
  validRoutingPattern,
  validRoutingRule,
} from "@/lib/routingGroupTransfer";
import { useRoutingDialogHeight } from "@/components/routing/useRoutingDialogHeight";
import { RoutingRefreshIcon } from "./RoutingRefreshIcon";
import { RoutingMemberPicker } from "@/components/routing/RoutingMemberPicker";
import {
  catalogRoutingMembers,
  routingMemberAvailable,
  selectRoutingMembers,
} from "@/lib/routingMemberSelection";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";
import { AccountLabel, AccountMask } from "@/components/AccountLabel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { RoutingGroupIcon } from "@/components/routing/RoutingGroupIcon";
import { RoutingEmptyIllustration } from "@/components/routing/RoutingEmptyIllustration";
import { RoutingRulesEditor } from "@/components/routing/RoutingRulesEditor";
import { RoutingSegments } from "@/components/routing/RoutingSegments";
import { RoutingExecutionSettings } from "@/components/routing/RoutingExecutionSettings";
import { copyText } from "@/lib/clipboard";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import { globalProviderPresets } from "@/config/globalProviderPresets";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  DropdownMenuItem,
} from "@/components/ui/dropdown-menu";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { AppDialog } from "@/components/ui/app-dialog";
import {
  useWorkspaceEditing,
  type EditingState,
} from "@/hooks/useWorkspaceEditing";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "@/components/ui/select";
import {
  intentRoutingApi,
  type ClassificationTrial,
  type RoutingCatalog,
  type RoutingGroup,
  type RoutingPlanPreview,
  type RoutingProviderOption,
} from "@/lib/api/intentRouting";

import "./routing.css";
import "./routing-group-form.css";

const knownErrors = new Set([
  "invalid",
  "conflict",
  "limit",
  "providerUnavailable",
  "notReady",
  "invalidOutput",
]);

const routingOptions = [
  ["smart", "routingSmart"],
  ["ordered", "routingOrdered"],
  ["round_robin", "routingRoundRobin"],
  ["least_used", "routingLeastUsed"],
  ["pace", "routingPace"],
  ["free_first", "routingFreeFirst"],
  ["fastest", "routingFastest"],
  ["weighted", "routingWeighted"],
  ["manual", "routingManual"],
] as const;
const sinkable = (routing: RoutingGroup["routing"]) =>
  !["round_robin", "weighted", "manual"].includes(routing ?? "ordered");

export function IntentRoutingPanel({
  onEditingChange,
  createRequest = 0,
  maskAccounts = false,
  showCreateButton = true,
}: {
  onEditingChange?: (state: EditingState) => void;
  createRequest?: number;
  maskAccounts?: boolean;
  showCreateButton?: boolean;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const groups = useQuery({
    queryKey: ["intent-routing-groups"],
    queryFn: intentRoutingApi.list,
  });
  const providers = useQuery({
    queryKey: ["intent-routing-providers"],
    queryFn: intentRoutingApi.providers,
  });
  const discovery = useQuery({
    queryKey: ["routing-catalog-discovery"],
    queryFn: intentRoutingApi.discoverCatalogs,
    enabled: groups.data?.autoCreate === true,
    staleTime: 300000,
    retry: false,
  });
  useEffect(() => {
    if (!discovery.dataUpdatedAt) return;
    void Promise.all(
      [
        "intent-routing-groups",
        "intent-routing-providers",
        "agent-model-catalog",
      ].map((key) => client.invalidateQueries({ queryKey: [key] })),
    );
  }, [discovery.dataUpdatedAt, client]);
  const templates = useMemo(
    () => routingTemplates(providers.data ?? []),
    [providers.data],
  );
  const [filter, setFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState("all");
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [importError, setImportError] = useState(false);
  const [imported, setImported] = useState<RoutingImportResult | null>(null);
  const [groupMenu, setGroupMenu] = useState<string | null>(null);
  const groupDrag = useRoutingRowDrag("data-routing-group", (from, to) => {
    void arrangeGroup(from, to);
  });
  const [selectedGroups, setSelectedGroups] = useState<Set<string> | null>(
    null,
  );
  const [confirmBulk, setConfirmBulk] = useState(false);
  const [pickingClassifier, setPickingClassifier] = useState(false);
  const [patternInput, setPatternInput] = useState("");
  const [draft, setDraft] = useState<RoutingGroup | null>(null);
  const patternsChanged =
    !!draft &&
    (JSON.stringify(draft.match) !==
      JSON.stringify(initialDraftPatterns(draft.id)) ||
      JSON.stringify(
        draft.members.map((member) => [member.providerId, member.modelId]),
      ) !==
        JSON.stringify(
          groups.data?.groups
            .find((group) => group.id === draft.id)
            ?.members.map((member) => [member.providerId, member.modelId]),
        ));
  function initialDraftPatterns(id: string) {
    return groups.data?.groups.find((group) => group.id === id)?.match;
  }
  const matchesKey = draft
    ? [draft.id, draft.match, draft.members, draft.name]
    : [];
  const matches = useQuery({
    queryKey: ["routing-draft-matches", ...matchesKey],
    queryFn: () => intentRoutingApi.matchedMembers(draft!),
    enabled: !!draft?.match?.length && !!draft.name.trim() && patternsChanged,
    retry: false,
    staleTime: 1000,
  });
  const matchedMembers = draft
    ? patternsChanged
      ? (matches.data ?? [])
      : (groups.data?.matchedMembers?.[draft.id] ?? [])
    : [];
  const [nextDraft, setNextDraft] = useState<RoutingGroup | null>(null);
  const importFormId = useId();
  const importInput = useRef<HTMLTextAreaElement>(null);
  const importButton = useRef<HTMLButtonElement>(null);
  const editorId = useId();
  const editorRef = useRoutingDialogHeight();
  const initialDraft = useRef<RoutingGroup | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const editButtons = useRef(new Map<string, HTMLButtonElement>());
  const [deleting, setDeleting] = useState<RoutingGroup | null>(null);
  const tableScroll = useRef<HTMLDivElement>(null);
  const [tableScrollEdges, setTableScrollEdges] = useState({
    left: false,
    right: false,
  });
  useEffect(() => {
    const viewport = tableScroll.current;
    if (!viewport) return;
    const update = () => {
      const next = {
        left: viewport.scrollLeft > 1,
        right:
          viewport.scrollWidth - viewport.clientWidth - viewport.scrollLeft > 1,
      };
      setTableScrollEdges((previous) =>
        previous.left === next.left && previous.right === next.right
          ? previous
          : next,
      );
    };
    update();
    viewport.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    const observer =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(viewport);
    if (viewport.firstElementChild)
      observer?.observe(viewport.firstElementChild);
    return () => {
      viewport.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      observer?.disconnect();
    };
  }, [groups.data]);
  const [pending, setPending] = useState(false);
  const [pendingGroups, setPendingGroups] = useState<Set<string>>(new Set());
  const activeGroups = useRef(new Set<string>());
  const catalogMutations = useRef<Promise<void> | null>(null);
  const [reordering, setReordering] = useState(false);
  const reorderRequest = useRef(false);
  const [refreshing, setRefreshing] = useState(false);
  const reloadRequest = useRef<object | null>(null);
  useEffect(
    () => () => {
      reloadRequest.current = null;
    },
    [],
  );
  const [deferredFocus, setDeferredFocus] = useState<string | null>(null);
  useEffect(() => {
    if (!pending && deferredFocus && !pendingGroups.has(deferredFocus)) {
      editButtons.current.get(deferredFocus)?.focus({ preventScroll: true });
      setDeferredFocus(null);
    }
  }, [pending, pendingGroups, deferredFocus]);
  const draftDirty =
    !!draft &&
    (JSON.stringify(draft) !== JSON.stringify(initialDraft.current) ||
      (!groups.data?.groups.some((group) => group.id === draft.id) &&
        (!!draft.name.trim() || draft.members.length > 0)) ||
      !!patternInput.trim());
  const handledCreate = useRef(0);
  useEffect(() => {
    if (
      createRequest > handledCreate.current &&
      groups.data &&
      providers.data &&
      !pending &&
      !deleting &&
      !groups.isError &&
      !providers.isError
    ) {
      handledCreate.current = createRequest;
      if (
        groups.data.groups.filter((group) => !group.autoGenerated).length >= 32
      ) {
        setError("intentRouting.limit");
      } else {
        requestEditor(newRoutingGroup());
      }
    }
  }, [createRequest, groups.data, providers.data, pending, deleting]);
  useWorkspaceEditing(
    draftDirty || (importOpen && !!importText.trim()),
    pending,
    onEditingChange,
  );
  const [error, setError] = useState<string | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  const [classification, setClassification] = useState('{"intentId":null}');
  const [preview, setPreview] = useState<RoutingPlanPreview | null>(null);
  const [previewPending, setPreviewPending] = useState(false);
  const [trialText, setTrialText] = useState("");
  const [trialConsent, setTrialConsent] = useState(false);
  const [trialResult, setTrialResult] = useState<ClassificationTrial | null>(
    null,
  );
  const [trialPending, setTrialPending] = useState(false);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );

  function invalidatePreview() {
    generation.current += 1;
    setPreview(null);
    setTrialResult(null);
    setTrialPending(false);
    setPreviewPending(false);
    setError(null);
  }
  function isGroupPending(id?: string) {
    return !!id && pendingGroups.has(id);
  }
  async function groupOperation(id: string, action: () => Promise<void>) {
    if (pending || activeGroups.current.has(id)) return;
    activeGroups.current.add(id);
    setPendingGroups(new Set(activeGroups.current));
    setError(null);
    try {
      await action();
    } catch (cause) {
      report(cause);
    } finally {
      activeGroups.current.delete(id);
      setPendingGroups(new Set(activeGroups.current));
    }
  }
  function mutateCatalog<T>(action: () => Promise<T>): Promise<T> {
    // Catalog membership/order writes require the complete current group list.
    const next = catalogMutations.current
      ? catalogMutations.current.then(action, action)
      : action();
    const settled = next.then(
      () => undefined,
      () => undefined,
    );
    catalogMutations.current = settled;
    void settled.then(() => {
      if (catalogMutations.current === settled) catalogMutations.current = null;
    });
    return next;
  }
  function currentGroups() {
    return (
      client.getQueryData<RoutingCatalog>(["intent-routing-groups"])?.groups ??
      groups.data?.groups ??
      []
    );
  }
  async function saveGroup(value: RoutingGroup) {
    const saved = await intentRoutingApi.save(value);
    client.setQueryData<RoutingCatalog>(
      ["intent-routing-groups"],
      (current) => {
        if (!current) return current;
        const previous = current.groups.find((group) => group.id === saved.id);
        if (previous && previous.revision > saved.revision) return current;
        return {
          ...current,
          groups: previous
            ? current.groups.map((group) =>
                group.id === saved.id ? saved : group,
              )
            : [...current.groups, saved],
        };
      },
    );
    return saved;
  }
  async function removeGroup(id: string, revision: number) {
    await intentRoutingApi.remove(id, revision);
    client.setQueryData<RoutingCatalog>(["intent-routing-groups"], (current) =>
      current
        ? {
            ...current,
            groups: current.groups.filter(
              (group) => group.id !== id || group.revision > revision,
            ),
          }
        : current,
    );
  }
  function edit(value: RoutingGroup | null) {
    invalidatePreview();
    setTrialConsent(false);
    setDraft(value);
  }
  function openEditor(value: RoutingGroup) {
    initialDraft.current = structuredClone(value);
    setImportOpen(false);
    setImportText("");
    setPickingClassifier(false);
    setPatternInput("");
    edit(structuredClone(value));
  }
  function newRoutingGroup() {
    return selectRoutingMembers(
      blankRoutingGroup(),
      catalogRoutingMembers(providers.data ?? []),
      true,
    );
  }
  function requestEditor(value: RoutingGroup) {
    if (draftDirty || (importOpen && !!importText.trim())) {
      setNextDraft(value);
    } else {
      openEditor(value);
    }
  }
  function useTemplate(template: ReturnType<typeof routingTemplates>[number]) {
    if (template.need || pending || deleting) return;
    const name = t(`routingPage.${template.key}`).slice(0, 80);
    const names = new Set(
      groups.data?.groups.map((entry) => entry.name.toLocaleLowerCase()),
    );
    let draftName = name;
    let number = 1;
    while (names.has(draftName.toLocaleLowerCase())) {
      const suffix = ` ${++number}`;
      draftName = `${name.slice(0, 80 - suffix.length)}${suffix}`;
    }
    requestEditor({
      ...template.group,
      id: crypto.randomUUID(),
      name: draftName,
    });
  }

  function templateNeed(template: ReturnType<typeof routingTemplates>[number]) {
    return template.need
      ? t(`routingPage.templateNeed_${template.need}`, { name: template.of })
      : "";
  }
  async function removeSelected() {
    if (
      pending ||
      !selectedGroups?.size ||
      !groups.data ||
      [...selectedGroups].some((id) => activeGroups.current.has(id))
    )
      return;
    setPending(true);
    setError(null);
    let removedAll = false;
    try {
      await mutateCatalog(async () => {
        for (const group of currentGroups().filter((group) =>
          selectedGroups.has(group.id),
        )) {
          await removeGroup(group.id, group.revision);
          setSelectedGroups((previous) => {
            const next = new Set(previous);
            next.delete(group.id);
            return next;
          });
        }
      });
      removedAll = true;
    } catch (cause) {
      report(cause);
    } finally {
      try {
        await refreshRoutingStatus();
      } catch (cause) {
        report(cause);
      }
      if (removedAll) {
        setSelectedGroups(null);
        setConfirmBulk(false);
      }
      setPending(false);
    }
  }
  async function shareGroup(group: RoutingGroup) {
    await groupOperation(group.id, async () => {
      await copyText(shareRoutingGroup(group));
      toast.success(t("routingPage.copied"));
    });
  }
  function exportGroup(group: RoutingGroup) {
    setError(null);
    setExported(null);
    let url: string | undefined;
    try {
      const blob = new Blob([shareRoutingGroup(group)], {
        type: "application/json",
      });
      url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `vary-routing-group-${group.id}.json`;
      anchor.click();
      setExported(group.name);
    } catch {
      report(null);
    } finally {
      if (url) {
        const downloadUrl = url;
        setTimeout(() => URL.revokeObjectURL(downloadUrl), 1000);
      }
    }
  }
  function closeImport() {
    if (pending) return;
    setImportOpen(false);
    setImportText("");
    setImported(null);
    setImportError(false);
    setError(null);
  }
  async function importGroup() {
    if (pending || !providers.data) return;
    setImportError(false);
    setError(null);
    let result: RoutingImportResult;
    try {
      result = importSharedRoutingGroup(importText, providers.data);
    } catch {
      setImportError(true);
      return;
    }
    setPending(true);
    try {
      await mutateCatalog(async () => {
        const names = new Set(
          currentGroups().map((group) => group.name.toLocaleLowerCase()),
        );
        const base = result.group.name;
        let number = 1;
        while (names.has(result.group.name.toLocaleLowerCase())) {
          const suffix = ` ${++number}`;
          result.group.name = `${base.slice(0, 80 - suffix.length)}${suffix}`;
        }
        result.group = await saveGroup(result.group);
        setImported(result);
        setImportText("");
        await refreshRoutingStatus();
      });
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function moveGroup(group: RoutingGroup, offset: number) {
    if (pending || draft || reorderRequest.current) return;
    const all = currentGroups();
    const visible = all.filter((candidate) =>
      visibleGroups?.some((entry) => entry.id === candidate.id),
    );
    const next =
      visible[visible.findIndex((entry) => entry.id === group.id) + offset];
    if (!next) return;
    await arrangeGroup(group.id, next.id);
  }
  async function arrangeGroup(id: string, target: string) {
    if (pending || draft || reorderRequest.current) return;
    reorderRequest.current = true;
    setReordering(true);
    setError(null);
    try {
      await mutateCatalog(async () => {
        const order = currentGroups().map((entry) => entry.id);
        const from = order.indexOf(id);
        const to = order.indexOf(target);
        if (from < 0 || to < 0 || from === to) return;
        order.splice(to, 0, ...order.splice(from, 1));
        await intentRoutingApi.arrange(order);
        await refreshRoutingStatus();
      });
    } catch (cause) {
      report(cause);
    } finally {
      reorderRequest.current = false;
      setReordering(false);
    }
  }
  async function duplicateGroup(group: RoutingGroup) {
    const availableProviders = providers.data;
    if (draft || !availableProviders) return;
    await groupOperation(group.id, () =>
      mutateCatalog(async () => {
        try {
          // A local copy retains unavailable seats and machine-local rules.
          const duplicate = importRoutingGroup(
            JSON.stringify(group),
            [
              ...availableProviders,
              ...[
                ...new Set(
                  [
                    ...group.members,
                    ...(group.classifierTarget ? [group.classifierTarget] : []),
                  ].map((member) => member.providerId),
                ),
              ]
                .filter(
                  (id) =>
                    !availableProviders.some((provider) => provider.id === id),
                )
                .map(
                  (id): RoutingProviderOption => ({
                    id,
                    name: id,
                    taskEligible: false,
                    classifierEligible: false,
                  }),
                ),
            ].map((provider) => ({
              ...provider,
              models: [
                ...(provider.models ?? []),
                ...[
                  ...group.members,
                  ...(group.classifierTarget ? [group.classifierTarget] : []),
                ]
                  .filter((member) => member.providerId === provider.id)
                  .map((member) => ({
                    id: member.modelId,
                    name: member.modelId,
                  })),
              ],
            })),
          );
          duplicate.enabled = true;
          duplicate.name = t("routingPage.copyName", {
            name: group.name,
          }).slice(0, 80);
          const base = duplicate.name;
          const names = new Set(
            currentGroups().map((entry) => entry.name.toLocaleLowerCase()),
          );
          let number = 1;
          while (names.has(duplicate.name.toLocaleLowerCase())) {
            const suffix = ` ${++number}`;
            duplicate.name = `${base.slice(0, 80 - suffix.length)}${suffix}`;
          }
          const saved = await saveGroup(duplicate);
          const order = currentGroups()
            .map((entry) => entry.id)
            .filter((id) => id !== saved.id);
          const sourceIndex = order.indexOf(group.id);
          order.splice(
            sourceIndex < 0 ? order.length : sourceIndex + 1,
            0,
            saved.id,
          );
          await intentRoutingApi.arrange(order);
          await refreshRoutingStatus();
        } catch (cause) {
          report(cause);
          await refreshRoutingStatus();
        }
      }),
    );
  }
  async function publishGroup(group: RoutingGroup, enabled: boolean) {
    await groupOperation(group.id, async () => {
      await saveGroup({ ...group, enabled });
      await refreshRoutingStatus();
    });
  }
  function report(cause: unknown) {
    const key =
      typeof cause === "string" ? cause.replace(/^intentRouting\./, "") : "";
    setError(
      knownErrors.has(key) ? `intentRouting.${key}` : "intentRouting.failed",
    );
  }
  async function refreshRoutingStatus() {
    await Promise.all([
      client.invalidateQueries({ queryKey: ["intent-routing-groups"] }),
      client.invalidateQueries({ queryKey: ["intent-routing-client"] }),
      client.invalidateQueries({ queryKey: ["intent-routing-providers"] }),
      client.invalidateQueries({ queryKey: ["gateway-catalog"] }),
      client.invalidateQueries({ queryKey: ["agent-model-catalog"] }),
    ]);
  }
  function addPattern(): boolean {
    if (!draft) return false;
    const pattern = patternInput.trim();
    if (!pattern) return true;
    if (!validRoutingPattern(pattern) || (draft.match?.length ?? 0) >= 32) {
      setError("intentRouting.invalidPattern");
      return false;
    }
    edit({ ...draft, match: [...new Set([...(draft.match ?? []), pattern])] });
    setPatternInput("");
    return true;
  }
  async function save() {
    if (!draft || pending || !valid) return;
    const pattern = patternInput.trim();
    if (
      pattern &&
      (!validRoutingPattern(pattern) || (draft.match?.length ?? 0) >= 32)
    ) {
      setError("intentRouting.invalidPattern");
      return;
    }
    if (activeGroups.current.has(draft.id)) return;
    setPending(true);
    setError(null);
    const submit = async () => {
      try {
        await saveGroup({
          ...draft,
          name: draft.name.trim(),
          sink: !!draft.sink && sinkable(draft.routing),
          ...(pattern
            ? { match: [...new Set([...(draft.match ?? []), pattern])] }
            : {}),
        });
      } catch (cause) {
        setPending(false);
        throw cause;
      }
      setPatternInput("");
      setPending(false);
      edit(null);
      await refreshRoutingStatus();
    };
    await groupOperation(draft.id, () =>
      draft.revision === 0 ? mutateCatalog(submit) : submit(),
    );
  }

  async function changeRouting(
    group: RoutingGroup,
    routing: NonNullable<RoutingGroup["routing"]>,
  ) {
    await groupOperation(group.id, async () => {
      await saveGroup({
        ...group,
        routing,
        sink: !!group.sink && sinkable(routing),
        ...(routing === "manual"
          ? { manualMemberId: group.manualMemberId ?? group.defaultMemberId }
          : {}),
      });
      await refreshRoutingStatus();
    });
  }
  function routingSelect(
    group: RoutingGroup,
    update: (routing: NonNullable<RoutingGroup["routing"]>) => void,
  ) {
    return (
      <Select
        value={group.routing ?? "ordered"}
        disabled={
          pending || isGroupPending(group.id) || (!!draft && group !== draft)
        }
        onValueChange={(value) =>
          update(value as NonNullable<RoutingGroup["routing"]>)
        }
      >
        <SelectTrigger
          className="h-8 w-auto min-w-24 gap-2 rounded-full text-xs"
          aria-label={t("intentRouting.routingStrategy")}
          title={`${t("intentRouting.routingStrategy")}: ${t(`routingPage.strategy.${group.routing ?? "ordered"}`)}`}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {routingOptions.map(([value, key]) => (
            <SelectItem key={value} value={value}>
              {t(`intentRouting.${key}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }
  function manualSelect(group: RoutingGroup, update: (id: string) => void) {
    return (
      <Select
        value={group.manualMemberId ?? group.defaultMemberId}
        disabled={
          pending || isGroupPending(group.id) || (!!draft && group !== draft)
        }
        onValueChange={update}
      >
        <SelectTrigger
          className="h-8 w-auto max-w-64 gap-2 text-xs"
          aria-label={t("intentRouting.manualMember")}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {[
            ...group.members,
            ...(group === draft
              ? matchedMembers
              : (groups.data?.matchedMembers?.[group.id] ?? [])),
          ].map((member) => (
            <SelectItem
              key={member.id}
              value={member.id}
              disabled={
                group.off?.includes(member.id) ||
                !routingMemberAvailable(providers.data ?? [], member)
              }
            >
              <AccountLabel>
                {providers.data?.find((p) => p.id === member.providerId)
                  ?.name || member.providerId}
              </AccountLabel>{" "}
              · {member.modelId}
              {member.effort ? ` :${member.effort}` : ""}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }
  async function changeManualMember(
    group: RoutingGroup,
    manualMemberId: string,
  ) {
    await groupOperation(group.id, async () => {
      await saveGroup({ ...group, manualMemberId });
      await refreshRoutingStatus();
    });
  }
  async function restoreGroup(id: string) {
    await groupOperation(id, () =>
      mutateCatalog(async () => {
        await intentRoutingApi.restore(id);
        await refreshRoutingStatus();
      }),
    );
  }
  async function reload() {
    if (reloadRequest.current) return;
    const request = {};
    reloadRequest.current = request;
    setRefreshing(true);
    setError(null);
    try {
      await intentRoutingApi.refreshCatalogs();
    } catch (cause) {
      if (reloadRequest.current === request) report(cause);
    } finally {
      if (reloadRequest.current === request) {
        try {
          await Promise.all([
            groups.refetch(),
            providers.refetch(),
            client.invalidateQueries({ queryKey: ["intent-routing-client"] }),
          ]);
        } catch (cause) {
          if (reloadRequest.current === request) report(cause);
        } finally {
          if (reloadRequest.current === request) {
            reloadRequest.current = null;
            setRefreshing(false);
          }
        }
      }
    }
  }
  async function remove() {
    if (!deleting) return;
    const target = deleting;
    await groupOperation(target.id, () =>
      mutateCatalog(async () => {
        try {
          await removeGroup(target.id, target.revision);
          if (draft?.id === target.id) edit(null);
        } finally {
          setDeleting((current) =>
            current?.id === target.id ? null : current,
          );
        }
        await refreshRoutingStatus();
      }),
    );
  }
  async function inspectPlan() {
    if (!draft) return;
    invalidatePreview();
    const request = generation.current;
    setPreviewPending(true);
    try {
      const result = await intentRoutingApi.preview(draft, classification);
      if (request === generation.current) setPreview(result);
    } catch (cause) {
      if (request === generation.current) report(cause);
    } finally {
      if (request === generation.current) setPreviewPending(false);
    }
  }

  async function runTrial() {
    if (!draft || !trialConsent || draft.routing === "manual") return;
    invalidatePreview();
    const request = generation.current;
    setTrialPending(true);
    try {
      const result = await intentRoutingApi.trial(draft, trialText, true);
      if (request === generation.current) setTrialResult(result);
    } catch (cause) {
      if (request === generation.current) report(cause);
    } finally {
      if (request === generation.current) setTrialPending(false);
    }
  }

  const memberOptions = draft?.members.map((m) => (
    <SelectItem key={m.id} value={m.id}>
      {m.modelId || t("intentRouting.model")} ·{" "}
      <AccountLabel>
        {providers.data?.find((p) => p.id === m.providerId)?.name ||
          m.providerId}
      </AccountLabel>
      {m.effort ? ` :${m.effort}` : ""}
    </SelectItem>
  ));
  function memberSummary(group: RoutingGroup) {
    return group.members
      .map((member) => {
        const name =
          providers.data?.find((p) => p.id === member.providerId)?.name ||
          member.providerId;
        const unavailable =
          providers.data && !routingMemberAvailable(providers.data, member);
        return `${name} · ${member.modelId}${member.effort ? ` :${member.effort}` : ""}${unavailable ? ` (${t("intentRouting.memberNotReady")})` : ""}`;
      })
      .join(" · ");
  }
  const query = filter.trim().toLocaleLowerCase();
  const visibleGroups = groups.data?.groups.filter(
    (group) =>
      group.id === draft?.id ||
      ((statusFilter === "all" ||
        group.enabled === (statusFilter === "enabled")) &&
        [
          group.name,
          group.modelAlias ?? `vary-auto/${group.id}`,
          ...group.members.flatMap((member) => [
            member.modelId,
            providers.data?.find((p) => p.id === member.providerId)?.name ||
              member.providerId,
          ]),
        ].some((value) => value.toLocaleLowerCase().includes(query))),
  );
  const valid =
    draft &&
    draft.name.trim() &&
    (draft.contextWindow == null ||
      draft.contextWindow === -1 ||
      draft.contextWindow === 0 ||
      (Number.isSafeInteger(draft.contextWindow) &&
        draft.contextWindow >= 2048)) &&
    (draft.members.length > 0 ||
      (draft.match?.length ?? 0) > 0 ||
      validRoutingPattern(patternInput.trim())) &&
    (draft.match?.length ?? 0) <= 32 &&
    (draft.match ?? []).every(validRoutingPattern) &&
    Number.isInteger(draft.firstToken ?? 0) &&
    (draft.firstToken ?? 0) >= 0 &&
    (draft.firstToken ?? 0) <= 3600 &&
    Number.isInteger(draft.patience ?? -1) &&
    (draft.patience ?? -1) >= -1 &&
    (draft.patience ?? -1) <= 240 &&
    draft.members.every(
      (m) =>
        m.providerId &&
        m.modelId.trim() &&
        Number.isInteger(m.weight ?? 1) &&
        (m.weight ?? 1) >= 1 &&
        (m.weight ?? 1) <= 1000,
    ) &&
    new Set(
      draft.members.map((m) =>
        JSON.stringify([m.providerId, m.modelId, m.effort ?? null]),
      ),
    ).size === draft.members.length &&
    (draft.routing !== "manual" ||
      [...draft.members, ...matchedMembers].some(
        (m) =>
          m.id === (draft.manualMemberId ?? draft.defaultMemberId) &&
          !draft.off?.includes(m.id),
      )) &&
    draft.rules.every(validRoutingRule) &&
    (!draft.classifierTarget ||
      (draft.classifierTarget.providerId &&
        draft.classifierTarget.modelId.trim()));

  const classifierModels =
    providers.data
      ?.filter((provider) => provider.classifierEligible)
      .flatMap((provider) =>
        (provider.models ?? []).map((model) => ({
          ...model,
          id: JSON.stringify([provider.id, model.id]),
          reference: `${provider.id}/${model.id}`,
          modelId: model.id,
          name: model.name || model.id,
          group: provider.name,
          contextWindow: model.contextWindow,
          disabled: pending || draft?.routing === "manual",
        })),
      ) ?? [];
  const selectedClassifier = draft?.classifierTarget
    ? JSON.stringify([
        draft.classifierTarget.providerId,
        draft.classifierTarget.modelId,
      ])
    : "";
  const classifierName = classifierModels.find(
    (model) => model.id === selectedClassifier,
  );
  const editorActions = (
    <div className="rt-editor-bar flex gap-2">
      <span className="grow" />
      <button
        type="button"
        className="rg-text"
        disabled={pending}
        onClick={() => edit(null)}
      >
        {t("common.cancel")}
      </button>
      <button
        type="submit"
        className="rg-text rg-primary"
        form={editorId}
        disabled={pending || !valid}
      >
        {t("common.save")}
      </button>
    </div>
  );
  const editor = draft && (
    <form
      ref={editorRef}
      id={editorId}
      aria-label={initialDraft.current?.name || t("intentRouting.create")}
      className="rt-group-editor rt-group-form rt-form-theme"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {(error || groups.isError || providers.isError) && (
        <p role="alert" className="text-sm text-destructive">
          {t(error || "intentRouting.failed")}
        </p>
      )}
      <fieldset disabled={pending} className="min-w-0">
        <div className="rt-editor-fields">
          <label className="rt-edit-name" data-routing-field="name">
            <span>{t("intentRouting.name")}</span>
            <input
              type="text"
              className="rg-input"
              autoComplete="off"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              ref={nameInput}
              placeholder={t("intentRouting.namePlaceholder")}
              required
              maxLength={80}
              value={draft.name}
              onChange={(e) => edit({ ...draft, name: e.target.value })}
            />
          </label>
          <div className="rt-setting-row" data-routing-field="routing">
            <span>{t("intentRouting.routingStrategy")}</span>
            <div className="rt-field-content">
              <RoutingSegments
                label={t("intentRouting.routingStrategy")}
                value={draft.routing ?? "ordered"}
                options={routingOptions.map(([value, key]) => ({
                  value,
                  label: t(`intentRouting.${key}`),
                }))}
                disabled={pending}
                onChange={(routing) =>
                  edit({
                    ...draft,
                    routing,
                    ...(routing === "manual"
                      ? {
                          manualMemberId:
                            draft.manualMemberId ?? draft.defaultMemberId,
                        }
                      : {}),
                  })
                }
              />
              <p className="rt-field-hint">
                {t(`intentRouting.routingHint_${draft.routing ?? "ordered"}`)}
              </p>
              {draft.routing === "manual" &&
                manualSelect(draft, (manualMemberId) =>
                  edit({ ...draft, manualMemberId }),
                )}
            </div>
          </div>
          <div
            className="rt-edit-members"
            data-routing-list
            data-routing-field="members"
          >
            <h4 className="font-medium">{t("intentRouting.members")}</h4>
            <RoutingMemberPicker
              key={draft.id}
              providers={providers.data ?? []}
              members={draft.members}
              disabled={pending}
              onSelect={(choices, selected) =>
                edit(selectRoutingMembers(draft, choices, selected))
              }
            />
          </div>
          <details key={draft.id} className="rt-form-advanced">
            <summary>
              <ChevronDown
                className="rt-advanced-chevron"
                size={13}
                aria-hidden
              />
              <span>{t("intentRouting.advancedSettings")}</span>
            </summary>
            <div className="rt-advanced-fields">
              <div className="rt-edit-patterns" data-routing-field="patterns">
                <span>{t("intentRouting.matchPatterns")}</span>
                <div className="rt-patterns">
                  <div className="rt-pattern-list">
                    {(draft.match ?? []).map((pattern) => (
                      <div key={pattern} className="rt-pattern-row">
                        <code>{pattern}</code>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          disabled={pending}
                          aria-label={`${t("intentRouting.removePattern")} ${pattern}`}
                          onClick={() =>
                            edit({
                              ...draft,
                              match: draft.match?.filter(
                                (value) => value !== pattern,
                              ),
                              manualMemberId:
                                (draft.match?.length ?? 0) <= 1 &&
                                draft.manualMemberId?.startsWith("match-")
                                  ? undefined
                                  : draft.manualMemberId,
                              off:
                                (draft.match?.length ?? 0) > 1
                                  ? draft.off
                                  : draft.off?.filter((id) =>
                                      draft.members.some(
                                        (member) => member.id === id,
                                      ),
                                    ),
                            })
                          }
                        >
                          {t("common.delete")}
                        </Button>
                      </div>
                    ))}
                    <div className="rt-pattern-add">
                      <input
                        type="text"
                        className="rg-input"
                        autoComplete="off"
                        autoCapitalize="off"
                        autoCorrect="off"
                        spellCheck={false}
                        disabled={pending || (draft.match?.length ?? 0) >= 32}
                        aria-label={t("intentRouting.matchPatterns")}
                        placeholder="openrouter/*:free"
                        value={patternInput}
                        onChange={(event) =>
                          setPatternInput(event.target.value)
                        }
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            addPattern();
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="rg-text"
                        disabled={
                          pending ||
                          !patternInput.trim() ||
                          (draft.match?.length ?? 0) >= 32
                        }
                        onClick={addPattern}
                      >
                        {t("intentRouting.addPattern")}
                      </button>
                    </div>
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {t("intentRouting.matchPatternsHelp")}
                  </p>
                  {patternsChanged &&
                  draft.match?.length &&
                  (matches.isPending || matches.isError) ? (
                    <p role={matches.isError ? "alert" : "status"}>
                      {t(
                        matches.isError
                          ? "intentRouting.failed"
                          : "common.loading",
                      )}
                    </p>
                  ) : null}
                  {!!matchedMembers.length && (
                    <div
                      className="rt-pattern-matches"
                      aria-label={t("intentRouting.matchedModels")}
                    >
                      {matchedMembers.map((member) => (
                        <div key={member.id}>
                          <Switch
                            className="mp-switch"
                            disabled={pending}
                            aria-label={`${t("intentRouting.memberEnabled")} ${member.modelId}`}
                            checked={!draft.off?.includes(member.id)}
                            onCheckedChange={(enabled) =>
                              edit({
                                ...draft,
                                off: enabled
                                  ? (draft.off ?? []).filter(
                                      (id) => id !== member.id,
                                    )
                                  : [...(draft.off ?? []), member.id],
                              })
                            }
                          />
                          <code>
                            <AccountLabel>{`${member.providerId}/${member.modelId}`}</AccountLabel>
                          </code>
                          <small>{t("intentRouting.byPattern")}</small>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>
              <div className="rt-setting-row" data-routing-field="model-lists">
                <span>{t("intentRouting.modelLists")}</span>
                <div className="rt-field-content">
                  <RoutingSegments
                    label={t("intentRouting.modelLists")}
                    value={draft.unlists ? "group" : "all"}
                    disabled={pending}
                    options={[
                      { value: "all", label: t("intentRouting.listModelsToo") },
                      {
                        value: "group",
                        label: t("intentRouting.listGroupOnly"),
                      },
                    ]}
                    onChange={(value) =>
                      edit({ ...draft, unlists: value === "group" })
                    }
                  />
                  <p className="rt-field-hint">
                    {t(
                      draft.unlists
                        ? "intentRouting.unlistsHelp"
                        : "intentRouting.listModelsHelp",
                    )}
                  </p>
                </div>
              </div>
              {sinkable(draft.routing) && (
                <div
                  className="rt-setting-row"
                  data-routing-field="rate-limits"
                >
                  <span>{t("intentRouting.sink")}</span>
                  <div className="rt-field-content">
                    <RoutingSegments
                      label={t("intentRouting.sink")}
                      value={draft.sink ? "back" : "keep"}
                      disabled={pending}
                      options={[
                        { value: "keep", label: t("intentRouting.sinkKeep") },
                        { value: "back", label: t("intentRouting.sinkBack") },
                      ]}
                      onChange={(value) =>
                        edit({ ...draft, sink: value === "back" })
                      }
                    />
                    <p className="rt-field-hint">
                      {t(
                        draft.sink
                          ? "intentRouting.sinkHelp"
                          : "intentRouting.sinkKeepHelp",
                      )}
                    </p>
                  </div>
                </div>
              )}
              <div className="rt-edit-routing">
                <RoutingExecutionSettings
                  key={draft.id}
                  group={draft}
                  disabled={pending}
                  memberWindows={draft.members.flatMap((member) => {
                    const capacity = providers.data
                      ?.find((provider) => provider.id === member.providerId)
                      ?.models?.find(
                        (model) => model.id === member.modelId,
                      )?.contextWindow;
                    return capacity ? [capacity] : [];
                  })}
                  onChange={edit}
                />
                <fieldset
                  disabled={draft.routing === "manual"}
                  className="min-w-0 space-y-5 disabled:opacity-60"
                >
                  {draft.routing === "manual" && (
                    <p className="text-xs text-muted-foreground">
                      {t("intentRouting.manualPaused")}
                    </p>
                  )}
                  <div className="rt-edit-rules">
                    <h4 className="font-medium">{t("intentRouting.rules")}</h4>
                    <RoutingRulesEditor
                      rules={draft.rules}
                      members={draft.members}
                      disabled={pending || draft.routing === "manual"}
                      onChange={(rules) => edit({ ...draft, rules })}
                    />
                  </div>
                  {(draft.classifierTarget ||
                    draft.rules.some(
                      (rule) => !rule.pause && rule.description.trim(),
                    )) && (
                    <div className="rt-edit-classifier">
                      <h4>{t("routingPage.intentToldBy")}</h4>
                      <div className="rt-classifier-pick">
                        <Popover
                          open={pickingClassifier}
                          onOpenChange={setPickingClassifier}
                        >
                          <PopoverTrigger asChild>
                            <button
                              type="button"
                              className={`rt-cond${draft.classifierTarget ? " on" : ""}`}
                              disabled={pending || draft.routing === "manual"}
                              aria-label={t("routingPage.intentToldBy")}
                            >
                              <AccountLabel>
                                {classifierName
                                  ? `${classifierName.name} · ${classifierName.group}`
                                  : draft.classifierTarget
                                    ? `${draft.classifierTarget.modelId} · ${providers.data?.find((provider) => provider.id === draft.classifierTarget?.providerId)?.name ?? draft.classifierTarget.providerId}`
                                    : t("routingPage.chooseClassifier")}
                              </AccountLabel>
                            </button>
                          </PopoverTrigger>
                          <PopoverContent
                            className="mp-model-popover rt-form-theme"
                            align="start"
                          >
                            <SharedModelPicker
                              models={classifierModels}
                              selected={
                                selectedClassifier ? [selectedClassifier] : []
                              }
                              onPick={(id) => {
                                const chosen = providers.data
                                  ?.filter(
                                    (provider) => provider.classifierEligible,
                                  )
                                  .flatMap((provider) =>
                                    (provider.models ?? []).map((model) => ({
                                      id: JSON.stringify([
                                        provider.id,
                                        model.id,
                                      ]),
                                      providerId: provider.id,
                                      modelId: model.id,
                                    })),
                                  )
                                  .find((model) => model.id === id);
                                if (
                                  chosen &&
                                  !pending &&
                                  draft.routing !== "manual"
                                ) {
                                  edit({
                                    ...draft,
                                    intentRecognitionEnabled: true,
                                    classifierTarget: {
                                      providerId: chosen.providerId,
                                      modelId: chosen.modelId,
                                    },
                                  });
                                  setPickingClassifier(false);
                                }
                              }}
                            />
                            {draft.classifierTarget && (
                              <button
                                type="button"
                                className="rt-clear-classifier"
                                onClick={() => {
                                  edit({
                                    ...draft,
                                    classifierTarget: null,
                                    intentRecognitionEnabled: false,
                                  });
                                  setPickingClassifier(false);
                                }}
                              >
                                {t("intentRouting.none")}
                              </button>
                            )}
                          </PopoverContent>
                        </Popover>
                        <span
                          className="rt-info"
                          tabIndex={0}
                          role="img"
                          aria-label={t("intentRouting.classifierNotice")}
                          title={t("intentRouting.classifierNotice")}
                        >
                          <Info size={14} />
                        </span>
                      </div>
                      <details className="rt-classifier-advanced">
                        <summary>{t("routingPage.classifierAdvanced")}</summary>
                        <div>
                          <label className="block space-y-1 text-sm">
                            {t("intentRouting.classifierProvider")}
                            <Select
                              disabled={pending}
                              value={
                                draft.classifierTarget
                                  ? `provider:${draft.classifierTarget.providerId}`
                                  : "none"
                              }
                              onValueChange={(value) =>
                                edit({
                                  ...draft,
                                  intentRecognitionEnabled:
                                    value !== "none" &&
                                    !!draft.classifierTarget?.modelId.trim(),
                                  classifierTarget:
                                    value !== "none"
                                      ? {
                                          providerId: value.slice(
                                            "provider:".length,
                                          ),
                                          modelId:
                                            draft.classifierTarget?.modelId ??
                                            "",
                                        }
                                      : null,
                                })
                              }
                            >
                              <SelectTrigger
                                aria-label={t(
                                  "intentRouting.classifierProvider",
                                )}
                              >
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="none">
                                  {t("intentRouting.none")}
                                </SelectItem>
                                {draft.classifierTarget &&
                                  !providers.data?.some(
                                    (p) =>
                                      p.id ===
                                        draft.classifierTarget?.providerId &&
                                      p.classifierEligible,
                                  ) && (
                                    <SelectItem
                                      value={`provider:${draft.classifierTarget.providerId}`}
                                    >
                                      {t("intentRouting.providerUnavailable")}
                                    </SelectItem>
                                  )}
                                {providers.data
                                  ?.filter((p) => p.classifierEligible)
                                  .map((p) => (
                                    <SelectItem
                                      key={p.id}
                                      value={`provider:${p.id}`}
                                    >
                                      <AccountLabel>{p.name}</AccountLabel>
                                      {p.appType ? ` · ${p.appType}` : ""}
                                    </SelectItem>
                                  ))}
                              </SelectContent>
                            </Select>
                          </label>
                          {draft.classifierTarget && (
                            <label className="block space-y-1 text-sm">
                              {t("intentRouting.classifierModel")}
                              <Input
                                required
                                maxLength={200}
                                value={draft.classifierTarget.modelId}
                                onChange={(e) =>
                                  edit({
                                    ...draft,
                                    intentRecognitionEnabled:
                                      !!e.target.value.trim(),
                                    classifierTarget: {
                                      ...draft.classifierTarget!,
                                      modelId: e.target.value,
                                    },
                                  })
                                }
                              />
                            </label>
                          )}
                          <label className="block space-y-1 text-sm">
                            {t("intentRouting.budget")}
                            <Input
                              type="number"
                              required
                              min={500}
                              max={5000}
                              step={1}
                              value={draft.classificationBudgetMs}
                              onChange={(e) =>
                                edit({
                                  ...draft,
                                  classificationBudgetMs: Number(
                                    e.target.value,
                                  ),
                                })
                              }
                            />
                          </label>
                        </div>
                      </details>
                    </div>
                  )}
                  <details className="rt-diagnostics">
                    <summary>{t("intentRouting.trialTitle")}</summary>
                    <p className="text-xs text-muted-foreground">
                      {t("intentRouting.trialNotice")}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      <AccountLabel>
                        {providers.data?.find(
                          (p) => p.id === draft.classifierTarget?.providerId,
                        )?.name || t("intentRouting.none")}
                      </AccountLabel>
                      {draft.classifierTarget &&
                        ` / ${draft.classifierTarget.modelId}`}
                    </p>
                    <label className="block space-y-1 text-sm">
                      {t("intentRouting.trialText")}
                      <Textarea
                        maxLength={16000}
                        value={trialText}
                        onChange={(event) => {
                          invalidatePreview();
                          setTrialConsent(false);
                          setTrialText(event.target.value);
                        }}
                      />
                    </label>
                    <label className="flex items-start gap-2 text-sm">
                      <input
                        type="checkbox"
                        checked={trialConsent}
                        onChange={(event) => {
                          invalidatePreview();
                          setTrialConsent(event.target.checked);
                        }}
                      />
                      {t("intentRouting.trialConsent")}
                    </label>
                    <Button
                      type="button"
                      variant="outline"
                      disabled={
                        !valid ||
                        draft.routing === "manual" ||
                        !draft.classifierTarget ||
                        !trialText.trim() ||
                        !trialConsent ||
                        trialPending
                      }
                      onClick={() => void runTrial()}
                    >
                      {t(
                        trialPending
                          ? "common.loading"
                          : "intentRouting.runTrial",
                      )}
                    </Button>
                    {trialResult && (
                      <div role="status" className="space-y-2 text-sm">
                        <p>
                          {t(
                            `intentRouting.trialOutcome.${trialResult.outcome}`,
                          )}
                        </p>
                        {trialResult.intentId && (
                          <p>
                            {
                              draft.rules.find(
                                (r) => r.id === trialResult.intentId,
                              )?.name
                            }
                          </p>
                        )}
                        <ol
                          aria-label={t("intentRouting.plan")}
                          className="space-y-1"
                        >
                          {trialResult.plan.candidates.map((member, index) => (
                            <li key={member.id} className="break-all">
                              {index + 1}.{" "}
                              <AccountLabel>
                                {providers.data?.find(
                                  (p) => p.id === member.providerId,
                                )?.name || member.providerId}
                              </AccountLabel>{" "}
                              / {member.modelId}
                            </li>
                          ))}
                        </ol>
                      </div>
                    )}
                  </details>
                </fieldset>
                <details className="rt-diagnostics">
                  <summary>{t("intentRouting.offlineTitle")}</summary>
                  <p className="text-xs text-muted-foreground">
                    {t("intentRouting.offlineNotice")}
                  </p>
                  <label className="block space-y-1 text-sm">
                    {t("intentRouting.classificationJson")}
                    <Textarea
                      maxLength={4096}
                      value={classification}
                      onChange={(e) => {
                        invalidatePreview();
                        setClassification(e.target.value);
                      }}
                    />
                  </label>
                  <Button
                    type="button"
                    variant="outline"
                    disabled={!valid || previewPending}
                    onClick={() => void inspectPlan()}
                  >
                    {t("intentRouting.preview")}
                  </Button>
                  {preview && (
                    <ol
                      className="space-y-1 text-sm"
                      aria-label={t("intentRouting.plan")}
                    >
                      {preview.candidates.map((m, index) => (
                        <li key={m.id} className="break-all">
                          {index + 1}.{" "}
                          <AccountLabel>
                            {providers.data?.find((p) => p.id === m.providerId)
                              ?.name || m.providerId}
                          </AccountLabel>{" "}
                          / {m.modelId}
                        </li>
                      ))}
                    </ol>
                  )}
                </details>
              </div>
              <div className="rt-edit-levels">
                {" "}
                <div className="rt-reference-setting">
                  <span id={`routing-levels-${draft.id}`}>
                    {t("intentRouting.levels")}
                  </span>
                  <div
                    className="rt-reference-seg"
                    role="radiogroup"
                    aria-labelledby={`routing-levels-${draft.id}`}
                  >
                    {[false, true].map((own) => (
                      <button
                        key={String(own)}
                        type="button"
                        role="radio"
                        aria-checked={!!draft.levels?.length === own}
                        onClick={() =>
                          edit({
                            ...draft,
                            levels: own ? ["low", "medium", "high"] : [],
                          })
                        }
                      >
                        {t(
                          own
                            ? "intentRouting.levelsNamed"
                            : "intentRouting.levelsShared",
                        )}
                      </button>
                    ))}
                  </div>
                  {!!draft.levels?.length && (
                    <div className="rt-reference-chips">
                      {routingLevels.map((level) => (
                        <button
                          key={level}
                          type="button"
                          aria-pressed={draft.levels?.includes(level)}
                          disabled={
                            draft.levels?.length === 1 &&
                            draft.levels.includes(level)
                          }
                          onClick={() =>
                            edit({
                              ...draft,
                              levels: routingLevels.filter((value) =>
                                value === level
                                  ? !draft.levels?.includes(value)
                                  : draft.levels?.includes(value),
                              ),
                            })
                          }
                        >
                          {level}
                        </button>
                      ))}
                    </div>
                  )}
                  <p>{t("intentRouting.levelsHelp")}</p>
                </div>
              </div>
              <details className="rt-edit-settings">
                <summary className="cursor-pointer text-sm text-muted-foreground">
                  {t("modelPicker.groupSettings")}
                </summary>
                <div className="mt-3 space-y-4">
                  <div className="flex items-start justify-between gap-5">
                    <div className="space-y-1">
                      <label
                        htmlFor={`routing-publication-${draft.id}`}
                        className="text-sm font-medium"
                      >
                        {t("intentRouting.publishGateway")}
                      </label>
                      <p
                        id={`routing-publication-help-${draft.id}`}
                        className="text-xs text-muted-foreground"
                      >
                        {t("intentRouting.publishGatewayHelp")}
                      </p>
                    </div>
                    <Switch
                      className="mp-switch"
                      id={`routing-publication-${draft.id}`}
                      aria-describedby={`routing-publication-help-${draft.id}`}
                      checked={draft.enabled}
                      disabled={pending}
                      onCheckedChange={(enabled) => edit({ ...draft, enabled })}
                    />
                  </div>
                  <label className="block space-y-1 text-sm">
                    {t("intentRouting.defaultTarget")}
                    <Select
                      disabled={pending}
                      value={draft.defaultMemberId}
                      onValueChange={(value) =>
                        edit({ ...draft, defaultMemberId: value })
                      }
                    >
                      <SelectTrigger
                        aria-label={t("intentRouting.defaultTarget")}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>{memberOptions}</SelectContent>
                    </Select>
                  </label>
                  <p className="text-xs text-muted-foreground">
                    {t("intentRouting.fallbackHelp")}
                  </p>
                </div>
              </details>
            </div>
          </details>
        </div>
      </fieldset>
    </form>
  );

  return (
    <AccountMask
      as="section"
      masked={maskAccounts}
      names={providers.data?.map((provider) => provider.name)}
      className="rt-group-panel"
    >
      <div className="rt-group-heading">
        <div className="rt-group-title">
          <h3>{t("intentRouting.title")}</h3>
        </div>
        <div className="rt-group-tools">
          <div className="rt-group-search">
            <Search
              aria-hidden="true"
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
            />
            <Input
              type="search"
              aria-label={t("intentRouting.filter")}
              placeholder={t("intentRouting.filter")}
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              className="h-9 pl-9"
            />
          </div>
          <div className="rt-group-status-filter">
            <select
              aria-label={t("routingPage.groupStatus")}
              value={statusFilter}
              onChange={(event) => setStatusFilter(event.target.value)}
            >
              <option value="all">{t("routingPage.allStatuses")}</option>
              <option value="enabled">{t("routingPage.groupEnabled")}</option>
              <option value="disabled">{t("routingPage.groupDisabled")}</option>
            </select>
          </div>
          <Button
            variant="ghost"
            size="sm"
            ref={importButton}
            aria-label={t("routingPage.import")}
            title={t("routingPage.import")}
            aria-haspopup="dialog"
            aria-expanded={importOpen}
            disabled={pending || !!draft || !!deleting || !!selectedGroups}
            onClick={() => {
              setImportOpen(true);
              setImported(null);
              setError(null);
              setImportError(false);
            }}
          >
            <Download size={12} strokeWidth={1.75} aria-hidden="true" />
            <span>{t("routingPage.import")}</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            aria-label={t("routingPage.refresh")}
            title={t("routingPage.refresh")}
            aria-busy={refreshing}
            disabled={pending || refreshing || groups.isPending || !!deleting}
            onClick={() => void reload()}
          >
            <RoutingRefreshIcon refreshing={refreshing} />
            <span>{t("routingPage.refresh")}</span>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            aria-label={t(
              selectedGroups ? "common.cancel" : "routingPage.manage",
            )}
            title={t(selectedGroups ? "common.cancel" : "routingPage.manage")}
            aria-pressed={!!selectedGroups}
            disabled={
              pending ||
              !!draft ||
              !!deleting ||
              importOpen ||
              (!selectedGroups &&
                (groups.isPending ||
                  groups.isError ||
                  !groups.data?.groups.length))
            }
            onClick={() => setSelectedGroups(selectedGroups ? null : new Set())}
          >
            <Settings size={12} strokeWidth={1.75} aria-hidden="true" />
            <span>
              {t(selectedGroups ? "common.cancel" : "routingPage.manage")}
            </span>
          </Button>
          {showCreateButton && (
            <Button
              ref={createButton}
              variant="outline"
              size="sm"
              disabled={
                groups.isPending ||
                groups.isError ||
                providers.isPending ||
                providers.isError ||
                pending ||
                !!deleting ||
                (groups.data?.groups.filter((g) => !g.autoGenerated).length ??
                  0) >= 32
              }
              onClick={() => requestEditor(newRoutingGroup())}
            >
              {t("intentRouting.create")}
            </Button>
          )}
        </div>
      </div>
      {groups.data && (
        <section
          className="rt-template-cards"
          aria-label={t("routingPage.templates")}
        >
          <p className="rt-template-intro">{t("routingPage.templateHint")}</p>
          {templates.map((template) => (
            <div
              key={template.key}
              className={template.need ? "off" : undefined}
            >
              <b>{t(`routingPage.${template.key}`)}</b>
              <p>{t(`routingPage.${template.key}Hint`)}</p>
              <span>
                <AccountLabel title>
                  {template.need
                    ? templateNeed(template)
                    : template.group.members
                        .map((member) => member.modelId)
                        .join(" → ")}
                </AccountLabel>
              </span>
              <div className="rt-template-actions">
                <button
                  type="button"
                  className="text primary"
                  disabled={
                    pending ||
                    !!deleting ||
                    providers.isPending ||
                    providers.isError ||
                    !!template.need
                  }
                  onClick={() => useTemplate(template)}
                >
                  {t("routingPage.addTemplate")}
                </button>
              </div>
            </div>
          ))}
        </section>
      )}
      {selectedGroups && !!groups.data?.groups.length && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-background p-3 text-xs">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              disabled={pending}
              checked={
                !!visibleGroups?.length &&
                visibleGroups.every((group) => selectedGroups.has(group.id))
              }
              onChange={(event) =>
                setSelectedGroups(
                  event.target.checked
                    ? new Set(visibleGroups?.map((group) => group.id))
                    : new Set(),
                )
              }
            />
            {t("routingPage.selectAll")}
          </label>
          <Button
            size="sm"
            variant="destructive"
            disabled={
              !selectedGroups.size ||
              pending ||
              [...selectedGroups].some(isGroupPending)
            }
            onClick={() => {
              setError(null);
              setConfirmBulk(true);
            }}
          >
            {t("routingPage.removeSelected", { count: selectedGroups.size })}
          </Button>
        </div>
      )}
      {exported && (
        <p role="status" className="text-xs text-muted-foreground">
          {t("routingPage.exportStarted", { name: exported })}
        </p>
      )}
      {discovery.isFetching && (
        <p role="status" className="text-xs text-muted-foreground">
          {t("intentRouting.discoveringCatalogs")}
        </p>
      )}
      {discovery.isError && (
        <p role="status" className="text-xs text-muted-foreground">
          {t("intentRouting.discoveryFailed")}
        </p>
      )}
      {!!groups.data?.omittedGroups && (
        <p role="status" className="text-sm text-muted-foreground">
          {t("intentRouting.autoOmitted", { count: groups.data.omittedGroups })}
        </p>
      )}
      {!draft &&
        !importOpen &&
        (error || groups.isError || providers.isError) && (
          <div role="alert" className="text-sm text-destructive">
            {t(error || "intentRouting.failed")}{" "}
            <Button
              variant="outline"
              size="sm"
              disabled={pending}
              onClick={() => void reload()}
            >
              {t("intentRouting.reload")}
            </Button>
          </div>
        )}
      {groups.isPending && <p role="status">{t("common.loading")}</p>}
      {groups.data && (
        <div
          className="rt-group-table-frame"
          data-scroll-left={tableScrollEdges.left || undefined}
          data-scroll-right={tableScrollEdges.right || undefined}
        >
          <div
            ref={tableScroll}
            className="rt-group-table-scroll"
            role="region"
            aria-label={t("intentRouting.title")}
            tabIndex={0}
          >
            <table
              className="rt-group-table"
              aria-label={t("intentRouting.title")}
            >
              <thead>
                <tr>
                  {[
                    "groupName",
                    "upstreamCount",
                    "requestCount",
                    "switchCount",
                    "agentErrorCount",
                    "actions",
                  ].map((key) => (
                    <th key={key} scope="col">
                      {t(`routingPage.${key}`)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody
                aria-label={t("intentRouting.title")}
                className="rt-group-list"
                data-routing-list
              >
                {visibleGroups?.map((group) => {
                  const notReady =
                    providers.data &&
                    group.members.some(
                      (member) =>
                        !group.off?.includes(member.id) &&
                        (group.routing !== "manual" ||
                          member.id ===
                            (group.manualMemberId ?? group.defaultMemberId)) &&
                        !routingMemberAvailable(providers.data ?? [], member),
                    );
                  const summary = memberSummary(group);
                  return (
                    <tr
                      key={group.id}
                      className="rt-group-row"
                      data-routing-group={group.id}
                      aria-busy={isGroupPending(group.id)}
                      data-dragging={
                        groupDrag.drag?.from === group.id || undefined
                      }
                      data-drag-target={
                        (groupDrag.drag?.to === group.id &&
                          groupDrag.drag.from !== group.id) ||
                        undefined
                      }
                      onClick={(event) => {
                        if (
                          pending ||
                          isGroupPending(group.id) ||
                          deleting ||
                          providers.isPending ||
                          providers.isError
                        )
                          return;
                        const control = (event.target as Element).closest(
                          "button, input, select, textarea, a, label, [role='combobox'], [tabindex]",
                        );
                        if (control && event.currentTarget.contains(control))
                          return;
                        if (selectedGroups) {
                          setSelectedGroups((previous) => {
                            const next = new Set(previous);
                            if (next.has(group.id)) next.delete(group.id);
                            else next.add(group.id);
                            return next;
                          });
                        } else requestEditor(group);
                      }}
                    >
                      <td className="rt-group-name-cell">
                        <div className="rt-group-identity">
                          {selectedGroups && (
                            <input
                              type="checkbox"
                              disabled={pending || isGroupPending(group.id)}
                              aria-label={group.name}
                              checked={selectedGroups.has(group.id)}
                              onChange={(event) =>
                                setSelectedGroups((previous) => {
                                  const next = new Set(previous);
                                  if (event.target.checked) next.add(group.id);
                                  else next.delete(group.id);
                                  return next;
                                })
                              }
                            />
                          )}
                          <button
                            type="button"
                            className="rt-ghandle"
                            disabled={
                              pending ||
                              isGroupPending(group.id) ||
                              reordering ||
                              !!draft ||
                              !!deleting
                            }
                            aria-label={`${t("routingPage.arrangeGroup")} ${group.name}`}
                            title={t("routingPage.dragHint")}
                            onPointerDown={(event) => {
                              event.preventDefault();
                              setGroupMenu(null);
                              groupDrag.start(event, group.id);
                            }}
                            onClick={(event) => {
                              event.stopPropagation();
                              groupDrag.consumeClick(group.id);
                            }}
                            onKeyDown={(event) => {
                              if (
                                event.altKey &&
                                ["ArrowUp", "ArrowDown"].includes(event.key)
                              ) {
                                event.preventDefault();
                                void moveGroup(
                                  group,
                                  event.key === "ArrowUp" ? -1 : 1,
                                );
                              }
                            }}
                          >
                            <span
                              role="img"
                              aria-label={t("intentRouting.globalResource")}
                            >
                              <RoutingGroupIcon
                                client="global"
                                icons={group.members.map((member) => {
                                  const provider = providers.data?.find(
                                    (provider) =>
                                      provider.id === member.providerId,
                                  );
                                  return {
                                    icon:
                                      globalProviderPresets.find(
                                        (preset) =>
                                          preset.id === provider?.icon,
                                      )?.icon ??
                                      provider?.icon ??
                                      (provider?.appType === "codex"
                                        ? "openai"
                                        : undefined),
                                    name: provider?.name ?? member.providerId,
                                  };
                                })}
                              />
                            </span>
                          </button>
                          <div className="min-w-0 flex-1">
                            <div className="rt-group-name">
                              <h4 className="font-medium" title={group.name}>
                                {group.name}
                              </h4>
                              {group.autoGenerated && (
                                <span className="text-xs text-muted-foreground">
                                  {t("intentRouting.autoGenerated")}
                                </span>
                              )}
                              {notReady && (
                                <span
                                  tabIndex={0}
                                  title={t("intentRouting.memberNotReady")}
                                  aria-label={t("intentRouting.memberNotReady")}
                                  className="text-muted-foreground"
                                >
                                  <Info
                                    aria-hidden="true"
                                    className="h-3.5 w-3.5"
                                  />
                                </span>
                              )}
                            </div>
                            {group.routing === "manual" ? (
                              <div
                                className="rt-picks"
                                role="radiogroup"
                                aria-label={t("intentRouting.manualMember")}
                              >
                                {[
                                  ...group.members,
                                  ...(groups.data?.matchedMembers?.[group.id] ??
                                    []),
                                ].map((member) => (
                                  <button
                                    type="button"
                                    key={member.id}
                                    role="radio"
                                    className="rt-pick"
                                    aria-checked={
                                      member.id ===
                                      (group.manualMemberId ??
                                        group.defaultMemberId)
                                    }
                                    disabled={
                                      pending ||
                                      isGroupPending(group.id) ||
                                      !!draft ||
                                      !!deleting ||
                                      group.off?.includes(member.id) ||
                                      !routingMemberAvailable(
                                        providers.data ?? [],
                                        member,
                                      )
                                    }
                                    onClick={() => {
                                      if (
                                        member.id !==
                                        (group.manualMemberId ??
                                          group.defaultMemberId)
                                      )
                                        void changeManualMember(
                                          group,
                                          member.id,
                                        );
                                    }}
                                  >
                                    <span className="rt-pick-dot" aria-hidden />
                                    {member.modelId}
                                    {member.effort ? ` :${member.effort}` : ""}
                                  </button>
                                ))}
                              </div>
                            ) : (
                              <p className="rt-group-summary">
                                <AccountLabel title>{summary}</AccountLabel>
                              </p>
                            )}
                          </div>
                        </div>
                      </td>
                      <td className="rt-group-number">
                        {
                          new Set(
                            [
                              ...group.members,
                              ...(groups.data?.matchedMembers?.[group.id] ??
                                []),
                            ].map((member) => member.providerId),
                          ).size
                        }
                      </td>
                      {["requests", "switches", "errors"].map((metric) => (
                        <td
                          key={metric}
                          className="rt-group-number rt-group-no-stat"
                          title={t("routingPage.groupStatsUnavailable")}
                        >
                          <span
                            aria-label={t("routingPage.groupStatsUnavailable")}
                          >
                            —
                          </span>
                        </td>
                      ))}
                      <td>
                        <div className="rt-group-actions">
                          {routingSelect(
                            group,
                            (routing) => void changeRouting(group, routing),
                          )}
                          <Switch
                            className="rt-group-toggle"
                            aria-label={`${t("intentRouting.publishGateway")} ${group.name}`}
                            checked={group.enabled}
                            disabled={
                              pending ||
                              isGroupPending(group.id) ||
                              !!draft ||
                              !!deleting ||
                              !!selectedGroups
                            }
                            title={t("intentRouting.publishGatewayHelp")}
                            onCheckedChange={(enabled) => {
                              if (group.autoGenerated && group.enabled)
                                setDeleting(group);
                              else void publishGroup(group, enabled);
                            }}
                          />
                          <Button
                            ref={(button) => {
                              if (button)
                                editButtons.current.set(group.id, button);
                              else editButtons.current.delete(group.id);
                            }}
                            variant="ghost"
                            size="sm"
                            className="rt-group-edit"
                            aria-label={t("common.edit")}
                            title={t("common.edit")}
                            aria-haspopup="dialog"
                            disabled={
                              pending ||
                              isGroupPending(group.id) ||
                              !!deleting ||
                              providers.isPending ||
                              providers.isError
                            }
                            onClick={() => requestEditor(group)}
                          >
                            <Pencil size={13} aria-hidden="true" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="rt-group-delete"
                            aria-label={t("common.delete")}
                            title={t("common.delete")}
                            aria-haspopup="dialog"
                            disabled={
                              pending ||
                              isGroupPending(group.id) ||
                              !!draft ||
                              !!deleting ||
                              !!selectedGroups
                            }
                            onClick={() => {
                              setError(null);
                              setDeleting(group);
                            }}
                          >
                            <Trash2 size={13} aria-hidden="true" />
                          </Button>
                          <DropdownMenu
                            open={groupMenu === group.id}
                            onOpenChange={(open) =>
                              setGroupMenu(open ? group.id : null)
                            }
                          >
                            <DropdownMenuTrigger asChild>
                              <Button
                                variant="ghost"
                                size="sm"
                                className="rt-group-more"
                                aria-label={`${t("routingPage.more")} ${group.name}`}
                                title={t("routingPage.more")}
                                disabled={
                                  pending ||
                                  isGroupPending(group.id) ||
                                  !!draft ||
                                  !!deleting ||
                                  !!selectedGroups
                                }
                              >
                                <Ellipsis size={13} aria-hidden="true" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent
                              className="mp-row-menu"
                              align="end"
                              onClick={(event) => event.stopPropagation()}
                            >
                              <DropdownMenuItem
                                disabled={isGroupPending(group.id)}
                                onSelect={() => void duplicateGroup(group)}
                              >
                                {t("routingPage.duplicate")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                disabled={isGroupPending(group.id)}
                                onSelect={() => void shareGroup(group)}
                              >
                                {t("routingPage.copyShare")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                disabled={isGroupPending(group.id)}
                                onSelect={() => exportGroup(group)}
                              >
                                {t("routingPage.export")}
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>
                      </td>
                    </tr>
                  );
                })}
                {!draft && visibleGroups?.length === 0 && (
                  <tr>
                    <td colSpan={6} className="rt-group-empty">
                      <div className="rt-group-empty-content">
                        {!groups.data.groups.length && (
                          <RoutingEmptyIllustration />
                        )}
                        <p>
                          {t(
                            groups.data.groups.length
                              ? "intentRouting.noMatches"
                              : "routingPage.noGroups",
                          )}
                        </p>
                      </div>
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
      {!!groups.data?.removedGroups?.length && (
        <div className="rt-ghidden">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="rt-gremoved"
                disabled={pending || !!draft || !!deleting}
              >
                {t("intentRouting.removedGroups", {
                  count: groups.data.removedGroups.length,
                })}
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent className="mp-row-menu">
              {groups.data.removedGroups.map((group) => (
                <DropdownMenuItem
                  key={group.id}
                  disabled={isGroupPending(group.id)}
                  onSelect={() => void restoreGroup(group.id)}
                >
                  {group.name} · {t("intentRouting.restoreGroup")}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}
      <ConfirmDialog
        isOpen={!!deleting}
        title={t("common.delete")}
        message={t("intentRouting.deletePrompt", { name: deleting?.name })}
        confirmText={t("intentRouting.confirmDelete")}
        pending={isGroupPending(deleting?.id)}
        onConfirm={() => void remove()}
        onCancel={() => setDeleting(null)}
      />

      <AppDialog
        open={importOpen}
        onOpenChange={(open) => {
          if (!open) closeImport();
        }}
        title={t(
          imported ? "routingPage.importComplete" : "routingPage.import",
        )}
        description={imported ? undefined : t("routingPage.importHint")}
        size="md"
        className="rt-import-dialog"
        pending={pending}
        initialFocusRef={importInput}
        onCloseAutoFocus={(event) => {
          if (importButton.current?.isConnected) {
            event.preventDefault();
            importButton.current.focus({ preventScroll: true });
          }
        }}
        footer={
          imported ? (
            <Button
              type="button"
              size="sm"
              disabled={pending}
              onClick={closeImport}
            >
              {t("common.done")}
            </Button>
          ) : (
            <>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={pending}
                onClick={closeImport}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="submit"
                form={importFormId}
                size="sm"
                disabled={!importText.trim() || pending || !providers.data}
              >
                {t("routingPage.reviewImport")}
              </Button>
            </>
          )
        }
      >
        {imported ? (
          <div
            role="status"
            aria-label={t("routingPage.importComplete")}
            className="rt-import-result"
          >
            <p>{t("routingPage.imported", { name: imported.group.name })}</p>
            {!!imported.missing.length && (
              <p>
                {t("routingPage.importMissing", {
                  models: imported.missing.join(", "),
                })}
              </p>
            )}
            {!!imported.instead.length && (
              <p>
                {t("routingPage.importInstead", {
                  models: imported.instead.join(", "),
                })}
              </p>
            )}
            {!!imported.omittedRules && (
              <p>
                {t("routingPage.importRules", { count: imported.omittedRules })}
              </p>
            )}
          </div>
        ) : (
          <form
            id={importFormId}
            className="space-y-3"
            onSubmit={(event) => {
              event.preventDefault();
              void importGroup();
            }}
          >
            <label className="block space-y-2 text-xs">
              <span>{t("routingPage.importLabel")}</span>
              <Textarea
                ref={importInput}
                disabled={pending}
                value={importText}
                maxLength={65536}
                rows={7}
                onChange={(event) => {
                  setImportText(event.target.value);
                  setImportError(false);
                }}
              />
            </label>
            {importError && (
              <p role="alert" className="text-xs text-destructive">
                {t("routingPage.invalidImport")}
              </p>
            )}
            {(error || providers.isError) && (
              <div role="alert" className="text-xs text-destructive">
                <p>{t(error || "intentRouting.failed")}</p>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={pending}
                  onClick={() => void reload()}
                >
                  {t("intentRouting.reload")}
                </Button>
              </div>
            )}
          </form>
        )}
      </AppDialog>

      <AppDialog
        open={!!draft}
        onOpenChange={(open) => {
          if (!open) edit(null);
        }}
        title={initialDraft.current?.name || t("intentRouting.create")}
        initialFocusRef={nameInput}
        onCloseAutoFocus={(event) => {
          const trigger = initialDraft.current
            ? editButtons.current.get(initialDraft.current.id)
            : null;
          if (trigger?.isConnected) {
            event.preventDefault();
            if (
              pending ||
              activeGroups.current.has(initialDraft.current?.id ?? "")
            )
              setDeferredFocus(initialDraft.current?.id ?? null);
            else trigger.focus({ preventScroll: true });
          }
        }}
        pending={pending}
        className="vary-routing rt-create-dialog rt-form-theme"
        style={{ maxWidth: "none" }}
        footer={editorActions}
      >
        {editor}
      </AppDialog>

      <ConfirmDialog
        isOpen={confirmBulk}
        title={t("routingPage.removeSelected", {
          count: selectedGroups?.size ?? 0,
        })}
        message={t("routingPage.removeSelectedHint", {
          count: selectedGroups?.size ?? 0,
        })}
        confirmText={t("intentRouting.confirmDelete")}
        pending={pending}
        errorMessage={error ? t(error) : undefined}
        onConfirm={() => void removeSelected()}
        onCancel={() => setConfirmBulk(false)}
      />
      <ConfirmDialog
        isOpen={!!nextDraft}
        title={t("intentRouting.discardTitle")}
        message={t("intentRouting.discardMessage")}
        confirmText={t("intentRouting.discardChanges")}
        onConfirm={() => {
          if (nextDraft) openEditor(nextDraft);
          setNextDraft(null);
        }}
        onCancel={() => setNextDraft(null)}
      />
    </AccountMask>
  );
}
