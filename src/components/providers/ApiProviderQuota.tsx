import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { globalProviderPresets } from "@/config/globalProviderPresets";
import {
  routingUpstreamsApi,
  type ApiProviderAllowance,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { SubscriptionQuotaSummary } from "./SubscriptionQuotaSummary";

type Snapshot = { quota?: ApiProviderAllowance; failed: boolean };
export function ApiProviderQuota({
  record,
  compact = true,
  disabled = false,
}: {
  record: RoutingUpstream;
  compact?: boolean;
  disabled?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const preset = globalProviderPresets.find(
    (entry) => entry.id === record.presetId,
  );
  const enabled =
    record.source.kind === "api" &&
    record.revision > 0 &&
    record.hasApiKey &&
    preset?.kind !== "vendor";
  const queryKey = ["routing-provider-quota", record.id, record.revision];
  const query = useQuery({
    queryKey,
    enabled,
    retry: false,
    staleTime: (query) =>
      (query.state.data as Snapshot | undefined)?.quota?.status ===
      "unsupported"
        ? 6 * 60 * 60_000
        : 60_000,
    gcTime: 6 * 60 * 60_000,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    queryFn: async (): Promise<Snapshot> => {
      const previous = client.getQueryData<Snapshot>(queryKey)?.quota;
      try {
        const quota = await routingUpstreamsApi.quota(
          record.id,
          record.revision,
        );
        if (quota.status === "unsupported" && previous?.status === "ready") {
          return { quota: previous, failed: true };
        }
        return { quota, failed: false };
      } catch {
        return {
          quota: previous?.status === "ready" ? previous : undefined,
          failed: true,
        };
      }
    },
  });
  const quota = query.data?.quota;
  if (!enabled || quota?.status === "unsupported") return null;
  const status =
    quota?.status === "unauthorized"
      ? "subscriptionProvider.quotaKeyRejected"
      : !query.data
        ? "codexProvider.quotaLoading"
        : query.data.failed
          ? quota
            ? "codexProvider.quotaStale"
            : "codexProvider.quotaFailed"
          : null;
  const items = (quota?.tiers ?? [])
    .filter((tier) => Number.isFinite(tier.utilization))
    .map((tier) => ({
      tier,
      label: t(
        `subscription.${tier.name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())}`,
        { defaultValue: tier.name },
      ),
    }));
  const summary = (
    <SubscriptionQuotaSummary
      label={`${t("subscriptionProvider.allowance")} ${record.name}`}
      items={items}
      status={status}
      fetching={query.isFetching}
      loading={!query.data && query.isFetching}
    >
      {quota?.balance != null && Number.isFinite(quota.balance) && (
        <span>
          {t("subscriptionProvider.balance", {
            amount: new Intl.NumberFormat(i18n.language, {
              style: "currency",
              currency: "USD",
            }).format(quota.balance),
          })}
        </span>
      )}
    </SubscriptionQuotaSummary>
  );
  if (compact) return summary;
  return (
    <div className="pv-field">
      <span className="pv-field-label">
        {t("subscriptionProvider.allowance")}
      </span>
      <div className="cx-quota">
        <div className="cx-quota-toolbar">
          <button
            type="button"
            aria-label={`${t("codexProvider.refreshQuota")} ${record.name}`}
            disabled={disabled || query.isFetching}
            onClick={() => void query.refetch({ cancelRefetch: false })}
          >
            <RefreshCw
              size={12}
              className={
                query.isFetching
                  ? "animate-spin motion-reduce:animate-none"
                  : undefined
              }
            />
          </button>
        </div>
        {summary}
        {items.map(({ tier, label }, index) => (
          <div key={`${tier.name}-${index}`}>
            <span>{label}</span>
            <span className="cx-meter">
              <i
                style={{
                  width: `${Math.max(0, Math.min(100, 100 - tier.utilization))}%`,
                }}
              />
            </span>
            {tier.resetsAt && Number.isFinite(Date.parse(tier.resetsAt)) && (
              <span>
                {t("codexProvider.resets", {
                  time: new Date(tier.resetsAt).toLocaleString(i18n.language),
                })}
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
