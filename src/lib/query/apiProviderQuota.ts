import type { QueryClient, UseQueryResult } from "@tanstack/react-query";
import { globalProviderPresets } from "@/config/globalProviderPresets";
import {
  routingUpstreamsApi,
  type ApiProviderAllowance,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";

interface AllowanceSnapshot {
  quota?: ApiProviderAllowance;
  failed: boolean;
  observedAt?: number;
}

export type ApiProviderQuotaObservation = UseQueryResult<AllowanceSnapshot>;

export const apiProviderQuotaKey = (record: RoutingUpstream) =>
  ["routing-provider-quota", record.id, record.revision] as const;

export function apiProviderQuotaEnabled(record: RoutingUpstream) {
  return (
    record.source.kind === "api" &&
    record.revision > 0 &&
    record.hasApiKey &&
    globalProviderPresets.find((entry) => entry.id === record.presetId)
      ?.kind !== "vendor"
  );
}

export function apiProviderQuotaOptions(
  record: RoutingUpstream,
  client: QueryClient,
  poll = false,
) {
  const queryKey = apiProviderQuotaKey(record);
  const missingInterfaceTtl = 6 * 60 * 60_000;
  const unsupportedTtl = (snapshot?: AllowanceSnapshot) => {
    const retry = snapshot?.quota?.retryAfterMs;
    return retry != null && Number.isFinite(retry)
      ? Math.max(1000, Math.min(missingInterfaceTtl, retry))
      : missingInterfaceTtl;
  };
  return {
    queryKey,
    enabled: apiProviderQuotaEnabled(record),
    retry: false,
    staleTime: (query: { state: { data?: AllowanceSnapshot } }) =>
      query.state.data?.quota?.status === "unsupported"
        ? unsupportedTtl(query.state.data)
        : 60_000,
    gcTime: missingInterfaceTtl,
    refetchInterval: (query: {
      state: { data?: AllowanceSnapshot; dataUpdatedAt: number };
    }) =>
      poll
        ? query.state.data?.quota?.status === "unsupported"
          ? Math.max(
              1000,
              unsupportedTtl(query.state.data) -
                (Date.now() - query.state.dataUpdatedAt),
            )
          : 60_000
        : false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    queryFn: async (): Promise<AllowanceSnapshot> => {
      const snapshot = client.getQueryData<AllowanceSnapshot>(queryKey);
      const previous = snapshot?.quota;
      try {
        const quota = await (client.getQueryState(queryKey)?.isInvalidated
          ? routingUpstreamsApi.quota(record.id, record.revision, true)
          : routingUpstreamsApi.quota(record.id, record.revision));
        if (quota.status === "unsupported" && previous?.status === "ready") {
          return {
            quota: previous,
            failed: true,
            observedAt: snapshot?.observedAt,
          };
        }
        return { quota, failed: false, observedAt: Date.now() };
      } catch {
        return {
          quota: previous?.status === "ready" ? previous : undefined,
          failed: true,
          observedAt: snapshot?.observedAt,
        };
      }
    },
  };
}
