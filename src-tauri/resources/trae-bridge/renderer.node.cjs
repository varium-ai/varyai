const { test } = require("node:test");
const assert = require("node:assert/strict");
const {
  createBridge: nativeBridge,
  rendererSource,
} = require("./renderer.cjs");
const stores = new WeakMap();
function createBridge(service, audit, refresh) {
  if (!stores.has(service)) {
    const values = new Map();
    stores.set(service, {
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => values.set(key, value),
      key: (index) => [...values.keys()][index] ?? null,
      get length() {
        return values.size;
      },
    });
  }
  return nativeBridge(service, audit, refresh, stores.get(service));
}
function fixture() {
  const calls = [];
  let nextModelId = 0;
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
      models.push({ ...x, custom_model_id: `test-${++nextModelId}` });
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
function encryptedFixture() {
  const f = fixture();
  let revision = 0;
  const nativeRow = (row) => {
    const { model_name, ...fields } = row;
    return {
      ...fields,
      name: `${row.provider}//${model_name}`,
      ak: `sealed-fixture-${++revision}`,
    };
  };
  f.service.addCustomModel = async (row) => {
    if (f.models.some((m) => m.name === `${row.provider}//${row.model_name}`))
      return { code: 4024 };
    f.models.push({ ...nativeRow(row), custom_model_id: `native-${revision}` });
    return { code: 0 };
  };
  f.service.updateCustomModel = async (row) => {
    const index = f.models.findIndex((m) => m.custom_model_id === row.id);
    if (row.action === "delete") f.models.splice(index, 1);
    else {
      assert.ok(
        !("provider" in row) && !("model_name" in row) && !("is_custom" in row),
      );
      const existing = f.models[index];
      Object.assign(
        existing,
        nativeRow({
          ...existing,
          model_name: existing.name.slice(existing.provider.length + 2),
          ...row,
        }),
      );
    }
    return { code: 0 };
  };
  return f;
}
test("shared delivery verifies encrypted native credentials across reloads and rejects external changes", async () => {
  const f = encryptedFixture();
  const config = {
    protocol: "openai",
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-local-credential",
    models: [{ model: "supplier/model-a", displayName: "Model A" }],
  };
  const id = "shared_catalog_connection";
  const bridge = createBridge(f.service, async () => ({}));
  await bridge.apply(id, "Vary AI", config);
  assert.equal(await bridge.matches(id, config), true);
  assert.equal(
    await createBridge(f.service, async () => ({})).matches(id, config),
    true,
  );
  assert.equal((await bridge.list())[0].name, "supplier/model-a");
  assert.equal(
    await bridge.matches(id, { ...config, apiKey: "different-credential" }),
    false,
  );
  const original = { ...f.models[0] };
  for (const change of [
    { ak: "externally-replaced-sealed-value" },
    { base_url: "https://changed.example/v1/chat/completions" },
    { provider: "custom_anthropic_compatible" },
    { name: "custom_openai_compatible//different-model" },
  ]) {
    Object.assign(f.models[0], original, change);
    assert.equal(await bridge.matches(id, config), false);
  }
  Object.assign(f.models[0], original);
  await bridge.apply(id, "Vary AI", config);
  assert.equal(f.models.length, 1);
  assert.equal(await bridge.matches(id, config), true);
  stores.get(f.service).setItem(`varyai.trae.verification.${id}`, "[null]");
  assert.equal(await bridge.matches(id, config), false);
  await bridge.apply(id, "Vary AI", config);
  const storage = stores.get(f.service);
  for (let i = 0; i < storage.length; i++) {
    const value = storage.getItem(storage.key(i));
    assert.ok(!value.includes(config.apiKey));
    assert.ok(!value.includes(f.models[0].ak));
  }
});

test("merged verification accepts the prior shared digest without weakening native identity checks", async () => {
  const f = encryptedFixture();
  const id = "shared_catalog_connection";
  const config = {
    protocol: "openai",
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-local-credential",
    models: [{ model: "supplier/model-a", displayName: "Model A" }],
  };
  const bridge = createBridge(f.service, async () => ({}));
  await bridge.apply(id, "VaryAI", config);
  const row = f.models[0];
  const proof = require("node:crypto")
    .createHash("sha256")
    .update(
      JSON.stringify([
        config.protocol,
        config.models[0].model,
        config.apiKey,
        row.base_url,
        row.provider,
        row.name,
        row.ak,
      ]),
    )
    .digest("hex");
  const storage = stores.get(f.service);
  storage.setItem(`varyai.trae.verification.${id}`, "[]");
  storage.setItem(
    `varyai.trae.model-verification.${id}`,
    JSON.stringify({ [row.custom_model_id]: proof }),
  );
  assert.equal(
    await createBridge(f.service, async () => ({})).matches(id, config),
    true,
  );
  assert.equal(
    await bridge.matches(id, { ...config, apiKey: "synthetic-changed" }),
    false,
  );
  const original = row.ak;
  row.ak = "synthetic-externally-changed";
  assert.equal(await bridge.matches(id, config), false);
  row.ak = original;
  await bridge.remove(id);
  assert.equal(storage.getItem(`varyai.trae.model-verification.${id}`), "{}");
});

test("starting selection resolves only an exact owned native model and propagates failures", async () => {
  const f = fixture();
  const storage = new Map();
  const local = {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
  };
  const selected = [];
  const bridge = nativeBridge(
    f.service,
    async () => ({}),
    async () => {},
    local,
    {
      select: async (id) => {
        selected.push(id);
        return true;
      },
    },
  );
  const config = {
    protocol: "openai",
    baseUrl: "http://127.0.0.1:1/v1",
    apiKey: "synthetic",
    models: [{ model: "fixture-a" }],
  };
  await bridge.apply("fixture", "Fixture", config);
  await assert.rejects(
    bridge.select("unowned", "fixture-a"),
    /starting_model_unavailable/,
  );
  await assert.rejects(
    bridge.select("fixture", "missing"),
    /starting_model_unavailable/,
  );
  assert.deepEqual(selected, []);
  assert.equal(await bridge.select("fixture", "fixture-a"), true);
  assert.deepEqual(selected, [f.models[0].custom_model_id]);
  const unavailable = nativeBridge(
    f.service,
    async () => ({}),
    async () => {},
    local,
    {
      select: async () => {
        throw Error("starting_model_unavailable");
      },
    },
  );
  await assert.rejects(
    unavailable.select("fixture", "fixture-a"),
    /starting_model_unavailable/,
  );
});
test("shared delivery recovers missing ownership only for its exact native route and readable label", async () => {
  for (const app of ["trae", "trae-intl"]) {
    const f = encryptedFixture();
    const id = "shared_catalog_connection";
    const config = {
      protocol: "openai",
      baseUrl: `http://127.0.0.1:1/connections/${app}/v1`,
      apiKey: "synthetic-local-credential",
      models: [{ model: "supplier/model-a" }, { model: "supplier/model-b" }],
    };
    const bridge = createBridge(f.service, async () => ({}));
    await bridge.apply(id, "Vary AI", config);
    const nativeIds = f.models.map((m) => m.custom_model_id);
    stores.get(f.service).setItem(`varyai.trae.models.${id}`, "[]");
    f.models.push({
      custom_model_id: "manual",
      name: "custom_openai_compatible//manual",
      display_name: "Manual",
      provider: "custom_openai_compatible",
      base_url: `${config.baseUrl}/chat/completions`,
      ak: "manual-sealed-value",
    });
    await createBridge(f.service, async () => ({})).apply(
      id,
      "Vary AI",
      config,
    );
    assert.deepEqual(
      f.models.slice(0, 2).map((m) => m.custom_model_id),
      nativeIds,
    );
    assert.equal(f.models.length, 3);
    assert.equal(await bridge.matches(id, config), true);
    await bridge.remove(id);
    assert.deepEqual(
      f.models.map((m) => m.custom_model_id),
      ["manual"],
    );
  }
});
test("shared ownership recovery refuses unrelated or ambiguous native models", async () => {
  for (const change of [
    { base_url: "https://external.example/v1/chat/completions" },
    { display_name: "Manual model" },
    { provider: "custom_anthropic_compatible" },
    { duplicate: true },
  ]) {
    const f = encryptedFixture();
    const id = "shared_catalog_connection";
    const config = {
      protocol: "openai",
      baseUrl: "http://127.0.0.1:1/connections/trae/v1",
      apiKey: "synthetic-local-credential",
      models: [{ model: "supplier/model-a" }],
    };
    const bridge = createBridge(f.service, async () => ({}));
    await bridge.apply(id, "Vary AI", config);
    stores.get(f.service).setItem(`varyai.trae.models.${id}`, "[]");
    if (change.duplicate)
      f.models.push({ ...f.models[0], custom_model_id: "ambiguous" });
    else Object.assign(f.models[0], change);
    const before = structuredClone(f.models);
    await assert.rejects(bridge.apply(id, "Vary AI", config));
    assert.deepEqual(f.models, before);
  }
});
test("shared catalog verification detects removed or changed native models without exporting credentials", async () => {
  const f = fixture();
  const bridge = createBridge(f.service, async () => ({}));
  const config = {
    protocol: "openai",
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-local",
    models: [{ model: "auto/shared", displayName: "Shared" }],
  };
  assert.equal(
    await bridge.matches("shared_catalog_connection", config),
    false,
  );
  await bridge.apply("shared_catalog_connection", "Vary AI", config);
  assert.equal(await bridge.matches("shared_catalog_connection", config), true);
  f.models[0].ak = "external";
  assert.equal(
    await bridge.matches("shared_catalog_connection", config),
    false,
  );
  f.models[0].ak = config.apiKey;
  f.models[0].base_url = "https://changed.example/v1/chat/completions";
  assert.equal(
    await bridge.matches("shared_catalog_connection", config),
    false,
  );
  f.models.length = 0;
  assert.equal(
    await bridge.matches("shared_catalog_connection", config),
    false,
  );
});
test("shared catalog verifies namespaced models and protected credentials across bridge reloads", async () => {
  const f = fixture();
  f.service.getModelList = async () => ({
    model_list: f.models.map(({ model_name, ak, ...row }) => ({
      ...row,
      name: `synthetic-owner//${model_name}`,
      ak: `synthetic-protected-${ak}`,
    })),
  });
  const config = {
    protocol: "openai",
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-private-local-credential",
    models: [{ model: "auto/shared", displayName: "Shared" }],
  };
  const id = "shared_catalog_connection";
  const bridge = createBridge(f.service, async () => ({}));
  await bridge.apply(id, "Vary AI", config);
  assert.equal(await bridge.matches(id, config), true);
  assert.equal(
    await createBridge(f.service, async () => ({})).matches(id, config),
    true,
  );
  assert.equal(
    await bridge.matches(id, {
      ...config,
      apiKey: "synthetic-other-credential",
    }),
    false,
  );
  const nativeId = f.models[0].custom_model_id;
  f.models[0].ak = "synthetic-external-change";
  assert.equal(await bridge.matches(id, config), false);
  await bridge.apply(id, "Vary AI", config);
  assert.equal(f.models.length, 1);
  assert.equal(f.models[0].custom_model_id, nativeId);
  assert.equal(await bridge.matches(id, config), true);
  f.models[0].model_name = "auto/foreign";
  assert.equal(await bridge.matches(id, config), false);
  const storage = stores.get(f.service);
  for (let index = 0; index < storage.length; index++) {
    const value = storage.getItem(storage.key(index));
    assert.ok(!value.includes(config.apiKey));
    assert.ok(!value.includes("synthetic-protected-"));
  }
});
test("shared catalog reapply recovers interrupted ownership without changing unrelated models", async () => {
  const f = fixture();
  const add = f.service.addCustomModel;
  f.service.addCustomModel = async (fields) =>
    f.models.some(
      (row) =>
        row.model_name === fields.model_name &&
        row.base_url === fields.base_url,
    )
      ? { code: 4024 }
      : add(fields);
  const id = "shared_catalog_connection";
  const config = {
    protocol: "openai",
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-local",
    models: [{ model: "auto/shared", displayName: "Shared" }],
  };
  const bridge = createBridge(f.service, async () => ({}));
  await bridge.apply(id, "Vary AI", config);
  const nativeId = f.models[0].custom_model_id;
  const foreign = {
    ...f.models[0],
    custom_model_id: "user-model",
    base_url: "https://user.example/v1/chat/completions",
  };
  f.models.push(foreign);
  stores.get(f.service).setItem(`varyai.trae.models.${id}`, "[]");
  assert.equal(await bridge.matches(id, config), false);
  await bridge.apply(id, "Vary AI", config);
  assert.equal(f.models.length, 2);
  assert.equal(f.models[0].custom_model_id, nativeId);
  assert.equal(f.models[0].display_name, "Shared · Vary AI");
  assert.deepEqual(f.models[1], foreign);
  assert.equal(await bridge.matches(id, config), true);
  f.models.push({ ...f.models[0], custom_model_id: "ambiguous-model" });
  stores.get(f.service).setItem(`varyai.trae.models.${id}`, "[]");
  await assert.rejects(
    bridge.apply(id, "Vary AI", config),
    /model_add_failed:4024/,
  );
  assert.equal(f.models.length, 3);
  assert.deepEqual(f.models[1], foreign);
});
test("native model failures retain only operation and numeric vendor code", async () => {
  const f = fixture();
  f.service.addCustomModel = async () => ({
    code: 409,
    message: "synthetic-private-detail",
  });
  const bridge = createBridge(f.service, async () => ({}));
  await assert.rejects(
    bridge.apply("fixture", "Fixture", {
      protocol: "openai",
      baseUrl: "https://fixture.example/v1",
      apiKey: "synthetic-secret",
      models: [{ model: "fixture-model" }],
    }),
    { message: "model_add_failed:409" },
  );
});
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
function minifiedFixture(method) {
  const calls = [],
    streams = [];
  const Client = new Function(
    "calls",
    "streams",
    `return class {
    async ${method}(request, ...metadata) {
      calls.push({request, metadata, client:this});
      return {service:request.service, method:request.method, data:request.data};
    }
    async request(request, ...metadata) {
      const envelope={payload:await this.${method}(request, ...metadata)};
      return envelope.payload;
    }
    requestForStream(request, ...metadata) {
      const controller={write(value){return value;}};
      const stream={getController:()=>controller};
      stream.done=(async()=>{
        const envelope={payload:await this.${method}(request, ...metadata)};
        return controller.write(envelope.payload);
      })();
      streams.push(stream);
      return stream;
    }
  }`,
  )(calls, streams);
  const client = new Client();
  return {
    calls,
    streams,
    client,
    proto: Client.prototype,
    service: { _clientManagerService: { getClient: async () => client } },
  };
}
test("named and minified converters inspect requests and streams without changing their native contracts", async () => {
  for (const method of ["convertReqToRustPayload", "ib", "r7"]) {
    const f = minifiedFixture(method),
      original = f.proto[method];
    let inspections = 0;
    const bridge = createBridge(f.service, async (value) => {
      inspections++;
      return {
        body: {
          messages: [
            {
              content: value.body.messages[0].content.map((part) => ({
                text: part.text.replaceAll("sensitive", "[REDACTED]"),
              })),
            },
          ],
        },
      };
    });
    await bridge.enable();
    await bridge.enable();
    const metadata = { credential: "synthetic-key" };
    const request = {
      service: "chat",
      method: "chat",
      data: {
        query: "sensitive",
        content: [{ text_content: "sensitive", extra: true }],
        custom_model: { ak: "synthetic-key" },
      },
      context: { keep: true },
    };
    const response = await f.client.request(
      request,
      metadata,
      "synthetic-device",
    );
    assert.equal(response.data.query, "[REDACTED]");
    const stream = f.client.requestForStream(
      request,
      metadata,
      "synthetic-device",
    );
    assert.equal(stream, f.streams[0]);
    assert.equal(
      (await stream.done).data.content[0].text_content,
      "[REDACTED]",
    );
    assert.equal(inspections, 2);
    assert.equal(request.data.query, "sensitive");
    for (const call of f.calls) {
      assert.equal(call.client, f.client);
      assert.equal(call.metadata[0], metadata);
      assert.equal(call.metadata[1], "synthetic-device");
      assert.equal(call.request.context, request.context);
      assert.equal(call.request.data.custom_model, request.data.custom_model);
      assert.equal(call.request.data.content[0].extra, true);
    }
    await f.client.request({
      service: "model",
      method: "model_list",
      data: {},
    });
    assert.equal(inspections, 2);
    bridge.disable();
    assert.equal(f.proto[method], original);
  }
});
test("minified converter blocks before forwarding and releases interception after audit failure", async () => {
  const f = minifiedFixture("ib"),
    original = f.proto.ib;
  const request = {
    service: "lite",
    method: "send_message",
    data: { query: "synthetic" },
  };
  let unavailable = false;
  const bridge = createBridge(f.service, async () => {
    if (unavailable) throw Error("synthetic outage");
    return { blocked: true };
  });
  await bridge.enable();
  await assert.rejects(f.client.request(request), /VaryAI/);
  await assert.rejects(f.client.requestForStream(request).done, /VaryAI/);
  assert.equal(f.calls.length, 0);
  unavailable = true;
  assert.equal((await f.client.request(request)).data, request.data);
  assert.equal(f.proto.ib, original);
  assert.equal(bridge.enabled, false);
});
test("unsupported converter layouts are rejected without patching a different method", async () => {
  for (const layout of ["missing", "ambiguous", "different-protocol"]) {
    const f = minifiedFixture("ib");
    if (layout === "missing") {
      f.proto.request = async function (request) {
        return this.ib(request);
      };
    } else if (layout === "ambiguous") {
      f.proto.request = async function (request) {
        const first = { payload: await this.ib(request) };
        return { first, payload: await this.ib(request) };
      };
    } else {
      f.proto.ib = async function (request) {
        return { service: request.service, data: request.data };
      };
    }
    const original = f.proto.ib,
      originalStream = f.proto.requestForStream;
    const bridge = createBridge(f.service, async () => ({}));
    await assert.rejects(bridge.enable(), /unsupported_client_version/);
    assert.equal(f.proto.ib, original);
    assert.equal(f.proto.requestForStream, originalStream);
    assert.equal(bridge.enabled, false);
  }
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

test("withdrawal refreshes a stale model cache before removing owned models", async () => {
  const f = fixture();
  let cached = [];
  f.service.getModelList = async () => ({ model_list: cached });
  const bridge = createBridge(
    f.service,
    async () => ({}),
    async () => {
      cached = f.models.map((model) => ({ ...model }));
    },
  );
  await bridge.apply("p1", "Fixture", {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: [{ model: "shared" }],
  });
  f.models.push({
    custom_model_id: "manual",
    display_name: "Manual",
    model_name: "other",
  });
  cached = [];
  await bridge.remove("p1");
  assert.deepEqual(
    f.models.map((model) => model.custom_model_id),
    ["manual"],
  );
});
test("native model cache is refreshed before matching and after writes across official switches", async () => {
  const f = fixture();
  f.models.push({ custom_model_id: "user", display_name: "User model" });
  let cached = f.models.map((model) => ({ ...model }));
  f.service.getModelList = async () => ({ model_list: cached });
  const refresh = async () => {
    cached = f.models.map((model) => ({ ...model }));
  };
  const add = f.service.addCustomModel;
  let additions = 0;
  f.service.addCustomModel = async (fields) => {
    additions++;
    if (f.models.some((model) => model.model_name === fields.model_name))
      return { code: 4024 };
    return add(fields);
  };
  const config = {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: Array.from({ length: 7 }, (_, index) => ({
      model: `model-${index}`,
    })),
  };
  // A previous interrupted write exists in native storage but not in its cached list.
  f.models.push({
    custom_model_id: "legacy",
    model_name: "model-0",
    display_name: "VaryAI/p1/model-0",
  });
  const b = createBridge(f.service, async () => ({}), refresh);
  await b.apply("p1", "Example", config);
  assert.equal(additions, 6);
  assert.equal(f.models.length, 8);
  assert.ok(
    f.models
      .slice(1)
      .every((model) => model.display_name.endsWith(" · Example")),
  );
  await b.apply("p1", "Renamed", config);
  assert.equal(additions, 6);
  await b.remove("p1");
  assert.deepEqual(
    f.models.map((model) => model.custom_model_id),
    ["user"],
  );
  await createBridge(f.service, async () => ({}), refresh).apply(
    "p1",
    "Example",
    config,
  );
  assert.equal(additions, 13);
  assert.equal(f.models.length, 8);
  assert.equal(new Set(f.models.map((model) => model.custom_model_id)).size, 8);
});
test("shared catalog names keep one VaryAI suffix within TRAE's display limit", async () => {
  const f = fixture();
  const bridge = createBridge(f.service, async () => ({}));
  const config = {
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-local",
    protocol: "openai",
    models: [
      {
        model: "vary-provider/model-first",
        displayName: "Model · Supplier · VaryAI",
      },
      {
        model: "vary-provider/model-second",
        displayName: `${"Long".repeat(30)} · Supplier · VaryAI`,
      },
    ],
  };
  await bridge.apply("shared_catalog_connection", "VaryAI", config);
  assert.equal(f.models[0].display_name, config.models[0].displayName);
  assert.ok(f.models.every((model) => model.display_name.length <= 64));
  assert.ok(
    f.models.every((model) => model.display_name.endsWith(" · VaryAI")),
  );
  assert.deepEqual(
    f.models.map((model) => model.model_name),
    config.models.map((model) => model.model),
  );
});
test("shared catalog refreshes expose final labels throughout replacement", async () => {
  const f = fixture();
  const snapshots = [];
  const record = () =>
    snapshots.push(f.models.map((model) => model.display_name));
  const add = f.service.addCustomModel;
  f.service.addCustomModel = async (fields) => {
    const result = await add(fields);
    record();
    return result;
  };
  const bridge = createBridge(f.service, async () => ({}), record);
  const config = {
    baseUrl: "http://127.0.0.1:1/connections/trae/v1",
    apiKey: "synthetic-local",
    protocol: "openai",
    models: [
      { model: "auto/shared-first", displayName: "First · VaryAI" },
      { model: "auto/shared-second", displayName: "Second · VaryAI" },
    ],
  };
  const id = "shared_catalog_connection";
  await bridge.apply(id, "VaryAI", config);
  await bridge.replace(id, "VaryAI", config);
  f.service.addCustomModel = async (fields) =>
    fields.ak === "synthetic-rejected" ? { code: 429 } : add(fields);
  await assert.rejects(
    bridge.replace(id, "VaryAI", { ...config, apiKey: "synthetic-rejected" }),
    /model_add_failed:429/,
  );
  assert.ok(snapshots.length > 2);
  for (const snapshot of snapshots)
    assert.ok(
      snapshot.every((name) =>
        config.models.some((m) => m.displayName === name),
      ),
    );
  assert.equal(await bridge.matches(id, config), true);
  assert.deepEqual(
    f.models.map((m) => m.model_name),
    config.models.map((m) => m.model),
  );
});
test("shared catalog persistence failures remove only the newly added native row", async () => {
  const f = fixture();
  const foreign = {
    custom_model_id: "manual",
    display_name: "Shared · VaryAI",
  };
  f.models.push(foreign);
  const bridge = createBridge(f.service, async () => ({}));
  stores.get(f.service).setItem = () => {
    throw Error("storage unavailable");
  };
  await assert.rejects(
    bridge.apply("shared_catalog_connection", "VaryAI", {
      baseUrl: "http://127.0.0.1:1/connections/trae/v1",
      apiKey: "synthetic-local",
      protocol: "openai",
      models: [{ model: "auto/shared", displayName: "Shared · VaryAI" }],
    }),
  );
  assert.deepEqual(f.models, [foreign]);
});
test("long model IDs fit TRAE's display name limit without changing request IDs", async () => {
  const f = fixture();
  const add = f.service.addCustomModel;
  f.service.addCustomModel = async (fields) => {
    if (fields.display_name.length > 64) return { code: -1 };
    const result = await add(fields);
    f.models.at(-1).custom_model_id = `model-${f.models.length}`;
    return result;
  };
  const id = "00000000-0000-4000-8000-000000000001";
  const names = [
    "example-org/a-very-long-model-name-first",
    "example-org/a-very-long-model-name-second",
  ];
  const b = createBridge(f.service, async () => ({}));
  const config = {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: names.map((model) => ({ model })),
  };
  await b.apply(id, "Example", config);
  assert.deepEqual(
    f.models.map((model) => model.model_name),
    names,
  );
  assert.equal(new Set(f.models.map((model) => model.display_name)).size, 2);
  assert.ok(f.models.every((model) => model.display_name.length <= 64));
  const displays = f.models.map((model) => model.display_name);
  await b.apply(id, "Example", config);
  assert.deepEqual(
    f.models.map((model) => model.display_name),
    displays,
  );
  await b.remove(id);
  assert.equal(f.models.length, 0);
});
test("readable labels migrate existing owned models and survive provider renames and bridge reloads", async () => {
  const f = fixture();
  const id = "00000000-0000-4000-8000-000000000001";
  const model = "example-org/model-a";
  const longModel = "example-org/a-model-name-that-exceeded-the-old-limit";
  const digest = require("node:crypto")
    .createHash("sha256")
    .update(longModel)
    .digest("hex")
    .slice(0, 20);
  f.models.push(
    { custom_model_id: "legacy", display_name: `VaryAI/${id}/${model}` },
    { custom_model_id: "hashed", display_name: `VaryAI/${id}/${digest}` },
    { custom_model_id: "user", display_name: "User model", ak: "private" },
  );
  const config = {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: [
      { model },
      { model: "model-b", displayName: "Model B" },
      { model: longModel },
    ],
  };
  await createBridge(f.service, async () => ({})).apply(id, "Example", config);
  assert.equal(f.models.length, 4);
  assert.equal(
    f.models.find((m) => m.custom_model_id === "legacy").display_name,
    "model-a · Example",
  );
  assert.ok(f.models.some((m) => m.display_name === "Model B · Example"));
  const ownership = JSON.parse(
    stores.get(f.service).getItem(`varyai.trae.models.${id}`),
  );
  assert.ok(
    ownership.every((row) => Object.keys(row).sort().join(",") === "id,model"),
  );
  assert.equal(
    f.models.find((m) => m.custom_model_id === "hashed").display_name,
    "a-model-name-that-exceeded-the-old-limit · Example",
  );
  await createBridge(f.service, async () => ({})).apply(id, "Renamed", config);
  assert.equal(f.models.length, 4);
  assert.ok(f.models.some((m) => m.display_name === "model-a · Renamed"));
  await createBridge(f.service, async () => ({})).remove(id);
  assert.deepEqual(
    f.models.map((m) => m.custom_model_id),
    ["user"],
  );
});
test("readable labels retain distinct names within the native limit and preserve user entries", async () => {
  const f = fixture();
  const name = "Example";
  f.models.push({ custom_model_id: "user", display_name: "Same · Example" });
  const config = {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: [
      { model: "org-a/model", displayName: "Same" },
      { model: "org-b/model", displayName: "Same" },
      { model: "long-model", displayName: "Long".repeat(30) },
    ],
  };
  await createBridge(f.service, async () => ({})).apply("p1", name, config);
  assert.equal(new Set(f.models.map((m) => m.display_name)).size, 4);
  assert.ok(f.models.every((m) => m.display_name.length <= 64));
  await createBridge(f.service, async () => ({})).apply("p1", name, config);
  assert.equal(f.models.length, 4);
  await createBridge(f.service, async () => ({})).remove("p1");
  assert.equal(f.models[0].custom_model_id, "user");
  assert.equal(f.models.length, 1);
});
test("built-in models without custom IDs are never updated or removed", async () => {
  const f = fixture();
  const builtIn = { name: "built-in", display_name: "Built-in" };
  f.models.push({ ...builtIn });
  const bridge = createBridge(f.service, async () => ({}));
  await bridge.apply("p1", "Example", {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: [{ model: "model" }],
  });
  assert.deepEqual(f.models[0], builtIn);
  assert.equal(f.models[1].display_name, "model · Example");
  await bridge.remove("p1");
  assert.deepEqual(f.models, [builtIn]);
});
test("failed label migration retains ownership and failed persistence rolls back new entries", async () => {
  const f = fixture();
  f.models.push({ custom_model_id: "legacy", display_name: "VaryAI/p1/model" });
  const update = f.service.updateCustomModel;
  f.service.updateCustomModel = async (fields) =>
    fields.action === "update" ? { code: -1 } : update(fields);
  const config = {
    baseUrl: "https://example.invalid/v1",
    apiKey: "synthetic",
    protocol: "openai",
    models: [{ model: "model" }],
  };
  await assert.rejects(
    createBridge(f.service, async () => ({})).apply("p1", "Example", config),
  );
  f.service.updateCustomModel = update;
  await createBridge(f.service, async () => ({})).remove("p1");
  assert.equal(f.models.length, 0);
  stores.get(f.service).setItem = () => {
    throw Error("storage unavailable");
  };
  await assert.rejects(
    createBridge(f.service, async () => ({})).apply("p1", "Example", config),
  );
  assert.equal(f.models.length, 0);
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
  assert.equal(refreshed, 3);
  await b.remove("test");
  assert.equal(refreshed, 5);
});

test("replacement removes models from lost provider IDs while preserving native models", async () => {
  const f = fixture();
  f.models.push({
    custom_model_id: "manual",
    name: "native",
    display_name: "Native",
    provider: "custom_openai_compatible",
    base_url: "https://native.example/v1",
  });
  const b = createBridge(f.service, async () => ({}));
  const config = {
    protocol: "openai",
    baseUrl: "https://example.test/v1",
    apiKey: "synthetic-old",
    models: [{ model: "shared" }, { model: "obsolete" }],
  };
  await b.apply("lost", "Old", config);
  const add = f.service.addCustomModel;
  f.service.addCustomModel = async (row) =>
    f.models.some(
      (existing) =>
        existing.model_name === row.model_name &&
        existing.base_url === row.base_url,
    )
      ? { code: 409 }
      : add(row);
  await b.replace("new", "New", {
    ...config,
    apiKey: "synthetic-new",
    models: [{ model: "shared" }],
  });
  assert.equal(f.models.length, 2);
  assert.equal(
    f.models.find((m) => m.model_name === "shared").ak,
    "synthetic-new",
  );
  assert.equal(f.models[0].custom_model_id, "manual");
  await b.replace("official", "Official", null);
  assert.deepEqual(
    f.models.map((m) => m.custom_model_id),
    ["manual"],
  );
});

test("failed replacement restores orphaned native models without persisting credentials", async () => {
  const f = fixture();
  const b = createBridge(f.service, async () => ({}));
  const config = {
    protocol: "openai",
    baseUrl: "https://example.test/v1",
    apiKey: "synthetic-old",
    models: [{ model: "shared" }, { model: "obsolete" }],
  };
  await b.apply("lost", "Old", config);
  const add = f.service.addCustomModel;
  f.service.addCustomModel = async (row) =>
    row.ak === "synthetic-new" ? { code: 429 } : add(row);
  await assert.rejects(
    b.replace("new", "New", { ...config, apiKey: "synthetic-new" }),
    /model_add_failed:429/,
  );
  assert.deepEqual(f.models.map((m) => m.model_name).sort(), [
    "obsolete",
    "shared",
  ]);
  assert.ok(
    f.models.every(
      (m) => m.ak === "synthetic-old" && m.display_name.endsWith(" · Old"),
    ),
  );
  const storage = stores.get(f.service);
  for (let i = 0; i < storage.length; i++)
    assert.ok(!storage.getItem(storage.key(i)).includes("synthetic-old"));
  await b.replace("official", "Official", null);
  assert.equal(f.models.length, 0);
});

test("replacement refuses destructive changes when native recovery credentials are unavailable", async () => {
  const f = fixture();
  const b = createBridge(f.service, async () => ({}));
  const config = {
    protocol: "openai",
    baseUrl: "https://example.test/v1",
    apiKey: "synthetic",
    models: [{ model: "old" }],
  };
  await b.apply("lost", "Old", config);
  delete f.models[0].ak;
  const before = structuredClone(f.models);
  await assert.rejects(
    b.replace("new", "New", config),
    /model_recovery_unavailable/,
  );
  assert.deepEqual(f.models, before);
});
