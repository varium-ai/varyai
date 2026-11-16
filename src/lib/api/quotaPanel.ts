import { invoke } from "@tauri-apps/api/core";

export type CreditsMode = "on" | "off" | "first" | "last";
export interface QuotaPolicy {
  week: boolean;
  expiring: boolean;
  credits: CreditsMode;
}
export interface QuotaPoint {
  at: number;
  remaining: number;
  resetsAt: number;
  span: number;
}
export interface QuotaControls {
  identity: string;
  policy: QuotaPolicy;
  resets: Array<{ id: string; until: number | null }>;
  pending: boolean;
  pendingCredit?: string | null;
}
export const quotaPanelApi = {
  history: (key: string) =>
    invoke<QuotaPoint[]>("quota_panel_history", { key }),
  note: (key: string, point: QuotaPoint) =>
    invoke<QuotaPoint[]>("quota_panel_note", { key, point }),
  codex: (account: string) =>
    invoke<QuotaControls>("quota_panel_codex", { account }),
  policy: (account: string, identity: string, policy: QuotaPolicy) =>
    invoke<QuotaPolicy>("quota_panel_policy", { account, identity, policy }),
  reset: (account: string, identity: string, credit: string) =>
    invoke<{
      code: "reset" | "nothing_to_reset" | "no_credit" | "already_redeemed";
      windows: number;
    }>("quota_panel_reset", { account, identity, credit, confirmed: true }),
};
