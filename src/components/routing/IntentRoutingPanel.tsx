import { Search, Trash2, Info } from "lucide-react";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { RoutingExecution } from "@/components/routing/RoutingExecution";
import { RoutingGroupIcon } from "@/components/routing/RoutingGroupIcon";
import { RoutingGroupSlot } from "@/components/routing/RoutingGroupSlot";
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
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
} from "@/lib/api/intentRouting";

const knownErrors = new Set([
  "invalid",
  "conflict",
  "limit",
  "providerUnavailable",
  "notReady",
  "invalidOutput",
]);

function newGroup(): RoutingGroup {
  const memberId = crypto.randomUUID();
  return {
    id: crypto.randomUUID(),
    name: "",
    revision: 0,
    client: "codex",
    enabled: false,
    members: [{ id: memberId, providerId: "", modelId: "" }],
    defaultMemberId: memberId,
    fallbackMemberIds: [],
    intentRecognitionEnabled: false,
    classifierTarget: null,
    classificationBudgetMs: 2000,
    rules: [],
    routing: "smart",
  };
}

export function IntentRoutingPanel() {
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
  const [filter, setFilter] = useState("");
  const [draft, setDraft] = useState<RoutingGroup | null>(null);
  const [nextDraft, setNextDraft] = useState<RoutingGroup | null>(null);
  const [creatingId, setCreatingId] = useState<string | null>(null);
  const initialDraft = useRef<RoutingGroup | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const createButton = useRef<HTMLButtonElement>(null);
  const editButtons = useRef(new Map<string, HTMLButtonElement>());
  const openedGroup = useRef<string | null>(null);
  const [deleting, setDeleting] = useState<RoutingGroup | null>(null);
  const [pending, setPending] = useState(false);
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
    edit(structuredClone(value));
  }
  function requestEditor(value: RoutingGroup) {
    if (
      draft &&
      JSON.stringify(draft) !== JSON.stringify(initialDraft.current)
    ) {
      setNextDraft(value);
    } else {
      openEditor(value);
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
    ]);
  }
  async function save() {
    if (!draft) return;
    setPending(true);
    setError(null);
    try {
      await intentRoutingApi.save(draft);
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
          {group.members.map((member) => (
            <SelectItem key={member.id} value={member.id}>
              {providers.data?.find((p) => p.id === member.providerId)?.name ||
                member.providerId}{" "}
              · {member.modelId}
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
      if (groups.data?.autoCreate) await refreshCatalogs();
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
    </SelectItem>
  ));
  function memberSummary(group: RoutingGroup) {
    return group.members
      .map((member) => {
        const name =
          providers.data?.find((p) => p.id === member.providerId)?.name ||
          member.providerId;
        return `${name} · ${member.modelId}`;
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
    draft.members.every(
      (m) =>
        m.providerId &&
        m.modelId.trim() &&
        Number.isInteger(m.weight ?? 1) &&
        (m.weight ?? 1) >= 1 &&
        (m.weight ?? 1) <= 1000,
    ) &&
    (draft.routing !== "manual" ||
      draft.members.some(
        (m) => m.id === (draft.manualMemberId ?? draft.defaultMemberId),
      )) &&
    draft.rules.every(
      (r) => r.name.trim() && r.description.trim() && r.targetMemberId,
    ) &&
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
  const editor = draft && (
    <form
      aria-label={initialDraft.current?.name || t("intentRouting.create")}
      className="space-y-5 bg-muted/20 p-3 sm:p-5"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div
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
      <fieldset disabled={pending} className="min-w-0 space-y-5">
        <label className="block space-y-1 text-sm">
          {t("intentRouting.name")}
          <Input
            ref={nameInput}
            required
            maxLength={80}
            value={draft.name}
            onChange={(e) => edit({ ...draft, name: e.target.value })}
          />
        </label>
        <div className="space-y-2">
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
          <p className="text-xs text-muted-foreground">
            {t(`intentRouting.routingHint_${draft.routing ?? "ordered"}`)}
          </p>
        </div>
        <div className="space-y-3">
          <h4 className="font-medium">{t("intentRouting.members")}</h4>
          {draft.members.map((member, index) => (
            <div
              key={member.id}
              className="grid min-w-0 gap-3 rounded-lg border border-border p-3 sm:grid-cols-2 [&>label]:min-w-0"
            >
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
                        m.id === member.id ? { ...m, providerId: value } : m,
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
                                  value === "auto" ? null : value === "free",
                              }
                            : m,
                        ),
                      })
                    }
                  >
                    <SelectTrigger aria-label={t("intentRouting.memberFree")}>
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
                  disabled={
                    draft.members.length === 1 ||
                    draft.defaultMemberId === member.id ||
                    draft.manualMemberId === member.id ||
                    draft.rules.some((r) => r.targetMemberId === member.id)
                  }
                  onClick={() =>
                    edit({
                      ...draft,
                      members: draft.members.filter((_, i) => i !== index),
                      fallbackMemberIds: draft.fallbackMemberIds.filter(
                        (id) => id !== member.id,
                      ),
                    })
                  }
                >
                  {t("common.delete")}
                </Button>
              </div>
            </div>
          ))}
          <Button
            type="button"
            variant="outline"
            disabled={draft.members.length >= 16}
            onClick={() =>
              edit({
                ...draft,
                members: [
                  ...draft.members,
                  { id: crypto.randomUUID(), providerId: "", modelId: "" },
                ],
              })
            }
          >
            {t("intentRouting.addMember")}
          </Button>
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
        </div>
        <fieldset
          disabled={draft.routing === "manual"}
          className="min-w-0 space-y-5 disabled:opacity-60"
        >
          {draft.routing === "manual" && (
            <p className="text-xs text-muted-foreground">
              {t("intentRouting.manualPaused")}
            </p>
          )}
          <div className="space-y-3">
            <h4 className="font-medium">{t("intentRouting.rules")}</h4>
            {draft.rules.map((rule, index) => (
              <div
                key={rule.id}
                className="space-y-3 rounded-lg border border-border p-3"
              >
                <code className="block break-all text-xs">{rule.id}</code>
                <label className="block space-y-1 text-sm">
                  {t("intentRouting.ruleName")}
                  <Input
                    required
                    maxLength={80}
                    value={rule.name}
                    onChange={(e) =>
                      edit({
                        ...draft,
                        rules: draft.rules.map((r) =>
                          r.id === rule.id ? { ...r, name: e.target.value } : r,
                        ),
                      })
                    }
                  />
                </label>
                <label className="block space-y-1 text-sm">
                  {t("intentRouting.description")}
                  <Textarea
                    required
                    maxLength={500}
                    value={rule.description}
                    onChange={(e) =>
                      edit({
                        ...draft,
                        rules: draft.rules.map((r) =>
                          r.id === rule.id
                            ? { ...r, description: e.target.value }
                            : r,
                        ),
                      })
                    }
                  />
                </label>
                <label className="block space-y-1 text-sm">
                  {t("intentRouting.target")}
                  <Select
                    disabled={pending}
                    value={rule.targetMemberId}
                    onValueChange={(value) =>
                      edit({
                        ...draft,
                        rules: draft.rules.map((r) =>
                          r.id === rule.id
                            ? { ...r, targetMemberId: value }
                            : r,
                        ),
                      })
                    }
                  >
                    <SelectTrigger aria-label={t("intentRouting.target")}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>{memberOptions}</SelectContent>
                  </Select>
                </label>
                <div className="flex flex-wrap items-center gap-2">
                  <label className="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      checked={rule.enabled}
                      onChange={(e) =>
                        edit({
                          ...draft,
                          rules: draft.rules.map((r) =>
                            r.id === rule.id
                              ? { ...r, enabled: e.target.checked }
                              : r,
                          ),
                        })
                      }
                    />
                    {t("intentRouting.ruleEnabled")}
                  </label>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={index === 0}
                    onClick={() => {
                      const rules = [...draft.rules];
                      [rules[index - 1], rules[index]] = [
                        rules[index],
                        rules[index - 1],
                      ];
                      edit({ ...draft, rules });
                    }}
                  >
                    {t("intentRouting.moveUp")}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      edit({
                        ...draft,
                        rules: draft.rules.filter((r) => r.id !== rule.id),
                      })
                    }
                  >
                    {t("common.delete")}
                  </Button>
                </div>
              </div>
            ))}
            <Button
              type="button"
              variant="outline"
              disabled={draft.rules.length >= 16}
              onClick={() =>
                edit({
                  ...draft,
                  rules: [
                    ...draft.rules,
                    {
                      id: crypto.randomUUID(),
                      name: "",
                      description: "",
                      enabled: true,
                      targetMemberId: draft.defaultMemberId,
                    },
                  ],
                })
              }
            >
              {t("intentRouting.addRule")}
            </Button>
          </div>
          <div className="space-y-3">
            <h4 className="font-medium">{t("intentRouting.classifier")}</h4>
            <p className="text-xs text-muted-foreground">
              {t("intentRouting.classifierNotice")}
            </p>
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
                    intentRecognitionEnabled: false,
                    classifierTarget:
                      value !== "none"
                        ? {
                            providerId: value.slice("provider:".length),
                            modelId: draft.classifierTarget?.modelId ?? "",
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
                      <SelectItem key={p.id} value={`provider:${p.id}`}>
                        {p.name}
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
          <div className="space-y-3 rounded-lg border border-border p-4">
            <h4 className="font-medium">{t("intentRouting.trialTitle")}</h4>
            <p className="text-xs text-muted-foreground">
              {t("intentRouting.trialNotice")}
            </p>
            <p className="text-xs text-muted-foreground">
              {providers.data?.find(
                (p) => p.id === draft.classifierTarget?.providerId,
              )?.name || t("intentRouting.none")}
              {draft.classifierTarget && ` / ${draft.classifierTarget.modelId}`}
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
              {t(trialPending ? "common.loading" : "intentRouting.runTrial")}
            </Button>
            {trialResult && (
              <div role="status" className="space-y-2 text-sm">
                <p>{t(`intentRouting.trialOutcome.${trialResult.outcome}`)}</p>
                {trialResult.intentId && (
                  <p>
                    {
                      draft.rules.find((r) => r.id === trialResult.intentId)
                        ?.name
                    }
                  </p>
                )}
                <ol aria-label={t("intentRouting.plan")} className="space-y-1">
                  {trialResult.plan.candidates.map((member, index) => (
                    <li key={member.id} className="break-all">
                      {index + 1}.{" "}
                      {providers.data?.find((p) => p.id === member.providerId)
                        ?.name || member.providerId}{" "}
                      / {member.modelId}
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </div>
        </fieldset>
        <div className="space-y-3 rounded-lg border border-border p-4">
          <h4 className="font-medium">{t("intentRouting.offlineTitle")}</h4>
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
                  {providers.data?.find((p) => p.id === m.providerId)?.name ||
                    m.providerId}{" "}
                  / {m.modelId}
                </li>
              ))}
            </ol>
          )}
        </div>
        {valid && (
          <RoutingExecution key={JSON.stringify(draft)} group={draft} />
        )}
        <div className="flex gap-2">
          <Button type="submit" disabled={!valid}>
            {t("common.save")}
          </Button>
          <Button type="button" variant="outline" onClick={() => edit(null)}>
            {t("common.cancel")}
          </Button>
        </div>
      </fieldset>
    </form>
  );

  return (
    <section className="space-y-4 rounded-xl glass-card p-5 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 className="font-semibold">{t("intentRouting.title")}</h3>
        <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto">
          <div className="relative min-w-0 flex-1 sm:w-64">
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
          {groups.data?.autoCreate && (
            <Button
              variant="ghost"
              size="sm"
              disabled={pending || groups.isFetching || !!deleting}
              onClick={() => void reload()}
            >
              {t("intentRouting.reload")}
            </Button>
          )}
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
            onClick={() => requestEditor(newGroup())}
          >
            {t("intentRouting.create")}
          </Button>
        </div>
      </div>
      <div className="flex items-center justify-between gap-5 py-2">
        <div className="space-y-1">
          <label
            htmlFor="intent-routing-auto-create"
            className="text-sm font-medium"
          >
            {t("intentRouting.autoCreate")}
          </label>
          <p
            id="intent-routing-auto-description"
            className="text-xs text-muted-foreground"
          >
            {t("intentRouting.autoCreateCompact")}
          </p>
        </div>
        <Switch
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
      {groups.data?.autoCreate &&
        catalogStatuses.map((status) => (
          <p
            key={status.providerId}
            role="status"
            className="text-xs text-muted-foreground"
          >
            {providers.data?.find((p) => p.id === status.providerId)?.name ||
              status.providerId}
            {": "}
            {t(`intentRouting.catalogStatus.${status.status}`, {
              count: status.modelCount,
            })}
          </p>
        ))}
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
          {t("intentRouting.empty")}
        </p>
      )}
      {(!!draft || !!creatingId || !!groups.data?.groups.length) && (
        <ul
          aria-label={t("intentRouting.title")}
          className="overflow-hidden rounded-xl border border-border bg-background divide-y divide-border"
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
                  className="flex cursor-pointer items-center gap-3 px-3 py-3 sm:gap-4 sm:px-4"
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
                    requestEditor(group);
                  }}
                >
                  <RoutingGroupIcon client={group.client} />
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
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
                    <p
                      className="mt-1 truncate text-xs text-muted-foreground sm:text-sm"
                      title={summary}
                    >
                      {summary}
                    </p>
                  </div>
                  <div className="flex max-w-full shrink-0 flex-wrap items-center justify-end gap-1 sm:gap-2">
                    {routingSelect(
                      group,
                      (routing) => void changeRouting(group, routing),
                    )}
                    {group.routing === "manual" &&
                      manualSelect(
                        group,
                        (id) => void changeManualMember(group, id),
                      )}
                    {!!group.rules.length && (
                      <span className="hidden rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground sm:inline-flex">
                        {t("intentRouting.ruleCount", {
                          count: group.rules.length,
                        })}
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
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={t("common.delete")}
                      title={t("common.delete")}
                      disabled={pending || !!draft || !!deleting}
                      onClick={() => {
                        setError(null);
                        setDeleting(group);
                      }}
                    >
                      <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
                    </Button>
                  </div>
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
