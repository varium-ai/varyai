import { invoke } from "@tauri-apps/api/core";

export interface TelemetryPreferences {
  enabled: boolean;
  available: boolean;
}

export const telemetryApi = {
  getPreferences: () =>
    invoke<TelemetryPreferences>("get_telemetry_preferences"),
  setEnabled: (enabled: boolean) =>
    invoke<TelemetryPreferences>("set_telemetry_enabled", { enabled }),
};
