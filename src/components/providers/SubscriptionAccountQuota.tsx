import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import {
  nativeSubscriptionApi,
  type NativeSubscriptionProvider,
} from "@/lib/api/nativeSubscription";
import { copilotGetUsageForAccount } from "@/lib/api/copilot";
import { subscriptionApi } from "@/lib/api/subscription";
import type { SubscriptionQuota, QuotaTier } from "@/types/subscription";

import { SubscriptionQuotaSummary } from "./SubscriptionQuotaSummary";

type Provider = NativeSubscriptionProvider | "github_copilot" | "xai_oauth";
type Snapshot = {
  quota?: Pick<SubscriptionQuota, "credentialStatus" | "success" | "tiers">;
  failed: boolean;
};

export function SubscriptionAccountQuota({
  provider,
  id,
  login,
  ready,
  disabled,
  compact = false,
}: {
  provider: Provider;
  id: string;
  login: string;
  ready: boolean;
  disabled: boolean;
  compact?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const queryKey = ["subscription-account-quota", provider, id];
  const query = useQuery({
    queryKey,
    enabled: ready && provider !== "chatgpt_api",
    staleTime: 60_000,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    queryFn: async (): Promise<Snapshot> => {
      const previous = client.getQueryData<Snapshot>(queryKey);
      const good = previous?.quota?.success ? previous.quota : undefined;
      try {
        let quota: Snapshot["quota"];
        if (provider === "github_copilot") {
          const usage = await copilotGetUsageForAccount(id);
          const tiers: QuotaTier[] = Object.entries(
            usage.quota_snapshots,
          ).flatMap(([name, detail]) => {
            if (!detail) return [];
            const exhaustedPool =
              name === "premium_interactions" && detail.has_quota === false;
            const percent = detail.percent_remaining;
            if (
              !exhaustedPool &&
              (detail.unlimited ||
                typeof percent !== "number" ||
                !Number.isFinite(percent))
            )
              return [];
            return [
              {
                name,
                utilization: exhaustedPool
                  ? 100
                  : Math.max(0, Math.min(100, 100 - (percent as number))),
                resetsAt: usage.quota_reset_date || null,
              },
            ];
          });
          quota = { credentialStatus: "valid", success: true, tiers };
        } else {
          const value =
            provider === "xai_oauth"
              ? await subscriptionApi.getXaiOauthQuota(id)
              : await nativeSubscriptionApi.quota(provider, id);
          quota = {
            credentialStatus: value.credentialStatus,
            success: value.success,
            tiers: value.tiers,
          };
        }
        if (quota.credentialStatus === "expired") {
          void client.invalidateQueries({
            queryKey: ["native-subscription-accounts", provider],
          });
          void client.invalidateQueries({
            queryKey: ["managed-auth-status", provider],
          });
          void client.invalidateQueries({ queryKey: ["routing-upstreams"] });
        } else if (
          quota.success &&
          (provider === "antigravity" || provider === "gemini_cli")
        ) {
          void client.invalidateQueries({
            queryKey: ["native-subscription-accounts", provider],
          });
          void client.invalidateQueries({ queryKey: ["routing-upstreams"] });
        }
        return {
          quota:
            !quota.success && quota.credentialStatus === "valid"
              ? (good ?? quota)
              : quota,
          failed: !quota.success,
        };
      } catch {
        void client.invalidateQueries({
          queryKey: ["managed-auth-status", provider],
        });
        return { quota: good, failed: true };
      }
    },
  });
  const quota = query.data?.quota;
  const tiers =
    ready && quota?.credentialStatus === "valid" && quota.success
      ? quota.tiers.filter((tier) => Number.isFinite(tier.utilization))
      : [];
  const tierLabel = (name: string) => {
    const known: Record<string, string> = {
      premium_interactions: t("usageScript.premiumRequests"),
      chat: t("subscriptionProvider.chatQuota"),
      completions: t("subscriptionProvider.completionQuota"),
    };
    if (known[name]) return known[name];
    return t(
      `subscription.${name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())}`,
      { defaultValue: name },
    );
  };
  const quotaGroups = (() => {
    const groups: Array<{
      key: string;
      tiers: Array<{ tier: QuotaTier; label: string }>;
    }> = [];
    for (const tier of tiers) {
      const separator = tier.name.indexOf(" · ");
      const group = separator > 0 ? tier.name.slice(0, separator) : "";
      const window = separator > 0 ? tier.name.slice(separator + 3) : "";
      const normalizedGroup = group
        .replace(/\s+models?$/i, "")
        .replace(/^Claude and GPT$/i, "Claude & GPT");
      const normalizedWindow =
        /^(5h|five_hour|.*five hour limit(?: remaining)?)$/i.test(window)
          ? "5h"
          : /^(7d|seven_day|weekly|.*weekly limit(?: remaining)?)$/i.test(
                window,
              )
            ? "7d"
            : window;
      const key = normalizedGroup || tier.name;
      let entry = groups.find((item) => item.key === key);
      if (!entry) {
        entry = { key, tiers: [] };
        groups.push(entry);
      }
      entry.tiers.push({
        tier,
        label: normalizedGroup
          ? `${normalizedGroup} ${normalizedWindow}`
          : tierLabel(tier.name),
      });
    }
    return groups;
  })();
  const status =
    !ready || quota?.credentialStatus === "expired"
      ? "quotaLoginRequired"
      : quota?.credentialStatus === "not_found"
        ? "quotaCredentialsMissing"
        : quota?.credentialStatus === "parse_error"
          ? "quotaCredentialsInvalid"
          : !query.data
            ? "quotaLoading"
            : query.data.failed
              ? tiers.length
                ? "quotaStale"
                : "quotaFailed"
              : tiers.length
                ? null
                : "quotaEmpty";
  if (compact) {
    return (
      <SubscriptionQuotaSummary
        label={`${t("subscriptionProvider.allowance")} ${login}`}
        fetching={query.isFetching}
        items={quotaGroups.flatMap((group) => group.tiers)}
        status={
          provider === "chatgpt_api"
            ? ready
              ? "subscriptionProvider.quotaUnavailable"
              : "codexProvider.quotaLoginRequired"
            : status
              ? `codexProvider.${status}`
              : null
        }
      />
    );
  }
  return (
    <div
      className="cx-quota"
      role="status"
      aria-label={`${t("subscriptionProvider.allowance")} ${login}`}
      aria-busy={query.isFetching}
    >
      {provider === "chatgpt_api" ? (
        <span>
          {t(
            ready
              ? "subscriptionProvider.quotaUnavailable"
              : "codexProvider.quotaLoginRequired",
          )}
        </span>
      ) : (
        <>
          <div className="cx-quota-toolbar">
            <button
              type="button"
              aria-label={`${t("codexProvider.refreshQuota")} ${login}`}
              disabled={disabled || !ready || query.isFetching}
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
          <div className="cx-quota-grid">
            {quotaGroups.map((group) => (
              <div className="cx-quota-group" key={group.key}>
                {group.tiers.map(({ tier, label }, index) => (
                  <div className="cx-quota-item" key={`${tier.name}-${index}`}>
                    <span className="cx-quota-label">{label}</span>
                    {!tier.display && (
                      <span className="cx-meter">
                        <i
                          style={{
                            width: `${Math.max(0, Math.min(100, 100 - tier.utilization))}%`,
                          }}
                        />
                      </span>
                    )}
                    <b>
                      {tier.display ??
                        t("codexProvider.remaining", {
                          n: Math.max(
                            0,
                            Math.min(
                              100,
                              Math.round((100 - tier.utilization) * 10) / 10,
                            ),
                          ),
                        })}
                    </b>
                    {tier.resetsAt &&
                      Number.isFinite(Date.parse(tier.resetsAt)) && (
                        <span className="cx-quota-reset">
                          {t("codexProvider.resets", {
                            time: new Date(tier.resetsAt).toLocaleString(
                              i18n.language,
                            ),
                          })}
                        </span>
                      )}
                  </div>
                ))}
              </div>
            ))}
          </div>
          {status && <span>{t(`codexProvider.${status}`)}</span>}
        </>
      )}
    </div>
  );
}
