// Runs as a short-lived utility in the installed WorkBuddy runtime, without a GUI.
// Credentials stay in memory and travel only through the private stdout pipe.
const fs = require("node:fs");
const path = require("node:path");

async function resolveSession() {
  const [mode, authPath] = process.argv.slice(1);
  if (!["session", "request"].includes(mode) || !authPath) throw new Error();
  const session = JSON.parse(fs.readFileSync(authPath, "utf8"));
  if (session.auth?.expiresAt > 0 && session.auth.expiresAt <= Date.now()) {
    return { error: "sign_in_required" };
  }
  const contents =
    process.platform === "win32"
      ? path.dirname(process.execPath)
      : path.resolve(path.dirname(process.execPath), "..");
  const resources = path.join(
    contents,
    process.platform === "win32" ? "resources" : "Resources",
  );
  const archive = path.join(resources, "app.asar");
  let crypto;
  let capabilities;
  // The installed client renamed its crypto module and exports in 5.11.3.
  try {
    crypto = require(archive + "/main/src.js");
  } catch {
    crypto = null;
  }
  if (typeof crypto?.t === "function" && typeof crypto?._ === "function") {
    crypto.t();
    capabilities = crypto._(
      process._linkedBinding("electron_browser_workbuddy_storage").loggerGet(),
    );
  } else {
    crypto = require(archive + "/main/dist.js");
    crypto.s();
    capabilities = crypto.T(
      process._linkedBinding("electron_browser_workbuddy_storage").loggerGet(),
    );
  }
  const fields = require(archive + "/main/credential-protection.js");
  fields.t();
  if (capabilities.symmetric.status !== "available") throw new Error();
  let codec;
  try {
    fields._({ policy: "fields", symmetricKey: capabilities.symmetric.value });
    codec = fields.v({
      purpose: "workbuddy-auth-session",
      resourceId: "auth/workbuddy-desktop",
      onUnavailable: () => {},
    });
    const decoded = codec.decode(session, [
      { path: ["auth", "accessToken"] },
    ]).value;
    if (
      typeof decoded?.account?.uid !== "string" ||
      !decoded.account.uid ||
      typeof decoded?.auth?.accessToken !== "string" ||
      !decoded.auth.accessToken
    )
      throw new Error();
    const product = JSON.parse(
      fs.readFileSync(archive + "/cli/product.json", "utf8"),
    );
    const pkg = JSON.parse(fs.readFileSync(archive + "/package.json", "utf8"));
    const result = {
      uid: decoded.account.uid,
      label:
        typeof decoded.account.nickname === "string"
          ? decoded.account.nickname
          : "",
      enterpriseId:
        typeof decoded.account.enterpriseId === "string"
          ? decoded.account.enterpriseId
          : "",
      accessToken: decoded.auth.accessToken,
      domain: decoded.auth.domain || "copilot.tencent.com",
      clientVersion: product.productVersion || pkg.version,
    };
    if (
      mode === "request" &&
      Number.isInteger(product.config?.turingSdk?.channelId)
    ) {
      try {
        const sdk = require(
          path.join(resources, "app.asar.unpacked/native/turing-sdk"),
        );
        if (sdk.isSupported()) {
          sdk.configure(
            product.config.turingSdk.channelId,
            product.applicationName || product.productName || "WorkBuddy",
            result.clientVersion,
            true,
          );
          const token = await sdk.fetchDeviceToken({
            usingCachedMessage: true,
            includesOutdatedMessage: true,
            includesDeviceInfo: true,
            timeoutMs: 5000,
          });
          if (typeof token === "string" && token) result.deviceToken = token;
        }
      } catch {}
    }
    return result;
  } finally {
    codec?.dispose();
    fields.b();
  }
}

resolveSession()
  .catch(() => ({ error: "credentials_protected" }))
  .then((result) => {
    process.stdout.write("\nVARYAI_WORKBUDDY_AUTH:" + JSON.stringify(result));
    process.exit(0);
  });
