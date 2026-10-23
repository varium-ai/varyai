import { invoke } from "@tauri-apps/api/core";

export interface GatewayPolicy {
  name: string;
  models: string[];
  tokenLimit: number | null;
  period: "never" | "daily" | "monthly";
}
export interface GatewayKey {
  id: string;
  policy: GatewayPolicy;
  revoked: boolean;
  usedTokens: number;
  resetsAt: number | null;
}
export const gatewayApi = {
  list: () => invoke<GatewayKey[]>("list_gateway_keys"),
  create: (policy: GatewayPolicy) =>
    invoke<{ key: GatewayKey; secret: string }>("create_gateway_key", {
      policy,
    }),
  update: (id: string, policy: GatewayPolicy) =>
    invoke<void>("update_gateway_key", { id, policy }),
  revoke: (id: string) => invoke<void>("revoke_gateway_key", { id }),
};
