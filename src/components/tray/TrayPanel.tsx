import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useIsFetching, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ArrowUp, ExternalLink, RefreshCw, Settings } from "lucide-react";
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { GlobalSecurityToggle } from "@/components/security/GlobalSecurityToggle";
import {
  securityEventResultLabel,
  securityEventRiskLabel,
} from "@/components/security/securityEventLabels";
import { useUpdate } from "@/contexts/UpdateContext";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import { securityApi } from "@/lib/api/security";
import { securityEventsApi } from "@/lib/api/securityEvents";
import { trayUsageKey } from "@/lib/api/trayUsage";
import { panelTab, trayPanelApi, type PanelTab } from "@/lib/trayPanel";
import { APP_IDS } from "@/config/appConfig";
import appIcon from "@/assets/icons/app-icon.png";
import "./tray-panel.css";
import { TrayQuotaPanel } from "./TrayQuotaPanel";
import { TrayUsagePanel } from "./TrayUsagePanel";

const tabs = ["agents", "quota", "usage", "security"] as const;
const quotaRoots = [
  "routing-upstreams",
  "provider-list-codex-accounts",
  "codex-provider-quota",
  "subscription-account-quota",
  "routing-provider-quota",
  "codex-quota-management",
  "quota-panel-history",
];
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
  const [refreshing, setRefreshing] = useState(false);
  const quotaFetching =
    useIsFetching({
      predicate: (query) => quotaRoots.includes(String(query.queryKey[0])),
    }) > 0;
  const refreshBusy = refreshing || (tab === "quota" && quotaFetching);
  const frame = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLElement>(null);
  const connections = useQuery({
    queryKey: ["agent-connections"],
    queryFn: agentConnectionsApi.list,
    enabled: active,
    refetchInterval: active ? 5000 : false,
  });
  async function refresh() {
    if (refreshing) return;
    setRefreshing(true);
    try {
      if (tab === "usage") {
        await client.invalidateQueries({ queryKey: trayUsageKey });
        return;
      }
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
          : tab === "quota"
            ? quotaRoots
            : ["security", "securityEvents", "proxyStatus"];
      await Promise.all(
        roots.map((root) => client.invalidateQueries({ queryKey: [root] })),
      );
    } finally {
      setRefreshing(false);
    }
  }
  useTauriEvent("tray-panel-refresh", () => {
    setActive(true);
    frame.current?.focus({ preventScroll: true });
    void refresh();
  });
  useTauriEvent<boolean>("tray-panel-active", (active) => setActive(active));
  useEffect(() => {
    frame.current?.focus({ preventScroll: true });
  }, []);
  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
  }, [tab]);
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
    void trayPanelApi
      .open(view === "quota" ? "providers" : view === "usage" ? "stats" : view)
      .catch(() => setError(true));
  }
  const selectedApps = APP_IDS.filter((app) =>
    connections.data?.some(
      (connection) => connection.enabled && connection.app === app,
    ),
  );
  return (
    <div className="tray-panel" data-tab={tab} ref={frame} tabIndex={-1}>
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
          disabled={refreshBusy}
          aria-busy={refreshBusy}
          onClick={() => void refresh()}
        >
          <RefreshCw
            size={14}
            className={
              refreshBusy
                ? "animate-spin motion-reduce:animate-none"
                : undefined
            }
          />
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
        style={
          {
            "--tab-index": tabs.indexOf(tab),
            "--tab-count": tabs.length,
          } as CSSProperties
        }
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
                    ? tabs.length - 1
                    : (tabs.indexOf(tab) +
                        (event.key === "ArrowRight" ? 1 : tabs.length - 1)) %
                      tabs.length;
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
        ref={body}
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
          {tab === "usage" && (
            <TrayUsagePanel active={active} onOpen={() => open("usage")} />
          )}
          {tab === "quota" && active && <TrayQuotaPanel />}
          {tab === "security" && active && <TraySecurity />}
        </div>
      </main>
      <footer className="tray-footer">
        {tab !== "usage" && (
          <ProfileSwitcher
            compact
            side="top"
            activeApp={selectedApps[0] ?? "claude"}
            selectedApps={selectedApps}
          />
        )}
        <div className="tray-footer-actions">
          {(tab === "quota" || tab === "security") && (
            <button onClick={() => open(tab)}>
              {t(
                tab === "quota"
                  ? "trayPanel.manageQuota"
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

function TraySecurity() {
  const { t, i18n } = useTranslation();
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
                className="tray-dot"
                data-result={event.result}
                aria-hidden="true"
              />
              <span>
                <b>{securityEventResultLabel(t, event)}</b>
                <small>
                  {event.appType ?? "—"} ·{" "}
                  {securityEventRiskLabel(t, event.riskLevel, true)}
                </small>
              </span>
              <time>
                {new Date(event.timestamp).toLocaleTimeString(i18n.language, {
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
