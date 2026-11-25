import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { TIER_I18N_KEYS } from "@/components/SubscriptionQuotaFooter";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { subscriptionApi } from "@/lib/api/subscription";
import {
  apiProviderQuotaEnabled,
  apiProviderQuotaOptions,
} from "@/lib/query/apiProviderQuota";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";

type Reading = {
  tiers: QuotaTier[];
  creditsExpiresAt?: string;
};

function quotaReading(
  quota: SubscriptionQuota & { creditsExpiresAt?: string },
): Reading[] {
  return quota.success && quota.credentialStatus === "valid" ? [quota] : [];
}

export function RoutingMemberQuotaDates({
  upstream,
  relayExpiry,
  modelId,
  loadingUpstreams,
}: {
  upstream?: RoutingUpstream;
  relayExpiry?: number;
  modelId: string;
  loadingUpstreams: boolean;
}) {
  const { i18n, t } = useTranslation();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ["routing-member-quota-dates", upstream?.id, upstream?.revision],
    enabled:
      !!upstream &&
      upstream.enabled !== false &&
      (upstream.source.kind === "codex" || !upstream.reauthRequired),
    staleTime: 60_000,
    refetchInterval: 60_000,
    retry: false,
    queryFn: async (): Promise<Reading[]> => {
      if (!upstream) return [];
      const { source } = upstream;
      if (source.kind === "codex") {
        const accounts = upstream.codexSettings?.accounts ?? ["native"];
        const results = await Promise.allSettled(
          accounts.map((id) => routingUpstreamsApi.codexQuota(id)),
        );
        return results.flatMap((result) =>
          result.status === "fulfilled" ? quotaReading(result.value) : [],
        );
      }
      if (source.kind === "subscription") {
        if (
          source.provider === "github_copilot" ||
          source.provider === "chatgpt_api"
        )
          return [];
        return quotaReading(
          source.provider === "xai_oauth"
            ? await subscriptionApi.getXaiOauthQuota(source.accountId)
            : await nativeSubscriptionApi.quota(
                source.provider,
                source.accountId,
              ),
        );
      }
      if (!apiProviderQuotaEnabled(upstream)) return [];
      const { quota } = await client.fetchQuery(
        apiProviderQuotaOptions(upstream, client),
      );
      return quota?.status === "ready" ? [quota] : [];
    },
  });

  const dates = new Map<string, Date>();
  const now = Date.now();
  function addDate(kind: string, time: number) {
    const date = new Date(time);
    if (!Number.isFinite(date.getTime()) || time <= now) return;
    const previous = dates.get(kind);
    if (!previous || time < previous.getTime()) dates.set(kind, date);
  }
  if (relayExpiry != null) addDate("expiry", relayExpiry * 1000);
  for (const reading of query.data ?? []) {
    if (reading.creditsExpiresAt)
      addDate("creditsExpiry", Date.parse(reading.creditsExpiresAt));
    for (const tier of reading.tiers) {
      const family = /^seven_day_(opus|sonnet|fable)$/.exec(tier.name)?.[1];
      if (
        family &&
        !modelId
          .toLowerCase()
          .split(/[^\w]+/)
          .includes(family)
      )
        continue;
      if (tier.resetsAt) addDate(tier.name, Date.parse(tier.resetsAt));
    }
  }
  const multipleAccounts =
    upstream?.source.kind === "codex" &&
    (upstream.codexSettings?.accounts.length ?? 0) > 1;
  return (
    <div className="rt-template-member-expiry">
      {dates.size === 0
        ? t(
            loadingUpstreams || query.isFetching
              ? "routingPage.memberQuotaLoading"
              : "routingPage.memberExpiryUnknown",
          )
        : Array.from(dates, ([kind, date]) => (
            <span key={kind}>
              {kind === "expiry"
                ? t("routingPage.memberExpiry")
                : kind === "creditsExpiry"
                  ? t("routingPage.memberCreditsExpiry")
                  : t(
                      multipleAccounts
                        ? "routingPage.memberEarliestReset"
                        : "routingPage.memberReset",
                      {
                        window: t(
                          TIER_I18N_KEYS[kind] ?? `subscription.${kind}`,
                          {
                            defaultValue: kind,
                          },
                        ),
                      },
                    )}{" "}
              <time dateTime={date.toISOString()}>
                {date.toLocaleString(i18n.language, {
                  dateStyle: "medium",
                  timeStyle: "short",
                })}
              </time>
            </span>
          ))}
    </div>
  );
}
