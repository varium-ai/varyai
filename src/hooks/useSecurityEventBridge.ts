import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { reportFrontendError } from "@/lib/frontendLogger";
import { securityKeys } from "@/hooks/useSecurityAudit";

export function useSecurityEventBridge(onRecorded: () => void) {
  const queryClient = useQueryClient();
  const callbackRef = useRef(onRecorded);
  callbackRef.current = onRecorded;

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    const refreshQueries = () => {
      try {
        void Promise.resolve(
          queryClient.invalidateQueries({ queryKey: securityKeys.all }),
        ).catch(() =>
          reportFrontendError("security_audit_invalidate", "failed"),
        );
      } catch {
        reportFrontendError("security_audit_invalidate", "failed");
      }
    };
    void (async () => {
      try {
        const off = await listen("security-audit-recorded", () => {
          if (disposed) return;

          refreshQueries();
          callbackRef.current();
        });
        if (disposed) off();
        else {
          unlisten = off;

          refreshQueries();
          callbackRef.current();
        }
      } catch {
        if (!disposed) reportFrontendError("security_audit_listen", "failed");
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}
