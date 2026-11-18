const test = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");
const source = fs.readFileSync(
  path.join(__dirname, "doubao-tool-risk.js"),
  "utf8",
);

test("native lifecycle preserves execution and exposes only bounded command identity", () => {
  const emitted = [];
  const context = vm.createContext({
    observer: (value) => emitted.push(JSON.parse(value)),
  });
  const install = vm.runInContext(
    `${source};varyaiInstallDoubaoToolObserver`,
    context,
  );
  const native = { onBeforeExecution() {}, onExecutionComplete() {} };
  const lifecycle = [native];
  assert.equal(install.call(lifecycle, "observer"), true);
  const ctx = {
    request: {
      name: "Bash",
      id: "synthetic-call",
      input: {
        command: "kubectl delete pod synthetic --dry-run=client",
        env: { private: "synthetic-marker" },
      },
    },
    nativeInstanceId: "synthetic-native",
    identity: { track: "agent", conversationId: "synthetic-session" },
  };
  const original = structuredClone(ctx);
  assert.equal(lifecycle[1].onBeforeExecution({ ctx }), undefined);
  assert.deepEqual(ctx, original);
  assert.equal(emitted.length, 1);
  assert.equal(emitted[0].session_id, "synthetic-session");
  assert.deepEqual(emitted[0].tool_input, {
    command: ctx.request.input.command,
  });
  assert.equal(JSON.stringify(emitted).includes("synthetic-marker"), false);
  assert.equal(install.call(lifecycle, "observer"), true);
  assert.equal(lifecycle.length, 2);
  vm.runInContext(
    'globalThis[Symbol.for("varyai.doubao.toolRisk")].dispose()',
    context,
  );
  assert.deepEqual(lifecycle, [native]);
});

test("provider tools, missing native scope and observer failures never interrupt execution", () => {
  const context = vm.createContext({
    observer() {
      throw Error("synthetic failure");
    },
  });
  const install = vm.runInContext(
    `${source};varyaiInstallDoubaoToolObserver`,
    context,
  );
  const lifecycle = [{ onBeforeExecution() {}, onExecutionComplete() {} }];
  assert.equal(install.call(lifecycle, "observer"), true);
  for (const ctx of [
    undefined,
    {},
    { request: { name: "Bash", input: { command: "echo synthetic" } } },
    {
      nativeInstanceId: "synthetic",
      request: { name: "Bash", input: { command: "echo synthetic" } },
    },
  ]) {
    assert.doesNotThrow(() => lifecycle[1].onBeforeExecution({ ctx }));
  }
});
