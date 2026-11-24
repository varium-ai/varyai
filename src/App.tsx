import { useEffect, useRef, useState } from "react";
import { trayPanelApi } from "@/lib/trayPanel";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  WorkspaceNavigation,
  type WorkspaceView,
} from "@/components/layout/WorkspaceNavigation";
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { RoutingPage } from "@/components/routing/RoutingPage";
import { SecurityCenterPage } from "@/components/security/SecurityCenterPage";
import { StatisticsPage } from "@/components/statistics/StatisticsPage";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { GatewayRuntimeStatus } from "@/components/GatewayRuntimeStatus";
import { APP_IDS } from "@/config/appConfig";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { useSettingsQuery } from "@/lib/query";
import { proxyApi } from "@/lib/api/proxy";
import { proxyKeys } from "@/lib/query/proxy";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { useSyncLanguage } from "@/hooks/useSyncLanguage";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { isMac, DRAG_REGION_ATTR } from "@/lib/platform";
import { GlobalSecurityToggle } from "@/components/security/GlobalSecurityToggle";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { EditingState } from "@/hooks/useWorkspaceEditing";

export default function App() {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const [view, setView] = useState<WorkspaceView>("agents");
  const [addingProvider, setAddingProvider] = useState(false);
  const [editor, setEditor] = useState<EditingState>({
    dirty: false,
    busy: false,
  });
  const [nextView, setNextView] = useState<WorkspaceView | null>(null);
  function navigate(next: WorkspaceView) {
    if (next === view || editor.busy) return;
    if (editor.dirty) {
      setNextView(next);
      return;
    }
    setAddingProvider(false);
    setView(next);
  }
  function addProvider() {
    setAddingProvider(true);
    setView("providers");
  }
  const navigation = useRef(navigate);
  navigation.current = navigate;
  useEffect(() => {
    void trayPanelApi
      .navigation()
      .then((next) => {
        if (next) navigation.current(next);
      })
      .catch(() => {});
  }, []);
  useTauriEvent("workspace-navigation", async () => {
    const next = await trayPanelApi.navigation();
    if (next) navigation.current(next);
  });
  const settings = useSettingsQuery();
  const showProfiles = settings.data?.showProfileSwitcher !== false;
  const connections = useQuery({
    queryKey: ["agent-connections"],
    queryFn: agentConnectionsApi.list,
    enabled: showProfiles,
    refetchInterval: showProfiles ? 5000 : false,
  });
  const selectedApps = APP_IDS.filter((app) =>
    connections.data?.some(
      (connection) => connection.enabled && connection.app === app,
    ),
  );
  const gateway = useQuery({
    queryKey: proxyKeys.status,
    queryFn: proxyApi.getProxyStatus,
    refetchInterval: 5000,
  });
  useUsageCacheBridge();
  useSyncLanguage(settings.data?.language);
  useEffect(() => {
    function keydown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === ",") {
        event.preventDefault();
        navigate("settings");
      }
    }
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [view, editor]);
  useTauriEvent("provider-switched", () => {
    void Promise.all(
      ["routing-upstreams", "agent-model-catalog", "intent-routing-groups"].map(
        (key) => client.invalidateQueries({ queryKey: [key] }),
      ),
    );
  });
  return (
    <div
      className="vary-workspace flex h-screen flex-col overflow-hidden"
      data-testid="global-workspace"
      lang={i18n.language}
    >
      {isMac() && (
        <div {...DRAG_REGION_ATTR} className="workspace-drag-region" />
      )}
      <div className="flex min-h-0 flex-1">
        <WorkspaceNavigation
          view={view}
          onSelect={navigate}
          footer={
            showProfiles && (
              <ProfileSwitcher
                compact
                nativeAgentPicker={false}
                side="top"
                activeApp={selectedApps[0] ?? "claude"}
                selectedApps={selectedApps}
                triggerClassName="workspace-profile-trigger"
              />
            )
          }
        />
        <main
          className={`workspace-content min-w-0 flex-1 ${view === "agents" ? "overflow-hidden" : "overflow-y-auto"}`}
          key={view}
        >
          {view === "agents" && (
            <AgentConnectionsPage
              onEditingChange={setEditor}
              onAddProvider={addProvider}
            />
          )}
          {view === "providers" && (
            <RoutingUpstreamsPanel
              startAdding={addingProvider}
              onEditingChange={setEditor}
            />
          )}
          {view === "routing" && (
            <RoutingPage
              onEditingChange={setEditor}
              onAddProvider={addProvider}
            />
          )}
          {view === "stats" && <StatisticsPage />}
          {view === "security" && (
            <SecurityCenterPage headerAction={<GlobalSecurityToggle />} />
          )}
          {view === "settings" && (
            <SettingsPage
              open
              onOpenChange={() => setView("agents")}
              globalResourcesOnly
            />
          )}
        </main>
      </div>
      <footer className="workspace-footer">
        <span>Vary AI</span>
        <GatewayRuntimeStatus
          running={gateway.data?.running}
          unavailable={gateway.isError}
        />
      </footer>
      <FirstRunNoticeDialog
        onConnected={() => {
          setAddingProvider(false);
          setView("agents");
        }}
      />
      <ConfirmDialog
        isOpen={!!nextView}
        title={t("providerFlow.discardTitle")}
        message={t("providerFlow.discardHint")}
        confirmText={t("providerFlow.discard")}
        onCancel={() => setNextView(null)}
        onConfirm={() => {
          if (nextView) {
            setAddingProvider(false);
            setView(nextView);
          }
          setNextView(null);
        }}
      />
    </div>
  );
}
