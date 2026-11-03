import { invoke } from "@tauri-apps/api/core";
import type { FetchedModel } from "./model-fetch";
import type { ProtocolDetection } from "./provider-protocol-detection";

export interface ProviderCopySource {
  id: string;
  revision: number;
}

export type UpstreamSource =
  | {
      kind: "api";
      baseUrl: string;
      apiFormat:
        | "anthropic"
        | "openai_chat"
        | "openai_responses"
        | "gemini_native";
      apiKey?: string;
      authType?: "api-key" | "bearer" | "both";
      models: unknown[];
      catalog: unknown[];
      mode: "vendor" | "picked" | "best" | "usage";
      groupsOnly: boolean;
    }
  | { kind: "codex" };
export interface RoutingUpstream {
  schemaVersion: 1;
  id: string;
  name: string;
  revision: number;
  source: UpstreamSource;
  hasApiKey: boolean;
  enabled?: boolean;
  presetId?: string | null;
  region?: string | null;
  codexSettings?: CodexProviderSettings | null;
  codex?: CodexProviderEditor | null;
  transport?: {
    primaryProtocol?: string;
    endpoints: Partial<
      Record<Extract<UpstreamSource, { kind: "api" }>["apiFormat"], string>
    >;
    fullUrl: boolean;
    keys: Array<{
      id: string;
      name: string;
      enabled: boolean;
      apiFormat?: string;
    }>;
  } | null;
}
export interface CodexProviderSettings {
  accounts: string[];
  mode: "vendor" | "picked" | "best" | "usage";
  models: Array<Record<string, unknown>>;
  groupsOnly: boolean;
}
export interface CodexProviderEditor {
  settings: CodexProviderSettings;
  accounts: Array<{
    id: string;
    login: string;
    plan?: string | null;
    native: boolean;
    ready: boolean;
  }>;
  models: Array<Record<string, unknown>>;
}
export const routingUpstreamsApi = {
  createCodex: () => invoke<RoutingUpstream>("create_codex_provider"),
  editor: async (id: string, revision: number): Promise<RoutingUpstream> => {
    return invoke<RoutingUpstream>("routing_upstream_editor", { id, revision });
  },
  saveCodex: (upstream: RoutingUpstream, settings: CodexProviderSettings) => {
    const { codex: _editor, ...record } = upstream;
    return invoke<RoutingUpstream>("save_codex_provider", {
      upstream: record,
      settings,
    });
  },
  codexModels: (id: string, revision: number, account: string) =>
    invoke<Array<Record<string, unknown>>>("codex_provider_models", {
      id,
      revision,
      account,
    }),
  codexQuota: (account: string) =>
    invoke<import("@/types/subscription").SubscriptionQuota>(
      "codex_provider_quota",
      { account },
    ),
  importCodexAccount: (path: string) =>
    invoke<void>("import_codex_provider_account", { path }),
  testCodexModel: (
    id: string,
    revision: number,
    account: string,
    model: string,
  ) =>
    invoke<{ ok: boolean; elapsedMs: number }>("test_codex_provider_model", {
      id,
      revision,
      account,
      model,
    }),
  discover: (id: string, revision: number) =>
    invoke<FetchedModel[]>("discover_routing_upstream_models", {
      id,
      revision,
    }),
  list: () => invoke<RoutingUpstream[]>("list_routing_upstreams"),
  save: (upstream: RoutingUpstream, copyFrom?: ProviderCopySource) => {
    const { codex: _editor, ...record } = upstream;
    return invoke<RoutingUpstream>("save_routing_upstream", {
      upstream: record,
      copyFrom,
    });
  },
  probe: (id: string, revision: number, model: string) =>
    invoke<ProtocolDetection[]>("probe_routing_upstream", {
      id,
      revision,
      model,
    }),
  remove: (id: string, revision: number) =>
    invoke<void>("delete_routing_upstream", { id, revision }),
};
