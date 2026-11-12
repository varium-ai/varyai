import { invoke } from "@tauri-apps/api/core";
export type NativeSubscriptionProvider =
  | "cursor"
  | "claude"
  | "chatgpt_api"
  | "gemini_cli"
  | "antigravity";
export interface SubscriptionAccount {
  id: string;
  login: string;
  project: string;
  reauthRequired?: boolean;
  plan?: string | null;
}
export interface SubscriptionLogin {
  id: string;
  url?: string;
  state: "waiting" | "done" | "failed";
  account?: SubscriptionAccount;
  error?: string;
}
export const nativeSubscriptionApi = {
  quota: (provider: NativeSubscriptionProvider, id: string) =>
    invoke<import("@/types/subscription").SubscriptionQuota>(
      "native_subscription_quota",
      { provider, id },
    ),
  accounts: (provider: NativeSubscriptionProvider) =>
    invoke<SubscriptionAccount[]>("native_subscription_accounts", { provider }),
  start: (
    provider: NativeSubscriptionProvider,
    targetAccountId?: string,
    proxy?: string,
  ) =>
    invoke<SubscriptionLogin>("native_subscription_start", {
      provider,
      targetAccountId,
      proxy,
    }),
  poll: (id: string) =>
    invoke<SubscriptionLogin>("native_subscription_poll", { id }),
  cancel: (id: string) => invoke<void>("native_subscription_cancel", { id }),
  callback: (id: string, url: string) =>
    invoke<void>("native_subscription_callback", { id, url }),
  import: (provider: NativeSubscriptionProvider, path?: string) =>
    invoke<SubscriptionAccount>("native_subscription_import", {
      provider,
      path,
    }),
  remove: (provider: NativeSubscriptionProvider, id: string) =>
    invoke<void>("native_subscription_remove", { provider, id }),
  project: (
    provider: NativeSubscriptionProvider,
    id: string,
    project: string,
  ) => invoke<void>("native_subscription_project", { provider, id, project }),
  models: (provider: NativeSubscriptionProvider, id: string) =>
    invoke<Array<Record<string, unknown>>>("native_subscription_models", {
      provider,
      id,
    }),
};
