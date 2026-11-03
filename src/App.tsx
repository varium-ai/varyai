import { useEffect, useState } from "react";
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
import { UpdateBadge } from "@/components/UpdateBadge";
import { useSettingsQuery } from "@/lib/query";
import { proxyApi } from "@/lib/api/proxy";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
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
  const settings = useSettingsQuery();
  const gateway = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: proxyApi.getProxyStatus,
    refetchInterval: 5000,
  });
  useUsageCacheBridge();
  useEffect(() => {
    const language = settings.data?.language;
    if (
      language &&
      ["en", "zh", "zh-TW", "ja"].includes(language) &&
      i18n.language !== language
    )
      void i18n.changeLanguage(language);
  }, [settings.data?.language, i18n]);
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
    >
      {isMac() && (
        <div {...DRAG_REGION_ATTR} className="workspace-drag-region" />
      )}
      <div className="flex min-h-0 flex-1">
        <WorkspaceNavigation view={view} onSelect={navigate} />
        <main className="min-w-0 flex-1 overflow-y-auto" key={view}>
          {view === "agents" && (
            <AgentConnectionsPage
              onEditingChange={setEditor}
              onAddProvider={() => {
                setAddingProvider(true);
                setView("providers");
              }}
            />
          )}
          {view === "providers" && (
            <RoutingUpstreamsPanel
              startAdding={addingProvider}
              onEditingChange={setEditor}
            />
          )}
          {view === "routing" && <RoutingPage onEditingChange={setEditor} />}
          {view === "stats" && (
            <StatisticsPage onOpenSecurity={() => setView("security")} />
          )}
          {view === "security" && (
            <>
              <div className="flex justify-end px-6 pt-5">
                <GlobalSecurityToggle />
              </div>
              <SecurityCenterPage />
            </>
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
        <div className="flex items-center gap-4">
          <span className="flex items-center gap-1.5">
            <span
              className={`h-1.5 w-1.5 rounded-full ${gateway.data?.running ? "bg-emerald-500" : "bg-slate-300"}`}
            />
            {t(
              gateway.isError
                ? "workspaceNav.gatewayUnavailable"
                : gateway.data?.running
                  ? "workspaceNav.gatewayRunning"
                  : "workspaceNav.gatewayStopped",
            )}
          </span>
          <UpdateBadge />
        </div>
      </footer>
      <FirstRunNoticeDialog />
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
