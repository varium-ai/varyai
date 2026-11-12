import { invoke } from "@tauri-apps/api/core";
import type { RoutingClientStatus } from "./intentRouting";
export interface AgentConnection {
  app: string;
  enabled: boolean;
  configured: boolean;
  recoveryPending: boolean;
  recoveryReason?:
    | "configuration_changed"
    | "connection_inactive"
    | "orphaned"
    | "bridge_pending"
    | "bridge_model_rejected"
    | "bridge_selection_pending"
    | null;
  model: string | null;
  restartRequired: boolean;
  reloadState?:
    | "current"
    | "stale_catalog"
    | "waiting_for_idle"
    | "restarting"
    | "failed"
    | "stopped"
    | null;
  desktopState?:
    | "stopped"
    | "current"
    | "restart_required"
    | "unknown"
    | "unsupported"
    | null;
  nativeCompatibility?: RoutingClientStatus["nativeCompatibility"] | null;
  configurationIssue?: RoutingClientStatus["configurationIssue"];
}
export interface CodexProxyIssue {
  target: string;
  proxy: string;
  source: "environment" | "system";
}
export interface AgentModel {
  id: string;
  reference?: string;
  name: string;
  contextWindow: number | null;
  group?: string;
  reasoningLevels?: string[];
  reasoning?: boolean;
  defaultReasoningLevel?: string | null;
  images?: boolean;
  icon?: string | null;
}
export interface AgentModelVisibility {
  only: boolean;
  ids: string[];
  order?: string[];
}
export interface AgentNativeModel {
  app: string;
  model: string | null;
  available: boolean;
}
export interface AgentModelControl {
  authorized: boolean;
  ready: boolean;
  pending: boolean;
}
export const agentConnectionsApi = {
  get: (app: string) =>
    invoke<AgentConnection>("get_agent_connection", { app }),
  nativeModel: (app: string) =>
    invoke<AgentNativeModel>("get_agent_native_model", { app }),
  modelControl: (app: string) =>
    invoke<AgentModelControl>("get_agent_model_control", { app }),
  authorizeModelControl: (app: string, confirmed: boolean) =>
    invoke<void>("authorize_agent_model_control", { app, confirmed }),
  list: () => invoke<AgentConnection[]>("list_agent_connections"),
  diagnoseCodexProxy: (force = false) =>
    invoke<CodexProxyIssue | null>("diagnose_codex_proxy", { force }),
  models: () => invoke<AgentModel[]>("get_agent_model_catalog"),
  nativeModels: () => invoke<AgentNativeModel[]>("get_agent_native_models"),
  set: (app: string, enabled: boolean, model: string | null = null) =>
    invoke<void>("set_agent_connection", { app, enabled, model }),
  reapply: (app: string) => invoke<void>("reapply_agent_connection", { app }),
  restart: (app: string, confirmed: boolean) =>
    invoke<void>("restart_agent_connection", { app, confirmed }),
  modelVisibility: (app: string) =>
    invoke<AgentModelVisibility>("get_agent_model_visibility", { app }),
  setModelVisibility: (app: string, visibility: AgentModelVisibility) =>
    invoke<void>("set_agent_model_visibility", { app, visibility }),
};
