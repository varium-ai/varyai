"use strict";
// Read-only native credentials and independent Qoder browser grants. No credentials
// or vendor error bodies are returned to the renderer or written to diagnostics.
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");
const crypto = require("node:crypto");
const { execFile } = require("node:child_process");
const QODER = "https://openapi.qoder.com.cn";
const ZCODE = "https://zcode.z.ai";
const CLIENT = "e883ade2-e6e3-4d6d-adf7-f92ceff5fdcb";
const CAMPAIGNS = "/sash/api/v1/me/campaigns";
const hash = (value) => crypto.createHash("sha256").update(value).digest("hex");
const positive = (value) => {
  if (typeof value === "string" && /^\d+(?:\.\d+)?$/.test(value))
    value = Number(value);
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? value
    : null;
};
const stamp = (value) => {
  if (typeof value === "string" && /^\d+$/.test(value)) value = Number(value);
  if (typeof value === "number" && Number.isFinite(value) && value > 0)
    return value < 1e11 ? value * 1000 : value;
  const n = typeof value === "string" ? Date.parse(value) : NaN;
  return Number.isFinite(n) ? n : null;
};
const failure = (outcome) =>
  Object.assign(new Error("Reward operation unavailable"), { outcome });
function nextDay(now, hour = 0) {
  return (
    Math.floor((now + (8 - hour) * 3600000) / 86400000) * 86400000 +
    86400000 -
    (8 - hour) * 3600000
  );
}
async function qoderCampaigns(call, claim, now = Date.now()) {
  const list = await call(CAMPAIGNS + "?forceRefresh=true", "GET");
  if (!Array.isArray(list?.campaigns)) throw failure("failed");
  const campaigns = list.campaigns.filter(
    (c) => c?.actionType === "CLAIM_BENEFIT",
  );
  const active = campaigns.filter(
    (c) =>
      stamp(c.startAt) !== null &&
      stamp(c.endAt) !== null &&
      stamp(c.startAt) <= now &&
      now < stamp(c.endAt),
  );
  const available = active.filter((c) => c.claimStatus === "CLAIMABLE");
  const chosen = available.length
    ? available
    : active.filter((c) => c.claimStatus === "CLAIMED");
  const until = chosen.length
    ? Math.min(...chosen.map((c) => stamp(c.endAt)))
    : Math.min(nextDay(now, 10), now + 30 * 60000);
  if (!chosen.length) return { outcome: "inactive", until };
  if (
    chosen.some(
      (c) =>
        typeof c.campaignId !== "string" ||
        !c.campaignId ||
        c.campaignId.length > 256,
    )
  )
    throw failure("failed");
  const windowKey = hash(
    chosen
      .map((c) => c.campaignId)
      .sort()
      .join("|"),
  );
  let credit =
    chosen.reduce((sum, c) => sum + (positive(c.benefit?.amount) ?? 0), 0) ||
    null;
  if (!available.length)
    return { outcome: "already_checked", credit, until, windowKey };
  if (!claim) return { outcome: "not_checked", credit, until, windowKey };
  credit = 0;
  for (const c of available) {
    const result = await call(
      `${CAMPAIGNS}/${encodeURIComponent(c.campaignId)}/claim`,
      "POST",
      {},
    );
    const body = result?.data ?? result;
    if (body?.status !== "CLAIMED")
      return { outcome: "failed", until: now + 30 * 60000 };
    credit +=
      positive(body.benefit?.amount) ?? positive(c.benefit?.amount) ?? 0;
  }
  return { outcome: "claimed", credit: credit || null, until, windowKey };
}
async function zcodePlans(call, claim, now = Date.now()) {
  const response = await call("/api/v1/marketing/touch?seq=0", "GET");
  if (
    String(response?.code) !== "0" ||
    !Array.isArray(response.data?.deliveries)
  )
    throw failure("failed");
  const ids = new Set();
  const walk = (value, depth = 0) => {
    if (!value || typeof value !== "object" || depth > 12) return;
    if (
      value.type === "claim_zcode_plan" &&
      typeof value.args?.plan_id === "string" &&
      value.args.plan_id.trim() &&
      value.args.plan_id.length <= 256
    )
      ids.add(value.args.plan_id.trim());
    for (const item of Object.values(value)) walk(item, depth + 1);
  };
  response.data.deliveries.forEach((d) => walk(d));
  if (ids.size > 64) throw failure("failed");
  const until = now + 30 * 60000;
  if (!ids.size) return { outcome: "inactive", until };
  const windowKey = hash([...ids].sort().join("|"));
  if (!claim) return { outcome: "not_checked", until, windowKey };
  let outcome = "already_checked";
  for (const plan_id of ids) {
    const result = await call("/api/v1/zcode-plan/billing/claim", "POST", {
      plan_id,
    });
    const code = String(result?.code);
    if (code === "3007")
      return { outcome: "own_app_required", until: nextDay(now), windowKey };
    if (code === "1004") return { outcome: "ineligible", until, windowKey };
    if (code === "1003") continue;
    const plan = result?.data?.plan;
    if (
      code !== "0" ||
      !plan ||
      typeof plan !== "object" ||
      Array.isArray(plan)
    )
      return { outcome: "failed", until };
    outcome = "claimed";
  }
  return { outcome, until, windowKey };
}
function decryptZcode(value, secret) {
  if (typeof value !== "string") throw failure("sign_in_required");
  if (!value.startsWith("enc:v1:")) return value;
  const parts = value.slice(7).split(".");
  if (parts.length !== 3) throw failure("credentials_protected");
  const [iv, tag, data] = parts.map((p) => Buffer.from(p, "base64url"));
  if (iv.length !== 12 || tag.length !== 16)
    throw failure("credentials_protected");
  const decipher = crypto.createDecipheriv(
    "aes-256-gcm",
    crypto.createHash("sha256").update(secret).digest(),
    iv,
  );
  decipher.setAuthTag(tag);
  return Buffer.concat([decipher.update(data), decipher.final()]).toString(
    "utf8",
  );
}
async function readJSON(file, fallback) {
  try {
    return JSON.parse(await fs.readFile(file, "utf8"));
  } catch (error) {
    if (error.code === "ENOENT" && fallback !== undefined) return fallback;
    throw failure("storage_error");
  }
}
async function saveJSON(file, value) {
  await fs.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
  const temporary = file + "." + crypto.randomUUID();
  try {
    await fs.writeFile(temporary, JSON.stringify(value), {
      mode: 0o600,
      flag: "wx",
    });
    await fs.rename(temporary, file);
  } finally {
    await fs.rm(temporary, { force: true }).catch(() => {});
  }
}
async function http(origin, route, token, method = "GET", body, extra = {}) {
  const response = await fetch(origin + route, {
    method,
    redirect: "error",
    signal: AbortSignal.timeout(15000),
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...extra,
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  if (response.status === 401 || response.status === 403)
    throw failure("sign_in_required");
  if (
    route.startsWith("/api/v1/deviceToken/poll") &&
    [202, 404].includes(response.status)
  )
    return null;
  const text = await response.text();
  if (text.length > 1024 * 1024) throw failure("failed");
  let result;
  try {
    result = JSON.parse(text);
  } catch {
    throw failure("failed");
  }
  if (
    !response.ok &&
    !(
      origin === ZCODE &&
      ["3007", "1003", "1004"].includes(String(result?.code))
    )
  )
    throw failure("failed");
  return result;
}
async function appPath(name) {
  const home = os.homedir();
  const candidates =
    process.platform === "darwin"
      ? [
          path.join("/Applications", name + ".app"),
          path.join(home, "Applications", name + ".app"),
        ]
      : process.platform === "win32"
        ? [
            path.join(
              process.env.LOCALAPPDATA ?? path.join(home, "AppData/Local"),
              "Programs",
              name,
            ),
          ]
        : [path.join("/opt", name.replaceAll(" ", "-"))];
  for (const candidate of candidates)
    if (await fs.stat(candidate).catch(() => null)) return candidate;
  throw failure("unsupported");
}
async function appVersion(name) {
  const root = await appPath(name);
  const file = path.join(
    root,
    process.platform === "darwin"
      ? "Contents/Info.plist"
      : "resources/app/package.json",
  );
  const text = await fs.readFile(file, "utf8");
  const version =
    process.platform === "darwin"
      ? /<key>CFBundleShortVersionString<\/key>\s*<string>([^<]+)<\/string>/.exec(
          text,
        )?.[1]
      : JSON.parse(text).version;
  if (!/^\d+\.\d+\.\d+(?:[-+.][\w.-]+)?$/.test(version ?? ""))
    throw failure("unsupported");
  return version;
}
async function qoderIdentity(uid) {
  const root = await appPath("Qoder CN");
  const binary = path.join(
    root,
    process.platform === "darwin"
      ? "Contents/Resources/umid/runtime-info"
      : "resources/umid/runtime-info" +
          (process.platform === "win32" ? ".exe" : ""),
  );
  const result = await new Promise((resolve, reject) => {
    const child = execFile(
      binary,
      process.platform === "linux" ? ["0"] : ["0", "--account-stdin"],
      { timeout: 20000, maxBuffer: 65536, windowsHide: true },
      (error, stdout) =>
        error ? reject(failure("own_app_required")) : resolve(stdout),
    );
    child.stdin.on("error", () => {});
    child.stdin.end(
      process.platform === "linux"
        ? undefined
        : JSON.stringify({ account: uid }) + "\n",
    );
  });
  const identity = JSON.parse(result.trim().split(/\r?\n/)[0]);
  if (
    ![identity.machineToken, identity.machineType].every(
      (s) =>
        typeof s === "string" &&
        s.length > 0 &&
        s.length <= 4096 &&
        /^[\x20-\x7e]+$/.test(s),
    )
  )
    throw failure("own_app_required");
  return {
    "Cosy-MachineToken": identity.machineToken,
    "Cosy-MachineType": identity.machineType,
    "Cosy-ClientType": "10",
    "User-Agent": "Qoder",
  };
}
async function qoderNativeAccount(native) {
  if (!native?.file) return null;
  const unavailable = (outcome) => ({ source: "native", unavailable: outcome });
  if (!native.password) return unavailable("credentials_protected");
  let saved;
  try {
    const bytes = await fs.readFile(native.file);
    if (bytes.length > 65536 || bytes.subarray(0, 3).toString() !== "v10")
      return unavailable("unsupported");
    const key = crypto.pbkdf2Sync(
      native.password,
      "saltysalt",
      1003,
      16,
      "sha1",
    );
    const decipher = crypto.createDecipheriv(
      "aes-128-cbc",
      key,
      Buffer.alloc(16, 32),
    );
    saved = JSON.parse(
      Buffer.concat([
        decipher.update(bytes.subarray(3)),
        decipher.final(),
      ]).toString("utf8"),
    );
  } catch {
    return unavailable("credentials_protected");
  }
  if (
    saved?.schemaVersion !== 1 ||
    typeof saved.token !== "string" ||
    !saved.token ||
    typeof saved.user?.id !== "string" ||
    !saved.user.id ||
    !stamp(saved.expiresAt)
  )
    return unavailable("credentials_unavailable");
  const account = {
    uid: saved.user.id,
    id: hash("qoder|" + saved.user.id),
    token: saved.token,
    source: "native",
    label: String(
      saved.user.name ??
        saved.user.nickname ??
        saved.user.email ??
        "Qoder CN account",
    ).slice(0, 256),
  };
  if (stamp(saved.expiresAt) <= Date.now() + 60000)
    return { ...account, unavailable: "own_app_required" };
  return account;
}
async function qoderAccount(input) {
  const native = await qoderNativeAccount(input.native);
  if (native && !native.unavailable) return native;
  try {
    return { ...(await qoderBrowserAccount(input)), source: "browser" };
  } catch (error) {
    if (native && error.outcome === "sign_in_required") return native;
    throw error;
  }
}
async function qoderBrowserAccount(input) {
  const file = path.join(input.directory, "qoder-auth.json");
  const store = await readJSON(file, {});
  let account = store.account;
  if (!account?.uid || !account.token) throw failure("sign_in_required");
  if (account.expiresAt && account.expiresAt < Date.now() + 60000) {
    if (!account.refreshToken) throw failure("sign_in_required");
    const result = await http(
      QODER,
      "/api/v1/deviceToken/refresh",
      null,
      "POST",
      { refresh_token: account.refreshToken },
    );
    const token = result?.device_token ?? result?.token;
    if (
      typeof token !== "string" ||
      !token ||
      typeof result.refresh_token !== "string"
    )
      throw failure("sign_in_required");
    account = {
      ...account,
      token,
      refreshToken: result.refresh_token,
      expiresAt: stamp(result.expires_at) ?? 0,
    };
    await saveJSON(file, { ...store, account });
  }
  return { ...account, id: hash("qoder|" + account.uid) };
}
async function zcodeAccount() {
  const root = path.join(os.homedir(), ".zcode/v2");
  const stored = await readJSON(path.join(root, "credentials.json"), {});
  const secret =
    process.env.ZCODE_CREDENTIAL_SECRET ||
    `zcode-credential-fallback:${os.platform()}:${os.homedir()}:${os.userInfo().username}`;
  let token, info;
  try {
    token = decryptZcode(stored.zcodejwttoken, secret);
    const provider = decryptZcode(
      stored["oauth:active_provider"] ?? "zai",
      secret,
    );
    if (!["zai", "bigmodel"].includes(provider))
      throw failure("sign_in_required");
    info = JSON.parse(
      decryptZcode(stored[`oauth:${provider}:user_info`] ?? "{}", secret),
    );
  } catch (error) {
    throw failure(error.outcome ?? "credentials_protected");
  }
  let claims;
  try {
    claims = JSON.parse(
      Buffer.from(token.split(".")[1], "base64url").toString(),
    );
  } catch {
    throw failure("sign_in_required");
  }
  if (claims.exp && claims.exp * 1000 <= Date.now())
    throw failure("sign_in_required");
  const uid =
    claims.sub ??
    claims.user_id ??
    claims.uid ??
    info.id ??
    info.user_id ??
    info.userId;
  const device = (await readJSON(path.join(root, "telemetry-state.json"), {}))
    .deviceMid;
  if (!uid || typeof device !== "string" || !device)
    throw failure("credentials_unavailable");
  return {
    token,
    device,
    uid: String(uid),
    id: hash("zcode|" + uid),
    source: "native",
    label: (
      [info.displayName, info.nickname, info.name, info.username, info.email]
        .find((value) => typeof value === "string" && value.trim())
        ?.trim() ?? `ZCode · ${hash("zcode|" + uid).slice(0, 6)}`
    ).slice(0, 256),
  };
}
async function login(input) {
  if (input.app !== "qoder") throw failure("unsupported");
  const file = path.join(input.directory, "qoder-auth.json");
  const store = await readJSON(file, {});
  if (input.mode === "login_cancel") {
    if (store.pending?.id === input.loginId) {
      delete store.pending;
      await saveJSON(file, store);
    }
    return { complete: true };
  }
  if (input.mode === "forget") {
    if (
      store.account &&
      hash("qoder|" + store.account.uid) !== input.expectedAccountId
    )
      throw failure("switch_required");
    await saveJSON(file, {});
    return { complete: true };
  }
  if (input.mode === "login_start") {
    const verifier = crypto.randomBytes(64).toString("base64url");
    const pending = {
      id: crypto.randomUUID(),
      nonce: crypto.randomUUID(),
      verifier,
      until: Date.now() + 10 * 60000,
    };
    const machine = (
      await fs
        .readFile(path.join(os.homedir(), ".qoder-cn/auth.machine-id"), "utf8")
        .catch(() => "")
    ).trim();
    // The installed desktop keeps this outside its encrypted account store.
    const nativeMachine =
      machine ||
      (
        await fs
          .readFile(
            path.join(
              os.homedir(),
              "Library/Application Support/com.qodercn.app.stable/auth.machine-id",
            ),
            "utf8",
          )
          .catch(() => "")
      ).trim();
    if (!nativeMachine) throw failure("own_app_required");
    await saveJSON(file, { ...store, pending });
    const params = new URLSearchParams({
      challenge: crypto
        .createHash("sha256")
        .update(verifier)
        .digest("base64url"),
      challenge_method: "S256",
      nonce: pending.nonce,
      machine_id: nativeMachine,
      client_id: CLIENT,
    });
    return {
      loginId: pending.id,
      url: "https://qoder.cn/device/selectAccounts?" + params,
    };
  }
  const pending = store.pending;
  if (!pending || input.loginId !== pending.id)
    throw failure("sign_in_required");
  if (pending.until < Date.now()) {
    delete store.pending;
    await saveJSON(file, store);
    throw failure("sign_in_required");
  }
  const params = new URLSearchParams({
    nonce: pending.nonce,
    verifier: pending.verifier,
    challenge_method: "S256",
  });
  const result = await http(QODER, "/api/v1/deviceToken/poll?" + params);
  if (!result?.token) return { complete: false };
  const profile = await http(QODER, "/api/v1/userinfo", result.token);
  const user = profile?.user ?? profile?.data?.user ?? profile?.data ?? profile;
  const uid = user?.uid ?? user?.id ?? user?.user_id ?? result.user_id;
  if (!uid || typeof result.token !== "string") throw failure("failed");
  if (result.user_id && String(result.user_id) !== String(uid))
    throw failure("switch_required");
  const account = {
    uid: String(uid),
    token: result.token,
    refreshToken: result.refresh_token ?? "",
    expiresAt: stamp(result.expires_at) ?? 0,
    label: String(
      user.name ?? user.nickname ?? user.email ?? "Qoder CN account",
    ).slice(0, 256),
  };
  await saveJSON(file, { account });
  return { complete: true };
}
async function run(input) {
  if (!["qoder", "zcode"].includes(input.app)) throw failure("unsupported");
  if (typeof fetch !== "function") throw failure("unsupported");
  if (
    ["login_start", "login_poll", "login_cancel", "forget"].includes(input.mode)
  )
    return login(input);
  if (!["status", "claim"].includes(input.mode)) throw failure("failed");
  let account;
  try {
    account =
      input.app === "qoder" ? await qoderAccount(input) : await zcodeAccount();
  } catch (error) {
    // An expired independent grant must still be removable and its automation
    // switch must remain accessible. No token is included in this status.
    if (input.app === "qoder" && error.outcome === "sign_in_required") {
      const saved = (
        await readJSON(path.join(input.directory, "qoder-auth.json"), {})
      ).account;
      if (saved?.uid)
        return {
          accountId: hash("qoder|" + saved.uid),
          label: String(saved.label ?? "Qoder CN account").slice(0, 256),
          source: "browser",
          nativeAvailable: Boolean(input.native),
          outcome: "sign_in_required",
        };
    }
    throw error;
  }
  const result = {
    accountId: account.id,
    label: account.label,
    source: account.source,
    nativeAvailable: input.app === "qoder" && Boolean(input.native),
  };
  if (input.app === "qoder") {
    // Backup authorization can be removed independently of the active native login.
    const backup = await readJSON(
      path.join(input.directory, "qoder-auth.json"),
      {},
    ).catch(() => null);
    result.browserAccountId = backup?.account?.uid
      ? hash("qoder|" + backup.account.uid)
      : null;
  }
  if (account.unavailable) return { ...result, outcome: account.unavailable };
  if (input.mode === "claim" && input.expectedAccountId !== account.id)
    return { ...result, outcome: "switch_required" };
  let status;
  if (input.app === "qoder") {
    const identity = await qoderIdentity(account.uid);
    status = await qoderCampaigns(
      (route, method, body) =>
        http(QODER, route, account.token, method, body, identity),
      input.mode === "claim",
    );
  } else {
    const version = await appVersion("ZCode");
    const headers = {
      "X-Device-Mid": account.device,
      "X-ZCode-App-Version": version,
      "X-Platform": `${os.platform()}-${os.arch()}`,
      "X-Client-Language": "en-US",
    };
    status = await zcodePlans(
      (route, method, body) =>
        http(ZCODE, route, account.token, method, body, headers),
      input.mode === "claim",
    );
  }
  const current =
    input.app === "qoder" ? await qoderAccount(input) : await zcodeAccount();
  if (current.id !== account.id) return { outcome: "switch_required" };
  return { ...result, ...status };
}
async function main() {
  try {
    let text = "";
    for await (const chunk of process.stdin) {
      text += chunk;
      if (text.length > 65536) throw failure("failed");
    }
    const result = await run(JSON.parse(text));
    process.stdout.write(JSON.stringify(result));
  } catch (error) {
    const allowed = [
      "failed",
      "inactive",
      "own_app_required",
      "sign_in_required",
      "credentials_protected",
      "credentials_unavailable",
      "unsupported",
      "storage_error",
    ];
    process.stdout.write(
      JSON.stringify({
        outcome: allowed.includes(error.outcome) ? error.outcome : "failed",
      }),
    );
  }
}
module.exports = {
  qoderCampaigns,
  qoderNativeAccount,
  zcodePlans,
  decryptZcode,
  run,
};
if (process.env.VARYAI_AGENT_REWARD_HELPER === "1") void main();
