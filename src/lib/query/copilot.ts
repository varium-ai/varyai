import { useQuery } from "@tanstack/react-query";
import { copilotGetUsage, copilotGetUsageForAccount } from "@/lib/api/copilot";
import type { QuotaTier } from "@/types/subscription";

const REFETCH_INTERVAL = 5 * 60 * 1000; // 5 minutes

export interface CopilotQuota {
  success: boolean;
  plan: string | null;
  resetDate: string | null;
  tiers: QuotaTier[];
  error: string | null;
  queriedAt: number | null;
}

export interface UseCopilotQuotaOptions {
  enabled?: boolean;
  /** 是否启用自动轮询（5 分钟）与窗口 focus 重取 */
  autoQuery?: boolean;
}

export function useCopilotQuota(
  accountId: string | null,
  options: UseCopilotQuotaOptions = {},
) {
  const { enabled = true, autoQuery = false } = options;
  return useQuery<CopilotQuota>({
    queryKey: ["copilot", "quota", accountId ?? "default"],
    queryFn: async (): Promise<CopilotQuota> => {
      const usage = accountId
        ? await copilotGetUsageForAccount(accountId)
        : await copilotGetUsage();

      const premium = usage.quota_snapshots.premium_interactions;
      const exhausted = premium?.has_quota === false;
      const percent = premium?.percent_remaining;
      const utilization = exhausted
        ? 100
        : typeof percent === "number" && Number.isFinite(percent)
          ? Math.max(0, Math.min(100, 100 - percent))
          : premium?.entitlement != null &&
              premium.entitlement > 0 &&
              premium.remaining != null
            ? Math.max(
                0,
                Math.min(
                  100,
                  (100 * (premium.entitlement - premium.remaining)) /
                    premium.entitlement,
                ),
              )
            : null;

      return {
        success: true,
        plan: usage.copilot_plan,
        resetDate: usage.quota_reset_date,
        tiers:
          utilization == null || (premium?.unlimited && !exhausted)
            ? []
            : [
                {
                  name: "premium",
                  utilization,
                  resetsAt: usage.quota_reset_date,
                },
              ],
        error: null,
        queriedAt: Date.now(),
      };
    },
    enabled,
    refetchInterval: autoQuery ? REFETCH_INTERVAL : false,
    refetchIntervalInBackground: autoQuery,
    refetchOnWindowFocus: autoQuery,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });
}
