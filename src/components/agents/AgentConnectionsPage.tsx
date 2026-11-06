import { Fragment, useId, useRef, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
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
  ChevronRight,
  ExternalLink,
  RefreshCw,
  ChevronDown,
} from "lucide-react";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";
import { ModelPickerPopoverContent } from "@/components/models/ModelPickerPopoverContent";
import { AgentModelVisibility } from "./AgentModelVisibility";
import { SortableAgentRow } from "./SortableAgentRow";
import { WorkspaceDragOverlay } from "@/components/WorkspaceDragOverlay";
import { restrictWorkspaceSort } from "@/lib/workspaceSorting";
import { WorkBuddyCheckinPanel } from "@/components/WorkBuddyCheckinPanel";
import { DoubaoWorkCheckinPanel } from "@/components/DoubaoWorkCheckinPanel";
import { AgentRewardsPanel } from "@/components/AgentRewardsPanel";
import { hasAgentRewards } from "@/lib/api/agent-rewards";
import { proxyApi } from "@/lib/api/proxy";
import { settingsApi } from "@/lib/api/settings";
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
import { AppDialog } from "@/components/ui/app-dialog";
import type { AppId } from "@/lib/api";
import { Popover, PopoverTrigger } from "@/components/ui/popover";

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
const nativePicker = (app: AppId | null) =>
  app !== null &&
  ["claude-desktop", "workbuddy", "qoder", "doubaowork"].includes(app);
const websites: Record<AppId, string> = {
  "claude-desktop": "https://claude.ai/download",
  claude: "https://code.claude.com/docs/en/overview",
  pi: "https://pi.dev/",
  workbuddy: "https://www.workbuddy.ai/",
  codex: "https://developers.openai.com/codex/",
  opencode: "https://opencode.ai/",
  gemini: "https://geminicli.com/",
  grokbuild: "https://grok.com/",
  openclaw: "https://openclaw.ai/",
  hermes: "https://github.com/NousResearch/hermes-agent",
  qoder: "https://qoder.com/",
  doubaowork: "https://www.doubao.com/work",
  trae: "https://www.trae.ai/",
  kimi: "https://www.kimi.com/code",
  qwen: "https://qwenlm.github.io/qwen-code-docs/",
  zcode: "https://zcode.ai/",
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
  const catalog = useQuery({
    queryKey: ["agent-model-catalog"],
    queryFn: agentConnectionsApi.models,
  });
  const nativeModels = useQueries({
    queries: APP_IDS.map((app) => ({
      queryKey: ["agent-native-models", app],
      queryFn: () =>
        nativePicker(app) || app === "trae"
          ? Promise.resolve({ app, model: null, available: false })
          : agentConnectionsApi.nativeModel(app),
      refetchInterval: 5000,
    })),
  });
  const gateway = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: proxyApi.getProxyStatus,
    refetchInterval: 5000,
  });
  const [setup, setSetup] = useState<AppId | null>(null);
  const [selected, setSelected] = useState("");
  const [operations, setOperations] = useState<
    Partial<Record<AppId, ConnectionOperation>>
  >({});
  const operationLocks = useRef(new Set<AppId>());
  const busy = Object.keys(operations).length > 0;
  const [error, setError] = useState<string | null>(null);
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
  const [browsing, setBrowsing] = useState<AppId | null>(null);
  const [disconnecting, setDisconnecting] = useState<AppId | null>(null);
  const orphanedDisconnect =
    disconnecting === "codex" &&
    states.data?.some(
      (state) => state.app === "codex" && state.recoveryReason === "orphaned",
    );
  const [restarting, setRestarting] = useState<AppId | null>(null);
  const updates = useAgentUpdates(
    APP_IDS.filter((app) => installations.data?.[app]),
  );
  const rowBusy = (app: AppId | null) =>
    app !== null && (!!operations[app] || updates.isBusy(app));
  useWorkspaceEditing(false, !!busy, onEditingChange);
  function beginOperation(app: AppId, operation: ConnectionOperation) {
    if (updates.isBusy(app) || operationLocks.current.has(app)) return false;
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
  function refreshConnectionViews(reapply = false) {
    // Configuration writes are complete; optional observations must not hold a row locked.
    void Promise.allSettled(
      [
        "agent-connections",
        "agent-native-models",
        "proxyStatus",
        "intent-routing-client",
        ...(reapply ? ["agent-model-catalog"] : []),
      ].map((key) => client.invalidateQueries({ queryKey: [key] })),
    );
  }
  async function reapply(app: AppId) {
    if (!beginOperation(app, "reapplying")) return;
    setError(null);
    try {
      await agentConnectionsApi.reapply(app);
    } catch (cause) {
      setError(
        cause === "agentConnections.conflict" ||
          cause === "agentConnections.noModels"
          ? cause
          : "agentConnections.failed",
      );
    } finally {
      refreshConnectionViews(true);
      endOperation(app);
    }
  }
  async function apply(
    app: AppId,
    enabled: boolean,
    model = selected || catalog.data?.[0]?.id || null,
  ) {
    if (!beginOperation(app, enabled ? "connecting" : "disconnecting")) return;
    setError(null);
    try {
      await agentConnectionsApi.set(app, enabled, enabled ? model : null);
      setSetup((current) => (current === app ? null : current));
      setPicking((current) => (current === app ? null : current));
      setDisconnecting((current) => (current === app ? null : current));
    } catch (cause) {
      setError(
        typeof cause === "string" && cause === "agentConnections.conflict"
          ? "agentConnections.conflict"
          : "agentConnections.failed",
      );
    } finally {
      refreshConnectionViews();
      endOperation(app);
    }
  }
  async function restart(app: AppId) {
    if (!beginOperation(app, "restarting")) return;
    setError(null);
    try {
      await agentConnectionsApi.restart(app, true);
      setRestarting(null);
    } catch (cause) {
      const allowed = [
        "agentConnections.conflict",
        "agentConnections.restartChanged",
        "agentConnections.restartBlocked",
        "agentConnections.restartUnsupported",
      ];
      setError(
        typeof cause === "string" && allowed.includes(cause)
          ? cause
          : "agentConnections.restartFailed",
      );
    } finally {
      refreshConnectionViews();
      endOperation(app);
    }
  }
  async function launchDoubao(enabled: boolean) {
    if (!beginOperation("doubaowork", "connecting")) return;
    setError(null);
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
    setError(null);
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
      (state) => state.app === app && (state.enabled || state.recoveryPending),
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
  const otherWithConnections = other.filter(hasSavedConnection);
  const otherWithoutConnections = other.filter(
    (app) => !hasSavedConnection(app),
  );
  function reorder({ active, over }: DragEndEvent) {
    if (
      compact ||
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
    const connection = connections[APP_IDS.indexOf(app)];
    const state = connection.data;
    const nativeQuery = nativeModels[APP_IDS.indexOf(app)];
    const enabled = !!state?.enabled;
    const orphaned = app === "codex" && state?.recoveryReason === "orphaned";
    const nativeUnavailable =
      app === "codex" &&
      state?.configured &&
      !state.recoveryPending &&
      !connection.isError &&
      [
        "unsupported",
        "revoked",
        "reconnect_required",
        "recovery_pending",
      ].includes(state.nativeCompatibility ?? "");
    const native = nativeQuery.data;
    const nativeAvailable = native?.available && !nativeQuery.isError;
    const nativeModel =
      app === "opencode"
        ? native?.model?.replace(/^[^/]+\//, "")
        : native?.model;
    const nativeLabel = nativeAvailable
      ? (nativeModel ?? t("agentConnections.nativeDefault"))
      : t(
          nativeQuery.isPending
            ? "common.loading"
            : "agentConnections.nativeUnavailable",
        );
    const currentModel = enabled
      ? (catalog.data?.find((model) => model.id === state?.model)?.name ??
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
        {t("homeAudit.doubaoLaunch")}
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
              : state.recoveryReason === "bridge_pending"
                ? "bridgePending"
                : state.recoveryReason === "connection_inactive"
                  ? gateway.data?.running
                    ? "inactive"
                    : "gatewayStopped"
                  : state.recoveryReason === "orphaned"
                    ? "orphaned"
                    : "recovery"
            : enabled
              ? gateway.isError
                ? "unavailable"
                : gateway.data?.running
                  ? state?.configured
                    ? app === "doubaowork"
                      ? "auditConnected"
                      : nativeUnavailable
                        ? "nativeUnavailable"
                        : state.desktopState === "restart_required"
                          ? "restartRequired"
                          : "configured"
                    : app === "doubaowork"
                      ? "auditWaiting"
                      : "recovery"
                  : "gatewayStopped"
              : "off";
    const modelCatalog = ["workbuddy", "qoder", "trae"].includes(app) ? (
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
                : app === "trae" && state?.recoveryReason === "bridge_pending"
                  ? "agentConnections.nativeCatalogPendingHint"
                  : "agentConnections.nativeCatalogHint",
            )}
            error={catalog.isError ? t("agentConnections.failed") : undefined}
          />
        </ModelPickerPopoverContent>
      </Popover>
    ) : null;
    return (
      <SortableAgentRow
        key={app}
        app={app}
        name={name(app)}
        sortable={!compact && shown.includes(app)}
        className={`mp-agent-row${compact && expanded === app ? " expanded" : ""}`}
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
            {!compact && (
              <AgentVersionTag
                app={app}
                updates={updates}
                disabled={rowBusy(app)}
              />
            )}
            {app === "doubaowork" && (
              <span className="mp-agent-note">
                {t("agentConnections.auditOnly")}
              </span>
            )}
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
            {app === "pi" && (
              <span className="mp-agent-note">
                {t("agentConnections.individual")}
              </span>
            )}
            {(state?.recoveryPending || nativeUnavailable) &&
              !connection.isError && (
                <button
                  type="button"
                  className="mp-agent-fix"
                  aria-label={t("agentConnections.reapply")}
                  title={t(
                    app === "trae"
                      ? "agentConnections.bridgeHint"
                      : "agentConnections.reapplyHint",
                  )}
                  disabled={rowBusy(app) || catalog.isPending}
                  onClick={() => void reapply(app)}
                >
                  <RefreshCw
                    size={11}
                    className={
                      operations[app] === "reapplying"
                        ? "animate-spin motion-reduce:animate-none"
                        : ""
                    }
                  />
                  <span>{t("agentConnections.reapply")}</span>
                </button>
              )}
            {state?.configured &&
              !state.recoveryPending &&
              state.desktopState === "restart_required" &&
              !connection.isError && (
                <button
                  type="button"
                  className="mp-agent-fix"
                  aria-label={t("agentConnections.restart")}
                  disabled={rowBusy(app)}
                  onClick={() => setRestarting(app)}
                >
                  <RefreshCw
                    size={11}
                    className={
                      operations[app] === "restarting"
                        ? "animate-spin motion-reduce:animate-none"
                        : ""
                    }
                  />
                  <span>{t("agentConnections.restart")}</span>
                </button>
              )}
          </div>
          <p
            className={`mp-agent-status ${status === "unavailable" ? "bad" : status === "checking" ? "checking" : enabled || operations[app] ? "wait" : ""}`}
            role="status"
            title={t(
              `agentConnections.states.${operations[app] && operations[app] !== "visibility" ? operations[app] : status}`,
            )}
          >
            <span className="mp-agent-dot" aria-hidden="true" />
            <span className="mp-agent-status-text">
              {t(
                `agentConnections.states.${operations[app] && operations[app] !== "visibility" ? operations[app] : status}`,
              )}
            </span>
          </p>
        </div>
        {compact && (
          <span className="mp-agent-summary-model" title={currentModel}>
            {app === "claude-desktop" && enabled
              ? t("agentConnections.allModels", {
                  count: catalog.data?.length ?? 0,
                })
              : currentModel}
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
          className={compact ? "mp-agent-panel-controls" : "contents"}
          hidden={compact && expanded !== app}
        >
          {app === "claude-desktop" && (
            <AgentModelVisibility
              app={app}
              name={name(app)}
              connected={enabled}
              models={catalog.data ?? []}
              disabled={
                (!!operations[app] && operations[app] !== "visibility") ||
                updates.isBusy(app) ||
                !!state?.recoveryPending ||
                connection.isPending ||
                connection.isError ||
                catalog.isPending ||
                catalog.isError
              }
              onBusyChange={(value) =>
                value ? beginOperation(app, "visibility") : endOperation(app)
              }
            />
          )}
          {(app === "workbuddy" ||
            app === "doubaowork" ||
            hasAgentRewards(app)) && (
            <button
              type="button"
              className="mp-agent-start mp-agent-checkin-trigger"
              aria-label={`${t("agentRewards.title")} ${name(app)}`}
              aria-expanded={checkinApp === app}
              aria-controls={`${detailsId}-checkin`}
              onClick={() => {
                setOpenedCheckins((previous) =>
                  previous.includes(app) ? previous : [...previous, app],
                );
                setCheckinApp((previous) => (previous === app ? null : app));
              }}
            >
              {t("agentRewards.title")}
              <ChevronDown size={11} aria-hidden="true" />
            </button>
          )}
          {app !== "trae" && modelCatalog}
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
              onOpenChange={(open) => setPicking(open ? app : null)}
            >
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className="mp-agent-start"
                  disabled={
                    rowBusy(app) ||
                    connection.isPending ||
                    connection.isError ||
                    (!!state?.recoveryPending &&
                      !orphaned &&
                      !(
                        app === "trae" &&
                        state.recoveryReason === "bridge_pending"
                      )) ||
                    (enabled &&
                      (catalog.isPending ||
                        catalog.isError ||
                        !catalog.data?.length))
                  }
                  aria-label={`${t("agentConnections.model")} ${name(app)}`}
                  title={
                    !enabled
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
                <SharedModelPicker
                  variant="agent"
                  description={
                    orphaned
                      ? t("agentConnections.orphanReconnectHint")
                      : app === "trae"
                        ? t("agentConnections.traeStartingModelHint")
                        : app === "deepseek-harness"
                          ? t("agentConnections.deepseekStartingModelHint")
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
                    ...(catalog.isError ? [] : (catalog.data ?? [])),
                  ]}
                  selected={
                    enabled && state?.model
                      ? [state.model]
                      : nativeAvailable
                        ? [`native-current/${app}`]
                        : []
                  }
                  onPick={(model) => void apply(app, true, model)}
                  onDisconnect={
                    enabled || orphaned
                      ? () => {
                          setPicking(null);
                          setDisconnecting(app);
                        }
                      : undefined
                  }
                  disconnectLabel={
                    orphaned ? t("agentConnections.disconnectOnly") : undefined
                  }
                />
                {(!catalog.data?.length || catalog.isError) && (
                  <p className="mt-3 text-xs text-muted-foreground">
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
            aria-busy={connection.isPending || !!operations[app]}
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
            {((app === "doubaowork" && !state?.configured) ||
              (enabled && !state?.recoveryPending && app !== "doubaowork")) && (
              <div className="mp-agent-detail-actions">
                {doubaoLauncher}
                {enabled && !state?.recoveryPending && app !== "doubaowork" && (
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={
                      rowBusy(app) || connection.isPending || connection.isError
                    }
                    onClick={() => openSetup(app)}
                  >
                    {t("agentConnections.individual")}
                  </Button>
                )}
              </div>
            )}
            {app === "trae" && (
              <>
                <p className="text-xs text-muted-foreground">
                  {t("agentConnections.bridgeHint")}
                </p>
                <p className="text-xs text-muted-foreground">
                  {t(`agentConnections.states.${status}`)}
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
              <DoubaoWorkCheckinPanel embedded />
            ) : hasAgentRewards(app) ? (
              <AgentRewardsPanel app={app} embedded />
            ) : null}
          </div>
        )}
      </SortableAgentRow>
    );
  }
  return (
    <section
      className={`vary-agents${compact ? " tray-agents" : ""}`}
      data-testid="agent-connections-page"
    >
      {!compact && (
        <header className="mp-agent-page-header">
          <h1 className="mp-page-heading">{t("agentConnections.title")}</h1>
        </header>
      )}
      {(states.isError || installations.isError || error) && (
        <p role="alert" className="mb-4 text-sm text-destructive">
          {t(error ?? "agentConnections.failed")}
        </p>
      )}
      {error === "agentConnections.noModels" && onAddProvider && (
        <Button
          variant="outline"
          size="sm"
          className="mb-4"
          onClick={onAddProvider}
        >
          {t("modelProviders.add")}
        </Button>
      )}
      <AgentUpdatesBar updates={updates} name={name} disabled={!!busy} />
      <AgentInstallDialog updates={updates} name={name} />
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        modifiers={[restrictWorkspaceSort]}
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
        <SortableContext items={shown} strategy={verticalListSortingStrategy}>
          <div className="mp-list">
            {(installations.isPending || states.isPending) &&
              shown.length === 0 && (
                <div
                  role="status"
                  aria-label={t("agentConnections.states.checking")}
                >
                  {[0, 1, 2].map((index) => (
                    <div
                      key={index}
                      className="mp-agent-row mp-agent-placeholder"
                      aria-hidden="true"
                    >
                      <span className="mp-agent-placeholder-icon" />
                      <span className="mp-agent-placeholder-name" />
                      <span className="mp-agent-placeholder-control" />
                    </div>
                  ))}
                </div>
              )}
            {shown.map((app, index) => (
              <Fragment key={app}>
                {compact &&
                  connectionRank(app) === 0 &&
                  (index === 0 || connectionRank(shown[index - 1]) > 0) && (
                    <p className="tray-unconfigured">
                      {t("trayPanel.notConfigured")}
                    </p>
                  )}
                {row(app)}
              </Fragment>
            ))}
            {!installations.isPending &&
              !states.isPending &&
              !states.isError &&
              !shown.length && (
                <p className="py-8 text-sm text-muted-foreground">
                  {t("agentConnections.empty")}
                </p>
              )}
          </div>
        </SortableContext>
        {!compact && <WorkspaceDragOverlay />}
      </DndContext>
      <details className="mp-agent-more">
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
          <div className="mp-agent-install-list divide-y">
            {otherWithConnections.map(row)}
            {otherWithoutConnections.map((app) => (
              <div
                key={app}
                role="group"
                aria-label={name(app)}
                className="mp-agent-install-row"
              >
                <div>
                  {compact && (
                    <span className="mp-agent-install-icon">
                      <ProviderIcon
                        icon={APP_ICON_NAME[app]}
                        name={name(app)}
                        size={22}
                      />
                    </span>
                  )}
                  <span className="mp-agent-install-name" title={name(app)}>
                    {name(app)}
                  </span>
                  <AgentVersionTag
                    app={app}
                    updates={updates}
                    disabled={rowBusy(app)}
                  />
                </div>
                <div className="mp-agent-install-actions">
                  {(updates.installing === app ||
                    (!installations.isError &&
                      installations.data?.[app] === false)) && (
                    <AgentInstallButton
                      app={app}
                      updates={updates}
                      disabled={rowBusy(app)}
                      onSelect={() =>
                        updates.requestInstall(app, websites[app])
                      }
                    />
                  )}
                  {supported.includes(app) &&
                    installations.data?.[app] === true && (
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
                        .catch(() => setError("agentConnections.failed"));
                    }}
                  >
                    <ExternalLink className="mr-2 h-3 w-3" />
                    {t("agentConnections.website")}
                  </Button>
                </div>
              </div>
            ))}
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="mt-3"
            disabled={installations.isFetching}
            onClick={() =>
              void Promise.all([
                client.invalidateQueries({
                  queryKey: ["agent-installations"],
                }),
                client.invalidateQueries({ queryKey: ["agent-cli-version"] }),
                client.invalidateQueries({ queryKey: ["agent-native-models"] }),
                client.invalidateQueries({
                  queryKey: ["agent-desktop-versions"],
                }),
              ])
            }
          >
            <RefreshCw className="mr-2 h-3 w-3" />
            {t("agentConnections.rescan")}
          </Button>
        </div>
      </details>
      <AppDialog
        open={!!restarting}
        pending={rowBusy(restarting)}
        onOpenChange={(open) => {
          if (!open && !rowBusy(restarting)) setRestarting(null);
        }}
        title={t("agentConnections.restartTitle", {
          app: restarting ? name(restarting) : "",
        })}
        description={t("agentConnections.restartConfirmHint")}
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
      >
        {error && (
          <p role="alert" className="vary-dialog-error">
            {t(error)}
          </p>
        )}
      </AppDialog>
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
          orphanedDisconnect
            ? "agentConnections.orphanDisconnectHint"
            : disconnecting === "doubaowork"
              ? "agentConnections.auditDisconnectHint"
              : disconnecting === "trae"
                ? "agentConnections.bridgeDisconnectHint"
                : "agentConnections.disconnectHint",
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
              onClick={() => disconnecting && void apply(disconnecting, false)}
            >
              {t(
                orphanedDisconnect
                  ? "agentConnections.disconnectOnly"
                  : "agentConnections.disconnectRestore",
              )}
            </Button>
          </>
        }
      >
        {error && (
          <p role="alert" className="vary-dialog-error">
            {t(error)}
          </p>
        )}
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
          {!nativePicker(setup) ? (
            <fieldset disabled={rowBusy(setup)} className="space-y-3">
              <legend className="mb-2 text-sm">
                {t("agentConnections.model")}
              </legend>
              <SharedModelPicker
                description={
                  setup === "trae"
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
              <p role="alert" className="text-sm text-amber-700">
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
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {t(error)}
            </p>
          )}
        </div>
      </AppDialog>
    </section>
  );
}
