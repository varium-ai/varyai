import { invoke } from "@tauri-apps/api/core";

export interface GatewayPolicy {
  name: string;
  models: string[];
  allModels?: boolean;
  tokenLimit: number | null;
  period: "never" | "daily" | "monthly";
}
export interface GatewayKey {
  id: string;
  policy: GatewayPolicy;
  revoked: boolean;
  usedTokens: number;
  resetsAt: number | null;
  agents?: string[];
  secretAvailable?: boolean;
}
export interface GatewayCatalog {
  agents: { id: string; localRouting: boolean }[];
  models: { agent: string; provider: string; model: string }[];
}
export interface GatewayUsage {
  keyId: string;
  window: number;
  agent: string;
  model: string;
  upstreamModel?: string | null;
  tokens: number;
  requests: number;
  unmetered: number;
  lastUsed: number;
}
export const gatewayApi = {
  catalog: () => invoke<GatewayCatalog>("get_gateway_catalog"),
  bind: (app: string, id: string | null) =>
    invoke<void>("bind_gateway_agent", { app, id }),
  usage: (id: string | null = null) =>
    invoke<GatewayUsage[]>("list_gateway_usage", { id }),
  list: () => invoke<GatewayKey[]>("list_gateway_keys"),
  create: (policy: GatewayPolicy) =>
    invoke<{ key: GatewayKey; secret: string }>("create_gateway_key", {
      policy,
    }),
  update: (id: string, policy: GatewayPolicy) =>
    invoke<void>("update_gateway_key", { id, policy }),
  revoke: (id: string) => invoke<void>("revoke_gateway_key", { id }),
  delete: (id: string) => invoke<void>("delete_gateway_key", { id }),
  reveal: (id: string) => invoke<string | null>("reveal_gateway_key", { id }),
};
