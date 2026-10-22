import { useQuery, useMutation } from "@tanstack/react-query";
import { omoApi, omoSlimApi } from "@/lib/api/omo";

// ── Factory ────────────────────────────────────────────────────

function createOmoQueryKeys(prefix: string) {
  return {
    all: [prefix] as const,
    currentProviderId: () => [prefix, "current-provider-id"] as const,
  };
}

function createOmoQueryHooks(
  variant: "omo" | "omo-slim",
  api: typeof omoApi | typeof omoSlimApi,
) {
  const keys = createOmoQueryKeys(variant);

  function useCurrentProviderId(enabled = true) {
    return useQuery({
      queryKey: keys.currentProviderId(),
      queryFn:
        "getCurrentOmoProviderId" in api
          ? (api as typeof omoApi).getCurrentOmoProviderId
          : (api as typeof omoSlimApi).getCurrentProviderId,
      enabled,
    });
  }

  function useReadLocalFile() {
    return useMutation({
      mutationFn: () => api.readLocalFile(),
    });
  }

  return {
    useCurrentProviderId,
    useReadLocalFile,
  };
}

// ── Instances ──────────────────────────────────────────────────

const omoHooks = createOmoQueryHooks("omo", omoApi);
const omoSlimHooks = createOmoQueryHooks("omo-slim", omoSlimApi);

export const useCurrentOmoProviderId = omoHooks.useCurrentProviderId;
export const useReadOmoLocalFile = omoHooks.useReadLocalFile;

export const useCurrentOmoSlimProviderId = omoSlimHooks.useCurrentProviderId;
export const useReadOmoSlimLocalFile = omoSlimHooks.useReadLocalFile;
