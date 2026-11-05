"use strict";
// Invoke only the client's existing account and daily-credit IPC operations.
function createTraeRewards(ipc = globalThis.vscode?.ipcRenderer) {
  const prefix = "vscode:sandbox::main-invoke-";
  const account = async () => {
    const user = await ipc.invoke(prefix + "getUserInfo");
    if (!user?.userId) return null;
    const digest = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode("trae|" + user.userId),
    );
    return {
      accountId: Array.from(new Uint8Array(digest), (b) =>
        b.toString(16).padStart(2, "0"),
      ).join(""),
      label: String(user.account?.username ?? "TRAE CN account").slice(0, 256),
    };
  };
  const credit = (data) => {
    const valid = (v) =>
      typeof v === "number" && Number.isFinite(v) && v > 0 ? v : 0;
    return valid(data?.credits) + valid(data?.extra_credits) || null;
  };
  const refused = (code) =>
    ({
      1001: "sign_in_required",
      9074: "own_app_required",
      9095: "ineligible",
    })[code] ?? "failed";
  return async (input) => {
    if (!ipc?.invoke) return { outcome: "unsupported" };
    try {
      const current = await account();
      if (!current) return { outcome: "sign_in_required" };
      const until =
        Math.floor((Date.now() + 8 * 3600000) / 86400000) * 86400000 +
        16 * 3600000;
      const result = { ...current, until, windowKey: String(until) };
      if (
        input.mode === "claim" &&
        input.expectedAccountId !== current.accountId
      )
        return { ...result, outcome: "switch_required" };
      if (!["status", "claim"].includes(input.mode))
        return { ...result, outcome: "failed" };
      const status = await ipc.invoke(prefix + "fetchCheckinCreditsStatus");
      if ((await account())?.accountId !== current.accountId)
        return { outcome: "switch_required" };
      if (status?.data?.code)
        return { ...result, outcome: refused(status.data.code) };
      const data = status?.data;
      if (status?.status !== 200 || typeof data?.enable !== "boolean")
        return { ...result, outcome: "failed" };
      result.credit = credit(data);
      if (!data.enable) return { ...result, outcome: "inactive" };
      if (typeof data.checked_in !== "boolean")
        return { ...result, outcome: "failed" };
      if (data.checked_in) return { ...result, outcome: "already_checked" };
      if (input.mode !== "claim") return { ...result, outcome: "not_checked" };
      const response = await ipc.invoke(
        prefix + "claimCheckinCredits",
        "varyai",
      );
      if ((await account())?.accountId !== current.accountId)
        return { outcome: "switch_required" };
      const code = response?.data?.code;
      if (response?.status !== 200 || (code !== undefined && code !== 0))
        return { ...result, outcome: refused(code) };
      if (code === undefined) {
        const confirmed = await ipc.invoke(
          prefix + "fetchCheckinCreditsStatus",
        );
        if ((await account())?.accountId !== current.accountId)
          return { outcome: "switch_required" };
        if (
          confirmed?.status !== 200 ||
          confirmed.data?.checked_in !== true ||
          confirmed.data?.code
        )
          return { ...result, outcome: "failed" };
      }
      return {
        ...result,
        outcome: "claimed",
        credit: credit(response.data) ?? result.credit,
      };
    } catch {
      return { outcome: "failed" };
    }
  };
}
module.exports = { createTraeRewards };
