import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import {
  apiProviderQuotaEnabled,
  apiProviderQuotaKey,
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
  const client = useQueryClient();
  const refresh = () =>
    void client.invalidateQueries(
      { queryKey: apiProviderQuotaKey(record), exact: true },
      { cancelRefetch: false },
    );
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
  const wallets =
    quota?.balances ??
    (quota?.balance != null ? [{ unit: "USD", amount: quota.balance }] : []);
  const amounts = wallets.flatMap(({ unit, amount }) => {
    if (!Number.isFinite(amount)) return [];
    if (unit === "magicubes" || unit === "credits") {
      return [
        t(`subscriptionProvider.${unit}Amount`, {
          amount: new Intl.NumberFormat(i18n.language).format(amount),
        }),
      ];
    }
    return /^[A-Z]{3}$/.test(unit)
      ? [
          new Intl.NumberFormat(i18n.language, {
            style: "currency",
            currency: unit,
          }).format(amount),
        ]
      : [];
  });
  const balanceLabels = amounts.map((amount, i) => (
    <span key={i}>{t("subscriptionProvider.balance", { amount })}</span>
  ));
  if (card) {
    if (quota?.status !== "ready" || (!items.length && !amounts.length))
      return null;
    const content = (
      <ProviderQuotaCard
        name={record.name}
        card={card}
        items={items}
        status={status}
        fetching={query.isFetching}
        loading={!query.data && query.isFetching}
        disabled={disabled}
        refresh={refresh}
      >
        {balanceLabels}
      </ProviderQuotaCard>
    );
    return card.renderGroup ? card.renderGroup(content) : content;
  }
  const summary = (
    <SubscriptionQuotaSummary
      label={`${t("subscriptionProvider.allowance")} ${record.name}`}
      items={items}
      status={status}
      fetching={query.isFetching}
      loading={!query.data && query.isFetching}
    >
      {balanceLabels}
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
            onClick={refresh}
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
