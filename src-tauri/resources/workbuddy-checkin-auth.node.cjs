const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

async function run({
  expired = false,
  keyAvailable = true,
  request = false,
  deviceFails = false,
} = {}) {
  const source = fs.readFileSync(
    path.join(__dirname, "workbuddy-checkin-auth.cjs"),
    "utf8",
  );
  const encrypted = {
    $wbEncrypted: 1,
    envelope: "synthetic-encrypted-envelope",
  };
  const session = {
    account: { uid: "synthetic-user" },
    auth: {
      accessToken: encrypted,
      expiresAt: expired ? 1 : 4102444800000,
      domain: "www.workbuddy.cn",
    },
  };
  const calls = { device: 0, disposed: 0 };
  let output = "";
  let complete;
  const finished = new Promise((resolve) => {
    complete = resolve;
  });
  const fakeFs = {
    readFileSync(file) {
      if (file === "/synthetic/auth.info") return JSON.stringify(session);
      if (file.endsWith("/cli/product.json"))
        return JSON.stringify({
          config: { turingSdk: { channelId: 1 } },
          applicationName: "WorkBuddy",
        });
      if (file.endsWith("/package.json"))
        return JSON.stringify({ version: "5.10.0-synthetic" });
      throw new Error("Unexpected file read");
    },
  };
  const fields = {
    t() {},
    _(bootstrap) {
      assert.equal(bootstrap.policy, "fields");
    },
    v() {
      return {
        decode(value, specs) {
          assert.deepEqual(JSON.parse(JSON.stringify(specs)), [
            { path: ["auth", "accessToken"] },
          ]);
          assert.deepEqual(JSON.parse(JSON.stringify(value)), session);
          return {
            value: {
              ...value,
              auth: { ...value.auth, accessToken: "synthetic-access-token" },
            },
          };
        },
        dispose() {
          calls.disposed++;
        },
      };
    },
    b() {
      calls.disposed++;
    },
  };
  const device = {
    isSupported: () => true,
    configure(channel, name, version) {
      assert.equal(channel, 1);
      assert.equal(name, "WorkBuddy");
      assert.equal(version, "5.10.0-synthetic");
    },
    async fetchDeviceToken() {
      calls.device++;
      if (deviceFails) throw new Error("synthetic-private-sdk-error");
      return "synthetic-device-token";
    },
  };
  const requireMock = (name) => {
    if (name === "node:fs") return fakeFs;
    if (name === "node:path") return path;
    if (name.endsWith("/main/dist.js"))
      return {
        s() {},
        T() {
          return {
            symmetric: keyAvailable
              ? { status: "available", value: { key: "synthetic-sdk-key" } }
              : { status: "unavailable" },
          };
        },
      };
    if (name.endsWith("/main/credential-protection.js")) return fields;
    if (name.endsWith("/native/turing-sdk")) return device;
    throw new Error("Unexpected module");
  };
  vm.runInNewContext(source, {
    require: requireMock,
    process: {
      execPath: "/synthetic/WorkBuddy.app/Contents/MacOS/Electron",
      argv: [
        "Electron",
        request ? "request" : "session",
        "/synthetic/auth.info",
      ],
      _linkedBinding: () => ({
        loggerGet: () => "synthetic-build-key-payload",
      }),
      stdout: {
        write(text) {
          output += text;
        },
      },
      exit(code) {
        assert.equal(code, 0);
        complete();
      },
    },
    console: { log() {}, error() {}, warn() {} },
  });
  await finished;
  const result = JSON.parse(output.split("VARYAI_WORKBUDDY_AUTH:")[1]);
  return { result, calls, output };
}

test("reads protected credentials through the installed SDK without requesting a device token", async () => {
  const { result, calls } = await run();
  assert.equal(result.accessToken, "synthetic-access-token");
  assert.equal(result.uid, "synthetic-user");
  assert.equal(result.clientVersion, "5.10.0-synthetic");
  assert.equal(calls.device, 0);
  assert.equal(calls.disposed, 2);
});

test("obtains native device verification only for an explicit request", async () => {
  const { result, calls } = await run({ request: true });
  assert.equal(result.deviceToken, "synthetic-device-token");
  assert.equal(calls.device, 1);
});

test("expired sessions do not invoke the device SDK", async () => {
  const { result, calls } = await run({ expired: true, request: true });
  assert.deepEqual(result, { error: "sign_in_required" });
  assert.equal(calls.device, 0);
});

test("unavailable keys return only a safe failure category", async () => {
  const { result, output } = await run({ keyAvailable: false });
  assert.deepEqual(result, { error: "credentials_protected" });
  assert(!output.includes("synthetic-sdk-key"));
  assert(!output.includes("synthetic-encrypted-envelope"));
});

test("device SDK errors do not expose their message or discard the valid session", async () => {
  const { result, output } = await run({ request: true, deviceFails: true });
  assert.equal(result.accessToken, "synthetic-access-token");
  assert.equal(result.deviceToken, undefined);
  assert(!output.includes("synthetic-private-sdk-error"));
});
