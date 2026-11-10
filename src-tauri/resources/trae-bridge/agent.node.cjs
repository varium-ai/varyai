"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { isChatTarget, bootstrapSource, PipeClient } = require("./agent.cjs");
const { PassThrough } = require("node:stream");
test("IDE services survive module renumbering without loading unrelated modules", async () => {
  const { workbenchServices } = require("./agent.cjs");
  for (const id of [19208, 99001]) {
    for (const state of ["ready", "uninitialized", "ambiguous", "missing"]) {
      const loaded = [],
        calls = [];
      const service = { getModelList() {} };
      const di = {
        isInitialized: () => state !== "uninitialized",
        resolve(key) {
          if (key === Symbol.for("aiAgent.IAiNativeModelService"))
            return service;
          if (key === Symbol.for("IModelService"))
            return {
              refreshModelStore: async (value) =>
                calls.push(["refresh", value]),
            };
          if (key === "iICubeModelManagementService")
            return {
              notifyModelChanges: (value) => calls.push(["notify", value]),
            };
          throw Error("Unexpected service");
        },
      };
      const factory = function () {
        return { mcf: "synthetic-container" };
      };
      const req = (key) => {
        loaded.push(String(key));
        assert.equal(String(key), String(id));
        return { mcf: { getInstance: () => di } };
      };
      req.m = {
        unrelated() {
          throw Error("must not load");
        },
      };
      if (state !== "missing") req.m[id] = factory;
      if (state === "ambiguous") req.m.other = factory;
      if (state !== "ready") {
        assert.throws(() => workbenchServices(req), /unsupported_ide_version/);
        assert.deepEqual(calls, []);
      } else {
        const actual = workbenchServices(req);
        assert.equal(actual.service, service);
        await actual.refresh();
        assert.deepEqual(calls, [
          ["refresh", { source: "varyai" }],
          ["notify", { source: "varyai" }],
        ]);
      }
      assert.equal(
        loaded.length,
        ["ready", "uninitialized"].includes(state) ? 1 : 0,
      );
    }
  }
});
test("model diagnostics expose only a recognized operation and numeric code", () => {
  const { safeModelError } = require("./agent.cjs");
  for (const value of [
    "model_add_failed:409",
    "model_restore_failed",
    "model_recovery_unavailable",
  ])
    assert.equal(safeModelError(value), value);
  for (const message of [
    "synthetic-private-detail",
    "model_add_failed:409 synthetic-secret",
    "model_remove_failed:secret",
    "model_update_failed:409\nprivate",
    "model_restore_failed:private",
    "model_recovery_unavailable private",
  ]) {
    assert.equal(safeModelError(message), "TRAE native bridge unavailable");
  }
});
test("accepts only local TRAE chat windows, not browser previews or other targets", () => {
  const root = "/Applications/Trae CN.app/Contents/Resources/app";
  assert.equal(
    isChatTarget(
      {
        type: "page",
        url: "vscode-file://vscode-app/Applications/Trae%20CN.app/Contents/Resources/app/out/vs/code/electron-browser/solo/solo-lite.html",
      },
      root,
    ),
    true,
  );
  assert.equal(
    isChatTarget(
      { type: "page", url: "https://example.com/solo/solo-lite.html" },
      root,
    ),
    false,
  );
  assert.equal(
    isChatTarget(
      {
        type: "page",
        url: "vscode-file://vscode-app/Other.app/Contents/Resources/app/out/vs/code/electron-browser/solo/solo-lite.html",
      },
      root,
    ),
    false,
  );
  assert.equal(
    isChatTarget(
      {
        type: "worker",
        url:
          "vscode-file://vscode-app" +
          encodeURI(root) +
          "/out/vs/code/electron-browser/workbench/workbench.html",
      },
      root,
    ),
    false,
  );
});
test("pipe correlates split responses and events without opening a TCP port", async () => {
  const input = new PassThrough(),
    output = new PassThrough();
  const events = [];
  const pipe = new PipeClient(input, output, (e) => events.push(e));
  let written = "";
  input.on("data", (d) => {
    written += d;
  });
  const pending = pipe.call("Runtime.enable", {}, "session");
  const request = JSON.parse(written.slice(0, -1));
  output.write('{"id":' + request.id + ',"res');
  output.write(
    'ult":{"ok":true}}\0{"method":"Target.targetCreated","params":{}}\0',
  );
  assert.deepEqual(await pending, { ok: true });
  assert.equal(events.length, 1);
  pipe.close();
});
test("bootstrap uses the common bridge and text audit binding", () => {
  const source = bootstrapSource();
  assert.match(source, /convertReqToRustPayload/);
  assert.match(source, /__varyaiTraeAudit/);
  assert.doesNotMatch(source, /getUserInfoWithToken/);
  assert.match(source, /previous.startingModel===1/);
  assert.match(source, /createStartingModelAdapter\(req\)/);
  assert.match(source, /__varyaiTraeSoloRequire/);
  new Function("return " + source);
});
test("Agent hooks its default connection client rather than the IDE client manager", () => {
  assert.match(bootstrapSource(), /IICubeAiChatConnectionService/);
  assert.match(bootstrapSource(), /getDefaultClient/);
});
test("closing the protocol rejects in-flight commands without closing the application pipe", async () => {
  const input = new PassThrough(),
    output = new PassThrough(),
    pipe = new PipeClient(input, output, () => {});
  const pending = pipe.call("Runtime.enable");
  pipe.close();
  await assert.rejects(pending, /native_pipe_closed/);
  assert.equal(input.destroyed, false);
});
test("resolves only an initialized Solo container and releases inspection references", async () => {
  const { initializeSolo } = require("./agent.cjs");
  const calls = [];
  const pipe = {
    async call(method, params) {
      calls.push(method);
      if (method === "Runtime.evaluate")
        return params.returnByValue
          ? { result: { value: false } }
          : { result: { objectId: "bootstrap" } };
      if (method === "Runtime.getProperties") {
        if (params.objectId === "bootstrap")
          return {
            internalProperties: [
              { name: "[[Scopes]]", value: { objectId: "scopes" } },
            ],
          };
        if (params.objectId === "scopes")
          return { result: [{ value: { objectId: "closure" } }] };
        return {
          result: [{ value: { type: "function", objectId: "runtime" } }],
        };
      }
      if (method === "Runtime.callFunctionOn") {
        assert.match(params.functionDeclaration, /isInitialized/);
        return { result: { value: true } };
      }
    },
  };
  await initializeSolo(pipe, "Agent");
  assert.equal(calls.at(-1), "Runtime.releaseObjectGroup");
  pipe.call = async (method) => {
    calls.push(method);
    return { result: { value: false } };
  };
  await assert.rejects(
    initializeSolo(pipe, "unsupported"),
    /unsupported_agent_version/,
  );
  assert.equal(calls.at(-1), "Runtime.releaseObjectGroup");
});
test("Solo container discovery accepts old and renumbered exports without loading unrelated modules", async () => {
  const { initializeSolo } = require("./agent.cjs");
  const vm = require("node:vm");
  for (const moduleId of [10052, 63503]) {
    for (const state of [
      "ready",
      "uninitialized",
      "ambiguous",
      "missing",
      "invalid",
    ]) {
      const loaded = [],
        calls = [];
      const di = {
        isInitialized: () => state !== "uninitialized",
        resolve() {},
      };
      if (state === "invalid") delete di.resolve;
      const runtime = (id) => {
        loaded.push(String(id));
        return { mcf: { getInstance: () => di } };
      };
      const factory = function (module, exports, require) {
        require.d(exports, { mcf: () => di });
      };
      runtime.m = {
        [moduleId]: factory,
        99999() {
          throw Error("unrelated module must not execute");
        },
      };
      if (state === "ambiguous") runtime.m[12345] = factory;
      if (state === "missing") delete runtime.m[moduleId];
      const context = { runtime };
      const pipe = {
        async call(method, params) {
          calls.push(method);
          if (method === "Runtime.evaluate")
            return params.returnByValue
              ? { result: { value: false } }
              : { result: { objectId: "bootstrap" } };
          if (method === "Runtime.getProperties") {
            if (params.objectId === "bootstrap")
              return {
                internalProperties: [
                  { name: "[[Scopes]]", value: { objectId: "scopes" } },
                ],
              };
            if (params.objectId === "scopes")
              return { result: [{ value: { objectId: "closure" } }] };
            return {
              result: [{ value: { type: "function", objectId: "runtime" } }],
            };
          }
          if (method === "Runtime.callFunctionOn")
            return {
              result: {
                value: vm.runInNewContext(
                  `(${params.functionDeclaration}).call(runtime)`,
                  context,
                ),
              },
            };
        },
      };
      if (state === "ready") {
        await initializeSolo(pipe, "Agent");
        assert.equal(context.__varyaiTraeSoloDI, di);
        assert.equal(context.__varyaiTraeSoloRequire, runtime);
      } else {
        await assert.rejects(
          initializeSolo(pipe, "Agent"),
          /unsupported_agent_version/,
        );
        assert.equal(context.__varyaiTraeSoloDI, undefined);
      }
      assert.deepEqual(
        loaded,
        ["ambiguous", "missing"].includes(state) ? [] : [String(moduleId)],
      );
      assert.equal(calls.at(-1), "Runtime.releaseObjectGroup");
    }
  }
});
test("usage shared by IDE and Agent is recorded once; message audits are never deduplicated", async () => {
  const { deduplicateUsage } = require("./agent.cjs");
  let count = 0;
  const send = deduplicateUsage(async () => {
    count++;
    return { recorded: true };
  });
  const usage = {
    sessionId: "session",
    requestId: "turn",
    usage: { input_tokens: 10 },
  };
  await Promise.all([send(usage), send(usage)]);
  assert.equal(count, 1);
  await send({ ...usage, sessionId: "another" });
  assert.equal(count, 2);
  await Promise.all([send({ body: {} }), send({ body: {} })]);
  assert.equal(count, 4);
  let failed = true;
  const retry = deduplicateUsage(async () => {
    if (failed) {
      failed = false;
      throw Error("offline");
    }
    return true;
  });
  await assert.rejects(retry(usage), /offline/);
  assert.equal(await retry(usage), true);
});
test(
  "macOS bundle lookup accepts the vendor URL canonical casing",
  { skip: process.platform !== "darwin" },
  () => {
    assert.equal(
      isChatTarget(
        {
          type: "page",
          url: "vscode-file://vscode-app/Applications/Trae%20CN.app/Contents/Resources/app/out/vs/code/electron-browser/solo/solo-lite.html",
        },
        "/Applications/TRAE CN.app/Contents/Resources/app",
      ),
      true,
    );
  },
);
test("Agent model operations use the native authenticated client without swallowing failures", async () => {
  const { nativeModelService } = require("./agent.cjs");
  const calls = [];
  let fail = false;
  const service = nativeModelService({
    getDefaultClient: async () => ({
      request: async (request) => {
        calls.push(request);
        return fail
          ? { code: 1 }
          : { code: 0, data: { model_list: [{ name: "model" }] } };
      },
    }),
  });
  assert.deepEqual(await service.getModelList(), {
    model_list: [{ name: "model" }],
  });
  const fields = { ak: "test-only", model_name: "model" };
  await service.addCustomModel(fields);
  await service.updateCustomModel(fields);
  assert.deepEqual(
    calls.map((c) => c.method),
    ["model_list", "add_custom_model", "update_custom_model"],
  );
  assert.equal(calls[1].data, fields);
  fail = true;
  await assert.rejects(service.getModelList(), /model_list_unavailable/);
});
test("passive connection accepts only the random local browser endpoint and never launches TRAE", async () => {
  const { connectEndpoint } = require("./agent.cjs");
  class FakeSocket extends EventTarget {
    static requests = [];
    static rejectDiscovery = false;
    constructor(url) {
      super();
      this.url = url;
      FakeSocket.current = this;
      queueMicrotask(() => this.dispatchEvent(new Event("open")));
    }
    send(message) {
      const request = JSON.parse(message);
      FakeSocket.requests.push(request);
      if (request.method === "Target.setDiscoverTargets")
        this.discovering = request.params.discover;
      const response =
        request.method === "Target.setDiscoverTargets" &&
        FakeSocket.rejectDiscovery
          ? { error: { code: -32601, message: "synthetic discovery failure" } }
          : {
              result:
                request.method === "Target.getTargets"
                  ? {
                      targetInfos: this.discovering
                        ? [{ targetId: "native-workbench", type: "page" }]
                        : [],
                    }
                  : { ok: true },
            };
      queueMicrotask(() =>
        this.dispatchEvent(
          new MessageEvent("message", {
            data: JSON.stringify({ id: request.id, ...response }),
          }),
        ),
      );
    }
    close() {
      this.closed = true;
      this.dispatchEvent(new Event("close"));
    }
  }
  await assert.rejects(
    connectEndpoint(
      "ws://example.com/devtools/browser/test",
      () => {},
      FakeSocket,
    ),
    /invalid_endpoint/,
  );
  await assert.rejects(
    connectEndpoint(
      "ws://127.0.0.1:8888/devtools/page/test",
      () => {},
      FakeSocket,
    ),
    /invalid_endpoint/,
  );
  const { pipe, socket } = await connectEndpoint(
    "ws://127.0.0.1:8888/devtools/browser/test",
    () => {},
    FakeSocket,
  );
  assert.deepEqual(FakeSocket.requests[0], {
    id: 1,
    method: "Target.setDiscoverTargets",
    params: { discover: true },
  });
  assert.deepEqual(await pipe.call("Target.getTargets"), {
    targetInfos: [{ targetId: "native-workbench", type: "page" }],
  });
  assert.deepEqual(await pipe.call("Runtime.enable"), { ok: true });
  socket.close();
  FakeSocket.rejectDiscovery = true;
  await assert.rejects(
    connectEndpoint(
      "ws://127.0.0.1:8888/devtools/browser/test",
      () => {},
      FakeSocket,
    ),
    /native_command_failed/,
  );
  assert.equal(FakeSocket.current.closed, true);
});

test("bootstrap replaces an older resident bridge and keeps the current bridge on repeated scans", async () => {
  const vm = require("node:vm");
  let released = 0,
    audits = 0,
    legacyAudits = 0;
  const proto = {
    async ib(request) {
      return {
        service: request.service,
        method: request.method,
        data: request.data,
      };
    },
    async request(request) {
      return { payload: await this.ib(request) };
    },
  };
  const original = proto.ib;
  const client = Object.create(proto);
  const context = {
    __varyaiTraeNativeBridge: {
      revision: 2,
      hookRevision: 7,
      auditBinding: "__varyaiTraeAuditV7",
      bridge: {
        replace() {},
        disable() {
          released++;
        },
      },
    },
    __varyaiTraeSoloDI: {
      isInitialized: () => true,
      resolve: (id) =>
        id === "IICubeAiChatConnectionService"
          ? { getDefaultClient: async () => client }
          : { notifyModelChanges: async () => {} },
    },
    document: { querySelector: () => null },
    crypto: { randomUUID: () => "synthetic-audit-id" },
    __varyaiTraeAuditV7() {
      legacyAudits++;
      throw Error("older helper is disabled");
    },
    __varyaiTraeAuditV11(payload) {
      audits++;
      const { id, value } = JSON.parse(payload);
      value.body.messages[0].content[0].text = "[REDACTED]";
      context.__varyaiTraeNativeBridge.resolve(id, { body: value.body }, false);
    },
    setTimeout,
    clearTimeout,
  };
  await vm.runInNewContext(bootstrapSource(), context);
  assert.equal(released, 1);
  assert.equal(context.__varyaiTraeNativeBridge.revision, 2);
  assert.equal(context.__varyaiTraeNativeBridge.modelVerification, 1);
  assert.equal(context.__varyaiTraeNativeBridge.hookRevision, 11);
  assert.equal(context.__varyaiTraeNativeBridge.startingModel, 1);
  const current = context.__varyaiTraeNativeBridge;
  assert.equal(typeof current.bridge.replace, "function");
  await current.bridge.enable();
  const wrapped = proto.ib;
  assert.notEqual(wrapped, original);
  await vm.runInNewContext(bootstrapSource(), context);
  assert.equal(context.__varyaiTraeNativeBridge, current);
  assert.equal(proto.ib, wrapped);
  assert.equal(released, 1);
  await vm.runInNewContext(
    `(async()=>{
    const previous=globalThis.__varyaiTraeNativeBridge;
    if(previous?.revision===2&&typeof previous.bridge?.replace==='function')return true;
    await previous?.bridge?.disable();
    throw Error('older helper attempted to replace the current renderer');
  })()`,
    context,
  );
  assert.equal(context.__varyaiTraeNativeBridge, current);
  assert.equal(proto.ib, wrapped);
  const result = await client.request({
    service: "chat",
    method: "chat",
    data: { query: "synthetic" },
  });
  assert.equal(result.payload.data.query, "[REDACTED]");
  assert.equal(audits, 1);
  assert.equal(legacyAudits, 0);
  assert.equal(current.bridge.enabled, true);
  const newer = {
    ...current,
    hookRevision: 12,
    auditBinding: "__varyaiTraeAuditV12",
  };
  context.__varyaiTraeNativeBridge = newer;
  await vm.runInNewContext(bootstrapSource(), context);
  assert.equal(context.__varyaiTraeNativeBridge, newer);
  assert.equal(proto.ib, wrapped);
  current.bridge.disable();
  assert.equal(proto.ib, original);
});
