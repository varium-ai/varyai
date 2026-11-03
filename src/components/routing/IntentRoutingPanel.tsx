import {
  Search,
  Trash2,
  Info,
  ChevronUp,
  ChevronDown,
  Download,
  Copy,
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
import { SharedModelPicker } from "@/components/models/SharedModelPicker";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { RoutingGroupIcon } from "@/components/routing/RoutingGroupIcon";
import { RoutingGroupSlot } from "@/components/routing/RoutingGroupSlot";
import { RoutingRulesEditor } from "@/components/routing/RoutingRulesEditor";
import { RoutingExecutionSettings } from "@/components/routing/RoutingExecutionSettings";
import { AutoGroupProviders } from "@/components/routing/AutoGroupProviders";
import { copyText } from "@/lib/clipboard";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";
import { globalProviderPresets } from "@/config/globalProviderPresets";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  useWorkspaceEditing,
  type EditingState,
} from "@/hooks/useWorkspaceEditing";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
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
  type RoutingGroup,
  type RoutingCatalogStatus,
  type RoutingPlanPreview,
  type RoutingProviderOption,
} from "@/lib/api/intentRouting";

const knownErrors = new Set([
  "invalid",
  "conflict",
  "limit",
  "providerUnavailable",
  "notReady",
  "invalidOutput",
]);

export function IntentRoutingPanel({
  onEditingChange,
  createRequest = 0,
  maskAccounts = false,
}: {
  onEditingChange?: (state: EditingState) => void;
  createRequest?: number;
  maskAccounts?: boolean;
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
    select: (options) =>
      maskAccounts
        ? options.map((provider, index) => ({
            ...provider,
            name: t("routingPage.maskedAccount", { n: index + 1 }),
          }))
        : options,
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
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [importError, setImportError] = useState(false);
  const [imported, setImported] = useState<RoutingImportResult | null>(null);
  const [shared, setShared] = useState<string | null>(null);
  const [groupMenu, setGroupMenu] = useState<string | null>(null);
  const groupDrag = useRoutingRowDrag("data-routing-group", (from, to) => {
    void arrangeGroup(from, to);
  });
  const memberDrag = useRoutingRowDrag("data-routing-member", (from, to) => {
    if (!draft) return;
    moveMemberTo(
      draft.members.findIndex((member) => member.id === from),
      draft.members.findIndex((member) => member.id === to),
    );
  });
  const [selectedGroups, setSelectedGroups] = useState<Set<string> | null>(
    null,
  );
  const [confirmBulk, setConfirmBulk] = useState(false);
  const [pickingModels, setPickingModels] = useState(false);
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
  const [creatingId, setCreatingId] = useState<string | null>(null);
  const initialDraft = useRef<RoutingGroup | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const editButtons = useRef(new Map<string, HTMLButtonElement>());
  const openedGroup = useRef<string | null>(null);
  const [deleting, setDeleting] = useState<RoutingGroup | null>(null);
  const [pending, setPending] = useState(false);
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
        requestEditor(blankRoutingGroup());
      }
    }
  }, [createRequest, groups.data, providers.data, pending, deleting]);
  useWorkspaceEditing(
    draftDirty || (importOpen && !!importText.trim()),
    pending,
    onEditingChange,
  );
  const [error, setError] = useState<string | null>(null);
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
  const refreshGeneration = useRef(0);
  const [catalogStatuses, setCatalogStatuses] = useState<
    RoutingCatalogStatus[]
  >([]);
  useEffect(
    () => () => {
      generation.current += 1;
      refreshGeneration.current += 1;
    },
    [],
  );
  useEffect(() => {
    if (
      creatingId &&
      groups.data?.groups.some((group) => group.id === creatingId)
    ) {
      setCreatingId(null);
    }
  }, [creatingId, groups.data]);
  useEffect(() => {
    if (draft && openedGroup.current !== draft.id) {
      openedGroup.current = draft.id;
      nameInput.current?.focus({ preventScroll: true });
    } else if (!draft && !pending && openedGroup.current) {
      const trigger =
        editButtons.current.get(openedGroup.current) ?? createButton.current;
      trigger?.focus({ preventScroll: true });
      openedGroup.current = null;
    }
  }, [draft?.id, pending]);

  function invalidatePreview() {
    generation.current += 1;
    setPreview(null);
    setTrialResult(null);
    setTrialPending(false);
    setPreviewPending(false);
    setError(null);
  }
  function edit(value: RoutingGroup | null) {
    invalidatePreview();
    setTrialConsent(false);
    setDraft(value);
  }
  function openEditor(value: RoutingGroup) {
    if (!groups.data?.groups.some((group) => group.id === value.id)) {
      setCreatingId(value.id);
    }
    initialDraft.current = structuredClone(value);
    setImportOpen(false);
    setImportText("");
    setPickingModels(false);
    setPickingClassifier(false);
    setPatternInput("");
    edit(structuredClone(value));
  }
  function requestEditor(value: RoutingGroup) {
    if (draftDirty || (importOpen && !!importText.trim())) {
      setNextDraft(value);
    } else {
      openEditor(value);
    }
  }
  function useTemplate(template: ReturnType<typeof routingTemplates>[number]) {
    if (template.need) return;
    requestEditor({
      ...template.group,
      name: t(`routingPage.${template.key}`),
    });
  }
  function templateNeed(template: ReturnType<typeof routingTemplates>[number]) {
    return template.need
      ? t(`routingPage.templateNeed_${template.need}`, { name: template.of })
      : "";
  }
  async function addTemplate(
    template: ReturnType<typeof routingTemplates>[number],
  ) {
    if (template.need || pending || draft || importOpen) return;
    setPending(true);
    setError(null);
    try {
      const name = t(`routingPage.${template.key}`).slice(0, 80);
      const group = { ...template.group, name };
      const names = new Set(
        groups.data?.groups.map((entry) => entry.name.toLocaleLowerCase()),
      );
      let number = 1;
      while (names.has(group.name.toLocaleLowerCase())) {
        const suffix = ` ${++number}`;
        group.name = `${name.slice(0, 80 - suffix.length)}${suffix}`;
      }
      await intentRoutingApi.save(group);
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function removeSelected() {
    if (!selectedGroups?.size || !groups.data) return;
    setPending(true);
    setError(null);
    setConfirmBulk(false);
    try {
      for (const group of groups.data.groups.filter((group) =>
        selectedGroups.has(group.id),
      )) {
        await intentRoutingApi.remove(group.id, group.revision);
        setSelectedGroups((previous) => {
          const next = new Set(previous);
          next.delete(group.id);
          return next;
        });
      }
      setSelectedGroups(null);
    } catch (cause) {
      report(cause);
    } finally {
      await refreshRoutingStatus();
      setPending(false);
    }
  }
  function exportGroup(group: RoutingGroup) {
    let text: string;
    try {
      text = shareRoutingGroup(group);
    } catch {
      report(null);
      return;
    }
    const blob = new Blob([text], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `vary-routing-group-${group.id}.json`;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  async function shareGroup(group: RoutingGroup) {
    setError(null);
    try {
      await copyText(shareRoutingGroup(group));
      setShared(group.id);
    } catch {
      report(null);
    }
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
    const names = new Set(
      groups.data?.groups.map((group) => group.name.toLocaleLowerCase()),
    );
    const base = result.group.name;
    let number = 1;
    while (names.has(result.group.name.toLocaleLowerCase())) {
      const suffix = ` ${++number}`;
      result.group.name = `${base.slice(0, 80 - suffix.length)}${suffix}`;
    }
    setPending(true);
    try {
      result.group = await intentRoutingApi.save(result.group);
      setImported(result);
      setImportText("");
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function moveGroup(group: RoutingGroup, offset: number) {
    if (pending || draft) return;
    const all = groups.data?.groups ?? [];
    const visible = all.filter((candidate) =>
      visibleGroups?.some((entry) => entry.id === candidate.id),
    );
    const next =
      visible[visible.findIndex((entry) => entry.id === group.id) + offset];
    if (!next) return;
    await arrangeGroup(group.id, next.id);
  }
  async function arrangeGroup(id: string, target: string) {
    if (pending || draft) return;
    const all = groups.data?.groups ?? [];
    const order = all.map((entry) => entry.id);
    const from = order.indexOf(id);
    const to = order.indexOf(target);
    if (from < 0 || to < 0 || from === to) return;
    order.splice(to, 0, ...order.splice(from, 1));
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.arrange(order);
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function duplicateGroup(group: RoutingGroup) {
    if (pending || draft || !providers.data) return;
    setPending(true);
    setError(null);
    try {
      // A local copy retains unavailable seats and machine-local rules.
      const duplicate = importRoutingGroup(
        JSON.stringify(group),
        [
          ...providers.data,
          ...[
            ...new Set(
              [
                ...group.members,
                ...(group.classifierTarget ? [group.classifierTarget] : []),
              ].map((member) => member.providerId),
            ),
          ]
            .filter(
              (id) => !providers.data?.some((provider) => provider.id === id),
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
              .map((member) => ({ id: member.modelId, name: member.modelId })),
          ],
        })),
      );
      duplicate.enabled = true;
      duplicate.name = t("routingPage.copyName", { name: group.name }).slice(
        0,
        80,
      );
      const base = duplicate.name;
      const names = new Set(
        groups.data?.groups.map((entry) => entry.name.toLocaleLowerCase()),
      );
      let number = 1;
      while (names.has(duplicate.name.toLocaleLowerCase())) {
        const suffix = ` ${++number}`;
        duplicate.name = `${base.slice(0, 80 - suffix.length)}${suffix}`;
      }
      const saved = await intentRoutingApi.save(duplicate);
      const order = (groups.data?.groups ?? []).map((entry) => entry.id);
      order.splice(order.indexOf(group.id) + 1, 0, saved.id);
      await intentRoutingApi.arrange(order);
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
      await refreshRoutingStatus();
    } finally {
      setPending(false);
    }
  }
  function removeMember(id: string) {
    if (!draft || pending) return;
    const members = draft.members.filter((m) => m.id !== id);
    edit({
      ...draft,
      members,
      off: draft.off?.filter((value) => value !== id),
      defaultMemberId:
        draft.defaultMemberId === id
          ? (members[0]?.id ?? "")
          : draft.defaultMemberId,
      manualMemberId:
        draft.manualMemberId === id ? members[0]?.id : draft.manualMemberId,
      fallbackMemberIds: draft.fallbackMemberIds.filter(
        (value) => value !== id,
      ),
      rules: draft.rules.flatMap((rule) => {
        const remaining = [
          rule.targetMemberId,
          ...(rule.fallbackMemberIds ?? []),
        ].filter((target) => target !== id);
        return remaining.length
          ? [
              {
                ...rule,
                targetMemberId: remaining[0],
                fallbackMemberIds: remaining.slice(1),
              },
            ]
          : [];
      }),
    });
  }
  function moveMember(index: number, offset: number) {
    moveMemberTo(index, index + offset);
  }
  function moveMemberTo(index: number, next: number) {
    if (!draft) return;
    const members = [...draft.members];
    if (
      index < 0 ||
      index >= members.length ||
      next < 0 ||
      next >= members.length
    )
      return;
    members.splice(next, 0, ...members.splice(index, 1));
    const eligible = new Set([
      draft.defaultMemberId,
      ...draft.fallbackMemberIds,
    ]);
    edit({
      ...draft,
      members,
      defaultMemberId: members[0].id,
      fallbackMemberIds: members
        .filter((m) => m.id !== members[0].id && eligible.has(m.id))
        .map((m) => m.id),
    });
  }
  async function publishGroup(group: RoutingGroup, enabled: boolean) {
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.save({ ...group, enabled });
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
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
    if (!draft) return;
    const pattern = patternInput.trim();
    if (
      pattern &&
      (!validRoutingPattern(pattern) || (draft.match?.length ?? 0) >= 32)
    ) {
      setError("intentRouting.invalidPattern");
      return;
    }
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.save(
        pattern
          ? { ...draft, match: [...new Set([...(draft.match ?? []), pattern])] }
          : draft,
      );
      setPatternInput("");
      edit(null);
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function changeRouting(
    group: RoutingGroup,
    routing: NonNullable<RoutingGroup["routing"]>,
  ) {
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.save({
        ...group,
        routing,
        ...(routing === "manual"
          ? { manualMemberId: group.manualMemberId ?? group.defaultMemberId }
          : {}),
      });
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  function routingSelect(
    group: RoutingGroup,
    update: (routing: NonNullable<RoutingGroup["routing"]>) => void,
  ) {
    return (
      <Select
        value={group.routing ?? "ordered"}
        disabled={pending || (!!draft && group !== draft)}
        onValueChange={(value) =>
          update(value as NonNullable<RoutingGroup["routing"]>)
        }
      >
        <SelectTrigger
          className="h-8 w-auto min-w-24 gap-2 rounded-full text-xs"
          aria-label={t("intentRouting.routingStrategy")}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(
            [
              ["smart", "routingSmart"],
              ["ordered", "routingOrdered"],
              ["round_robin", "routingRoundRobin"],
              ["least_used", "routingLeastUsed"],
              ["pace", "routingPace"],
              ["free_first", "routingFreeFirst"],
              ["fastest", "routingFastest"],
              ["weighted", "routingWeighted"],
              ["manual", "routingManual"],
            ] as const
          ).map(([value, key]) => (
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
        disabled={pending || (!!draft && group !== draft)}
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
              disabled={group.off?.includes(member.id)}
            >
              {providers.data?.find((p) => p.id === member.providerId)?.name ||
                member.providerId}{" "}
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
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.save({ ...group, manualMemberId });
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function restoreGroup(id: string) {
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.restore(id);
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
  }
  async function toggleAutoCreate(enabled: boolean) {
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.setAutoCreate(enabled);
      if (enabled) await refreshCatalogs();
      else setCatalogStatuses([]);
    } catch (cause) {
      report(cause);
    } finally {
      await refreshRoutingStatus();
      setPending(false);
    }
  }
  async function refreshCatalogs() {
    const request = ++refreshGeneration.current;
    const statuses = await intentRoutingApi.refreshCatalogs();
    if (request === refreshGeneration.current) setCatalogStatuses(statuses);
  }
  async function reload() {
    setPending(true);
    setError(null);
    try {
      await refreshCatalogs();
    } catch (cause) {
      report(cause);
    } finally {
      await Promise.all([
        groups.refetch(),
        providers.refetch(),
        client.invalidateQueries({ queryKey: ["intent-routing-client"] }),
      ]);
      setPending(false);
    }
  }
  async function remove() {
    if (!deleting) return;
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.remove(deleting.id, deleting.revision);
      if (draft?.id === deleting.id) edit(null);
      setDeleting(null);
      await refreshRoutingStatus();
    } catch (cause) {
      report(cause);
    } finally {
      setPending(false);
    }
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
      {providers.data?.find((p) => p.id === m.providerId)?.name || m.providerId}
      {m.effort ? ` :${m.effort}` : ""}
    </SelectItem>
  ));
  function memberSummary(group: RoutingGroup) {
    return group.members
      .map((member) => {
        const name =
          providers.data?.find((p) => p.id === member.providerId)?.name ||
          member.providerId;
        return `${name} · ${member.modelId}${member.effort ? ` :${member.effort}` : ""}`;
      })
      .join(" · ");
  }
  const query = filter.trim().toLocaleLowerCase();
  const visibleGroups = groups.data?.groups.filter(
    (group) =>
      group.id === draft?.id ||
      [
        group.name,
        group.modelAlias ?? `vary-auto/${group.id}`,
        ...group.members.flatMap((member) => [
          member.modelId,
          providers.data?.find((p) => p.id === member.providerId)?.name ||
            member.providerId,
        ]),
      ].some((value) => value.toLocaleLowerCase().includes(query)),
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

  function foldEditor() {
    if (pending || !draft) return;
    if (JSON.stringify(draft) !== JSON.stringify(initialDraft.current)) {
      setError("intentRouting.saveOrCancel");
    } else {
      edit(null);
    }
  }
  const existingDraft =
    !!draft && !!groups.data?.groups.some((group) => group.id === draft.id);
  const classifierModels =
    providers.data
      ?.filter((provider) => provider.classifierEligible)
      .flatMap((provider) =>
        (provider.models ?? []).map((model) => ({
          ...model,
          id: JSON.stringify([provider.id, model.id]),
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
  const editor = draft && (
    <form
      aria-label={initialDraft.current?.name || t("intentRouting.create")}
      className="rt-group-editor"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div
        data-routing-editor-heading
        role={existingDraft ? "button" : undefined}
        tabIndex={existingDraft ? 0 : undefined}
        aria-expanded={existingDraft ? true : undefined}
        aria-disabled={existingDraft ? pending : undefined}
        title={existingDraft ? t("intentRouting.foldHint") : undefined}
        className={
          existingDraft
            ? "cursor-pointer rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            : undefined
        }
        onClick={existingDraft ? foldEditor : undefined}
        onKeyDown={
          existingDraft
            ? (event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  foldEditor();
                }
              }
            : undefined
        }
      >
        <h4 className="min-w-0 truncate font-semibold">
          {initialDraft.current?.name || t("intentRouting.create")}
        </h4>
      </div>
      {(error || groups.isError || providers.isError) && (
        <p role="alert" className="text-sm text-destructive">
          {t(error || "intentRouting.failed")}
        </p>
      )}
      <fieldset disabled={pending} className="min-w-0">
        <div className="rt-editor-fields">
          <label className="rt-edit-name">
            <span>{t("intentRouting.name")}</span>
            <Input
              ref={nameInput}
              required
              maxLength={80}
              value={draft.name}
              onChange={(e) => edit({ ...draft, name: e.target.value })}
            />
          </label>
          <div className="rt-edit-strategy">
            <label className="block text-sm">
              {t("intentRouting.routingStrategy")}
            </label>
            {routingSelect(draft, (routing) =>
              edit({
                ...draft,
                routing,
                ...(routing === "manual"
                  ? {
                      manualMemberId:
                        draft.manualMemberId ?? draft.defaultMemberId,
                    }
                  : {}),
              }),
            )}
            {draft.routing === "manual" &&
              manualSelect(draft, (manualMemberId) =>
                edit({ ...draft, manualMemberId }),
              )}
            {!["round_robin", "weighted", "manual"].includes(
              draft.routing ?? "",
            ) && (
              <div className="rt-reference-setting">
                <span id={`routing-sink-${draft.id}`}>
                  {t("intentRouting.sink")}
                </span>
                <div
                  className="rt-reference-seg"
                  role="radiogroup"
                  aria-labelledby={`routing-sink-${draft.id}`}
                >
                  {[false, true].map((sink) => (
                    <button
                      key={String(sink)}
                      type="button"
                      role="radio"
                      aria-checked={!!draft.sink === sink}
                      onClick={() => edit({ ...draft, sink })}
                    >
                      {t(
                        sink
                          ? "intentRouting.sinkBack"
                          : "intentRouting.sinkKeep",
                      )}
                    </button>
                  ))}
                </div>
                <p>{t("intentRouting.sinkHelp")}</p>
              </div>
            )}
          </div>
          <div className="rt-edit-members" data-routing-list>
            <h4 className="font-medium">{t("intentRouting.members")}</h4>
            {draft.members.map((member, index) => (
              <div
                key={member.id}
                data-routing-member={member.id}
                data-dragging={memberDrag.drag?.from === member.id || undefined}
                data-drag-target={
                  (memberDrag.drag?.to === member.id &&
                    memberDrag.drag.from !== member.id) ||
                  undefined
                }
                className="rt-member-row min-w-0 border-b border-border py-3"
              >
                <div className="flex items-center gap-2">
                  <Switch
                    className="mp-switch"
                    disabled={pending}
                    aria-label={`${t("intentRouting.memberEnabled")} ${member.modelId}`}
                    checked={!draft.off?.includes(member.id)}
                    onCheckedChange={(enabled) =>
                      edit({
                        ...draft,
                        off: enabled
                          ? (draft.off ?? []).filter((id) => id !== member.id)
                          : [...(draft.off ?? []), member.id],
                      })
                    }
                  />
                  <button
                    type="button"
                    className="rt-mhandle"
                    aria-label={`${t("routingPage.arrangeModel")} ${member.modelId}`}
                    title={t("routingPage.dragHint")}
                    onPointerDown={(event) =>
                      memberDrag.start(event, member.id)
                    }
                    onKeyDown={(event) => {
                      if (
                        event.altKey &&
                        ["ArrowUp", "ArrowDown"].includes(event.key)
                      ) {
                        event.preventDefault();
                        moveMember(index, event.key === "ArrowUp" ? -1 : 1);
                      }
                    }}
                  >
                    {index + 1}
                  </button>
                  <div className="min-w-0 flex-1">
                    <p className="break-words text-sm font-medium">
                      {member.modelId}
                      {member.effort ? ` :${member.effort}` : ""}
                    </p>
                    <p className="text-xs text-muted-foreground">
                      {providers.data?.find((p) => p.id === member.providerId)
                        ?.name ?? member.providerId}
                    </p>
                  </div>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        className={`rt-cond rt-fixed${member.effort ? " on" : ""}`}
                        disabled={pending}
                        aria-label={`${t("intentRouting.memberEffort")} ${member.modelId}`}
                        title={t("intentRouting.memberEffortHelp")}
                      >
                        {member.effort ?? t("intentRouting.memberEffortAgent")}
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent className="mp-row-menu" align="end">
                      {["agent", ...routingLevels].map((level) => {
                        const effort = level === "agent" ? undefined : level;
                        const duplicate = draft.members.some(
                          (other) =>
                            other.id !== member.id &&
                            other.providerId === member.providerId &&
                            other.modelId === member.modelId &&
                            (other.effort ?? undefined) === effort,
                        );
                        return (
                          <DropdownMenuItem
                            key={level}
                            disabled={duplicate}
                            onSelect={() =>
                              edit({
                                ...draft,
                                members: draft.members.map((old) =>
                                  old.id === member.id
                                    ? { ...old, effort }
                                    : old,
                                ),
                              })
                            }
                          >
                            {level === "agent"
                              ? t("intentRouting.memberEffortAgent")
                              : level}
                          </DropdownMenuItem>
                        );
                      })}
                    </DropdownMenuContent>
                  </DropdownMenu>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    aria-label={t("modelPicker.moveUp")}
                    disabled={index === 0}
                    onClick={() => moveMember(index, -1)}
                  >
                    <ChevronUp className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    aria-label={t("modelPicker.moveDown")}
                    disabled={index === draft.members.length - 1}
                    onClick={() => moveMember(index, 1)}
                  >
                    <ChevronDown className="h-4 w-4" />
                  </Button>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    aria-label={`${t("modelProviders.removeModel")} ${member.modelId}`}
                    disabled={pending}
                    onClick={() => removeMember(member.id)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
                <details className="mt-2 text-xs text-muted-foreground">
                  <summary className="cursor-pointer">
                    {t("modelPicker.memberSettings")}
                  </summary>
                  <div className="mt-3 grid min-w-0 gap-3 sm:grid-cols-2 [&>label]:min-w-0">
                    <label className="space-y-1 text-sm">
                      {t("intentRouting.provider")}
                      <Select
                        required
                        disabled={pending}
                        value={member.providerId}
                        onValueChange={(value) =>
                          edit({
                            ...draft,
                            members: draft.members.map((m) =>
                              m.id === member.id
                                ? { ...m, providerId: value }
                                : m,
                            ),
                          })
                        }
                      >
                        <SelectTrigger aria-label={t("intentRouting.provider")}>
                          <SelectValue
                            placeholder={t("intentRouting.selectProvider")}
                          />
                        </SelectTrigger>
                        <SelectContent>
                          {member.providerId &&
                            !providers.data?.some(
                              (p) => p.id === member.providerId,
                            ) && (
                              <SelectItem value={member.providerId}>
                                {t("intentRouting.providerUnavailable")}
                              </SelectItem>
                            )}
                          {providers.data?.map((p) => (
                            <SelectItem key={p.id} value={p.id}>
                              {p.name}
                              {p.appType ? ` · ${p.appType}` : ""}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      {member.providerId &&
                        !providers.data?.some(
                          (p) => p.id === member.providerId && p.taskEligible,
                        ) && (
                          <span className="block text-xs text-muted-foreground">
                            {t("intentRouting.memberNotReady")}
                          </span>
                        )}
                    </label>
                    <label className="space-y-1 text-sm">
                      {t("intentRouting.memberEffort")}
                      <select
                        className="h-9 w-full rounded-md border border-input bg-background px-2"
                        disabled={pending}
                        aria-label={t("intentRouting.memberEffort")}
                        value={member.effort ?? "agent"}
                        onChange={(event) =>
                          edit({
                            ...draft,
                            members: draft.members.map((m) =>
                              m.id === member.id
                                ? {
                                    ...m,
                                    effort:
                                      event.target.value === "agent"
                                        ? undefined
                                        : event.target.value,
                                  }
                                : m,
                            ),
                          })
                        }
                      >
                        <option value="agent">
                          {t("intentRouting.memberEffortAgent")}
                        </option>
                        {routingLevels.map((level) => (
                          <option key={level} value={level}>
                            {level}
                          </option>
                        ))}
                      </select>
                      <span className="block text-xs text-muted-foreground">
                        {t("intentRouting.memberEffortHelp")}
                      </span>
                    </label>
                    <div className="flex items-start">
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        disabled={pending || draft.members.length >= 256}
                        onClick={() => {
                          const taken = new Set(
                            draft.members
                              .filter(
                                (m) =>
                                  m.providerId === member.providerId &&
                                  m.modelId === member.modelId,
                              )
                              .map((m) => m.effort ?? "agent"),
                          );
                          const effort = ["agent", ...routingLevels].find(
                            (level) => !taken.has(level),
                          );
                          if (!effort) return;
                          const copy = {
                            ...member,
                            id: crypto.randomUUID(),
                            effort: effort === "agent" ? undefined : effort,
                          };
                          const members = [...draft.members];
                          members.splice(index + 1, 0, copy);
                          edit({ ...draft, members });
                        }}
                      >
                        {t("intentRouting.duplicateMember")}
                      </Button>
                    </div>
                    <label className="space-y-1 text-sm">
                      {t("intentRouting.model")}
                      <Input
                        required
                        maxLength={200}
                        value={member.modelId}
                        onChange={(e) =>
                          edit({
                            ...draft,
                            members: draft.members.map((m) =>
                              m.id === member.id
                                ? { ...m, modelId: e.target.value }
                                : m,
                            ),
                          })
                        }
                      />
                    </label>
                    {draft.routing === "weighted" && (
                      <label className="space-y-1 text-sm">
                        {t("intentRouting.memberWeight")}
                        <Input
                          type="number"
                          required
                          min={1}
                          max={1000}
                          step={1}
                          value={member.weight ?? 1}
                          onChange={(e) =>
                            edit({
                              ...draft,
                              members: draft.members.map((m) =>
                                m.id === member.id
                                  ? { ...m, weight: Number(e.target.value) }
                                  : m,
                              ),
                            })
                          }
                        />
                      </label>
                    )}
                    {draft.routing === "free_first" && (
                      <label className="space-y-1 text-sm">
                        {t("intentRouting.memberFree")}
                        <Select
                          value={
                            member.free == null
                              ? "auto"
                              : member.free
                                ? "free"
                                : "paid"
                          }
                          disabled={pending}
                          onValueChange={(value) =>
                            edit({
                              ...draft,
                              members: draft.members.map((m) =>
                                m.id === member.id
                                  ? {
                                      ...m,
                                      free:
                                        value === "auto"
                                          ? null
                                          : value === "free",
                                    }
                                  : m,
                              ),
                            })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("intentRouting.memberFree")}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="auto">
                              {t("intentRouting.freeAuto")}
                            </SelectItem>
                            <SelectItem value="free">
                              {t("intentRouting.freeYes")}
                            </SelectItem>
                            <SelectItem value="paid">
                              {t("intentRouting.freeNo")}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </label>
                    )}
                    <div className="flex flex-wrap gap-2 sm:col-span-2">
                      <label className="flex items-center gap-2 text-sm">
                        <input
                          type="checkbox"
                          disabled={draft.routing === "manual"}
                          checked={draft.fallbackMemberIds.includes(member.id)}
                          onChange={(e) =>
                            edit({
                              ...draft,
                              fallbackMemberIds: e.target.checked
                                ? [...draft.fallbackMemberIds, member.id]
                                : draft.fallbackMemberIds.filter(
                                    (id) => id !== member.id,
                                  ),
                            })
                          }
                        />
                        {t("intentRouting.fallback")}
                        {draft.fallbackMemberIds.includes(member.id)
                          ? ` #${draft.fallbackMemberIds.indexOf(member.id) + 1}`
                          : ""}
                      </label>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        disabled={pending}
                        onClick={() => removeMember(member.id)}
                      >
                        {t("common.delete")}
                      </Button>
                    </div>
                  </div>
                </details>
              </div>
            ))}
            <Button
              type="button"
              variant="outline"
              disabled={draft.members.length >= 256 && !pickingModels}
              aria-expanded={pickingModels}
              onClick={() => setPickingModels(!pickingModels)}
            >
              {t("intentRouting.addMember")}
            </Button>
            {pickingModels && (
              <div className="space-y-3 border-y py-4">
                <SharedModelPicker
                  multiple
                  models={
                    providers.data?.flatMap((provider) =>
                      (provider.models ?? []).map((model) => ({
                        ...model,
                        id: JSON.stringify([provider.id, model.id]),
                        modelId: model.id,
                        group: provider.name,
                        disabled:
                          !provider.taskEligible ||
                          (draft.members.length >= 256 &&
                            !draft.members.some(
                              (m) =>
                                m.providerId === provider.id &&
                                m.modelId === model.id,
                            )),
                      })),
                    ) ?? []
                  }
                  selected={draft.members.map((m) =>
                    JSON.stringify([m.providerId, m.modelId]),
                  )}
                  onPick={(key) => {
                    const [providerId, modelId] = JSON.parse(key) as [
                      string,
                      string,
                    ];
                    const existing = draft.members.find(
                      (m) =>
                        m.providerId === providerId && m.modelId === modelId,
                    );
                    if (existing) {
                      removeMember(existing.id);
                      return;
                    }
                    if (draft.members.length >= 256) return;
                    const member = {
                      id: crypto.randomUUID(),
                      providerId,
                      modelId,
                    };
                    edit({
                      ...draft,
                      members: [...draft.members, member],
                      defaultMemberId: draft.defaultMemberId || member.id,
                      fallbackMemberIds: draft.members.length
                        ? [...draft.fallbackMemberIds, member.id]
                        : [],
                    });
                  }}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setPickingModels(false)}
                >
                  {t("common.close")}
                </Button>
              </div>
            )}
            <div className="rt-patterns">
              <span className="text-sm">
                {t("intentRouting.matchPatterns")}
              </span>
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
                  <Input
                    disabled={pending || (draft.match?.length ?? 0) >= 32}
                    aria-label={t("intentRouting.matchPatterns")}
                    placeholder="openrouter/*:free"
                    value={patternInput}
                    onChange={(event) => setPatternInput(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        addPattern();
                      }
                    }}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={
                      pending ||
                      !patternInput.trim() ||
                      (draft.match?.length ?? 0) >= 32
                    }
                    onClick={addPattern}
                  >
                    {t("intentRouting.addPattern")}
                  </Button>
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
                    matches.isError ? "intentRouting.failed" : "common.loading",
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
                        {maskAccounts
                          ? member.modelId
                          : `${member.providerId}/${member.modelId}`}
                      </code>
                      <small>{t("intentRouting.byPattern")}</small>
                    </div>
                  ))}
                </div>
              )}
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
                    htmlFor={`routing-unlists-${draft.id}`}
                    className="text-sm font-medium"
                  >
                    {t("intentRouting.unlists")}
                  </label>
                  <p className="text-xs text-muted-foreground">
                    {t("intentRouting.unlistsHelp")}
                  </p>
                </div>
                <Switch
                  id={`routing-unlists-${draft.id}`}
                  className="mp-switch"
                  checked={!!draft.unlists}
                  disabled={pending}
                  onCheckedChange={(unlists) => edit({ ...draft, unlists })}
                />
              </div>
              <p className="text-xs text-muted-foreground">
                {t(`intentRouting.routingHint_${draft.routing ?? "ordered"}`)}
              </p>
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
                  <SelectTrigger aria-label={t("intentRouting.defaultTarget")}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>{memberOptions}</SelectContent>
                </Select>
              </label>
              <p className="text-xs text-muted-foreground">
                {t("intentRouting.fallbackHelp")}
              </p>
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
          </details>
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
                          {classifierName
                            ? `${classifierName.name} · ${classifierName.group}`
                            : draft.classifierTarget
                              ? `${draft.classifierTarget.modelId} · ${providers.data?.find((provider) => provider.id === draft.classifierTarget?.providerId)?.name ?? draft.classifierTarget.providerId}`
                              : t("routingPage.chooseClassifier")}
                        </button>
                      </PopoverTrigger>
                      <PopoverContent
                        className="mp-model-popover"
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
                                  id: JSON.stringify([provider.id, model.id]),
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
                                        draft.classifierTarget?.modelId ?? "",
                                    }
                                  : null,
                            })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("intentRouting.classifierProvider")}
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
                                  p.id === draft.classifierTarget?.providerId &&
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
                                  {p.name}
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
                              classificationBudgetMs: Number(e.target.value),
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
                  {providers.data?.find(
                    (p) => p.id === draft.classifierTarget?.providerId,
                  )?.name || t("intentRouting.none")}
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
                    trialPending ? "common.loading" : "intentRouting.runTrial",
                  )}
                </Button>
                {trialResult && (
                  <div role="status" className="space-y-2 text-sm">
                    <p>
                      {t(`intentRouting.trialOutcome.${trialResult.outcome}`)}
                    </p>
                    {trialResult.intentId && (
                      <p>
                        {
                          draft.rules.find((r) => r.id === trialResult.intentId)
                            ?.name
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
                          {providers.data?.find(
                            (p) => p.id === member.providerId,
                          )?.name || member.providerId}{" "}
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
                      {providers.data?.find((p) => p.id === m.providerId)
                        ?.name || m.providerId}{" "}
                      / {m.modelId}
                    </li>
                  ))}
                </ol>
              )}
            </details>
          </div>
          <div className="rt-editor-bar flex gap-2">
            {existingDraft && (
              <Button
                type="button"
                variant="ghost"
                className="rt-remove"
                disabled={pending}
                onClick={() => {
                  setError(null);
                  setDeleting(initialDraft.current);
                }}
              >
                {t("common.delete")}
              </Button>
            )}
            <span className="grow" />
            <Button type="button" variant="outline" onClick={() => edit(null)}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!valid}>
              {t("common.save")}
            </Button>
          </div>
        </div>
      </fieldset>
    </form>
  );

  return (
    <section className="rt-group-panel">
      <div className="rt-group-heading">
        <h3 className="label">{t("intentRouting.title")}</h3>
        <span className="note rt-group-hint">
          {t("routingPage.groupsHint")}
        </span>
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
          {!selectedGroups &&
            groups.data?.groups.some((group) => !group.autoGenerated) && (
              <details className="rt-template-menu">
                <summary className="cursor-pointer text-xs text-muted-foreground">
                  {t("routingPage.templates")}
                </summary>
                <div>
                  {templates.map((template) => (
                    <button
                      key={template.key}
                      type="button"
                      disabled={
                        pending || !!deleting || importOpen || !!template.need
                      }
                      title={
                        templateNeed(template) ||
                        t(`routingPage.${template.key}Hint`)
                      }
                      onClick={(event) => {
                        useTemplate(template);
                        event.currentTarget
                          .closest("details")
                          ?.removeAttribute("open");
                      }}
                    >
                      {t(`routingPage.${template.key}`)}
                    </button>
                  ))}
                </div>
              </details>
            )}
          <Button
            variant="ghost"
            size="sm"
            aria-expanded={importOpen}
            disabled={pending || !!draft || !!deleting || !!selectedGroups}
            onClick={() => {
              setImportOpen(!importOpen);
              setImported(null);
              setError(null);
              setImportError(false);
            }}
          >
            {t("routingPage.import")}
          </Button>
          {(groups.data?.groups.length ?? 0) > 1 && (
            <Button
              variant="ghost"
              size="sm"
              aria-pressed={!!selectedGroups}
              disabled={pending || !!draft || !!deleting || importOpen}
              onClick={() =>
                setSelectedGroups(selectedGroups ? null : new Set())
              }
            >
              {t(selectedGroups ? "common.cancel" : "routingPage.select")}
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            disabled={pending || groups.isFetching || !!deleting}
            onClick={() => void reload()}
          >
            {t("intentRouting.reload")}
          </Button>
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
            onClick={() => requestEditor(blankRoutingGroup())}
          >
            {t("intentRouting.create")}
          </Button>
        </div>
      </div>
      {importOpen && (
        <form
          className="space-y-3 rounded-lg border border-border bg-background p-4"
          onSubmit={(event) => {
            event.preventDefault();
            void importGroup();
          }}
        >
          {imported ? (
            <>
              <div
                role="status"
                aria-label={t("routingPage.importComplete")}
                className="rt-import-result"
              >
                <p>
                  {t("routingPage.imported", { name: imported.group.name })}
                </p>
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
                    {t("routingPage.importRules", {
                      count: imported.omittedRules,
                    })}
                  </p>
                )}
              </div>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => {
                  setImportOpen(false);
                  setImported(null);
                }}
              >
                {t("common.done")}
              </Button>
            </>
          ) : (
            <>
              <p className="text-xs text-muted-foreground">
                {t("routingPage.importHint")}
              </p>
              <label className="block space-y-2 text-xs">
                {t("routingPage.importLabel")}
                <Textarea
                  disabled={pending}
                  value={importText}
                  maxLength={65536}
                  rows={6}
                  onChange={(event) => {
                    setImportText(event.target.value);
                    setImportError(false);
                  }}
                />
              </label>
              {importError && (
                <p role="alert" className="text-sm text-destructive">
                  {t("routingPage.invalidImport")}
                </p>
              )}
              <div className="flex gap-2">
                <Button
                  size="sm"
                  type="submit"
                  disabled={!importText.trim() || pending || !providers.data}
                >
                  {t("routingPage.reviewImport")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  type="button"
                  disabled={pending}
                  onClick={() => {
                    setImportOpen(false);
                    setImportText("");
                  }}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </>
          )}
        </form>
      )}
      <div className="rt-gfound">
        <div className="txt">
          <label htmlFor="intent-routing-auto-create">
            {t("intentRouting.autoCreate")}
          </label>
          <small id="intent-routing-auto-description">
            {t(
              groups.data?.autoCreate
                ? "intentRouting.autoCreateCompact"
                : "intentRouting.autoCreateOff",
            )}
          </small>
          {groups.data?.autoCreate && (
            <AutoGroupProviders
              providers={providers.data ?? []}
              disabled={
                pending ||
                !!draft ||
                !!deleting ||
                providers.isPending ||
                providers.isError
              }
            />
          )}
        </div>
        <Switch
          className="mp-switch"
          id="intent-routing-auto-create"
          aria-describedby="intent-routing-auto-description"
          checked={groups.data?.autoCreate ?? false}
          disabled={
            groups.isPending ||
            groups.isError ||
            pending ||
            !!draft ||
            !!deleting
          }
          onCheckedChange={(enabled) => void toggleAutoCreate(enabled)}
        />
      </div>
      {!draft &&
        !selectedGroups &&
        !importOpen &&
        groups.data &&
        !groups.data.groups.some((group) => !group.autoGenerated) && (
          <div className="rt-template-cards">
            <p className="rt-template-intro">{t("routingPage.templateHint")}</p>
            {templates.map((template) => (
              <div
                key={template.key}
                className={template.need ? "off" : undefined}
              >
                <b>{t(`routingPage.${template.key}`)}</b>
                <p>{t(`routingPage.${template.key}Hint`)}</p>
                <span title={templateNeed(template)}>
                  {template.need
                    ? templateNeed(template)
                    : template.group.members
                        .map((member) => member.modelId)
                        .join(" \u2192 ")}
                </span>
                {!template.need && (
                  <div className="rt-template-actions">
                    <button
                      type="button"
                      className="text primary"
                      disabled={pending || !!deleting}
                      onClick={() => void addTemplate(template)}
                    >
                      {t("routingPage.addTemplate")}
                    </button>
                    <button
                      type="button"
                      className="text"
                      disabled={pending || !!deleting}
                      onClick={() => useTemplate(template)}
                    >
                      {t("routingPage.editTemplate")}
                    </button>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      {selectedGroups && (
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
            disabled={!selectedGroups.size || pending}
            onClick={() => setConfirmBulk(true)}
          >
            {t("routingPage.removeSelected", { count: selectedGroups.size })}
          </Button>
        </div>
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
      {(catalogStatuses.length ? catalogStatuses : (discovery.data ?? [])).map(
        (status) => (
          <p
            key={status.providerId}
            role="status"
            className="text-xs text-muted-foreground"
          >
            {providers.data?.find((p) => p.id === status.providerId)?.name ||
              status.providerId}
            {": "}
            {t(
              `intentRouting.${status.reason ? `catalogReason.${status.reason}` : `catalogStatus.${status.status}`}`,
              {
                count: status.modelCount,
              },
            )}
          </p>
        ),
      )}
      {!!groups.data?.omittedGroups && (
        <p role="status" className="text-sm text-muted-foreground">
          {t("intentRouting.autoOmitted", { count: groups.data.omittedGroups })}
        </p>
      )}
      {!draft && (error || groups.isError || providers.isError) && (
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
      {!draft && groups.data?.groups.length === 0 && (
        <p className="text-sm text-muted-foreground">
          {t(
            groups.data.autoCreate
              ? "intentRouting.emptyAuto"
              : "intentRouting.empty",
          )}
        </p>
      )}
      {(!!draft || !!creatingId || !!groups.data?.groups.length) && (
        <ul
          aria-label={t("intentRouting.title")}
          className="rt-group-list"
          data-routing-list
        >
          {creatingId &&
            !groups.data?.groups.some((group) => group.id === creatingId) && (
              <RoutingGroupSlot
                key={creatingId}
                expanded={draft?.id === creatingId}
                editor={draft?.id === creatingId ? editor : null}
                onCollapseComplete={() => setCreatingId(null)}
              />
            )}
          {visibleGroups?.map((group) => {
            const notReady =
              providers.data &&
              group.members.some(
                (member) =>
                  !group.off?.includes(member.id) &&
                  (group.routing !== "manual" ||
                    member.id ===
                      (group.manualMemberId ?? group.defaultMemberId)) &&
                  !providers.data?.some(
                    (p) => p.id === member.providerId && p.taskEligible,
                  ),
              );
            const summary = memberSummary(group);
            return (
              <RoutingGroupSlot
                key={group.id}
                expanded={draft?.id === group.id}
                editor={draft?.id === group.id ? editor : null}
              >
                <div
                  className="rt-group-row"
                  data-routing-group={group.id}
                  data-dragging={groupDrag.drag?.from === group.id || undefined}
                  data-drag-target={
                    (groupDrag.drag?.to === group.id &&
                      groupDrag.drag.from !== group.id) ||
                    undefined
                  }
                  onContextMenu={(event) => {
                    event.preventDefault();
                    if (!pending && !draft && !deleting) setGroupMenu(group.id);
                  }}
                  onClick={(event) => {
                    if (
                      pending ||
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
                  {selectedGroups && (
                    <input
                      type="checkbox"
                      disabled={pending}
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
                  <DropdownMenu
                    open={groupMenu === group.id}
                    onOpenChange={(open) =>
                      setGroupMenu(open ? group.id : null)
                    }
                  >
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        className="rt-ghandle"
                        disabled={pending || !!draft || !!deleting}
                        aria-label={`${t("routingPage.arrangeGroup")} ${group.name}`}
                        title={t("routingPage.dragHint")}
                        onPointerDown={(event) => {
                          event.preventDefault();
                          setGroupMenu(null);
                          groupDrag.start(event, group.id);
                        }}
                        onClick={(event) => {
                          event.stopPropagation();
                          if (!groupDrag.consumeClick(group.id))
                            setGroupMenu(
                              groupMenu === group.id ? null : group.id,
                            );
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
                                (provider) => provider.id === member.providerId,
                              );
                              return {
                                icon:
                                  globalProviderPresets.find(
                                    (preset) => preset.id === provider?.icon,
                                  )?.icon ?? provider?.icon,
                                name: provider?.name ?? member.providerId,
                              };
                            })}
                          />
                        </span>
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent
                      className="mp-row-menu"
                      align="start"
                      onClick={(event) => event.stopPropagation()}
                    >
                      <DropdownMenuItem
                        disabled={visibleGroups?.[0]?.id === group.id}
                        onSelect={() => void moveGroup(group, -1)}
                      >
                        <ChevronUp size={13} />
                        {t("intentRouting.moveUp")}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={visibleGroups?.at(-1)?.id === group.id}
                        onSelect={() => void moveGroup(group, 1)}
                      >
                        <ChevronDown size={13} />
                        {t("intentRouting.moveDown")}
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        onSelect={() => void duplicateGroup(group)}
                      >
                        <Copy size={13} />
                        {t("routingPage.duplicate")}
                      </DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => void shareGroup(group)}>
                        <Copy size={13} />
                        {t("routingPage.copyShare")}
                      </DropdownMenuItem>
                      <DropdownMenuItem onSelect={() => exportGroup(group)}>
                        <Download size={13} />
                        {t("routingPage.export")}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                  <div className="min-w-0 flex-1">
                    <div className="rt-group-name">
                      <h4 className="truncate font-medium" title={group.name}>
                        {group.name}
                      </h4>
                      <code
                        className="max-w-64 truncate rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground"
                        title={group.modelAlias ?? `vary-auto/${group.id}`}
                      >
                        {group.modelAlias ?? `vary-auto/${group.id}`}
                      </code>
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
                          <Info aria-hidden="true" className="h-3.5 w-3.5" />
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
                          ...(groups.data?.matchedMembers?.[group.id] ?? []),
                        ].map((member) => (
                          <button
                            type="button"
                            key={member.id}
                            role="radio"
                            className="rt-pick"
                            aria-checked={
                              member.id ===
                              (group.manualMemberId ?? group.defaultMemberId)
                            }
                            disabled={
                              pending ||
                              !!draft ||
                              !!deleting ||
                              group.off?.includes(member.id) ||
                              !providers.data?.some(
                                (provider) =>
                                  provider.id === member.providerId &&
                                  provider.taskEligible,
                              )
                            }
                            onClick={() => {
                              if (
                                member.id !==
                                (group.manualMemberId ?? group.defaultMemberId)
                              )
                                void changeManualMember(group, member.id);
                            }}
                          >
                            <span className="rt-pick-dot" aria-hidden />
                            {member.modelId}
                            {member.effort ? ` :${member.effort}` : ""}
                          </button>
                        ))}
                      </div>
                    ) : (
                      <p className="rt-group-summary" title={summary}>
                        {summary}
                      </p>
                    )}
                  </div>
                  {!selectedGroups && (
                    <div className="rt-group-actions">
                      {routingSelect(
                        group,
                        (routing) => void changeRouting(group, routing),
                      )}
                      {!!group.rules.length && (
                        <span className="hidden rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground sm:inline-flex">
                          {t("intentRouting.ruleCount", {
                            count: group.rules.length,
                          })}
                        </span>
                      )}
                      <Switch
                        className="mp-switch"
                        aria-label={`${t("intentRouting.publishGateway")} ${group.name}`}
                        checked={group.enabled}
                        disabled={pending || !!draft || !!deleting}
                        onCheckedChange={(enabled) => {
                          if (group.autoGenerated && !enabled)
                            setDeleting(group);
                          else void publishGroup(group, enabled);
                        }}
                      />
                      {shared === group.id && (
                        <span role="status" className="rt-shared">
                          {t("routingPage.copied")}
                        </span>
                      )}
                      <Button
                        ref={(button) => {
                          if (button) editButtons.current.set(group.id, button);
                          else editButtons.current.delete(group.id);
                        }}
                        variant="ghost"
                        size="sm"
                        aria-expanded={false}
                        disabled={
                          pending ||
                          !!deleting ||
                          providers.isPending ||
                          providers.isError
                        }
                        onClick={() => requestEditor(group)}
                      >
                        {t("common.edit")}
                      </Button>
                    </div>
                  )}
                </div>
              </RoutingGroupSlot>
            );
          })}
          {!draft && visibleGroups?.length === 0 && (
            <li className="px-4 py-6 text-center text-sm text-muted-foreground">
              {t("intentRouting.noMatches")}
            </li>
          )}
        </ul>
      )}
      {!!groups.data?.removedGroups?.length && groups.data.autoCreate && (
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
                  onSelect={() => void restoreGroup(group.id)}
                >
                  {group.name} · {t("intentRouting.restoreGroup")}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}
      {deleting && (
        <div className="space-y-3 rounded-lg border border-destructive/40 p-4">
          <p>{t("intentRouting.deletePrompt", { name: deleting.name })}</p>
          <div className="flex gap-2">
            <Button disabled={pending} onClick={() => void remove()}>
              {t("intentRouting.confirmDelete")}
            </Button>
            <Button
              variant="outline"
              disabled={pending}
              onClick={() => setDeleting(null)}
            >
              {t("common.cancel")}
            </Button>
          </div>
        </div>
      )}

      <ConfirmDialog
        isOpen={confirmBulk}
        title={t("routingPage.removeSelected", {
          count: selectedGroups?.size ?? 0,
        })}
        message={t("routingPage.removeSelectedHint", {
          count: selectedGroups?.size ?? 0,
        })}
        confirmText={t("intentRouting.confirmDelete")}
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
    </section>
  );
}
