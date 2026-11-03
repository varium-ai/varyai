const { test } = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs"),
  path = require("node:path");
const vm = require("node:vm");
const crypto = require("node:crypto");
const os = require("node:os");
const worker =
  "/Applications/Qoder CN.app/Contents/Resources/app.asar.unpacked/node_modules/@qoder-ai/qoder-cn-agent-sdk/dist/_worker/qoder-worker-runtime.obf.mjs";
const script = path.resolve(
  __dirname,
  "../../src-tauri/src/proxy/qoder_codec.cjs",
);
function run(input, workerPath = worker) {
  return spawnSync(process.execPath, [script], {
    env: { ...process.env, VARYAI_QODER_WORKER: workerPath },
    input: JSON.stringify(input),
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
}
test("trust checks require the exact CA in Qoder's runtime store", () => {
  const directory = fs.mkdtempSync(
    path.join(os.tmpdir(), "varyai-qoder-trust-"),
  );
  try {
    fs.writeFileSync(
      path.join(directory, "ca.cnf"),
      "[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=Synthetic Qoder CA\n[ca]\nbasicConstraints=critical,CA:true\nkeyUsage=critical,keyCertSign,cRLSign\n",
    );
    const generate = (name) =>
      spawnSync("/usr/bin/openssl", [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "1",
        "-keyout",
        path.join(directory, `${name}-key.pem`),
        "-out",
        path.join(directory, `${name}.pem`),
        "-config",
        path.join(directory, "ca.cnf"),
      ]);
    assert.equal(generate("ca").status, 0);
    assert.equal(generate("other").status, 0);
    const ca = fs.readFileSync(path.join(directory, "ca.pem"), "utf8");
    const other = fs.readFileSync(path.join(directory, "other.pem"), "utf8");
    const source = fs.readFileSync(script, "utf8");
    const probe = (store) => {
      let output = "";
      const process = {
        stdout: {
          write: (value) => {
            output += value;
          },
        },
      };
      vm.runInNewContext(source, {
        require: (name) => {
          if (name === "node:fs")
            return {
              readFileSync: () => JSON.stringify({ operation: "trust", ca }),
            };
          if (name === "node:crypto") return crypto;
          if (name === "node:tls") return { getCACertificates: store };
          throw new Error("Unexpected dependency");
        },
        process,
      });
      return { result: JSON.parse(output), exitCode: process.exitCode };
    };
    assert.deepEqual(
      probe((kind) => {
        assert.equal(kind, "system");
        return [];
      }),
      { result: { trusted: false }, exitCode: undefined },
    );
    assert.deepEqual(
      probe(() => [other]),
      { result: { trusted: false }, exitCode: undefined },
    );
    assert.deepEqual(
      probe(() => [ca]),
      { result: { trusted: true }, exitCode: undefined },
    );
    assert.deepEqual(probe(undefined), {
      result: { error: "qoder_codec_unavailable" },
      exitCode: 1,
    });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
test("unknown installed workers fail closed without evaluating client code", () => {
  const r = run({ operation: "decode", body: "invalid" }, __filename);
  assert.equal(r.status, 1);
  assert.deepEqual(JSON.parse(r.stdout), { error: "qoder_codec_unavailable" });
  assert.equal(r.stderr, "");
});
test(
  "installed Qoder codec preserves large payloads and original account context",
  { skip: !fs.existsSync(worker) },
  () => {
    const payload = {
      session_id: "synthetic-session",
      model_config: { key: "synthetic", source: "system" },
      messages: [
        {
          role: "user",
          contents: [{ type: "text", text: "SYNTHETIC ".repeat(35000) }],
        },
      ],
      tools: [{ name: "synthetic_tool" }],
      parameters: { temperature: 0.2 },
      future: { keep: true },
    };
    const info = "synthetic-encrypted-user-info";
    const headers = {
      authorization:
        "Bearer test." +
        Buffer.from(JSON.stringify({ info })).toString("base64url") +
        ".test",
      "cosy-machineid": "synthetic-machine",
      "cosy-version": "test",
      "cosy-user": "synthetic-user",
      "cosy-key": "synthetic-key",
    };
    const encoded = run({ operation: "encode", body: payload, headers });
    assert.equal(encoded.status, 0, encoded.stdout);
    const wire = JSON.parse(encoded.stdout);
    assert.equal(
      Buffer.from(wire.body, "base64").includes(Buffer.from("SYNTHETIC ")),
      false,
    );
    assert.equal(
      JSON.parse(
        Buffer.from(wire.authorization.slice(7).split(".")[1], "base64url"),
      ).info,
      info,
    );
    const decoded = run({ operation: "decode", body: wire.body });
    assert.equal(decoded.status, 0);
    assert.deepEqual(JSON.parse(decoded.stdout).body, payload);
  },
);
