import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import {
  apiProviderQuotaEnabled,
  apiProviderQuotaOptions,
  type ApiProviderQuotaObservation,
} from "@/lib/query/apiProviderQuota";
import { SubscriptionQuotaSummary } from "./SubscriptionQuotaSummary";
import { ProviderQuotaCard, type QuotaCardOptions } from "./ProviderQuotaCard";

interface ApiProviderQuotaProps {
  record: RoutingUpstream;
  compact?: boolean;
  disabled?: boolean;
  card?: QuotaCardOptions;
}

export function ApiProviderQuota(props: ApiProviderQuotaProps) {
  const client = useQueryClient();
  const query = useQuery(
    apiProviderQuotaOptions(props.record, client, !!props.card),
  );
  return <ApiProviderQuotaView {...props} observation={query} />;
}

export function ApiProviderQuotaView({
  record,
  compact = true,
  disabled = false,
  card,
  observation: query,
}: ApiProviderQuotaProps & { observation: ApiProviderQuotaObservation }) {
  const { t, i18n } = useTranslation();
  const quota = query.data?.quota;
  if (!apiProviderQuotaEnabled(record) || quota?.status === "unsupported")
    return null;
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
  if (card)
    return (
      <ProviderQuotaCard
        name={record.name}
        card={card}
        items={items}
        status={status}
        fetching={query.isFetching}
        loading={!query.data && query.isFetching}
        disabled={disabled}
        sample={
          query.data?.observedAt
            ? {
                scope: `api:${record.id}:${record.revision}`,
                at: query.data.observedAt,
              }
            : undefined
        }
        refresh={() => void query.refetch({ cancelRefetch: false })}
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
      </ProviderQuotaCard>
    );
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
