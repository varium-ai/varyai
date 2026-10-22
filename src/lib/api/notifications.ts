import { invoke } from "@tauri-apps/api/core";
import {
  isPermissionGranted as pluginIsPermissionGranted,
  requestPermission as pluginRequestPermission,
} from "@tauri-apps/plugin-notification";
import {
  notificationPermissionsApi,
  type NotificationPermissionStatus,
} from "./notificationPermissions";

function usesNativePermissions(status: NotificationPermissionStatus) {
  return status.platform === "macos";
}

function granted(status: NotificationPermissionStatus) {
  return ["authorized", "provisional", "ephemeral"].includes(
    status.authorization,
  );
}

export async function isPermissionGranted(): Promise<boolean> {
  const status = await notificationPermissionsApi.getStatus();
  return usesNativePermissions(status)
    ? granted(status)
    : pluginIsPermissionGranted();
}

export async function requestPermission(): Promise<NotificationPermission> {
  let status = await notificationPermissionsApi.getStatus();
  if (!usesNativePermissions(status)) return pluginRequestPermission();
  if (status.canRequest) {
    await notificationPermissionsApi.request();
    status = await notificationPermissionsApi.getStatus();
  }
  return granted(status) ? "granted" : "denied";
}

/** Resolves when the native backend accepts submission, not when a banner appears. */
export function sendNotification(options: {
  title: string;
  body: string;
}): Promise<void> {
  return invoke("send_system_notification", options);
}
