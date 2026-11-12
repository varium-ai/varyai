"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { PassThrough } = require("node:stream");
const path = require("node:path");
const { pathToFileURL } = require("node:url");
const { isTarget, Pipe, apply } = require("./agent.cjs");

test("accepts only the installed client's main renderer, excluding previews and browser tabs", () => {
  const resources = path.resolve("synthetic/Resources");
  for (const [app, entry] of [
    ["workbuddy", "renderer/index.html"],
    ["qoder", "out/renderer/index.html"],
  ]) {
    const url = pathToFileURL(path.join(resources, "app.asar", entry)).href;
    assert.equal(
      isTarget({ type: "page", url: url + "?theme=dark#chat" }, resources, app),
      true,
    );
    for (const target of [
      { type: "iframe", url },
      { type: "page", url: "https://synthetic.invalid/" },
      { type: "page", url: url.replace("index.html", "preview.html") },
    ]) {
      assert.equal(isTarget(target, resources, app), false);
    }
  }
  assert.equal(
    isTarget(
      {
        type: "page",
        url: "qoder-cn-app://renderer/index.html?workbenchScope=primary#/",
      },
      resources,
      "qoder",
    ),
    true,
  );
  for (const url of [
    "qoder-cn-app://renderer/preview.html?workbenchScope=primary",
    "qoder-cn-app://renderer/index.html?workbenchScope=plugin",
    "qoder-cn-app://other/index.html?workbenchScope=primary",
  ]) {
    assert.equal(isTarget({ type: "page", url }, resources, "qoder"), false);
  }
});
test("private pipe handles fragmented and interleaved replies without exporting protocol errors", async () => {
  const input = new PassThrough(),
    output = new PassThrough(),
    pipe = new Pipe(input, output);
  const call = pipe.call("Target.getTargets");
  output.write('{"id":1,"result":');
  output.write('{"targetInfos":[]}}\0');
  assert.deepEqual(await call, { targetInfos: [] });
  const failed = pipe.call("Runtime.evaluate");
  output.write('{"id":2,"error":{"message":"synthetic-private-diagnostic"}}\0');
  await assert.rejects(failed, /^Error: model_default_unavailable$/);
  pipe.close();
});
test("Qoder fails closed unless exactly one enabled Vary provider is verified and selection is read back", async () => {
  const calls = [];
  const pipe = {
    async call(method, params) {
      calls.push({ method, params });
      return calls.length === 1
        ? { result: { value: null } }
        : { result: { value: { applied: true } } };
    },
  };
  const plan = {
    app: "qoder",
    revision: "synthetic",
    model: "synthetic",
    restore: false,
  };
  await assert.rejects(
    apply(pipe, "session", plan),
    /^Error: model_default_unavailable$/,
  );
  assert.equal(calls.length, 1);
  assert.ok(calls[0].params.expression.includes('providerKey==="varyai"'));
  const broken = {
    async call() {
      return { exceptionDetails: { text: "synthetic-private-error" } };
    },
  };
  await assert.rejects(
    apply(broken, "session", plan, "byok:varyai"),
    /^Error: model_default_unavailable$/,
  );
});
