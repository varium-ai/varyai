import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ArrowUp, ExternalLink, RefreshCw, Settings } from "lucide-react";
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { RoutingStage } from "@/components/routing/RoutingStage";
import { GlobalSecurityToggle } from "@/components/security/GlobalSecurityToggle";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useUpdate } from "@/contexts/UpdateContext";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { intentRoutingApi, type RoutingGroup } from "@/lib/api/intentRouting";
import { proxyApi } from "@/lib/api/proxy";
import { routingTraceApi } from "@/lib/api/routingTrace";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { securityApi } from "@/lib/api/security";
import { securityEventsApi } from "@/lib/api/securityEvents";
import { routingModelLabel } from "@/lib/routingModelLabel";
import { panelTab, trayPanelApi, type PanelTab } from "@/lib/trayPanel";
import { APP_IDS } from "@/config/appConfig";
import appIcon from "@/assets/icons/app-icon.png";
import "@/components/routing/routing.css";
import "./tray-panel.css";

const tabs = ["agents", "routing", "security"] as const;
const securityFilter = { view: "security_events" as const };
export function TrayPanel() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const update = useUpdate();
  const [tab, setTab] = useState(() =>
    panelTab(localStorage.getItem("vary:tray-tab")),
  );
  const [active, setActive] = useState(true);
  const [error, setError] = useState(false);
  const frame = useRef<HTMLDivElement>(null);
  const connections = useQuery({
    queryKey: ["agent-connections"],
    queryFn: agentConnectionsApi.list,
    enabled: active,
    refetchInterval: active ? 5000 : false,
  });
  function refresh() {
    const roots =
      tab === "agents"
        ? [
            "agent-connections",
            "agent-installations",
            "agent-native-models",
            "agent-model-catalog",
            "agent-cli-version",
            "agent-desktop-versions",
          ]
        : tab === "routing"
          ? ["routing-trace", "intent-routing-groups", "proxyStatus"]
          : ["security", "securityEvents", "proxyStatus"];
    void Promise.all(
      roots.map((root) => client.invalidateQueries({ queryKey: [root] })),
    );
  }
  useTauriEvent("tray-panel-refresh", () => {
    setActive(true);
    frame.current?.focus({ preventScroll: true });
    refresh();
  });
  useTauriEvent<boolean>("tray-panel-active", (active) => setActive(active));
  useEffect(() => {
    frame.current?.focus({ preventScroll: true });
  }, []);
  useEffect(() => {
    const blur = () => setActive(false);
    const focus = () => setActive(true);
    window.addEventListener("blur", blur);
    window.addEventListener("focus", focus);
    return () => {
      window.removeEventListener("blur", blur);
      window.removeEventListener("focus", focus);
    };
  }, []);
  useEffect(() => {
    function escape(event: KeyboardEvent) {
      if (
        event.key !== "Escape" ||
        event.defaultPrevented ||
        document.querySelector(
          '[role="dialog"][data-state="open"], [role="menu"][data-state="open"], [data-radix-popper-content-wrapper]',
        )
      )
        return;
      void trayPanelApi.hide().catch(() => setError(true));
    }
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, []);
  function open(view: PanelTab | "settings" | "providers") {
    void trayPanelApi.open(view).catch(() => setError(true));
  }
  const selectedApps = APP_IDS.filter((app) =>
    connections.data?.some(
      (connection) => connection.enabled && connection.app === app,
    ),
  );
  return (
    <div className="tray-panel" ref={frame} tabIndex={-1}>
      <header className="tray-header">
        <div className="tray-brand">
          <img src={appIcon} alt="" />
          Vary AI
        </div>
        {update.hasUpdate && (
          <button
            className="tray-update"
            data-available
            onClick={update.resetDismiss}
          >
            <ArrowUp size={14} />
            {t("trayPanel.update")}
          </button>
        )}
        <button
          title={t("trayPanel.refresh")}
          aria-label={t("trayPanel.refresh")}
          onClick={refresh}
        >
          <RefreshCw size={14} />
        </button>
        <button
          title={t("trayPanel.openMain")}
          aria-label={t("trayPanel.openMain")}
          onClick={() => open(tab)}
        >
          <ExternalLink size={14} />
        </button>
        <button
          title={t("trayPanel.settings")}
          aria-label={t("trayPanel.settings")}
          onClick={() => open("settings")}
        >
          <Settings size={14} />
        </button>
      </header>
      <div
        className="tray-tabs"
        role="tablist"
        aria-label={t("trayPanel.title")}
        style={{ "--tab-index": tabs.indexOf(tab) } as CSSProperties}
      >
        <span className="tray-tab-selection" aria-hidden="true" />
        {tabs.map((key) => (
          <button
            key={key}
            id={`tray-tab-${key}`}
            role="tab"
            tabIndex={key === tab ? 0 : -1}
            aria-selected={key === tab}
            aria-controls="tray-content"
            onKeyDown={(event) => {
              if (
                !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
              )
                return;
              event.preventDefault();
              const index =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? 2
                    : (tabs.indexOf(tab) +
                        (event.key === "ArrowRight" ? 1 : 2)) %
                      3;
              setTab(tabs[index]);
              localStorage.setItem("vary:tray-tab", tabs[index]);
              document.getElementById(`tray-tab-${tabs[index]}`)?.focus();
            }}
            onClick={() => {
              setTab(key);
              localStorage.setItem("vary:tray-tab", key);
            }}
          >
            {t(`trayPanel.${key}`)}
          </button>
        ))}
      </div>
      {error && (
        <p className="tray-error" role="alert">
          {t("trayPanel.failed")}
        </p>
      )}
      <main
        className="tray-body"
        id="tray-content"
        role="tabpanel"
        aria-labelledby={`tray-tab-${tab}`}
      >
        <div className="tray-content">
          {tab === "agents" && active && (
            <AgentConnectionsPage
              compact
              onAddProvider={() => open("providers")}
            />
          )}
          {tab === "routing" && active && <TrayRouting />}
          {tab === "security" && active && <TraySecurity />}
        </div>
      </main>
      <footer className="tray-footer">
        <ProfileSwitcher
          compact
          side="top"
          activeApp={selectedApps[0] ?? "claude"}
          selectedApps={selectedApps}
        />
        <div className="tray-footer-actions">
          {tab !== "agents" && (
            <button onClick={() => open(tab)}>
              {t(
                tab === "routing"
                  ? "trayPanel.manageRouting"
                  : "trayPanel.manageSecurity",
              )}
            </button>
          )}
          <button
            onClick={() => void trayPanelApi.quit().catch(() => setError(true))}
          >
            {t("trayPanel.quit")}
          </button>
        </div>
      </footer>
    </div>
  );
}

function TrayRouting() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const traces = useQuery({
    queryKey: ["routing-trace"],
    queryFn: routingTraceApi.snapshot,
    refetchInterval: 1000,
  });
  const catalog = useQuery({
    queryKey: ["agent-model-catalog"],
    queryFn: agentConnectionsApi.models,
  });
  const groups = useQuery({
    queryKey: ["intent-routing-groups"],
    queryFn: intentRoutingApi.list,
  });
  const gateway = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: proxyApi.getProxyStatus,
  });
  const [selected, setSelected] = useState<string>();
  const [pending, setPending] = useState<string>();
  const [failed, setFailed] = useState(false);
  const rows = traces.data?.requests.slice(0, 8) ?? [];
  const row = rows.find((r) => r.id === selected) ?? rows[0];
  const upstreams = useQuery({
    queryKey: ["routing-upstreams"],
    queryFn: routingUpstreamsApi.list,
    enabled: rows.some((row) => row.candidates.length > 0),
    staleTime: 30000,
  });
  async function save(group: RoutingGroup, patch: Partial<RoutingGroup>) {
    setPending(group.id);
    setFailed(false);
    try {
      await intentRoutingApi.save({ ...group, ...patch });
    } catch {
      setFailed(true);
    } finally {
      void client.invalidateQueries({ queryKey: ["intent-routing-groups"] });
      setPending(undefined);
    }
  }
  return (
    <section className="tray-routing vary-routing">
      <p className="tray-caption">
        {t(
          traces.isError
            ? "trayPanel.failed"
            : traces.data?.running
              ? "trayPanel.gatewayRunning"
              : "routingPage.gatewayStopped",
        )}
      </p>
      {row && (
        <div className="tray-card">
          <RoutingStage
            layout="compact"
            upstreams={upstreams.data}
            row={row}
            requestedModel={
              routingModelLabel(row, catalog.data ?? []) ??
              t("routingPage.modelUnavailable")
            }
            accountName={(c) => c.providerName}
            animate={!!traces.data?.running && !selected}
            replay={false}
            speed={1}
            currentAccounts={traces.data?.accounts}
            concurrent={selected ? [] : (traces.data?.requests ?? [])}
            liveObservations
            address={
              gateway.data
                ? `${gateway.data.address}:${gateway.data.port}`
                : undefined
            }
          />
        </div>
      )}
      <div className="tray-card tray-requests">
        {!rows.length && (
          <p className="tray-empty">{t("routingPage.noRequests")}</p>
        )}
        {rows.map((r) => (
          <button
            key={r.id}
            className={row?.id === r.id ? "selected" : ""}
            aria-pressed={row?.id === r.id}
            title={t(`routingPage.state.${r.state}`)}
            onClick={() => setSelected(r.id)}
          >
            <span
              className={`tray-dot ${r.state === "failed" || (r.state !== "cancelled" && (r.status ?? 0) >= 400) ? "bad" : ""}`}
              data-state={r.state}
              aria-hidden="true"
            />
            <span>
              <b>{r.agent}</b>
              <small>
                {routingModelLabel(r, catalog.data ?? []) ??
                  t("routingPage.modelUnavailable")}
              </small>
            </span>
            <time>
              {new Date(r.startedAt).toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
              })}
            </time>
            <span>{r.status === 499 ? "—" : (r.status ?? "—")}</span>
          </button>
        ))}
      </div>
      <p className="tray-section-title">{t("intentRouting.title")}</p>
      {failed && (
        <p role="alert" className="tray-error">
          {t("trayPanel.failed")}
        </p>
      )}
      <ul className="tray-groups" aria-label={t("intentRouting.title")}>
        {groups.data?.groups.map((group) => (
          <li key={group.id}>
            <button
              className="tray-group-name"
              title={group.name}
              onClick={() => void trayPanelApi.open("routing")}
            >
              <b>{group.name}</b>
              <small>
                {group.members.length} {t("trayPanel.models")}
              </small>
            </button>
            <Select
              value={group.routing ?? "smart"}
              disabled={!!pending}
              onValueChange={(routing: NonNullable<RoutingGroup["routing"]>) =>
                void save(group, {
                  routing,
                })
              }
            >
              <SelectTrigger
                className="tray-strategy-trigger"
                aria-label={`${t("intentRouting.routingStrategy")} ${group.name}`}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent
                className="tray-strategy-menu"
                align="end"
                sideOffset={4}
                collisionPadding={8}
              >
                {[
                  "smart",
                  "ordered",
                  "round_robin",
                  "least_used",
                  "pace",
                  "free_first",
                  "fastest",
                  "weighted",
                  "manual",
                ].map((strategy) => (
                  <SelectItem
                    className="tray-strategy-option"
                    key={strategy}
                    value={strategy}
                  >
                    {t(`routingPage.strategy.${strategy}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Switch
              aria-label={group.name}
              checked={group.enabled}
              disabled={!!pending || !!group.autoGenerated}
              onCheckedChange={(enabled) => void save(group, { enabled })}
            />
          </li>
        ))}
        {groups.isError && (
          <li className="tray-empty">{t("trayPanel.failed")}</li>
        )}
      </ul>
    </section>
  );
}

function TraySecurity() {
  const { t } = useTranslation();
  const settings = useQuery({
    queryKey: ["security", "settings"],
    queryFn: securityApi.getSettings,
  });
  const gateway = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: proxyApi.getProxyStatus,
    refetchInterval: 5000,
  });
  const events = useQuery({
    queryKey: ["securityEvents", "page", securityFilter, 1, 8],
    queryFn: () => securityEventsApi.getEvents(securityFilter, 1, 8),
    refetchInterval: 5000,
  });
  return (
    <section className="tray-security">
      <div className="tray-card tray-protection">
        <GlobalSecurityToggle />
        <p>
          {t(
            settings.isError || gateway.isError
              ? "trayPanel.failed"
              : !settings.data?.enabled
                ? "trayPanel.securityOff"
                : gateway.data?.running
                  ? "trayPanel.securityRunning"
                  : "trayPanel.securityStopped",
          )}
        </p>
      </div>
      <p className="tray-section-title">{t("trayPanel.recentEvents")}</p>
      <div className="tray-card tray-events">
        {events.isError ? (
          <p className="tray-empty" role="alert">
            {t("trayPanel.eventsFailed")}
          </p>
        ) : events.isPending ? (
          <p className="tray-empty" role="status">
            {t("common.loading")}
          </p>
        ) : !events.data?.items.length ? (
          <p className="tray-empty">{t("trayPanel.noEvents")}</p>
        ) : (
          events.data.items.map((event) => (
            <div key={event.key}>
              <span
                className={`tray-dot ${event.result === "blocked" ? "bad" : ""}`}
              />
              <span>
                <b>{t(`securityEvents.result.${event.result}`)}</b>
                <small>
                  {event.appType ?? "—"} · {event.riskLevel ?? "—"}
                </small>
              </span>
              <time>
                {new Date(event.timestamp * 1000).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </time>
            </div>
          ))
        )}
      </div>
    </section>
  );
}
