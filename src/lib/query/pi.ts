import { useQuery, type QueryClient } from "@tanstack/react-query";
import { piApi } from "@/lib/api/pi";

export const piKeys = {
  all: ["pi"] as const,
  currentState: ["pi", "currentState"] as const,
  routing: ["pi", "routing"] as const,
  routingPreview: ["pi", "routingPreview"] as const,
  sessionDiscovery: ["pi", "sessionDiscovery"] as const,
};

export const invalidatePiProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: piKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: piKeys.routing }),
    queryClient.invalidateQueries({ queryKey: piKeys.routingPreview }),
    queryClient.invalidateQueries({ queryKey: ["providers", "pi"] }),
  ]);
};

export const invalidatePiDirectoryCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: piKeys.all }),
    queryClient.invalidateQueries({ queryKey: ["providers", "pi"] }),
    queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
    queryClient.invalidateQueries({ queryKey: ["sessions"] }),
  ]);
};

export function usePiCurrentState(enabled = true) {
  return useQuery({
    queryKey: piKeys.currentState,
    queryFn: () => piApi.getCurrentState(),
    enabled,
  });
}

export function usePiRoutingStatus(enabled = true) {
  return useQuery({
    queryKey: piKeys.routing,
    queryFn: () => piApi.getRoutingStatus(),
    enabled,
    refetchInterval: 2000,
  });
}
