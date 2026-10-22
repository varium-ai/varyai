import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { failoverApi } from "@/lib/api/failover";

// ========== 熔断器 Hooks ==========

/**
 * 获取供应商健康状态
 */
export function useProviderHealth(
  providerId: string,
  appType: string,
  enabled = true,
) {
  return useQuery({
    queryKey: ["providerHealth", providerId, appType],
    queryFn: () => failoverApi.getProviderHealth(providerId, appType),
    enabled: enabled && !!providerId && !!appType,
    refetchInterval: 5000, // 每 5 秒刷新一次
    retry: false,
  });
}

// ========== 故障转移队列 Hooks（新） ==========

/**
 * 获取故障转移队列
 */
export function useFailoverQueue(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["failoverQueue", appType],
    queryFn: () => failoverApi.getFailoverQueue(appType),
    enabled: enabled && !!appType,
  });
}

/**
 * 添加供应商到故障转移队列
 */
export function useAddToFailoverQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => failoverApi.addToFailoverQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
    },
  });
}

/**
 * 从故障转移队列移除供应商
 */
export function useRemoveFromFailoverQueue() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      providerId,
    }: {
      appType: string;
      providerId: string;
    }) => failoverApi.removeFromFailoverQueue(appType, providerId),
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["failoverQueue", variables.appType],
      });
      queryClient.invalidateQueries({
        queryKey: ["providers", variables.appType],
      });
      // 清除该供应商的健康状态缓存（退出队列后不再需要健康监控）
      queryClient.invalidateQueries({
        queryKey: ["providerHealth", variables.providerId, variables.appType],
      });
    },
  });
}

// ========== 自动故障转移开关 Hooks ==========

/**
 * 获取指定应用的自动故障转移开关状态
 */
export function useAutoFailoverEnabled(appType: string, enabled = true) {
  return useQuery({
    queryKey: ["autoFailoverEnabled", appType],
    queryFn: () => failoverApi.getAutoFailoverEnabled(appType),
    enabled: enabled && !!appType,
    // 默认值为 false（与后端保持一致）
    placeholderData: false,
  });
}
