import { invoke } from "@tauri-apps/api/core";
export interface CodexBrowserLogin {
  id: string;
  url: string;
  status: "waiting" | "done" | "failed";
  callbackBlocked: boolean;
  account?: { id: string; login: string } | null;
  error?: string | null;
}
export const codexBrowserLoginApi = {
  start: (targetAccountId?: string, proxy?: string) =>
    invoke<CodexBrowserLogin>("codex_browser_login_start", {
      targetAccountId: targetAccountId ?? null,
      proxy: proxy || null,
    }),
  poll: (id: string) =>
    invoke<CodexBrowserLogin>("codex_browser_login_poll", { id }),
  callback: (id: string, callbackUrl: string) =>
    invoke<void>("codex_browser_login_callback", { id, callbackUrl }),
  cancel: (id: string) => invoke<boolean>("codex_browser_login_cancel", { id }),
};
