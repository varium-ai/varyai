import { invoke } from "@tauri-apps/api/core";
import type { ClaudeApiFormat, ClaudeApiKeyField } from "@/types";

export type ProbeStatus =
  | "verified"
  | "authentication"
  | "endpoint"
  | "rate_limited"
  | "unavailable"
  | "redirect"
  | "rejected"
  | "timeout"
  | "network"
  | "invalid_config"
  | "invalid_response";
export interface ProbeOutcome {
  status: ProbeStatus;
  statusCode?: number | null;
}
export interface ProtocolDetection {
  format: ClaudeApiFormat;
  text: ProbeOutcome;
  stream: ProbeOutcome | null;
}
export interface DetectionConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
  anthropicAuthField?: ClaudeApiKeyField;
}

/** Explicit inference requests; never call this during background discovery. */
export function detectProviderProtocols(
  config: DetectionConfig,
): Promise<ProtocolDetection[]> {
  return invoke("detect_provider_protocols", { config });
}
