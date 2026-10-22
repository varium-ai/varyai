const { test } = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs"),
  path = require("node:path");
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
