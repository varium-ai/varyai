import { invoke } from "@tauri-apps/api/core";

export type WorkBuddyCheckinOutcome =
  | "not_checked"
  | "claimed"
  | "already_checked"
  | "inactive"
  | "ineligible"
  | "own_app_required"
  | "sign_in_required"
  | "credentials_protected"
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

export interface WorkBuddyCheckinAccount {
  id: string;
  label: string;
  source: "native" | "browser";
  status: WorkBuddyCheckinStatus;
}

export interface WorkBuddyCheckinAccounts {
  accounts: WorkBuddyCheckinAccount[];
  nativeStatus: WorkBuddyCheckinStatus;
}

export const workbuddyCheckinApi = {
  status: () => invoke<WorkBuddyCheckinStatus>("get_workbuddy_checkin_status"),
  accounts: () =>
    invoke<WorkBuddyCheckinAccounts>("get_workbuddy_checkin_accounts"),
  checkIn: (accountId?: string) =>
    invoke<WorkBuddyCheckinStatus>("check_in_workbuddy", { accountId }),
  setAutoEnabled: (enabled: boolean, accountId?: string) =>
    invoke<WorkBuddyCheckinStatus>("set_workbuddy_auto_checkin", {
      enabled,
      accountId,
    }),
  startLogin: () =>
    invoke<{ loginId: string; url: string }>("start_workbuddy_checkin_login"),
  pollLogin: (loginId: string) =>
    invoke<{ complete: boolean; accountId: string | null }>(
      "poll_workbuddy_checkin_login",
      { loginId },
    ),
  cancelLogin: (loginId: string) =>
    invoke<void>("cancel_workbuddy_checkin_login", { loginId }),
  removeAccount: (accountId: string) =>
    invoke<void>("remove_workbuddy_checkin_account", { accountId }),
};
