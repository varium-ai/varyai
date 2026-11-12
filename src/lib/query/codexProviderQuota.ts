import { useQueries, useQueryClient } from "@tanstack/react-query";
import {
  routingUpstreamsApi,
  type CodexProviderEditor,
} from "@/lib/api/routingUpstreams";
import type { SubscriptionQuota } from "@/types/subscription";

interface AllowanceSnapshot {
  quota?: Pick<
    SubscriptionQuota,
    "credentialStatus" | "success" | "tiers" | "codex"
  >;
  failed: boolean;
}

export function useCodexProviderQuotas(
  accounts: CodexProviderEditor["accounts"],
) {
  const client = useQueryClient();
  return useQueries({
    queries: accounts.map((account) => {
      const queryKey = [
        "codex-provider-quota",
        account.id,
        account.login,
      ] as const;
      return {
        queryKey,
        queryFn: async (): Promise<AllowanceSnapshot> => {
          const previous = client.getQueryData<AllowanceSnapshot>(queryKey);
          const lastGood = previous?.quota?.success
            ? previous.quota
            : undefined;
          try {
            const { credentialStatus, success, tiers, codex } =
              await routingUpstreamsApi.codexQuota(account.id);
            const quota = { credentialStatus, success, tiers, codex };
            if (credentialStatus === "expired") {
              void client.invalidateQueries({
                queryKey: ["routing-upstreams"],
              });
              void client.invalidateQueries({
                queryKey: ["managed-auth-status", "codex_oauth"],
              });
            }
            return {
              quota:
                !success && credentialStatus === "valid"
                  ? (lastGood ?? quota)
                  : quota,
              failed: !success,
            };
          } catch {
            // Cache failed attempts too; retain meters without exposing vendor errors.
            return { quota: lastGood, failed: true };
          }
        },
        enabled: account.ready,
        staleTime: 60_000,
        retry: false,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      };
    }),
  });
}
