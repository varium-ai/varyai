import { invoke } from "@tauri-apps/api/core";

export interface RoutingCandidate {
  providerId: string;
  sourceProviderId?: string | null;
  providerName: string;
  model: string;
  fixedEffort?: string | null;
  contextWindow?: number | null;
  account?: RoutingAccount | null;
}
export type RoutingStepKind =
  | "strategy"
  | "pinned"
  | "affinity"
  | "rule"
  | "paused"
  | "attempt"
  | "retry"
  | "first_token_timeout"
  | "context"
  | "images"
  | "rejected"
  | "connected"
  | "completed"
  | "failed"
  | "cancelled";
export interface RoutingTraceStep {
  kind: RoutingStepKind;
  atMs: number;
  candidate?: number | null;
  status?: number | null;
}
export interface RoutingTrace {
  id: string;
  startedAt: number;
  agent: string;
  kind?: string | null;
  session?: string | null;
  model: string;
  groupName: string;
  strategy: string;
  candidates: RoutingCandidate[];
  steps: RoutingTraceStep[];
  state: "routing" | "streaming" | "completed" | "failed" | "cancelled";
  status?: number | null;
  durationMs: number;
  firstTokenMs?: number | null;
  outputTokensPerSecond?: number | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheReadTokens?: number | null;
  estimatedTokens?: number | null;
  costUsd?: string | null;
}
export interface RoutingAccount {
  id?: string;
  label?: string | null;
  plan?: string | null;
  kind?: "account" | "key" | "provider";
  providerId: string;
  providerName: string;
  model: string;
  restUntil: number;
  sunkAt?: number | null;
  windows: Array<{
    usedPercent: number;
    windowSeconds: number;
    resetsAt: number;
  }>;
  firstTokenMs?: number | null;
  outputTokensPerSecond?: number | null;
}
export interface RoutingTraceSnapshot {
  requests: RoutingTrace[];
  accounts: RoutingAccount[];
  capacity: number;
  running: boolean;
  days: string[];
  historyError?: boolean;
}
export interface RoutingTraceHistory {
  requests: RoutingTrace[];
  cut: boolean;
}
export const routingTraceApi = {
  snapshot: () => invoke<RoutingTraceSnapshot>("routing_trace_snapshot"),
  history: (day: string) =>
    invoke<RoutingTraceHistory>("routing_trace_history", { day }),
};
