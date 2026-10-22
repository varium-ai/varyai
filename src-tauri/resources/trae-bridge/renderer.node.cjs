const { test } = require("node:test");
const assert = require("node:assert/strict");
const { createBridge, rendererSource } = require("./renderer.cjs");
function fixture() {
  const calls = [];
  const proto = {
    async convertReqToRustPayload(q) {
      calls.push(q);
      return q;
    },
  };
  const client = Object.create(proto);
  const models = [];
  const service = {
    _clientManagerService: { getClient: async () => client },
    getModelList: async () => ({ model_list: models }),
    addCustomModel: async (x) => {
      models.push({ ...x, custom_model_id: "test-id" });
      return { code: 0 };
    },
    updateCustomModel: async (x) => {
      const i = models.findIndex((m) => m.custom_model_id === x.id);
      if (x.action === "delete") models.splice(i, 1);
      else Object.assign(models[i], x);
      return { code: 0 };
    },
  };
  return { service, client, calls, models, proto };
}
test("redacts both native text carriers without changing credentials or protocol metadata", async () => {
  const f = fixture();
  const original = f.proto.convertReqToRustPayload;
  const b = createBridge(f.service, async (x) => ({
    body: {
      messages: x.body.messages.map((m) => ({
        ...m,
        content: m.content.map((v) => ({
          ...v,
          text: v.text.replaceAll("sensitive", "[REDACTED]"),
        })),
      })),
    },
  }));
  await b.enable();
  const q = {
    service: "lite",
    method: "send_message",
    data: {
      query: "sensitive",
      content: [{ type: "text", text_content: "sensitive", other: 1 }],
      custom_model: { ak: "credential" },
      model_name: "model",
    },
    context: { x: 1 },
  };
  const result = await f.client.convertReqToRustPayload(q);
  assert.equal(result.data.query, "[REDACTED]");
  assert.equal(result.data.content[0].text_content, "[REDACTED]");
  assert.equal(result.data.custom_model.ak, "credential");
  assert.equal(q.data.query, "sensitive");
  b.disable();
  assert.equal(f.proto.convertReqToRustPayload, original);
});
test("block never forwards and audit failure releases the hook", async () => {
  const f = fixture();
  const b = createBridge(f.service, async () => ({ blocked: true }));
  await b.enable();
  await assert.rejects(
    f.client.convertReqToRustPayload({
      service: "lite",
      method: "send_message",
      data: { query: "sensitive" },
    }),
    /VaryAI/,
  );
  assert.equal(f.calls.length, 0);
  b.disable();
  const c = createBridge(f.service, async () => {
    throw Error("offline");
  });
  await c.enable();
  await f.client.convertReqToRustPayload({
    service: "lite",
    method: "send_message",
    data: { query: "normal" },
  });
  assert.equal(f.calls.length, 1);
  assert.equal(c.enabled, false);
});
test("model management returns no credential and removes only owned models", async () => {
  const f = fixture();
  f.models.push({
    custom_model_id: "user",
    display_name: "User",
    ak: "secret",
  });
  const b = createBridge(f.service, async () => ({}));
  const config = {
    baseUrl: "http://127.0.0.1:1/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: [{ model: "m1" }, { model: "m2" }],
  };
  const result = await b.apply("p1", "Example", config);
  assert.equal(result.length, 2);
  assert.ok(!JSON.stringify(await b.list()).includes("secret"));
  await b.remove("p1");
  assert.equal(f.models.length, 1);
  assert.equal(f.models[0].custom_model_id, "user");
});
test("renderer resolves the existing case-sensitive module URL inside the window", () => {
  assert.match(rendererSource(), /location.href.split/);
  assert.ok(!rendererSource().includes("/Applications/TRAE CN.app"));
});
test("native usage parsing forwards numeric accounting only and preserves stream messages", async () => {
  const f = fixture();
  let controller;
  f.proto.requestForStream = function () {
    controller = { write: (v) => v, complete: (v) => v };
    return { getController: () => controller };
  };
  const events = [];
  const b = createBridge(f.service, async (x) => {
    events.push(x);
    return { body: x.body };
  });
  await b.enable();
  await f.client.convertReqToRustPayload({
    service: "lite",
    method: "send_message",
    data: { chat_session_id: "session", model_name: "m", query: "normal" },
  });
  f.client.requestForStream({
    service: "lite",
    method: "subscribe_events",
    data: { chat_session_id: "session", reply_to_message_id: "reply" },
  });
  const v = {
    data: {
      event: "token_usage",
      payload: {
        prompt_tokens_total: 101,
        completion_tokens_total: 8,
        ak: "secret",
      },
    },
  };
  assert.equal(controller.write(v), v);
  controller.write({
    data: { event: "done", payload: { reply_to_message_id: "reply" } },
  });
  await new Promise((resolve) => setImmediate(resolve));
  const usage = events.find((x) => x.usage);
  assert.equal(usage.usage.input_tokens, 101);
  assert.equal(usage.usage.output_tokens, 8);
  assert.ok(!JSON.stringify(usage).includes("secret"));
  b.disable();
});
test("native accounting accepts protobuf numeric strings and skips empty default totals", async () => {
  const f = fixture();
  let controller;
  f.proto.requestForStream = () => ({
    getController: () => (controller = { write: (v) => v }),
  });
  const reports = [];
  const b = createBridge(f.service, async (x) => {
    reports.push(x);
    return { body: x.body };
  });
  await b.enable();
  f.client.requestForStream({
    service: "lite",
    method: "subscribe_events",
    data: { chat_session_id: "s", reply_to_message_id: "r" },
  });
  controller.write({
    data: {
      event: "token_usage",
      payload: {
        prompt_tokens_total: 0,
        prompt_tokens: 0,
        input: "32",
        output: "9",
      },
    },
  });
  controller.write({
    data: { event: "done", payload: { reply_to_message_id: "r" } },
  });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(reports[0].usage.input_tokens, 32);
  assert.equal(reports[0].usage.output_tokens, 9);
  b.disable();
});
test("new-session initial_message is audited before session creation", async () => {
  const f = fixture();
  const b = createBridge(f.service, async (x) => ({
    body: {
      messages: [
        {
          content: x.body.messages[0].content.map((p) => ({
            ...p,
            text: "[REDACTED]",
          })),
        },
      ],
    },
  }));
  await b.enable();
  const q = {
    service: "lite",
    method: "create_chat_session",
    data: {
      local_project_id: "keep",
      initial_message: {
        query: "sensitive",
        content: [{ type: "text", text_content: "sensitive" }],
      },
    },
  };
  const result = await f.client.convertReqToRustPayload(q);
  assert.equal(result.data.initial_message.query, "[REDACTED]");
  assert.equal(
    result.data.initial_message.content[0].text_content,
    "[REDACTED]",
  );
  assert.equal(result.data.local_project_id, "keep");
  assert.equal(q.data.initial_message.query, "sensitive");
  b.disable();
});
test("native model storage receives full protocol endpoints without duplicating paths", async () => {
  for (const [protocol, baseUrl, expected] of [
    [
      "openai",
      "https://upstream.example/v1",
      "https://upstream.example/v1/chat/completions",
    ],
    [
      "openai",
      "https://upstream.example/v1/chat/completions/",
      "https://upstream.example/v1/chat/completions",
    ],
    [
      "openai-responses",
      "https://upstream.example/v1",
      "https://upstream.example/v1/responses",
    ],
    [
      "anthropic",
      "https://upstream.example",
      "https://upstream.example/v1/messages",
    ],
    [
      "anthropic",
      "https://upstream.example/v1",
      "https://upstream.example/v1/messages",
    ],
  ]) {
    const f = fixture();
    await createBridge(f.service, async () => ({})).apply("test", "Test", {
      protocol,
      baseUrl,
      apiKey: "fake-test-only",
      models: [{ model: "test" }],
    });
    assert.equal(f.models[0].base_url, expected);
  }
});
test("native model edits refresh the desktop model selector", async () => {
  const f = fixture();
  let refreshed = 0;
  const b = createBridge(
    f.service,
    async () => ({}),
    async () => {
      refreshed++;
    },
  );
  await b.apply("test", "Test", {
    protocol: "openai",
    baseUrl: "https://upstream.example/v1",
    apiKey: "fake-only",
    models: [{ model: "test" }],
  });
  assert.equal(refreshed, 1);
  await b.remove("test");
  assert.equal(refreshed, 2);
});
