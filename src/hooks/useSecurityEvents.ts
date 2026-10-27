import { useEffect, useRef, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { securityEventsApi } from "@/lib/api";
import { reportFrontendError } from "@/lib/frontendLogger";
import type { SecurityEventFilter } from "@/types/securityEvents";

/** Normalize only contract-approved empty values; let IPC reject invalid combinations. */
export function normalizeSecurityEventFilter(
  filter: SecurityEventFilter,
): SecurityEventFilter {
  const normalized = { ...filter };
  for (const key of ["sessionId", "keyword"] as const) {
    if (normalized[key] !== undefined) normalized[key] = normalized[key].trim();
  }
  for (const key of Object.keys(normalized) as (keyof SecurityEventFilter)[]) {
    if (normalized[key] === undefined || normalized[key] === "") {
      delete normalized[key];
    }
  }
  return normalized;
}

export const securityEventsKeys = {
  all: ["securityEvents"] as const,
  pages: ["securityEvents", "page"] as const,
  summaries: ["securityEvents", "summary"] as const,
  page: (filter: SecurityEventFilter, page: number, pageSize: number) =>
    [
      "securityEvents",
      "page",
      normalizeSecurityEventFilter(filter),
      page,
      pageSize,
    ] as const,
  summary: (filter: SecurityEventFilter) =>
    [
      "securityEvents",
      "summary",
      normalizeSecurityEventFilter(filter),
    ] as const,
};

/** Failure to invalidate one query domain must not block others or notification catch-up. */
async function safelyInvalidate(action: () => Promise<unknown>): Promise<void> {
  try {
    await action();
  } catch {
    reportFrontendError("security_events_invalidate", "failed");
  }
}

/** Persisted-event signals refetch active summaries; each list controls its own page refresh. */
export function invalidateSecurityEventsQueries(client: QueryClient) {
  return Promise.all([
    safelyInvalidate(() =>
      client.invalidateQueries({
        queryKey: securityEventsKeys.summaries,
        refetchType: "active",
      }),
    ),
    safelyInvalidate(() =>
      client.invalidateQueries({
        queryKey: securityEventsKeys.pages,
        refetchType: "none",
      }),
    ),
  ]);
}

/** Legacy and unified clear paths invalidate all three domains without a persisted-event signal. */
export function invalidateSecurityEventDomains(client: QueryClient) {
  return Promise.all([
    safelyInvalidate(() =>
      client.invalidateQueries({ queryKey: ["security"] }),
    ),
    safelyInvalidate(() =>
      client.invalidateQueries({ queryKey: ["toolRisk"] }),
    ),
    invalidateSecurityEventsQueries(client),
  ]);
}

export function useSecurityEvents(
  filter: SecurityEventFilter,
  page: number,
  pageSize: number,
) {
  const client = useQueryClient();
  const normalizedFilter = normalizeSecurityEventFilter(filter);
  const query = useQuery({
    queryKey: securityEventsKeys.page(normalizedFilter, page, pageSize),
    queryFn: ({ signal }) => {
      // IPC cannot be aborted; consume the signal so Query discards cancelled request results.
      void signal;
      return securityEventsApi.getEvents(normalizedFilter, page, pageSize);
    },
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
  const clear = useMutation({
    mutationKey: ["securityEvents", "clearEvents"],
    mutationFn: (frozenFilter: SecurityEventFilter) =>
      securityEventsApi.clearEvents(frozenFilter),
    onSuccess: async () => {
      await invalidateSecurityEventDomains(client);
    },
  });
  return {
    eventsResult: query.data,
    isLoading: query.isLoading,
    isError: query.isError,
    refetch: query.refetch,
    clearEvents: clear.mutateAsync,
    isClearing: clear.isPending,
  };
}

export function useSecurityEventsSummary(
  filter: SecurityEventFilter,
  options?: { refetchInterval?: number },
) {
  const normalizedFilter = normalizeSecurityEventFilter(filter);
  return useQuery({
    queryKey: securityEventsKeys.summary(normalizedFilter),
    queryFn: ({ signal }) => {
      void signal;
      return securityEventsApi.getSummary(normalizedFilter);
    },
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    refetchInterval: options?.refetchInterval,
    refetchIntervalInBackground: false,
  });
}

/** Payload-free signals refresh page one; deeper pages remain snapshots until manual refresh. */
export function useSecurityEventsLiveRefresh(
  page: number,
  onRefresh: () => unknown,
) {
  const client = useQueryClient();
  const [hasUpdates, setHasUpdates] = useState(false);
  const current = useRef({ page, onRefresh });
  current.current = { page, onRefresh };

  useEffect(() => {
    let disposed = false;
    const subscriptions: UnlistenFn[] = [];
    const refresh = () => {
      if (disposed) return;
      if (current.current.page !== 1) {
        setHasUpdates(true);
        return;
      }
      try {
        void Promise.resolve(current.current.onRefresh()).catch(() => {
          if (!disposed)
            reportFrontendError("security_events_refresh", "failed");
        });
      } catch {
        reportFrontendError("security_events_refresh", "failed");
      }
    };
    // Clear emits no recorded signal; observe page invalidation to notify deeper pages.
    const unsubscribeCache = client.getQueryCache().subscribe((event) => {
      if (disposed || current.current.page === 1) return;
      if (
        event.type === "updated" &&
        event.action.type === "invalidate" &&
        event.query.queryKey[0] === "securityEvents" &&
        event.query.queryKey[1] === "page"
      ) {
        setHasUpdates(true);
      }
    });
    // Already-stale queries may not emit invalidation again; successful clears still notify deeper pages.
    const unsubscribeMutations = client
      .getMutationCache()
      .subscribe((event) => {
        if (
          disposed ||
          current.current.page === 1 ||
          event.type !== "updated" ||
          event.action.type !== "success"
        )
          return;
        const key = event.mutation.options.mutationKey;
        if (
          (key?.[0] === "securityEvents" && key[1] === "clearEvents") ||
          (key?.[0] === "security" && key[1] === "clearLogs") ||
          (key?.[0] === "toolRisk" && key[1] === "clearEvents")
        ) {
          setHasUpdates(true);
        }
      });
    void Promise.all(
      ["security-audit-recorded", "tool-risk-recorded"].map(async (name) => {
        try {
          const off = await listen(name, refresh);
          if (disposed) off();
          else subscriptions.push(off);
          return true;
        } catch {
          if (!disposed)
            reportFrontendError("security_events_listen", "failed");
          return false;
        }
      }),
    ).then((subscribed) => {
      // Catch up after subscription; failure of one listener must not block the other source.
      if (subscribed.some(Boolean)) refresh();
    });
    return () => {
      disposed = true;
      unsubscribeCache();
      unsubscribeMutations();
      subscriptions.forEach((off) => off());
    };
  }, [client]);

  return { hasUpdates, resetUpdates: () => setHasUpdates(false) };
}
