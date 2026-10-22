import { invoke } from "@tauri-apps/api/core";

export interface NotificationPermissionStatus {
  authorization:
    | "notDetermined"
    | "denied"
    | "authorized"
    | "provisional"
    | "ephemeral"
    | "unknown";
  platform: string;
  isDev: boolean;
  canRequest: boolean;
  canOpenSettings: boolean;
  alertsEnabled: boolean | null;
  soundEnabled: boolean | null;
}

export const notificationPermissionsApi = {
  getStatus: () =>
    invoke<NotificationPermissionStatus>("get_notification_permission_status"),
  request: () => invoke<void>("request_notification_permission"),
  openSettings: () => invoke<void>("open_notification_settings"),
};
