import { invoke } from "@tauri-apps/api/core";

export type DoubaoCheckinOutcome =
  | "not_checked"
  | "claimed"
  | "already_checked"
  | "inactive"
  | "switch_required"
  | "sign_in_required"
  | "connection_required"
  | "unsupported"
  | "failed";

export interface DoubaoCheckinAccount {
  id: string;
  label: string;
  current: boolean;
  autoEnabled: boolean;
  outcome: DoubaoCheckinOutcome;
  checkedAt: number | null;
  credit: number | null;
  streak: number | null;
  totalDays: number | null;
}

export interface DoubaoCheckinStatus {
  connected: boolean;
  day: string;
  outcome: DoubaoCheckinOutcome;
  accounts: DoubaoCheckinAccount[];
}

export const doubaoCheckinApi = {
  accounts: () => invoke<DoubaoCheckinStatus>("get_doubao_checkin_accounts"),
  checkIn: (accountId: string) =>
    invoke<DoubaoCheckinStatus>("check_in_doubao", { accountId }),
  setAutoEnabled: (accountId: string, enabled: boolean) =>
    invoke<DoubaoCheckinStatus>("set_doubao_auto_checkin", {
      accountId,
      enabled,
    }),
};
