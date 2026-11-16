import type { TFunction } from "i18next";
import type { RequestLog } from "@/types/usage";

export type UsageSource = Pick<RequestLog, "providerId" | "providerName">;

// Match both the internal ID and the backend's fallback name. A saved provider
// can have a session-like name, and a resolved provider name takes precedence.
const SESSION_SOURCE_APPS = new Map([
  ["_session", "Claude"],
  ["_codex_session", "Codex"],
  ["_gemini_session", "Gemini"],
  ["_opencode_session", "OpenCode"],
  ["_grok_session", "Grok Build"],
  ["_pi_session", "Pi"],
  // Only Pi session details receive this fallback; historical rollups do not.
  ["varyai", "Pi"],
]);

export function getSessionSourceApp(source: UsageSource): string | undefined {
  const app = SESSION_SOURCE_APPS.get(source.providerId);
  return app && source.providerName === `${app} (Session)` ? app : undefined;
}

export function getUsageSourceLabel(source: UsageSource, t: TFunction): string {
  return (
    getSessionSourceApp(source) ||
    source.providerName ||
    t("usage.unknownProvider")
  );
}
