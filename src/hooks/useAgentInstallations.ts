import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { proxyApi } from "@/lib/api/proxy";

export function useAgentInstallations() {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ["agent-installations"],
    queryFn: proxyApi.getAgentInstallations,
    refetchInterval: 30_000,
  });
  const previousPresence = useRef<Record<string, boolean>>();
  useEffect(() => {
    if (!query.data) return;
    const previous = previousPresence.current;
    previousPresence.current = query.data;
    if (
      previous &&
      Object.keys({ ...previous, ...query.data }).some(
        (app) => !!previous[app] !== !!query.data[app],
      )
    ) {
      void client.invalidateQueries(
        { queryKey: ["agent-desktop-versions"] },
        { cancelRefetch: false },
      );
    }
  }, [query.data, client]);
  useEffect(() => {
    const refresh = () => {
      void client.invalidateQueries(
        { queryKey: ["agent-installations"] },
        { cancelRefetch: false },
      );
      // Version queries otherwise keep a five-minute snapshot of missing apps.
      void client.invalidateQueries(
        { queryKey: ["agent-desktop-versions"] },
        { cancelRefetch: false },
      );
    };
    const visible = () => {
      if (document.visibilityState === "visible") refresh();
    };
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", visible);
    const unlisten = isTauri()
      ? getCurrentWindow()
          .onFocusChanged(({ payload }) => {
            if (payload) refresh();
          })
          .catch(() => undefined)
      : undefined;
    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", visible);
      void unlisten?.then((dispose) => dispose?.());
    };
  }, [client]);
  return query;
}
