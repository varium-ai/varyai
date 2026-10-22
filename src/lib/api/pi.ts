import { invoke } from "@tauri-apps/api/core";
import type { UsageScript } from "@/types";

export interface PiCurrentState {
  enabledProviderIds: string[];
  defaultProviderId: string | null;
}

export type PiSessionDiscovery =
  | {
      status: "available";
    }
  | {
      status: "requires_project_context";
      configuredPath: string;
    }
  | {
      status: "unavailable";
      reason: string;
    };

export interface PiRoutingStatus {
  enabled: boolean;
  configured: boolean;
  running: boolean;
  providerIds: string[];
  modelCount: number;
  issues: { providerId: string; modelId: string | null; code: string }[];
  lastRequestAt: number | null;
  incompleteUsageCount?: number;
  errorCode: string | null;
}

export const piApi = {
  getRoutingStatus: () => invoke<PiRoutingStatus>("get_pi_routing_status"),
  previewRouting: (providerIds: string[]) =>
    invoke<PiRoutingStatus>("preview_pi_routing", { providerIds }),
  configureRouting: (providerIds: string[]) =>
    invoke<void>("configure_pi_routing", { providerIds }),
  disableRouting: () => invoke<void>("disable_pi_routing"),
  async getCurrentState(): Promise<PiCurrentState> {
    return await invoke("get_pi_current_state");
  },

  async updateProviderUsageScript(
    id: string,
    usageScript: UsageScript,
  ): Promise<boolean> {
    return await invoke("update_pi_provider_usage_script", {
      id,
      usageScript,
    });
  },

  async getSessionDiscovery(): Promise<PiSessionDiscovery> {
    return await invoke("get_pi_session_discovery");
  },
};
