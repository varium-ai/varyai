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
  | { kind: "codex" }
  | {
      kind: "subscription";
      provider:
        | "github_copilot"
        | "xai_oauth"
        | "claude"
        | "chatgpt_api"
        | "gemini_cli"
        | "antigravity";
      accountId: string;
      models: unknown[];
      catalog: unknown[];
      mode: "vendor" | "picked" | "best" | "usage";
      groupsOnly: boolean;
    };
export interface RoutingUpstream {
  schemaVersion: 1;
  id: string;
  name: string;
  notes?: string | null;
  icon?: string | null;
  revision: number;
  source: UpstreamSource;
  hasApiKey: boolean;
  subscriptionPlans?: Array<string | null>;
  reauthRequired?: boolean;
  enabled?: boolean;
  presetId?: string | null;
  region?: string | null;
  codexSettings?: CodexProviderSettings | null;
  codex?: CodexProviderEditor | null;
  transport?: {
    primaryName?: string | null;
    primaryOff?: boolean;
    preferredKey?: string | null;
    primaryProtocol?: string;
    endpoints: Partial<
      Record<Extract<UpstreamSource, { kind: "api" }>["apiFormat"], string>
    >;
    fullUrl: boolean;
    keys: Array<{
      id: string;
      name: string;
      enabled: boolean;
      key?: string;
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
    aliases?: string[];
  }>;
  models: Array<Record<string, unknown>>;
}
export const routingUpstreamsApi = {
  keys: (id: string, revision: number) =>
    invoke<Array<{ id: string; label: string; running: number | null }>>(
      "routing_upstream_keys",
      { id, revision },
    ),
  revealKey: (id: string, revision: number, keyId: string) =>
    invoke<string>("reveal_routing_upstream_key", { id, revision, keyId }),
  fetchIcon: (url: string, automatic = false) =>
    invoke<string>("fetch_provider_icon", { url, automatic }),
  createCodex: () => invoke<RoutingUpstream>("create_codex_provider"),
  editor: async (id: string, revision: number): Promise<RoutingUpstream> => {
    return invoke<RoutingUpstream>("routing_upstream_editor", { id, revision });
  },
  saveCodex: (upstream: RoutingUpstream, settings: CodexProviderSettings) => {
    const {
      codex: _editor,
      subscriptionPlans: _plans,
      reauthRequired: _reauth,
      ...record
    } = upstream;
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
    invoke<{ id: string }>("import_codex_provider_account", { path }),
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
    const {
      codex: _editor,
      subscriptionPlans: _plans,
      reauthRequired: _reauth,
      ...record
    } = upstream;
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
