import { Fragment, useId, useRef, useState } from "react";
import {
  useMutation,
  useMutationState,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useAgentFeedback } from "@/lib/agentFeedback";
import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  CheckCircle2,
  Gift,
  ChevronRight,
  ExternalLink,
  RefreshCw,
  ChevronDown,
  PackageCheck,
  PackagePlus,
  X,
} from "lucide-react";
import {
  agentConnectionsApi,
  type CodexProxyIssue,
} from "@/lib/api/agentConnections";
import { CodexProxyWarning } from "./CodexProxyWarning";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import { AgentModelVisibility } from "./AgentModelVisibility";
import { SortableAgentRow } from "./SortableAgentRow";
import { WorkspaceDragOverlay } from "@/components/WorkspaceDragOverlay";
import { restrictWorkspaceSort } from "@/lib/workspaceSorting";
import { WorkBuddyCheckinPanel } from "@/components/WorkBuddyCheckinPanel";
import { DoubaoWorkCheckinPanel } from "@/components/DoubaoWorkCheckinPanel";
import { AgentRewardsPanel } from "@/components/AgentRewardsPanel";
import { agentRewardsApi, hasAgentRewards } from "@/lib/api/agent-rewards";
import { workbuddyCheckinApi } from "@/lib/api/workbuddy-checkin";
import { doubaoCheckinApi } from "@/lib/api/doubao-checkin";
import { intentRoutingApi } from "@/lib/api/intentRouting";
import { proxyApi } from "@/lib/api/proxy";
import { settingsApi } from "@/lib/api/settings";
import { isMac, isWindows } from "@/lib/platform";
import { useAgentUpdates } from "@/hooks/useAgentUpdates";
import { useAgentInstallations } from "@/hooks/useAgentInstallations";
import {
  useWorkspaceEditing,
  type EditingState,
} from "@/hooks/useWorkspaceEditing";
import {
  AgentInstallButton,
  AgentInstallDialog,
  AgentUpdatesBar,
  AgentVersionTag,
} from "./AgentUpdates";
import { APP_ICON_NAME, APP_IDS } from "@/config/appConfig";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { SearchInput } from "@/components/ui/search-input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AppDialog } from "@/components/ui/app-dialog";
import type { AppId } from "@/lib/api";
import { Popover, PopoverTrigger } from "@/components/ui/popover";
import { SearchEmptyState } from "@/components/ui/search-empty-state";
import "./agent-directory.css";

const supported: readonly AppId[] = APP_IDS;
const orderStorageKey = "vary-agent-order";

function readAgentOrder(): AppId[] {
  try {
    const saved: unknown = JSON.parse(
      localStorage.getItem(orderStorageKey) ?? "null",
    );
    if (Array.isArray(saved)) {
      const known = saved.filter((app): app is AppId => APP_IDS.includes(app));
      return [...new Set([...known, ...APP_IDS])];
    }
  } catch {
    // Unavailable storage and invalid saved values use the default order.
  }
  return [...APP_IDS];
}

type ConnectionOperation =
  | "connecting"
  | "disconnecting"
  | "reapplying"
  | "restarting"
  | "visibility";
const nativePicker = (app: AppId | null) => app === "doubaowork";
const modelControlApps = ["workbuddy", "workbuddy-ai", "qoder"] as const;
const needsModelControl = (app: AppId | null) =>
  modelControlApps.some((known) => known === app);
const isTrae = (app: AppId | null) => app === "trae" || app === "trae-intl";
const websites: Record<AppId, string> = {
  t3code: "https://t3.codes/",
  "claude-desktop": "https://claude.ai/download",
  claude: "https://code.claude.com/docs/en/overview",
  pi: "https://pi.dev/",
  workbuddy: "https://www.workbuddy.cn/",
  "workbuddy-ai": "https://www.workbuddy.ai/",
  codex: "https://developers.openai.com/codex/",
  opencode: "https://opencode.ai/",
  gemini: "https://geminicli.com/",
  grokbuild: "https://grok.com/",
  openclaw: "https://openclaw.ai/",
  hermes: "https://github.com/NousResearch/hermes-agent",
  qoder: "https://qoder.com/",
  doubaowork: "https://www.doubao.com/work",
  trae: "https://www.trae.cn/",
  "trae-intl": "https://www.trae.ai/",
  kimi: "https://www.kimi.com/code",
  qwen: "https://qwenlm.github.io/qwen-code-docs/",
  zcode: "https://zcode.z.ai/",
  "deepseek-harness": "https://www.deepseek.com/harness/",
};
export function AgentConnectionsPage({
  onAddProvider,
  onEditingChange,
  compact = false,
}: {
  onAddProvider?: () => void;
  onEditingChange?: (state: EditingState) => void;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const pageId = useId();
  const [search, setSearch] = useState("");
  const missingTab = useRef<HTMLButtonElement>(null);
  const [installationTab, setInstallationTab] = useState("installed");
  const name = (app: AppId) =>
    app === "claude" ? "Claude Code" : t(`apps.${app}`);
  const client = useQueryClient();
  const connections = useQueries({
    queries: APP_IDS.map((app) => ({
      queryKey: ["agent-connections", app],
      queryFn: () => agentConnectionsApi.get(app),
      refetchInterval: 5000,
    })),
  });
  const states = {
    data: connections.flatMap((query) => (query.data ? [query.data] : [])),
    isError: connections.some((query) => query.isError),
    isPending: connections.some((query) => query.isPending),
  };
  const installations = useAgentInstallations();
  const [rescanning, setRescanning] = useState(false);
  const rescanBusy = rescanning || installations.isFetching;
  const workbuddyCheckin = useQuery({
    queryKey: ["workbuddyCheckin", "accounts"],
    queryFn: workbuddyCheckinApi.accounts,
    enabled: !!installations.data?.workbuddy,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const doubaoCheckin = useQuery({
    queryKey: ["doubaoCheckin"],
    queryFn: doubaoCheckinApi.accounts,
    enabled: !!installations.data?.doubaowork,
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
  const [doubaoAccountId, setDoubaoAccountId] = useState<string | null>(null);
  const doubaoAccount =
    doubaoCheckin.data?.accounts.find(
      (account) =>
        account.id ===
        (doubaoAccountId ??
          doubaoCheckin.data?.accounts.find((a) => a.current)?.id),
    ) ??
    (!doubaoCheckin.data?.connected
      ? doubaoCheckin.data?.accounts[0]
      : undefined);
  const rewardApps = APP_IDS.filter(hasAgentRewards);
  const rewards = useQueries({
    queries: rewardApps.map((app) => ({
      queryKey: ["agentRewards", app],
      queryFn: () => agentRewardsApi.status(app),
      enabled: !!installations.data?.[app],
      staleTime: 60_000,
      refetchInterval: 60_000,
    })),
  });
  const checkinLabel = (app: AppId) => {
    const autoEnabled =
      app === "workbuddy"
        ? (workbuddyCheckin.data?.nativeStatus?.autoConfigured ??
          workbuddyCheckin.data?.accounts.some(
            (account) => account.status.autoEnabled,
          ))
        : app === "doubaowork"
          ? (doubaoAccount?.autoEnabled ??
            doubaoCheckin.data?.autoConfigured ??
            (doubaoCheckin.data ? false : undefined))
          : hasAgentRewards(app)
            ? (rewards[rewardApps.indexOf(app)]?.data?.autoConfigured ??
              rewards[rewardApps.indexOf(app)]?.data?.autoEnabled)
            : undefined;
    return t(
      autoEnabled === undefined
        ? "agentRewards.title"
        : autoEnabled
          ? "agentRewards.autoLabel"
          : "agentRewards.manualLabel",
    );
  };
  const catalog = useQuery({
    queryKey: ["agent-model-catalog"],
    queryFn: agentConnectionsApi.models,
  });
  const savedRoutingGroups = useQuery({
    queryKey: ["intent-routing-groups"],
    queryFn: intentRoutingApi.list,
    enabled:
      !catalog.isPending &&
      states.data.some(
        ({ enabled, model }) =>
          enabled &&
          model &&
          (model.startsWith("vary-auto/") || model.startsWith("auto/")) &&
          !catalog.data?.some(({ id }) => id === model),
      ),
  });
  const desktopVisibility = useQuery({
    queryKey: ["agent-model-visibility", "claude-desktop"],
    queryFn: () => agentConnectionsApi.modelVisibility("claude-desktop"),
    enabled:
      states.data?.some((state) => state.app === "claude-desktop") === true,
  });
  const modelControls = useQueries({
    queries: modelControlApps.map((app) => ({
      queryKey: ["agent-model-control", app],
      queryFn: () => agentConnectionsApi.modelControl(app),
      refetchInterval: 3000,
    })),
  });
  const nativeModels = useQuery({
    queryKey: ["agent-native-models"],
    queryFn: agentConnectionsApi.nativeModels,
    staleTime: 30_000,
    refetchInterval: 30_000,
  });
  const gateway = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: proxyApi.getProxyStatus,
    refetchInterval: 5000,
  });
  const [codexProxyIssue, setCodexProxyIssue] =
    useState<CodexProxyIssue | null>(null);
  const codexConnection = connections[APP_IDS.indexOf("codex")];
  const codexProxyDiagnosticEnabled =
    !!codexConnection.data?.enabled &&
    !!codexConnection.data.configured &&
    !codexConnection.data.recoveryPending &&
    !codexConnection.data.configurationIssue &&
    !codexConnection.isError &&
    !codexConnection.isPending &&
    !gateway.isError &&
    !gateway.isPending &&
    !!gateway.data?.running;
  const [setup, setSetup] = useState<AppId | null>(null);
  const [selected, setSelected] = useState("");
  const [operations, setOperations] = useState<
    Partial<Record<AppId, ConnectionOperation>>
  >({});
  const operationLocks = useRef(new Set<AppId>());
  const busy = Object.keys(operations).length > 0;
  const restartFeedback = useAgentFeedback();
  const [errors, setErrors] = useState<Partial<Record<AppId, string>>>({});
  function setError(app: AppId, error: string | null) {
    if (error === null) restartFeedback.clear(app);
    setErrors((current) => ({ ...current, [app]: error ?? undefined }));
  }
  function errorMessage(app: AppId | null) {
    const error = app ? errors[app] : undefined;
    return error ? (
      <p role="alert" className="text-sm text-destructive break-words">
        {t(error)}
      </p>
    ) : null;
  }
  const moreTools = useRef<HTMLDetailsElement>(null);
  const emptyTitleId = useId();
  const [expanded, setExpanded] = useState<AppId | null>(null);
  const [checkinApp, setCheckinApp] = useState<AppId | null>(null);
  const [openedCheckins, setOpenedCheckins] = useState<AppId[]>([]);
  const [appOrder, setAppOrder] = useState(readAgentOrder);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );
  const [picking, setPicking] = useState<AppId | null>(null);
  const [managingDesktopModels, setManagingDesktopModels] = useState(false);
  const [browsing, setBrowsing] = useState<AppId | null>(null);
  const [disconnecting, setDisconnecting] = useState<AppId | null>(null);
  const [restarting, setRestarting] = useState<AppId | null>(null);
  const [modelControlRequest, setModelControlRequest] = useState<{
    app: AppId;
    model: string;
  } | null>(null);
  const [authorizingModel, setAuthorizingModel] = useState(false);
  const updates = useAgentUpdates(
    APP_IDS.filter((app) => installations.data?.[app]),
  );
  // Mutation-cache state survives navigation while native operations continue.
  const pendingDisconnects = useMutationState({
    filters: {
      mutationKey: ["agent-disconnect"],
      exact: true,
      status: "pending",
    },
    select: (mutation) => mutation.state.variables as AppId,
  });
  const pendingRestarts = useMutationState({
    filters: { mutationKey: ["agent-restart"], exact: true, status: "pending" },
    select: (mutation) => mutation.state.variables as AppId,
  });
  const rowOperation = (app: AppId): ConnectionOperation | undefined =>
    pendingDisconnects.includes(app)
      ? "disconnecting"
      : pendingRestarts.includes(app)
        ? "restarting"
        : operations[app];
  const rowBusy = (app: AppId | null) =>
    app !== null && (!!rowOperation(app) || updates.isBusy(app));
  useWorkspaceEditing(false, !!busy || authorizingModel, onEditingChange);
  function beginOperation(app: AppId, operation: ConnectionOperation) {
    if (rowBusy(app) || operationLocks.current.has(app)) return false;
    operationLocks.current.add(app);
    setOperations((current) => ({ ...current, [app]: operation }));
    return true;
  }
  function endOperation(app: AppId) {
    operationLocks.current.delete(app);
    setOperations((current) => {
      const next = { ...current };
      delete next[app];
      return next;
    });
  }
  async function rescanInstallations() {
    if (rescanBusy) return;
    setRescanning(true);
    try {
      await Promise.all(
        [
          "agent-installations",
          "agent-cli-version",
          "agent-native-models",
          "agent-desktop-versions",
        ].map((key) => client.invalidateQueries({ queryKey: [key] })),
      );
      toast.success(t("agentConnections.rescanSuccess"));
    } catch {
      toast.error(t("agentConnections.rescanFailed"));
    } finally {
      setRescanning(false);
    }
  }
  function browseTools() {
    if (!compact) {
      setInstallationTab("notInstalled");
      missingTab.current?.focus();
      return;
    }
    const details = moreTools.current;
    if (!details) return;
    details.open = true;
    details.querySelector("summary")?.focus();
  }
  function refreshConnectionViews(reapply = false, applied?: AppId) {
    // Configuration writes are complete; optional observations must not hold a row locked.
    void Promise.allSettled(
      [
        "agent-connections",
        "agent-native-models",
        "agent-model-control",
        "proxyStatus",
        "intent-routing-client",
        ...(reapply ? ["agent-model-catalog"] : []),
      ].map((key) => client.invalidateQueries({ queryKey: [key] })),
    );
    if (applied && applied !== "doubaowork") {
      void connections[APP_IDS.indexOf(applied)]
        .refetch({ cancelRefetch: false })
        .then(({ data, isSuccess }) => {
          if (
            isSuccess &&
            data?.app === applied &&
            data.enabled &&
            data.configured &&
            !data.recoveryPending &&
            data.desktopState === "restart_required"
          ) {
            setRestarting((current) => current ?? applied);
          }
        });
    }
  }
  async function reapply(app: AppId) {
    if (!beginOperation(app, "reapplying")) return;
    setError(app, null);
    let applied = false;
    try {
      await agentConnectionsApi.reapply(app);
      applied = true;
    } catch (cause) {
      setError(
        app,
        typeof cause === "string" &&
          ["agentConnections.noModels", "agentConnections.conflict"].includes(
            cause,
          )
          ? cause
          : "agentConnections.failed",
      );
    } finally {
      refreshConnectionViews(true, applied ? app : undefined);
      endOperation(app);
    }
  }
  async function apply(
    app: AppId,
    enabled: boolean,
    model = selected || catalog.data?.[0]?.id || null,
  ) {
    if (!beginOperation(app, enabled ? "connecting" : "disconnecting")) return;
    setError(app, null);
    let applied = false;
    try {
      await agentConnectionsApi.set(app, enabled, enabled ? model : null);
      applied = enabled;
      if (enabled) {
        toast.success(
          `${name(app)}: ${t(
            needsModelControl(app) || app === "claude-desktop" || isTrae(app)
              ? "agentConnections.restartHint"
              : "agentConnections.sessionHint",
          )}`,
          { closeButton: true, duration: 8000 },
        );
      }
      setSetup((current) => (current === app ? null : current));
      setPicking((current) => (current === app ? null : current));
      setDisconnecting((current) => (current === app ? null : current));
      return true;
    } catch (cause) {
      setError(
        app,
        enabled &&
          typeof cause === "string" &&
          ["agentConnections.conflict", "agentConnections.noModels"].includes(
            cause,
          )
          ? cause
          : "agentConnections.failed",
      );
      return false;
    } finally {
      refreshConnectionViews(false, applied ? app : undefined);
      endOperation(app);
    }
  }
  async function pickModel(app: AppId, model: string) {
    if (needsModelControl(app)) {
      const control =
        modelControls[modelControlApps.findIndex((known) => known === app)];
      if (control.isPending || control.isError) {
        setError(app, "agentConnections.modelControlUnavailable");
        return;
      }
      if (!control.data?.authorized) {
        setPicking(null);
        setModelControlRequest({ app, model });
        return;
      }
      await apply(app, true, model);
      return;
    }
    await apply(app, true, model);
  }
  async function authorizeModel() {
    if (!modelControlRequest || authorizingModel) return;
    const { app, model } = modelControlRequest;
    setAuthorizingModel(true);
    setError(app, null);
    try {
      await agentConnectionsApi.authorizeModelControl(app, true);
      if (await apply(app, true, model)) {
        setModelControlRequest(null);
      }
    } catch {
      setError(app, "agentConnections.modelControlUnavailable");
    } finally {
      setAuthorizingModel(false);
      refreshConnectionViews();
    }
  }
  const disconnectMutation = useMutation({
    mutationKey: ["agent-disconnect"],
    mutationFn: (app: AppId) => agentConnectionsApi.set(app, false, null),
    retry: false,
    onError: (_cause, app) => {
      setError(app, "agentConnections.failed");
      toast.error(`${name(app)}: ${t("agentConnections.failed")}`);
    },
    onSettled: (_data, _error, app) => {
      refreshConnectionViews();
      operationLocks.current.delete(app);
    },
  });
  function disconnect(app: AppId) {
    if (rowBusy(app) || operationLocks.current.has(app)) return;
    operationLocks.current.add(app);
    setError(app, null);
    setDisconnecting(null);
    disconnectMutation.mutate(app);
  }
  const restartMutation = useMutation({
    mutationKey: ["agent-restart"],
    mutationFn: (app: AppId) => agentConnectionsApi.restart(app, true),
    retry: false,
    onError: (cause: unknown, app) => {
      const allowed = [
        "agentConnections.restartBlocked",
        "agentConnections.restartUnsupported",
        "agentConnections.restartQuitFailed",
        "agentConnections.restartOpenFailed",
      ];
      const message =
        typeof cause === "string" && allowed.includes(cause)
          ? cause
          : "agentConnections.restartFailed";
      restartFeedback.show(app, t(message, { app: name(app) }));
    },
    onSettled: (_data, _error, app) => {
      refreshConnectionViews();
      operationLocks.current.delete(app);
    },
  });
  function restart(app: AppId) {
    if (rowBusy(app) || operationLocks.current.has(app)) return;
    operationLocks.current.add(app);
    setError(app, null);
    setRestarting(null);
    restartMutation.mutate(app);
  }
  async function launchDoubao(enabled: boolean) {
    if (!beginOperation("doubaowork", "connecting")) return;
    setError("doubaowork", null);
    try {
      if (!enabled) await agentConnectionsApi.set("doubaowork", true, null);
      await proxyApi.launchDoubaoWorkForAudit();
    } catch (cause) {
      const allowed = [
        "homeAudit.doubaoQuitFirst",
        "homeAudit.doubaoNotInstalled",
        "homeAudit.doubaoPlatform",
      ];
      setError(
        "doubaowork",
        typeof cause === "string" && allowed.includes(cause)
          ? cause
          : "homeAudit.doubaoLaunchFailed",
      );
    } finally {
      refreshConnectionViews();
      endOperation("doubaowork");
    }
  }
  function openSetup(app: AppId) {
    setSetup(app);
    setError(app, null);
    setSelected(
      states.data?.find((state) => state.app === app)?.model ??
        catalog.data?.[0]?.id ??
        "",
    );
    void client.invalidateQueries({ queryKey: ["agent-model-catalog"] });
  }
  function connect(app: AppId) {
    const state = states.data?.find((entry) => entry.app === app);
    if (
      !state?.enabled &&
      state?.recoveryPending &&
      state.recoveryReason === "configuration_changed"
    ) {
      // Explicitly reconnect against the externally edited baseline. Polling
      // must never reapply our old receipt or change another manager's config.
      void reapply(app);
      return;
    }
    if (app === "doubaowork") {
      void apply(app, true, null);
      return;
    }
    if (!catalog.data?.length || catalog.isError) return openSetup(app);
    const previous = states.data?.find((state) => state.app === app)?.model;
    const model = catalog.data.some((entry) => entry.id === previous)
      ? previous
      : catalog.data[0].id;
    void apply(app, true, model);
  }
  const hasSavedConnection = (app: AppId) =>
    states.data.some(
      (state) =>
        state.app === app &&
        (state.enabled ||
          state.recoveryPending ||
          state.desktopState === "restart_required" ||
          state.reloadState === "stale_catalog" ||
          state.reloadState === "failed"),
    );
  const shown = appOrder.filter(
    (app) =>
      installations.data?.[app] !== false &&
      (installations.data?.[app] || hasSavedConnection(app)),
  );
  const connectionRank = (app: AppId) => {
    const query = connections[APP_IDS.indexOf(app)];
    return query.data?.enabled ? 2 : query.isPending || query.isError ? 1 : 0;
  };
  if (compact) shown.sort((a, b) => connectionRank(b) - connectionRank(a));
  const other = APP_IDS.filter((app) => !shown.includes(app));
  const searchTerm = search.trim().toLocaleLowerCase();
  const filtered = !compact && searchTerm !== "";
  const matchesName = (app: AppId) =>
    app.includes(searchTerm) ||
    name(app).toLocaleLowerCase().includes(searchTerm);
  const visible = filtered ? shown.filter(matchesName) : shown;
  const available = filtered ? other.filter(matchesName) : other;
  const sortableVisible =
    !compact && !filtered
      ? visible.filter((app) => {
          const query = connections[APP_IDS.indexOf(app)];
          return !query.isPending || query.data !== undefined;
        })
      : [];
  function reorder({ active, over }: DragEndEvent) {
    if (
      compact ||
      filtered ||
      !over ||
      active.id === over.id ||
      !shown.includes(active.id as AppId) ||
      !shown.includes(over.id as AppId)
    )
      return;
    const next = arrayMove(
      appOrder,
      appOrder.indexOf(active.id as AppId),
      appOrder.indexOf(over.id as AppId),
    );
    setAppOrder(next);
    try {
      // Keep undetected tools so a rescan restores their saved positions.
      localStorage.setItem(orderStorageKey, JSON.stringify(next));
    } catch {
      // Sorting remains available for this session when storage is unavailable.
    }
  }
  const announcePosition = (id: string | number, overId: string | number) =>
    t("agentConnections.sorting.position", {
      app: name(id as AppId),
      position: shown.indexOf(overId as AppId) + 1,
      count: shown.length,
    });
  function row(app: AppId) {
    const operation = rowOperation(app);
    const connection = connections[APP_IDS.indexOf(app)];
    const state = connection.data;
    const enabled = !!state?.enabled;
    const orphaned = app === "codex" && state?.recoveryReason === "orphaned";
    const inactiveCodexIntent =
      app === "codex" &&
      state?.enabled === false &&
      state.recoveryReason === "connection_inactive";
    const reloadStatus =
      app !== "codex"
        ? null
        : state?.reloadState === "failed"
          ? enabled
            ? "reloadFailed"
            : "disconnectedReloadFailed"
          : !enabled
            ? null
            : state?.reloadState === "waiting_for_idle"
              ? "waitingForIdle"
              : state?.reloadState === "restarting"
                ? "reloading"
                : state?.reloadState === "stale_catalog"
                  ? "catalogStale"
                  : null;
    const desktopRestart = state?.desktopState === "restart_required";
    const modelControl = needsModelControl(app)
      ? modelControls[modelControlApps.findIndex((known) => known === app)]
          ?.data
      : undefined;
    const codexReloadRestart =
      app === "codex" &&
      !state?.recoveryPending &&
      (state?.reloadState === "stale_catalog" ||
        state?.reloadState === "failed" ||
        state?.reloadState === "waiting_for_idle");
    const canRestart =
      app !== "doubaowork" &&
      !!state &&
      (enabled
        ? state.restartRequired ||
          desktopRestart ||
          codexReloadRestart ||
          !!modelControl?.pending
        : app !== "codex" && desktopRestart);
    const native = nativeModels.data?.find((model) => model.app === app);
    const nativeAvailable = native?.available && !nativeModels.isError;
    const nativeModel =
      app === "opencode"
        ? native?.model?.replace(/^[^/]+\//, "")
        : native?.model;
    const hasNativeModelQuery =
      !nativePicker(app) && !needsModelControl(app) && !isTrae(app);
    const nativeLabel = nativeAvailable
      ? (nativeModel ?? t("agentConnections.nativeDefault"))
      : t(
          nativeModels.isPending && hasNativeModelQuery
            ? "common.loading"
            : "agentConnections.nativeUnavailable",
        );
    const currentModel =
      needsModelControl(app) && (!enabled || !modelControl?.authorized)
        ? t("agentConnections.nativeDefault")
        : enabled
          ? (catalog.data?.find((model) => model.id === state?.model)?.name ??
            savedRoutingGroups.data?.groups.find(
              (group) =>
                (group.modelAlias ?? `vary-auto/${group.id}`) === state?.model,
            )?.name ??
            state?.model ??
            t("agentConnections.pickModel"))
          : nativeLabel;
    const detailsId = `${pageId}-${app}-details`;
    const doubaoLauncher = app === "doubaowork" && !state?.configured && (
      <Button
        size="sm"
        disabled={rowBusy(app) || connection.isPending || connection.isError}
        onClick={() => void launchDoubao(enabled)}
      >
        {t(
          state?.desktopState === "current" ||
            state?.desktopState === "restart_required"
            ? "homeAudit.doubaoRestart"
            : "homeAudit.doubaoLaunch",
        )}
      </Button>
    );
    const status =
      connection.isPending || (enabled && gateway.isPending)
        ? "checking"
        : connection.isError
          ? "unavailable"
          : state?.recoveryPending
            ? state.configurationIssue
              ? state.configurationIssue
              : state.recoveryReason === "bridge_selection_pending"
                ? "bridgeSelectionPending"
                : state.recoveryReason === "bridge_model_rejected"
                  ? "bridgeModelRejected"
                  : state.recoveryReason === "bridge_pending"
                    ? "bridgePending"
                    : state.recoveryReason === "connection_inactive"
                      ? gateway.data?.running
                        ? "inactive"
                        : "gatewayStopped"
                      : state.recoveryReason === "orphaned"
                        ? "orphaned"
                        : "recovery"
            : app === "codex" && codexProxyDiagnosticEnabled && codexProxyIssue
              ? "codexProxyBlocked"
              : reloadStatus
                ? reloadStatus
                : enabled && modelControl?.pending && !desktopRestart
                  ? "modelControlPending"
                  : enabled && canRestart
                    ? "restartRequired"
                    : enabled
                      ? gateway.isError
                        ? "unavailable"
                        : gateway.data?.running
                          ? state?.configured
                            ? app === "doubaowork"
                              ? "auditConnected"
                              : state.desktopState === "restart_required"
                                ? "restartRequired"
                                : "configured"
                            : app === "doubaowork"
                              ? "auditWaiting"
                              : "recovery"
                          : "gatewayStopped"
                      : "off";
    const restartNotice = restartFeedback.messages[app];
    const bypassed = status === "recovery";
    const ready =
      !compact &&
      !operation &&
      (status === "configured" || status === "auditConnected");
    const reapplyButton = state?.recoveryPending &&
      !["bridge_pending", "bridge_selection_pending"].includes(
        state.recoveryReason ?? "",
      ) &&
      !connection.isError && (
        <button
          type="button"
          className={`mp-agent-fix${bypassed ? " mp-agent-reconnect" : ""}`}
          aria-label={t("agentConnections.reapply")}
          title={t(
            isTrae(app)
              ? "agentConnections.bridgeHint"
              : "agentConnections.reapplyHint",
            { app: name(app) },
          )}
          disabled={rowBusy(app) || catalog.isPending}
          onClick={() => void reapply(app)}
        >
          <RefreshCw
            size={11}
            className={
              operation === "reapplying"
                ? "animate-spin motion-reduce:animate-none"
                : ""
            }
          />
          <span>{t("agentConnections.reapply")}</span>
        </button>
      );
    const modelCatalog = isTrae(app) ? (
      <Popover
        open={browsing === app}
        onOpenChange={(open) => setBrowsing(open ? app : null)}
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            className="mp-agent-start"
            disabled={rowBusy(app) || catalog.isPending}
            aria-label={`${t("agentConnections.modelList")} ${name(app)}`}
          >
            <span>
              {catalog.isError
                ? t("agentConnections.failed")
                : catalog.isPending
                  ? t("common.loading")
                  : t("agentConnections.allModels", {
                      count: catalog.data?.length ?? 0,
                    })}
            </span>
            <ChevronDown size={11} />
          </button>
        </PopoverTrigger>
        <ModelPickerPopoverContent
          onDismiss={() => setBrowsing(null)}
          aria-label={t("agentConnections.modelListTitle", {
            agent: name(app),
          })}
        >
          <SharedModelPicker
            models={catalog.isError ? [] : (catalog.data ?? [])}
            selected={catalog.data?.map((model) => model.id) ?? []}
            readOnly
            title={t("agentConnections.modelListTitle", {
              agent: name(app),
            })}
            description={t(
              !enabled
                ? "agentConnections.nativeCatalogDisconnectedHint"
                : isTrae(app) && state?.recoveryReason === "bridge_pending"
                  ? "agentConnections.nativeCatalogPendingHint"
                  : "agentConnections.nativeCatalogHint",
            )}
            error={catalog.isError ? t("agentConnections.failed") : undefined}
          />
        </ModelPickerPopoverContent>
      </Popover>
    ) : null;
    const checkinStatuses =
      app === "workbuddy"
        ? workbuddyCheckin.data?.accounts.length
          ? workbuddyCheckin.data.accounts.map((account) => account.status)
          : workbuddyCheckin.data
            ? [workbuddyCheckin.data.nativeStatus]
            : []
        : app === "doubaowork"
          ? doubaoAccount
            ? [doubaoAccount]
            : []
          : hasAgentRewards(app) && rewards[rewardApps.indexOf(app)]?.data
            ? [rewards[rewardApps.indexOf(app)].data!]
            : [];
    const rewardsQuery = hasAgentRewards(app)
      ? rewards[rewardApps.indexOf(app)]
      : undefined;
    const checkinInactive =
      app === "zcode" &&
      !rewardsQuery?.isError &&
      rewardsQuery?.data?.outcome === "inactive";
    const checkinComplete =
      checkinStatuses.length > 0 &&
      checkinStatuses.every((status) =>
        ["claimed", "already_checked"].includes(
          ("savedOutcome" in status ? status.savedOutcome : null) ??
            status.outcome,
        ),
      );
    const checkinCredit =
      checkinComplete &&
      checkinStatuses.every((status) => status.credit != null)
        ? checkinStatuses.reduce((sum, status) => sum + (status.credit ?? 0), 0)
        : null;
    const checkinTriggerLabel = checkinInactive
      ? t("agentRewards.noActivity")
      : checkinLabel(app);
    const checkinTrigger = (app === "workbuddy" ||
      app === "doubaowork" ||
      hasAgentRewards(app)) && (
      <button
        type="button"
        className="mp-agent-start mp-agent-checkin-trigger"
        data-complete={checkinComplete}
        data-inactive={checkinInactive}
        aria-label={`${checkinTriggerLabel} ${name(app)}`}
        aria-describedby={`${detailsId}-checkin-summary`}
        aria-expanded={checkinApp === app}
        aria-controls={`${detailsId}-checkin`}
        onClick={() => {
          setOpenedCheckins((previous) =>
            previous.includes(app) ? previous : [...previous, app],
          );
          setCheckinApp((previous) => (previous === app ? null : app));
        }}
      >
        {checkinComplete ? (
          <CheckCircle2 size={14} aria-hidden="true" />
        ) : (
          <Gift size={14} aria-hidden="true" />
        )}
        <span id={`${detailsId}-checkin-summary`}>
          {t("dailyCheckin.title")} ·{" "}
          {checkinComplete
            ? t(
                app === "zcode"
                  ? checkinCredit != null
                    ? "dailyCheckin.claimedReward"
                    : "dailyCheckin.claimed"
                  : checkinCredit != null
                    ? "dailyCheckin.todayReward"
                    : "dailyCheckin.todayChecked",
                { credit: checkinCredit },
              )
            : checkinTriggerLabel}
        </span>
        {checkinComplete && (
          <span className="sr-only">{checkinLabel(app)}</span>
        )}
        <ChevronDown size={11} aria-hidden="true" />
      </button>
    );
    const restartButton = app !== "doubaowork" &&
      (operation === "restarting" || (canRestart && !connection.isError)) && (
        <button
          type="button"
          className="mp-agent-fix"
          aria-label={t("agentConnections.restart")}
          title={
            enabled
              ? undefined
              : t("agentConnections.states.restoredRestartRequired")
          }
          aria-busy={operation === "restarting"}
          disabled={rowBusy(app)}
          onClick={() => {
            setError(app, null);
            setRestarting(app);
          }}
        >
          <RefreshCw
            size={11}
            aria-hidden="true"
            className={
              operation === "restarting"
                ? "animate-spin motion-reduce:animate-none"
                : ""
            }
          />
          <span>{t("agentConnections.restart")}</span>
        </button>
      );
    const actions = (
      <>
        {!compact && checkinTrigger}
        {app === "doubaowork" && (
          <span className="mp-agent-note">
            {t("agentConnections.auditOnly")}
          </span>
        )}
        {(updates.installing === app ||
          (!installations.isError && installations.data?.[app] === false)) && (
          <AgentInstallButton
            app={app}
            updates={updates}
            disabled={rowBusy(app)}
            onSelect={() => updates.requestInstall(app, websites[app])}
          />
        )}
        {!bypassed && reapplyButton}
        {app === "codex" && (
          <CodexProxyWarning
            key={`${gateway.data?.address}:${gateway.data?.port}`}
            enabled={codexProxyDiagnosticEnabled}
            busy={rowBusy(app)}
            suspended={
              !!setup ||
              !!disconnecting ||
              !!picking ||
              !!browsing ||
              !!restarting ||
              !!modelControlRequest ||
              !!updates.pending ||
              !!updates.installTarget
            }
            onIssueChange={setCodexProxyIssue}
            onDisconnect={() => setDisconnecting("codex")}
          />
        )}
        {compact && enabled && restartButton}
      </>
    );
    const version = !compact && (
      <AgentVersionTag app={app} updates={updates} disabled={rowBusy(app)} />
    );
    return (
      <SortableAgentRow
        key={app}
        app={app}
        name={name(app)}
        sortable={!compact && !filtered}
        className={`mp-agent-row${compact ? (expanded === app ? " expanded" : "") : " agent-list-row"}`}
        aria-label={t(`apps.${app}`)}
        onClick={
          compact
            ? (event) => {
                if (
                  event.currentTarget.contains(event.target as Node) &&
                  !(event.target as Element).closest(
                    "button, a, input, label, select, textarea, [role='button'], [role='switch'], [role='combobox'], .mp-agent-expanded, .mp-agent-checkin",
                  ) &&
                  window.getSelection()?.isCollapsed !== false
                )
                  setExpanded(expanded === app ? null : app);
              }
            : undefined
        }
      >
        <div className="mp-agent-icon">
          <ProviderIcon
            icon={APP_ICON_NAME[app]}
            name={t(`apps.${app}`)}
            size={compact ? 22 : 32}
          />
        </div>
        <div className="mp-agent-who">
          <div className="mp-agent-name">
            <h2>{name(app)}</h2>
            {!compact && enabled && restartButton}
            {version}
            {actions}
          </div>
          <p
            className={`mp-agent-status ${restartNotice ? "" : bypassed || status === "unavailable" || status === "codexProxyBlocked" ? "bad" : status === "checking" ? "checking" : ready ? "agent-list-ready" : enabled || operation ? "wait" : ""}`}
            role="status"
            aria-atomic="true"
            title={
              restartNotice ??
              t(
                `agentConnections.states.${operation && operation !== "visibility" ? operation : status}`,
                { app: name(app) },
              )
            }
          >
            {restartNotice ? (
              <span className="agent-feedback">{restartNotice}</span>
            ) : (
              <>
                {bypassed ? (
                  <X size={13} strokeWidth={3} aria-hidden="true" />
                ) : (
                  <span className="mp-agent-dot" aria-hidden="true" />
                )}
                <span className="mp-agent-status-text">
                  {t(
                    `agentConnections.states.${operation && operation !== "visibility" ? operation : status}`,
                    { app: name(app) },
                  )}
                </span>
              </>
            )}
            {!enabled && restartButton}
            {bypassed && reapplyButton}
          </p>
          {app === "deepseek-harness" &&
            state?.recoveryReason === "configuration_changed" && (
              <p className="text-xs text-muted-foreground">
                {t("agentConnections.deepseekRecoveryHint")}
              </p>
            )}
        </div>
        {compact && (
          <span className="mp-agent-summary-model" title={currentModel}>
            {currentModel}
          </span>
        )}
        {compact && (
          <Button
            variant="ghost"
            className="mp-agent-next"
            size="sm"
            aria-label={`${t("agentConnections.details")} ${name(app)}`}
            aria-expanded={expanded === app}
            aria-controls={expanded === app ? detailsId : undefined}
            onClick={() => setExpanded(expanded === app ? null : app)}
          >
            <span className="sr-only">
              {t(
                expanded === app
                  ? "agentConnections.collapse"
                  : "agentConnections.details",
              )}
            </span>
            <ChevronRight size={10} />
          </Button>
        )}
        <div
          className={
            compact ? "mp-agent-panel-controls" : "agent-list-controls"
          }
          hidden={compact && expanded !== app}
        >
          {compact && checkinTrigger}
          {app === "doubaowork" && (
            <span
              className="mp-agent-start"
              role="note"
              aria-label={`${t("agentConnections.modelList")} ${name(app)}`}
              title={t("agentConnections.auditOnlyHint")}
            >
              <span>{t("agentConnections.officialModels")}</span>
            </span>
          )}
          {!nativePicker(app) && (
            <Popover
              open={picking === app}
              onOpenChange={(open) => {
                setPicking(open ? app : null);
                setManagingDesktopModels(false);
              }}
            >
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="mp-agent-start"
                  disabled={
                    rowBusy(app) ||
                    connection.isPending ||
                    connection.isError ||
                    (app === "claude-desktop" &&
                      (desktopVisibility.isPending ||
                        desktopVisibility.isError)) ||
                    (!!state?.recoveryPending &&
                      !orphaned &&
                      !inactiveCodexIntent &&
                      !(
                        isTrae(app) &&
                        ["bridge_pending", "bridge_selection_pending"].includes(
                          state.recoveryReason ?? "",
                        )
                      )) ||
                    (enabled &&
                      (catalog.isPending ||
                        catalog.isError ||
                        !catalog.data?.length))
                  }
                  aria-label={`${t("agentConnections.model")} ${name(app)}`}
                  title={
                    needsModelControl(app)
                      ? t(
                          app === "qoder"
                            ? "agentConnections.modelControlHint"
                            : "agentConnections.workbuddyModelControlHint",
                        )
                      : !enabled
                        ? `${nativeLabel}\n${t(
                            nativeAvailable
                              ? native.model
                                ? "agentConnections.nativeModelHint"
                                : "agentConnections.nativeDefaultHint"
                              : "agentConnections.nativeUnavailable",
                          )}`
                        : currentModel
                  }
                >
                  {!enabled && nativeAvailable && (
                    <span className="mp-agent-model-icon" aria-hidden="true">
                      <ProviderIcon
                        icon={APP_ICON_NAME[app]}
                        name={name(app)}
                        size={14}
                      />
                    </span>
                  )}
                  <span>{currentModel}</span>
                  <ChevronDown size={11} />
                </button>
              </PopoverTrigger>
              <ModelPickerPopoverContent
                data-variant="agent"
                onDismiss={() => setPicking(null)}
                aria-label={`${t("agentConnections.model")} ${name(app)}`}
              >
                {app === "claude-desktop" && managingDesktopModels ? (
                  <>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setManagingDesktopModels(false)}
                    >
                      {t("agentConnections.backToModels")}
                    </Button>
                    <AgentModelVisibility
                      inline
                      app={app}
                      name={name(app)}
                      connected={enabled}
                      models={catalog.data ?? []}
                      disabled={
                        (!!operation && operation !== "visibility") ||
                        updates.isBusy(app) ||
                        !!state?.recoveryPending ||
                        connection.isPending ||
                        connection.isError ||
                        catalog.isPending ||
                        catalog.isError
                      }
                      onBusyChange={(value) =>
                        value
                          ? beginOperation(app, "visibility")
                          : endOperation(app)
                      }
                    />
                  </>
                ) : (
                  <>
                    <SharedModelPicker
                      variant="agent"
                      description={
                        orphaned
                          ? t("agentConnections.orphanReconnectHint")
                          : isTrae(app)
                            ? t("agentConnections.traeStartingModelHint")
                            : app === "deepseek-harness"
                              ? t("agentConnections.deepseekStartingModelHint")
                              : needsModelControl(app)
                                ? t(
                                    app === "qoder"
                                      ? "agentConnections.modelControlHint"
                                      : "agentConnections.workbuddyModelControlHint",
                                  )
                                : app === "claude-desktop"
                                  ? t(
                                      "agentConnections.desktopStartingModelHint",
                                    )
                                  : undefined
                      }
                      disabled={rowBusy(app)}
                      models={[
                        ...(!enabled && nativeAvailable
                          ? [
                              {
                                id: `native-current/${app}`,
                                name: nativeLabel,
                                source: "native" as const,
                                logo: APP_ICON_NAME[app],
                                disabled: true,
                              },
                            ]
                          : []),
                        ...(catalog.isError ? [] : (catalog.data ?? [])).filter(
                          (model) =>
                            app !== "claude-desktop" ||
                            (!!desktopVisibility.data &&
                              desktopVisibility.data.ids.includes(model.id) ===
                                desktopVisibility.data.only),
                        ),
                      ]}
                      selected={
                        enabled &&
                        state?.model &&
                        (!needsModelControl(app) || modelControl?.authorized)
                          ? [state.model]
                          : nativeAvailable
                            ? [`native-current/${app}`]
                            : []
                      }
                      onPick={(model) => void pickModel(app, model)}
                      onDismiss={() => setPicking(null)}
                      onDisconnect={
                        enabled || orphaned || inactiveCodexIntent
                          ? () => {
                              setPicking(null);
                              setDisconnecting(app);
                            }
                          : undefined
                      }
                      disconnectLabel={
                        orphaned || inactiveCodexIntent
                          ? t("agentConnections.disconnectOnly")
                          : undefined
                      }
                    />
                    {app === "claude-desktop" && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setManagingDesktopModels(true)}
                      >
                        {t("agentConnections.manageVisibleModels")}
                      </Button>
                    )}
                  </>
                )}
                {(!catalog.data?.length || catalog.isError) && (
                  <p role="note" className="mp-am-menu-note">
                    {t("agentConnections.noModels")}
                  </p>
                )}
              </ModelPickerPopoverContent>
            </Popover>
          )}
          <Switch
            className="mp-switch"
            aria-label={t(`apps.${app}`)}
            checked={enabled}
            aria-busy={connection.isPending || !!operation}
            disabled={
              rowBusy(app) ||
              connection.isPending ||
              connection.isError ||
              (catalog.isPending && !enabled && app !== "doubaowork")
            }
            onCheckedChange={(checked) =>
              checked ? connect(app) : setDisconnecting(app)
            }
          />
        </div>
        {compact && expanded === app && (
          <div
            id={detailsId}
            className="mp-agent-expanded"
            role="region"
            aria-label={`${t("agentConnections.details")} ${name(app)}`}
          >
            {compact && (
              <AgentVersionTag
                app={app}
                updates={updates}
                disabled={rowBusy(app)}
              />
            )}
            {doubaoLauncher && (
              <div className="mp-agent-detail-actions">{doubaoLauncher}</div>
            )}
            {isTrae(app) && (
              <>
                <p className="text-xs text-muted-foreground">
                  {t("agentConnections.bridgeHint", { app: name(app) })}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t("agentConnections.traeHooksPurpose")}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t("agentConnections.traeHooksSetup")}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t(`agentConnections.states.${status}`, { app: name(app) })}
                </p>
                <div className="mp-agent-detail-actions">{modelCatalog}</div>
              </>
            )}
          </div>
        )}
        {!compact && doubaoLauncher && (
          <div className="mp-agent-checkin">
            <div className="mp-agent-detail-actions">{doubaoLauncher}</div>
          </div>
        )}
        {openedCheckins.includes(app) && (
          <div
            id={`${detailsId}-checkin`}
            className="mp-agent-checkin"
            hidden={checkinApp !== app || (compact && expanded !== app)}
          >
            {app === "workbuddy" ? (
              <WorkBuddyCheckinPanel embedded />
            ) : app === "doubaowork" ? (
              <DoubaoWorkCheckinPanel
                embedded
                selectedAccountId={doubaoAccountId}
                onSelectedAccountChange={setDoubaoAccountId}
              />
            ) : hasAgentRewards(app) ? (
              <AgentRewardsPanel app={app} embedded />
            ) : null}
          </div>
        )}
        {errors[app] && (
          <div className="mp-agent-error">
            {errorMessage(app)}
            {errors[app] === "agentConnections.noModels" && onAddProvider && (
              <Button variant="outline" size="sm" onClick={onAddProvider}>
                {t("modelProviders.add")}
              </Button>
            )}
          </div>
        )}
      </SortableAgentRow>
    );
  }
  function rowPlaceholder(app: AppId) {
    return (
      <div
        key={`${app}-loading`}
        className={`mp-agent-row mp-agent-placeholder${compact ? "" : " agent-list-row"}`}
        role="status"
        aria-label={`${name(app)}: ${t("agentConnections.states.checking")}`}
      >
        <span className="mp-agent-placeholder-icon" aria-hidden="true" />
        <span className="mp-agent-placeholder-copy" aria-hidden="true">
          <span className="mp-agent-placeholder-name" />
          <span className="mp-agent-placeholder-status" />
        </span>
        <span className="mp-agent-placeholder-control" aria-hidden="true" />
      </div>
    );
  }
  const rescanButton = (variant: "outline" | "ghost", className?: string) => (
    <Button
      variant={variant}
      size="sm"
      className={className}
      disabled={rescanBusy}
      aria-busy={rescanBusy}
      onClick={rescanInstallations}
    >
      <RefreshCw
        aria-hidden="true"
        className={`mr-2 h-3 w-3${rescanBusy ? " animate-spin" : ""}`}
      />
      {t("agentConnections.rescan")}
    </Button>
  );
  const noMatches = (
    <SearchEmptyState
      title={t("agentConnections.directory.noMatches")}
      hint={t("agentConnections.directory.noMatchesHint")}
    />
  );
  const installedPanel = (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      modifiers={compact ? undefined : [restrictWorkspaceSort]}
      onDragEnd={reorder}
      accessibility={{
        screenReaderInstructions: {
          draggable: t("agentConnections.sorting.hint"),
        },
        announcements: {
          onDragStart: ({ active }) =>
            t("agentConnections.sorting.pickedUp", {
              app: name(active.id as AppId),
            }),
          onDragOver: ({ active, over }) =>
            over ? announcePosition(active.id, over.id) : undefined,
          onDragEnd: ({ active, over }) =>
            over
              ? t("agentConnections.sorting.dropped", {
                  position: announcePosition(active.id, over.id),
                })
              : t("agentConnections.sorting.cancelled"),
          onDragCancel: () => t("agentConnections.sorting.cancelled"),
        },
      }}
    >
      <SortableContext
        items={sortableVisible}
        strategy={verticalListSortingStrategy}
      >
        <div className={compact ? "mp-list" : "agent-directory-rows"}>
          {(installations.isPending || states.isPending) &&
            shown.length === 0 && (
              <div
                role="status"
                aria-label={t("agentConnections.states.checking")}
              >
                {[0, 1, 2].map((index) => (
                  <div
                    key={index}
                    className={`mp-agent-row mp-agent-placeholder${compact ? "" : " agent-list-row"}`}
                    aria-hidden="true"
                  >
                    <span className="mp-agent-placeholder-icon" />
                    <span className="mp-agent-placeholder-copy">
                      <span className="mp-agent-placeholder-name" />
                      <span className="mp-agent-placeholder-status" />
                    </span>
                    <span className="mp-agent-placeholder-control" />
                  </div>
                ))}
              </div>
            )}
          {visible.map((app, index) => {
            const connection = connections[APP_IDS.indexOf(app)];
            return (
              <Fragment key={app}>
                {compact &&
                  connectionRank(app) === 0 &&
                  (index === 0 || connectionRank(shown[index - 1]) > 0) && (
                    <p className="tray-unconfigured">
                      {t("trayPanel.notConfigured")}
                    </p>
                  )}
                {connection.isPending && !connection.data
                  ? rowPlaceholder(app)
                  : row(app)}
              </Fragment>
            );
          })}
          {!installations.isPending &&
            !states.isPending &&
            !states.isError &&
            !installations.isError &&
            !shown.length && (
              <div
                className="mp-agent-empty"
                role="region"
                aria-labelledby={emptyTitleId}
              >
                <span className="mp-agent-empty-icon" aria-hidden="true">
                  <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
                    <rect
                      x="3"
                      y="4"
                      width="18"
                      height="16"
                      rx="3"
                      stroke="currentColor"
                      strokeWidth="1.5"
                    />
                    <path
                      d="m7 9 3 3-3 3m6 0h4"
                      stroke="currentColor"
                      strokeWidth="1.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                </span>
                <div className="mp-agent-empty-copy">
                  <h2 id={emptyTitleId}>{t("agentConnections.emptyTitle")}</h2>
                  <p>{t("agentConnections.empty")}</p>
                </div>
                <div className="mp-agent-empty-actions">
                  <Button size="sm" onClick={browseTools}>
                    {t("agentConnections.browseTools")}
                  </Button>
                  {compact && rescanButton("outline")}
                </div>
              </div>
            )}
        </div>
      </SortableContext>
      {!compact && <WorkspaceDragOverlay />}
    </DndContext>
  );
  const installationList = (
    <div
      className={
        compact ? "mp-agent-install-list divide-y" : "agent-directory-rows"
      }
    >
      {available.map((app) => (
        <div
          key={app}
          role="group"
          aria-label={name(app)}
          className="mp-agent-install-row"
        >
          <div>
            <span className="mp-agent-install-icon">
              <ProviderIcon
                icon={APP_ICON_NAME[app]}
                name={name(app)}
                size={compact ? 22 : 32}
              />
            </span>
            <span className="mp-agent-install-name" title={name(app)}>
              {name(app)}
            </span>
            {app === "codex" &&
              ["failed", "stale_catalog"].includes(
                connections[APP_IDS.indexOf(app)].data?.reloadState ?? "",
              ) && (
                <span className="mp-agent-status bad" role="status">
                  {t(
                    `agentConnections.states.${
                      connections[APP_IDS.indexOf(app)].data?.reloadState ===
                      "failed"
                        ? "reloadFailed"
                        : "catalogStale"
                    }`,
                  )}
                </span>
              )}
            {installations.data?.[app] === true && (
              <AgentVersionTag
                app={app}
                updates={updates}
                disabled={rowBusy(app)}
              />
            )}
          </div>
          <div className="mp-agent-install-actions">
            {(updates.installing === app ||
              (!installations.isError &&
                installations.data?.[app] === false)) && (
              <AgentInstallButton
                app={app}
                updates={updates}
                disabled={rowBusy(app)}
                onSelect={() => updates.requestInstall(app, websites[app])}
              />
            )}
            {supported.includes(app) && installations.data?.[app] === true && (
              <Button
                size="sm"
                variant="outline"
                disabled={!installations.data?.[app] || rowBusy(app)}
                onClick={() => connect(app)}
              >
                {t("agentConnections.connect")}
              </Button>
            )}
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                void settingsApi
                  .openExternal(websites[app])
                  .catch(() => setError(app, "agentConnections.failed"));
              }}
            >
              <ExternalLink className="mr-2 h-3 w-3" />
              {t("agentConnections.website")}
            </Button>
          </div>
          {errorMessage(app)}
        </div>
      ))}
    </div>
  );
  return (
    <section
      className={`vary-agents ${compact ? "tray-agents" : "flex h-full min-h-0 flex-col"}`}
      data-testid="agent-connections-page"
    >
      {!compact && (
        <header className="agent-directory-header">
          <h1 className="mp-page-heading vary-page-title">
            {t("agentConnections.title")}
          </h1>
          {rescanButton("outline")}
        </header>
      )}
      {(states.isError || installations.isError) && (
        <p role="alert" className="mb-4 text-sm text-destructive">
          {t("agentConnections.failed")}
        </p>
      )}
      <AgentUpdatesBar
        updates={updates}
        name={name}
        disabled={!!busy || pendingRestarts.length > 0}
      />
      <AgentInstallDialog updates={updates} name={name} />
      {compact ? (
        <>
          {installedPanel}
          <details ref={moreTools} className="mp-agent-more">
            <summary>
              <span className="mp-agent-more-title">
                <span>
                  {t("agentConnections.moreTools", { count: other.length })}
                </span>
                <ChevronRight className="h-3 w-3" />
              </span>
              <span className="mp-agent-more-hint">
                {t("agentConnections.moreHint")}
              </span>
            </summary>
            <div className="mp-agent-more-body">
              {installationList}
              {rescanButton("ghost", "mt-3")}
            </div>
          </details>
        </>
      ) : (
        <Tabs
          className="flex min-h-0 flex-1 flex-col"
          value={installationTab}
          onValueChange={setInstallationTab}
        >
          <div className="agent-directory-toolbar shrink-0">
            <TabsList
              className="agent-directory-tabs"
              aria-label={t("agentConnections.directory.tabs")}
              slidingIndicator={false}
            >
              <TabsTrigger value="installed">
                <PackageCheck size={16} aria-hidden="true" />
                {t("agentConnections.directory.installed")}
                <span className="agent-directory-count">{shown.length}</span>
              </TabsTrigger>
              <TabsTrigger value="notInstalled" ref={missingTab}>
                <PackagePlus size={16} aria-hidden="true" />
                {t("agentConnections.directory.notInstalled")}
                {installations.data && (
                  <span className="agent-directory-count">{other.length}</span>
                )}
              </TabsTrigger>
            </TabsList>
            <SearchInput
              wrapperClassName="agent-directory-search"
              className="leading-5"
              aria-label={t("agentConnections.directory.search")}
              placeholder={t("agentConnections.directory.search")}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
          <TabsContent
            value="installed"
            className="min-h-0 flex-1 overflow-y-auto overscroll-y-contain focus-visible:ring-inset focus-visible:ring-offset-0"
          >
            {filtered && shown.length > 0 && visible.length === 0
              ? noMatches
              : installedPanel}
          </TabsContent>
          <TabsContent
            value="notInstalled"
            className="min-h-0 flex-1 overflow-y-auto overscroll-y-contain focus-visible:ring-inset focus-visible:ring-offset-0"
          >
            {installations.isPending ? (
              <p role="status">{t("agentConnections.states.checking")}</p>
            ) : installations.data && other.length > 0 ? (
              available.length > 0 ? (
                installationList
              ) : (
                noMatches
              )
            ) : installations.data && !installations.isError ? (
              <div className="mp-agent-empty">
                <div className="mp-agent-empty-copy">
                  <h2 role="status">
                    {t("agentConnections.directory.allInstalled")}
                  </h2>
                </div>
              </div>
            ) : null}
          </TabsContent>
        </Tabs>
      )}
      <AppDialog
        open={!!modelControlRequest}
        pending={authorizingModel}
        onOpenChange={(open) => {
          if (!open && !authorizingModel) setModelControlRequest(null);
        }}
        title={t("agentConnections.modelControlTitle", {
          app: modelControlRequest ? name(modelControlRequest.app) : "",
        })}
        description={t(
          modelControlRequest?.app === "qoder"
            ? "agentConnections.modelControlConsent"
            : "agentConnections.workbuddyModelControlConsent",
        )}
        footer={
          <>
            <Button
              variant="outline"
              disabled={authorizingModel}
              onClick={() => setModelControlRequest(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              className="mp-primary"
              disabled={authorizingModel}
              onClick={() => void authorizeModel()}
            >
              {t("agentConnections.modelControlAllow")}
            </Button>
          </>
        }
      >
        {errorMessage(modelControlRequest?.app ?? null)}
      </AppDialog>
      <AppDialog
        open={!!restarting}
        pending={rowBusy(restarting)}
        onOpenChange={(open) => {
          if (!open && !rowBusy(restarting)) setRestarting(null);
        }}
        title={t("agentConnections.restartTitle", {
          app: restarting ? name(restarting) : "",
        })}
        description={t(
          restarting === "codex"
            ? isWindows()
              ? "agentConnections.codexRestartConfirmHintWindows"
              : "agentConnections.codexRestartConfirmHint"
            : isMac()
              ? "agentConnections.restartConfirmHintMac"
              : isWindows()
                ? "agentConnections.restartConfirmHintWindows"
                : "agentConnections.restartConfirmHint",
        )}
        footer={
          <>
            <Button
              variant="outline"
              disabled={rowBusy(restarting)}
              onClick={() => setRestarting(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              className="mp-primary"
              disabled={rowBusy(restarting)}
              onClick={() => restarting && void restart(restarting)}
            >
              {t("agentConnections.restart")}
            </Button>
          </>
        }
      />
      <AppDialog
        open={!!disconnecting}
        pending={rowBusy(disconnecting)}
        onOpenChange={(open) => {
          if (!open && !rowBusy(disconnecting)) setDisconnecting(null);
        }}
        title={t("agentConnections.disconnectTitle", {
          app: disconnecting ? name(disconnecting) : "",
        })}
        description={t(
          disconnecting === "doubaowork"
            ? "agentConnections.auditDisconnectHint"
            : isTrae(disconnecting)
              ? "agentConnections.bridgeDisconnectHint"
              : "agentConnections.disconnectHint",
          { app: disconnecting ? name(disconnecting) : "" },
        )}
        footer={
          <>
            <Button
              variant="outline"
              disabled={rowBusy(disconnecting)}
              onClick={() => setDisconnecting(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              className="mp-primary"
              disabled={rowBusy(disconnecting)}
              onClick={() => disconnecting && disconnect(disconnecting)}
            >
              {t("agentConnections.disconnectRestore")}
            </Button>
          </>
        }
      >
        {disconnecting === "codex" && (
          <p className="text-sm text-muted-foreground">
            {t("agentConnections.codexDisconnectNote")}
          </p>
        )}
        {(disconnecting === "codex" || disconnecting === "claude") && (
          <p className="text-sm text-muted-foreground">
            {t("agentConnections.disconnectReopenNote", {
              app: name(disconnecting),
            })}
          </p>
        )}
        {errorMessage(disconnecting)}
      </AppDialog>
      <AppDialog
        open={!!setup}
        pending={rowBusy(setup)}
        size="md"
        onOpenChange={(open) => {
          if (!open && !rowBusy(setup)) setSetup(null);
        }}
        title={t("agentConnections.setupTitle", {
          app: setup ? t(`apps.${setup}`) : "",
        })}
        description={t("agentConnections.setupHint")}
        footer={
          <>
            <Button
              variant="outline"
              disabled={rowBusy(setup)}
              onClick={() => setSetup(null)}
            >
              {t("common.cancel")}
            </Button>
            <Button
              className="mp-primary"
              disabled={
                rowBusy(setup) ||
                !selected ||
                catalog.isError ||
                !catalog.data?.some((model) => model.id === selected)
              }
              onClick={() => setup && void apply(setup, true)}
            >
              {t("agentConnections.connect")}
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          {isTrae(setup) && (
            <div className="space-y-2 text-sm text-muted-foreground">
              <p>{t("agentConnections.traeHooksPurpose")}</p>
              <p>{t("agentConnections.traeHooksSetup")}</p>
            </div>
          )}
          {!nativePicker(setup) && !needsModelControl(setup) ? (
            <fieldset disabled={rowBusy(setup)} className="space-y-3">
              <legend className="mb-2 text-sm">
                {t("agentConnections.model")}
              </legend>
              <SharedModelPicker
                description={
                  isTrae(setup)
                    ? t("agentConnections.traeStartingModelHint")
                    : undefined
                }
                models={catalog.data ?? []}
                selected={[selected]}
                onPick={setSelected}
              />
            </fieldset>
          ) : (
            <p className="text-sm text-muted-foreground">
              {t("agentConnections.nativePicker")}
            </p>
          )}
          {(!catalog.data?.length || catalog.isError) && (
            <div>
              <p role="alert" className="text-sm text-[var(--warning)]">
                {t("agentConnections.noModels")}
              </p>
              {onAddProvider && (
                <Button
                  variant="outline"
                  className="mt-3"
                  onClick={() => {
                    setSetup(null);
                    onAddProvider();
                  }}
                >
                  {t("modelProviders.add")}
                </Button>
              )}
            </div>
          )}
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t(
              setup === "codex"
                ? "intentRouting.clientUnifiedConsent"
                : "agentConnections.consent",
            )}
          </p>
          {errorMessage(setup)}
        </div>
      </AppDialog>
    </section>
  );
}
