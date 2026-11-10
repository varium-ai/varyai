import { invoke } from "@tauri-apps/api/core";

export type RewardApp = "trae" | "qoder" | "zcode";
export const hasAgentRewards = (app: string): app is RewardApp =>
  ["trae", "qoder", "zcode"].includes(app);
export type RewardOutcome =
  | "not_checked"
  | "claimed"
  | "already_checked"
  | "inactive"
  | "ineligible"
  | "own_app_required"
  | "sign_in_required"
  | "credentials_protected"
  | "credentials_unavailable"
  | "connection_required"
  | "switch_required"
  | "unsupported"
  | "failed";
export interface AgentRewardStatus {
  source?: "native" | "browser" | null;
  nativeAvailable?: boolean;
  browserAccountId?: string | null;
  app: RewardApp;
  accountId: string | null;
  label: string | null;
  autoEnabled: boolean;
  autoConfigured?: boolean;
  outcome: RewardOutcome;
  credit: number | null;
  checkedAt: number | null;
  nextCheckAt: number | null;
}
export const agentRewardsApi = {
  nativeAccount: () =>
    invoke<AgentRewardStatus>("connect_qoder_native_rewards"),
  status: (app: RewardApp) =>
    invoke<AgentRewardStatus>("get_agent_rewards", { app }),
  claim: (app: RewardApp, accountId: string) =>
    invoke<AgentRewardStatus>("claim_agent_rewards", { app, accountId }),
  setAuto: (app: RewardApp, accountId: string, enabled: boolean) =>
    invoke<AgentRewardStatus>("set_agent_rewards_auto", {
      app,
      accountId,
      enabled,
    }),
  startLogin: (app: RewardApp) =>
    invoke<{ loginId: string; url: string }>("start_agent_rewards_login", {
      app,
    }),
  pollLogin: (app: RewardApp, loginId: string) =>
    invoke<{ complete: boolean }>("poll_agent_rewards_login", { app, loginId }),
  cancelLogin: (app: RewardApp, loginId: string) =>
    invoke<void>("cancel_agent_rewards_login", { app, loginId }),
  forget: (app: RewardApp, accountId: string) =>
    invoke<void>("forget_agent_rewards_account", { app, accountId }),
};
