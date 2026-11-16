import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { QueryClient } from "@tanstack/react-query";
import type { WorkspaceView } from "@/components/layout/WorkspaceNavigation";

export type PanelTab = "agents" | "quota" | "routing" | "security";
export function isTrayPanel(search: string): boolean {
  return new URLSearchParams(search).get("panel") === "tray";
}
export function panelTab(value: string | null): PanelTab {
  return value === "quota" || value === "routing" || value === "security"
    ? value
    : "agents";
}
const roots = new Set([
  "agent-connections",
  "agent-native-models",
  "agent-model-catalog",
  "agent-installations",
  "proxyStatus",
  "intent-routing-groups",
  "routing-trace",
  "security",
  "securityEvents",
  "profiles",
  "routing-upstreams",
  "settings",
]);
export function sharedQueryRoot(key: readonly unknown[]): string | null {
  return typeof key[0] === "string" && roots.has(key[0]) ? key[0] : null;
}
export const trayPanelApi = {
  hide: () => invoke<void>("tray_panel_hide"),
  resize: (height: number) => invoke<void>("tray_panel_resize", { height }),
  open: (view: WorkspaceView) => invoke<void>("tray_panel_open", { view }),
  navigation: () => invoke<WorkspaceView | null>("take_workspace_navigation"),
  quit: () => invoke<void>("tray_panel_quit"),
};

/** Send only allowlisted root keys. Remote invalidation must never echo back. */
export function connectWorkspaceQueries(client: QueryClient): () => void {
  let disposed = false;
  let remote = false;
  let off: (() => void) | undefined;
  const pending = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const unsubscribe = client.getQueryCache().subscribe((event) => {
    if (
      remote ||
      event.type !== "updated" ||
      event.action.type !== "invalidate"
    )
      return;
    const root = sharedQueryRoot(event.query.queryKey);
    if (!root) return;
    pending.add(root);
    if (timer) return;
    timer = setTimeout(() => {
      timer = undefined;
      for (const root of pending)
        void invoke("broadcast_workspace_change", { root }).catch(() => {});
      pending.clear();
    }, 50);
  });
  void listen<unknown>("workspace-invalidated", ({ payload }) => {
    if (typeof payload !== "string" || !roots.has(payload)) return;
    remote = true;
    void client.invalidateQueries({ queryKey: [payload] });
    remote = false;
  })
    .then((stop) => {
      if (disposed) stop();
      else off = stop;
    })
    .catch(() => {});
  return () => {
    disposed = true;
    unsubscribe();
    off?.();
    if (timer) clearTimeout(timer);
  };
}
