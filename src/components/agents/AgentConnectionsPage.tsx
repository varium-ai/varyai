import { useId, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
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
import { LongConversationSettings } from "./LongConversationSettings";
import { WorkBuddyCheckinPanel } from "@/components/WorkBuddyCheckinPanel";
import { DoubaoWorkCheckinPanel } from "@/components/DoubaoWorkCheckinPanel";
import { proxyApi } from "@/lib/api/proxy";
import { settingsApi } from "@/lib/api/settings";
import { useAgentUpdates } from "@/hooks/useAgentUpdates";
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
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import type { AppId } from "@/lib/api";
import { Popover, PopoverTrigger } from "@/components/ui/popover";

const supported: readonly AppId[] = APP_IDS;
type ConnectionOperation =
  | "connecting"
  | "disconnecting"
  | "reapplying"
  | "restarting"
  | "visibility";
const nativePicker = (app: AppId | null) =>
  app !== null &&
  ["claude-desktop", "workbuddy", "qoder", "trae", "doubaowork"].includes(app);
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
  doubaowork: "https://www.doubao.com/",
  trae: "https://www.trae.ai/",
  kimi: "https://www.kimi.com/code",
  qwen: "https://qwenlm.github.io/qwen-code-docs/",
  zcode: "https://zcode.ai/",
};
export function AgentConnectionsPage({
  onAddProvider,
  onEditingChange,
}: {
  onAddProvider?: () => void;
  onEditingChange?: (state: EditingState) => void;
}) {
  const { t } = useTranslation();
  const pageId = useId();
  const name = (app: AppId) =>
    app === "claude" ? "Claude Code" : t(`apps.${app}`);
  const client = useQueryClient();
  const states = useQuery({
    queryKey: ["agent-connections"],
    queryFn: agentConnectionsApi.list,
    refetchInterval: 5000,
  });
  const installations = useQuery({
    queryKey: ["agent-installations"],
    queryFn: proxyApi.getAgentInstallations,
    refetchInterval: 30000,
  });
  const catalog = useQuery({
    queryKey: ["agent-model-catalog"],
    queryFn: agentConnectionsApi.models,
  });
  const nativeModels = useQuery({
    queryKey: ["agent-native-models"],
    queryFn: agentConnectionsApi.nativeModels,
    refetchInterval: 5000,
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
  const [sharedSettingsBusy, setSharedSettingsBusy] = useState(false);
  const busy = sharedSettingsBusy || Object.keys(operations).length > 0;
  const rowBusy = (app: AppId | null) =>
    sharedSettingsBusy || (app !== null && !!operations[app]);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<AppId | null>(null);
  const [picking, setPicking] = useState<AppId | null>(null);
  const [disconnecting, setDisconnecting] = useState<AppId | null>(null);
  const [restarting, setRestarting] = useState<AppId | null>(null);
  const updates = useAgentUpdates(
    APP_IDS.filter((app) => installations.data?.[app]),
  );
  useWorkspaceEditing(false, updates.busy || !!busy, onEditingChange);
  function beginOperation(app: AppId, operation: ConnectionOperation) {
    if (updates.busy || sharedSettingsBusy || operationLocks.current.has(app))
      return false;
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
  const shown = supported.filter(
    (app) =>
      installations.data?.[app] ||
      states.data?.some(
        (state) =>
          state.app === app && (state.enabled || state.recoveryPending),
      ),
  );
  const other = APP_IDS.filter((app) => !shown.includes(app));
  function row(app: AppId) {
    const state = states.data?.find((state) => state.app === app);
    const enabled = !!state?.enabled;
    const native = nativeModels.data?.find((entry) => entry.app === app);
    const nativeAvailable = native?.available && !nativeModels.isError;
    const nativeLabel = nativeAvailable
      ? (native.model ?? t("agentConnections.nativeDefault"))
      : t(
          nativeModels.isPending
            ? "common.loading"
            : "agentConnections.nativeUnavailable",
        );
    const currentModel = enabled
      ? (catalog.data?.find((model) => model.id === state?.model)?.name ??
        state?.model ??
        t("agentConnections.pickModel"))
      : nativeLabel;
    const detailsId = `${pageId}-${app}-details`;
    const status = states.isError
      ? "unavailable"
      : state?.recoveryPending
        ? state.recoveryReason === "bridge_pending"
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
                  : state.desktopState === "restart_required"
                    ? "restartRequired"
                    : "configured"
                : app === "doubaowork"
                  ? "auditWaiting"
                  : "recovery"
              : "gatewayStopped"
          : "off";
    return (
      <article
        key={app}
        className={`mp-agent-row${expanded === app ? " expanded" : ""}`}
        aria-label={t(`apps.${app}`)}
        onClick={(event) => {
          if (
            event.currentTarget.contains(event.target as Node) &&
            !(event.target as Element).closest(
              "button, a, input, label, select, textarea, [role='button'], [role='switch'], [role='combobox'], .mp-agent-expanded",
            ) &&
            window.getSelection()?.isCollapsed !== false
          )
            setExpanded(expanded === app ? null : app);
        }}
      >
        <div className="mp-agent-icon">
          <ProviderIcon
            icon={APP_ICON_NAME[app]}
            name={t(`apps.${app}`)}
            size={22}
          />
        </div>
        <div className="mp-agent-who">
          <div className="mp-agent-name">
            <h2>{name(app)}</h2>
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
            {state?.recoveryPending && !states.isError && (
              <button
                type="button"
                className="mp-agent-fix"
                aria-label={t("agentConnections.reapply")}
                title={t(
                  app === "trae"
                    ? "agentConnections.bridgeHint"
                    : "agentConnections.reapplyHint",
                )}
                disabled={rowBusy(app) || updates.busy || catalog.isPending}
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
              !states.isError && (
                <button
                  type="button"
                  className="mp-agent-fix"
                  aria-label={t("agentConnections.restart")}
                  disabled={rowBusy(app) || updates.busy}
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
          <AgentVersionTag app={app} updates={updates} disabled={busy} />
          <p
            className={`mp-agent-status ${status === "unavailable" ? "bad" : enabled || operations[app] ? "wait" : ""}`}
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
        <Button
          variant="ghost"
          className="mp-agent-next"
          size="sm"
          aria-label={`${t("agentConnections.details")} ${name(app)}`}
          aria-expanded={expanded === app}
          aria-controls={expanded === app ? detailsId : undefined}
          onClick={() => setExpanded(expanded === app ? null : app)}
        >
          {t(
            expanded === app
              ? "agentConnections.collapse"
              : "agentConnections.details",
          )}
          <ChevronRight size={10} />
        </Button>
        {app === "claude-desktop" && (
          <AgentModelVisibility
            app={app}
            name={name(app)}
            connected={enabled}
            models={catalog.data ?? []}
            disabled={
              sharedSettingsBusy ||
              (!!operations[app] && operations[app] !== "visibility") ||
              updates.busy ||
              !!state?.recoveryPending ||
              states.isPending ||
              states.isError ||
              catalog.isPending ||
              catalog.isError
            }
            onBusyChange={(value) =>
              value ? beginOperation(app, "visibility") : endOperation(app)
            }
          />
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
                  updates.busy ||
                  states.isPending ||
                  states.isError ||
                  !!state?.recoveryPending ||
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
              onDismiss={() => setPicking(null)}
              aria-label={`${t("agentConnections.model")} ${name(app)}`}
            >
              <SharedModelPicker
                title={`${t("agentConnections.model")} · ${name(app)}`}
                description={
                  !enabled ? t("agentConnections.nativeModelHint") : undefined
                }
                disabled={rowBusy(app)}
                models={[
                  ...(!enabled && nativeAvailable
                    ? [
                        {
                          id: `native-current/${app}`,
                          name: nativeLabel,
                          group: name(app),
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
          aria-busy={!!operations[app]}
          disabled={
            rowBusy(app) ||
            updates.busy ||
            states.isPending ||
            states.isError ||
            (catalog.isPending && !enabled && app !== "doubaowork")
          }
          onCheckedChange={(checked) =>
            checked ? connect(app) : setDisconnecting(app)
          }
        />
        {expanded === app && (
          <div
            id={detailsId}
            className="mp-agent-expanded"
            role="region"
            aria-label={`${t("agentConnections.details")} ${name(app)}`}
          >
            <h3>{t("agentConnections.instructions")}</h3>
            <p>
              {t(
                app === "doubaowork"
                  ? "agentConnections.auditOnlyHint"
                  : app === "trae"
                    ? "agentConnections.bridgeHint"
                    : state?.recoveryPending
                      ? "agentConnections.reapplyHint"
                      : !enabled || !state?.configured
                        ? "agentConnections.connectHint"
                        : app === "qoder"
                          ? "agentConnections.nativePicker"
                          : app === "claude-desktop" || app === "workbuddy"
                            ? "agentConnections.restartHint"
                            : "agentConnections.sessionHint",
              )}
            </p>
            {app !== "doubaowork" && <p>{t("agentConnections.verifyHint")}</p>}
            {state?.model && !nativePicker(app) && (
              <div className="mp-agent-current">
                <span>{t("agentConnections.model")}</span>
                <code>{state.model}</code>
              </div>
            )}
            <div className="mp-agent-detail-actions">
              {enabled && !state?.recoveryPending && app !== "doubaowork" && (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={rowBusy(app) || updates.busy || states.isError}
                  onClick={() => openSetup(app)}
                >
                  {t("agentConnections.individual")}
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
                <ExternalLink size={12} />
                {t("agentConnections.website")}
              </Button>
              {(enabled || state?.recoveryPending) && (
                <Button
                  size="sm"
                  variant="ghost"
                  className="mp-agent-disconnect"
                  disabled={rowBusy(app) || updates.busy || states.isError}
                  onClick={() => setDisconnecting(app)}
                >
                  {t("agentConnections.restore")}
                </Button>
              )}
            </div>
            {app === "workbuddy" && <WorkBuddyCheckinPanel />}
            {app === "doubaowork" && <DoubaoWorkCheckinPanel />}
            {["codex", "claude", "grokbuild"].includes(app) && (
              <LongConversationSettings
                disabled={busy || updates.busy}
                onBusyChange={setSharedSettingsBusy}
              />
            )}
          </div>
        )}
      </article>
    );
  }
  return (
    <section className="vary-agents" data-testid="agent-connections-page">
      <h1 className="mp-page-heading">{t("agentConnections.title")}</h1>
      <p className="mp-lead">{t("agentConnections.subtitle")}</p>
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
      <div className="mp-list">
        {(states.isPending || installations.isPending) && (
          <p className="py-8 text-sm text-muted-foreground" role="status">
            {t("common.loading")}
          </p>
        )}
        {shown.map(row)}
        {!installations.isPending && !shown.length && (
          <p className="py-8 text-sm text-muted-foreground">
            {t("agentConnections.empty")}
          </p>
        )}
      </div>
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
          <div className="divide-y">
            {other.map((app) => (
              <div
                key={app}
                role="group"
                aria-label={name(app)}
                className="mp-agent-install-row"
              >
                <div>
                  <span>{name(app)}</span>
                  <AgentVersionTag
                    app={app}
                    updates={updates}
                    disabled={!!busy}
                  />
                </div>
                <div className="mp-agent-install-actions">
                  {(updates.installing === app ||
                    (!installations.isError &&
                      installations.data?.[app] === false)) && (
                    <AgentInstallButton
                      app={app}
                      updates={updates}
                      disabled={!!busy}
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
                        disabled={
                          !installations.data?.[app] ||
                          rowBusy(app) ||
                          updates.busy
                        }
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
            disabled={updates.busy || installations.isFetching}
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
      <Dialog
        open={!!restarting}
        onOpenChange={(open) => {
          if (!open && !rowBusy(restarting)) setRestarting(null);
        }}
      >
        <DialogContent
          className="mp-dialog max-w-md"
          onInteractOutside={(event) => {
            if (rowBusy(restarting)) event.preventDefault();
          }}
          onEscapeKeyDown={(event) => {
            if (rowBusy(restarting)) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle>
              {t("agentConnections.restartTitle", {
                app: restarting ? name(restarting) : "",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("agentConnections.restartConfirmHint")}
            </DialogDescription>
          </DialogHeader>
          {error && (
            <p role="alert" className="px-6 text-sm text-destructive">
              {t(error)}
            </p>
          )}
          <DialogFooter>
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
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={!!disconnecting}
        onOpenChange={(open) => {
          if (!open && !rowBusy(disconnecting)) setDisconnecting(null);
        }}
      >
        <DialogContent
          className="mp-dialog max-w-md"
          onInteractOutside={(event) => {
            if (rowBusy(disconnecting)) event.preventDefault();
          }}
          onEscapeKeyDown={(event) => {
            if (rowBusy(disconnecting)) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle>
              {t("agentConnections.disconnectTitle", {
                app: disconnecting ? name(disconnecting) : "",
              })}
            </DialogTitle>
            <DialogDescription>
              {t(
                disconnecting === "doubaowork"
                  ? "agentConnections.auditDisconnectHint"
                  : disconnecting === "trae"
                    ? "agentConnections.bridgeDisconnectHint"
                    : "agentConnections.disconnectHint",
              )}
            </DialogDescription>
          </DialogHeader>
          {error && (
            <p role="alert" className="px-6 text-sm text-destructive">
              {t(error)}
            </p>
          )}
          <DialogFooter>
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
              {t("agentConnections.disconnectRestore")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={!!setup}
        onOpenChange={(open) => {
          if (!open && !rowBusy(setup)) setSetup(null);
        }}
      >
        <DialogContent
          className="mp-dialog max-w-lg"
          onInteractOutside={(event) => {
            if (rowBusy(setup)) event.preventDefault();
          }}
          onEscapeKeyDown={(event) => {
            if (rowBusy(setup)) event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle>
              {t("agentConnections.setupTitle", {
                app: setup ? t(`apps.${setup}`) : "",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("agentConnections.setupHint")}
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 space-y-4 overflow-y-auto px-6 py-4">
            {!nativePicker(setup) ? (
              <fieldset disabled={rowBusy(setup)} className="space-y-3">
                <legend className="mb-2 text-sm">
                  {t("agentConnections.model")}
                </legend>
                <SharedModelPicker
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
          <DialogFooter>
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
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
