"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const {
  qoderCampaigns,
  zcodePlans,
  decryptZcode,
  run,
} = require("./agent-rewards.cjs");
const now = Date.UTC(2026, 10, 5, 4);
const campaign = {
  campaignId: "synthetic-daily",
  actionType: "CLAIM_BENEFIT",
  claimStatus: "CLAIMABLE",
  startAt: Date.UTC(2026, 10, 5, 2),
  endAt: Date.UTC(2026, 10, 6, 2),
  benefit: { amount: 100 },
};
test("Qoder status never claims and respects the vendor campaign window", async () => {
  const requests = [];
  const call = async (path, method) => {
    requests.push({ path, method });
    return { campaigns: [campaign] };
  };
  const result = await qoderCampaigns(call, false, now);
  assert.equal(result.outcome, "not_checked");
  assert.equal(result.credit, 100);
  assert.equal(result.until, campaign.endAt);
  assert.deepEqual(
    requests.map((r) => r.method),
    ["GET"],
  );
});
test("Qoder claims only current benefit campaigns and confirms the claim body", async () => {
  for (const status of ["CLAIMED", "unknown"]) {
    const paths = [];
    const call = async (path, method) => {
      paths.push(path);
      return method === "GET"
        ? {
            campaigns: [
              { ...campaign, campaignId: "expired", endAt: now - 1 },
              { ...campaign, campaignId: "future", startAt: now + 1 },
              { ...campaign, campaignId: "referral", actionType: "INVITE" },
              campaign,
            ],
          }
        : { status, benefit: { amount: 123 } };
    };
    const result = await qoderCampaigns(call, true, now);
    assert.equal(result.outcome, status === "CLAIMED" ? "claimed" : "failed");
    assert.equal(paths.length, 2);
    assert.ok(paths[1].endsWith("/synthetic-daily/claim"));
    if (status === "CLAIMED") assert.equal(result.credit, 123);
  }
});
test("Qoder claimed and absent campaigns never post a claim", async () => {
  for (const campaigns of [[], [{ ...campaign, claimStatus: "CLAIMED" }]]) {
    let posts = 0;
    const result = await qoderCampaigns(
      async (_, method) => {
        if (method === "POST") posts++;
        return { campaigns };
      },
      true,
      now,
    );
    assert.equal(posts, 0);
    assert.equal(
      result.outcome,
      campaigns.length ? "already_checked" : "inactive",
    );
    assert.ok(result.until > now);
  }
});
test("Qoder accepts the official RFC3339 and numeric-text campaign timestamps", async () => {
  for (const startAt of [
    String(campaign.startAt / 1000),
    String(campaign.startAt),
    new Date(campaign.startAt).toISOString(),
  ]) {
    const result = await qoderCampaigns(
      async () => ({
        campaigns: [
          { ...campaign, startAt, endAt: String(campaign.endAt / 1000) },
        ],
      }),
      false,
      now,
    );
    assert.equal(result.outcome, "not_checked");
    assert.equal(result.until, campaign.endAt);
  }
  const textual = await qoderCampaigns(
    async () => ({ campaigns: [{ ...campaign, benefit: { amount: "75" } }] }),
    false,
    now,
  );
  assert.equal(textual.credit, 75);
});
test("ZCode discovers only explicit claim actions and defers captcha to the official app", async () => {
  for (const code of [0, 1003, 3007, 1004, 9999]) {
    const requests = [];
    const call = async (path, method, body) => {
      requests.push({ path, method, body });
      if (method === "GET")
        return {
          code: 0,
          data: {
            deliveries: [
              {
                action: {
                  type: "claim_zcode_plan",
                  args: { plan_id: "synthetic-plan" },
                },
              },
              { action: { type: "open_url", args: { plan_id: "unrelated" } } },
            ],
          },
        };
      return { code };
    };
    const result = await zcodePlans(call, true, now);
    assert.equal(
      result.outcome,
      {
        0: "claimed",
        1003: "already_checked",
        3007: "own_app_required",
        1004: "ineligible",
      }[code] ?? "failed",
    );
    assert.deepEqual(
      requests.filter((r) => r.method === "POST").map((r) => r.body),
      [{ plan_id: "synthetic-plan" }],
    );
  }
});
test("ZCode refresh is read-only and malformed activity data is not reported as inactive", async () => {
  const result = await zcodePlans(
    async (_, method) => {
      assert.equal(method, "GET");
      return { code: 0, data: { deliveries: [] } };
    },
    false,
    now,
  );
  assert.equal(result.outcome, "inactive");
  await assert.rejects(
    zcodePlans(
      async () => ({ code: 0, data: { deliveries: "broken" } }),
      false,
      now,
    ),
  );
});
test("ZCode native credential decryption authenticates its envelope", () => {
  const crypto = require("node:crypto");
  const secret = "synthetic-only";
  const iv = Buffer.alloc(12, 1);
  const cipher = crypto.createCipheriv(
    "aes-256-gcm",
    crypto.createHash("sha256").update(secret).digest(),
    iv,
  );
  const encrypted = Buffer.concat([
    cipher.update("synthetic-session"),
    cipher.final(),
  ]);
  const value = `enc:v1:${iv.toString("base64url")}.${cipher.getAuthTag().toString("base64url")}.${encrypted.toString("base64url")}`;
  assert.equal(decryptZcode(value, secret), "synthetic-session");
  assert.throws(() => decryptZcode(value, "wrong-synthetic-key"));
});
test("Qoder device authorization is independent, cancellable and never claims", async () => {
  const fs = require("node:fs/promises");
  const os = require("node:os");
  const path = require("node:path");
  const crypto = require("node:crypto");
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "varyai-reward-auth-"));
  const originalHome = os.homedir;
  const originalFetch = global.fetch;
  const native = path.join(
    root,
    "Library/Application Support/com.qodercn.app.stable",
  );
  const directory = path.join(root, "vary-private");
  const file = path.join(directory, "qoder-auth.json");
  const read = async () => JSON.parse(await fs.readFile(file, "utf8"));
  try {
    os.homedir = () => root;
    await fs.mkdir(native, { recursive: true });
    await fs.writeFile(
      path.join(native, "auth.machine-id"),
      "synthetic-device",
    );
    await fs.writeFile(
      path.join(native, "auth.v1.dat"),
      "synthetic-native-encrypted-session",
    );
    let requests = 0;
    global.fetch = async (url, options) => {
      requests++;
      assert.equal(options.method, "GET");
      assert.ok(!url.includes("/campaigns"));
      if (url.includes("deviceToken/poll"))
        return new Response(
          JSON.stringify({
            token: "synthetic-device-token",
            refresh_token: "synthetic-refresh",
            user_id: "synthetic-user",
          }),
        );
      return new Response(
        JSON.stringify({ uid: "synthetic-user", name: "Synthetic account" }),
      );
    };
    const start = await run({ app: "qoder", mode: "login_start", directory });
    const pending = (await read()).pending;
    const url = new URL(start.url);
    assert.equal(url.origin, "https://qoder.cn");
    assert.equal(
      url.searchParams.get("challenge"),
      crypto.createHash("sha256").update(pending.verifier).digest("base64url"),
    );
    assert.ok(!JSON.stringify(start).includes(pending.verifier));
    assert.equal(requests, 0);
    await run({
      app: "qoder",
      mode: "login_cancel",
      directory,
      loginId: "stale-id",
    });
    assert.ok((await read()).pending);
    await run({
      app: "qoder",
      mode: "login_cancel",
      directory,
      loginId: start.loginId,
    });
    await assert.rejects(
      run({
        app: "qoder",
        mode: "login_poll",
        directory,
        loginId: start.loginId,
      }),
    );
    assert.equal(requests, 0);
    const next = await run({ app: "qoder", mode: "login_start", directory });
    assert.deepEqual(
      await run({
        app: "qoder",
        mode: "login_poll",
        directory,
        loginId: next.loginId,
      }),
      { complete: true },
    );
    const saved = await read();
    assert.ok(!saved.pending && !saved.enabled);
    assert.equal(saved.account.uid, "synthetic-user");
    if (process.platform !== "win32")
      assert.equal((await fs.stat(file)).mode & 0o777, 0o600);
    assert.equal(
      await fs.readFile(path.join(native, "auth.v1.dat"), "utf8"),
      "synthetic-native-encrypted-session",
    );
    const before = requests;
    assert.equal(
      (
        await run({
          app: "qoder",
          mode: "claim",
          directory,
          expectedAccountId: "stale-id",
        })
      ).outcome,
      "switch_required",
    );
    assert.equal(requests, before);
    await assert.rejects(
      run({
        app: "qoder",
        mode: "forget",
        directory,
        expectedAccountId: "stale-id",
      }),
    );
    assert.ok((await read()).account);
    const expired = await read();
    expired.account.expiresAt = 1;
    await fs.writeFile(file, JSON.stringify(expired));
    global.fetch = async () => new Response("{}", { status: 401 });
    const signedOut = await run({ app: "qoder", mode: "status", directory });
    assert.equal(signedOut.outcome, "sign_in_required");
    assert.match(signedOut.accountId, /^[a-f0-9]{64}$/);
    assert.ok(!JSON.stringify(signedOut).includes("synthetic-device-token"));
    await run({
      app: "qoder",
      mode: "forget",
      directory,
      expectedAccountId: crypto
        .createHash("sha256")
        .update("qoder|synthetic-user")
        .digest("hex"),
    });
    assert.deepEqual(await read(), {});
    const mismatch = await run({
      app: "qoder",
      mode: "login_start",
      directory,
    });
    global.fetch = async (url) =>
      new Response(
        JSON.stringify(
          url.includes("deviceToken/poll")
            ? { token: "synthetic-device-token", user_id: "first-user" }
            : { uid: "different-user" },
        ),
      );
    await assert.rejects(
      run({
        app: "qoder",
        mode: "login_poll",
        directory,
        loginId: mismatch.loginId,
      }),
    );
    assert.ok(!(await read()).account);
  } finally {
    os.homedir = originalHome;
    global.fetch = originalFetch;
    await fs.rm(root, { recursive: true, force: true });
  }
});
