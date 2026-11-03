import { invoke } from "@tauri-apps/api/core";

export type WorkBuddyCheckinOutcome =
  | "not_checked"
  | "claimed"
  | "already_checked"
  | "inactive"
  | "ineligible"
  | "own_app_required"
  | "sign_in_required"
  | "credentials_unavailable"
  | "failed";

export interface WorkBuddyCheckinStatus {
  autoEnabled: boolean;
  day: string;
  outcome: WorkBuddyCheckinOutcome;
  checkedAt: number | null;
  credit: number | null;
  streak: number | null;
  creditDelayed: boolean;
}

export const workbuddyCheckinApi = {
  status: () => invoke<WorkBuddyCheckinStatus>("get_workbuddy_checkin_status"),
  checkIn: () => invoke<WorkBuddyCheckinStatus>("check_in_workbuddy"),
  setAutoEnabled: (enabled: boolean) =>
    invoke<WorkBuddyCheckinStatus>("set_workbuddy_auto_checkin", { enabled }),
};
