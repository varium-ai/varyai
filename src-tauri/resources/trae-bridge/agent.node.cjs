"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { isChatTarget, bootstrapSource, PipeClient } = require("./agent.cjs");
const { PassThrough } = require("node:stream");
test("model diagnostics expose only a recognized operation and numeric code", () => {
  const { safeModelError } = require("./agent.cjs");
  assert.equal(safeModelError("model_add_failed:409"), "model_add_failed:409");
  for (const message of [
    "synthetic-private-detail",
    "model_add_failed:409 synthetic-secret",
    "model_remove_failed:secret",
    "model_update_failed:409\nprivate",
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
    constructor(url) {
      super();
      this.url = url;
      FakeSocket.current = this;
      queueMicrotask(() => this.dispatchEvent(new Event("open")));
    }
    send(message) {
      const request = JSON.parse(message);
      queueMicrotask(() =>
        this.dispatchEvent(
          new MessageEvent("message", {
            data: JSON.stringify({ id: request.id, result: { ok: true } }),
          }),
        ),
      );
    }
    close() {
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
  assert.deepEqual(await pipe.call("Runtime.enable"), { ok: true });
  socket.close();
});
