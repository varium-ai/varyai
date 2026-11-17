const test = require("node:test");
const assert = require("node:assert/strict");

async function fixture() {
  const { default: register } = await import("./pi-session-id.mjs");
  const handlers = new Map();
  register({ on: (name, handler) => handlers.set(name, handler) });
  const context = {
    model: { baseUrl: "http://127.0.0.1:19876/connections/pi/v1" },
    sessionManager: { getSessionId: () => "synthetic-session-one" },
  };
  return { invoke: handlers.get("before_provider_headers"), context };
}

test("native Pi session identity follows session switches for both routing paths", async () => {
  const { invoke, context } = await fixture();
  assert.equal(typeof invoke, "function");
  for (const baseUrl of [
    "http://127.0.0.1:19876/connections/pi/v1",
    "http://localhost:19876/pi/p/synthetic/v1beta",
    "http://[::1]:19876/pi/v1",
  ]) {
    context.model.baseUrl = baseUrl;
    const event = { headers: { "synthetic-user-header": "preserved" } };
    invoke(event, context);
    assert.equal(
      event.headers["x-vary-pi-session-id"],
      "synthetic-session-one",
    );
    assert.equal(event.headers["synthetic-user-header"], "preserved");
  }
  context.sessionManager.getSessionId = () => "synthetic-session-two";
  const event = { headers: {} };
  invoke(event, context);
  assert.equal(event.headers["x-vary-pi-session-id"], "synthetic-session-two");
  context.model.baseUrl = "https://example.test/v1";
  invoke(event, context);
  assert.equal(event.headers["x-vary-pi-session-id"], undefined);
});

test("external providers, unrelated local endpoints and invalid identities stay untouched", async () => {
  const { invoke, context } = await fixture();
  assert.equal(typeof invoke, "function");
  for (const baseUrl of [
    "https://example.test/pi/v1",
    "http://127.0.0.1:19876/v1",
    "http://127.0.0.1:19876/connections/other/v1",
    "invalid",
    undefined,
  ]) {
    context.model.baseUrl = baseUrl;
    const event = { headers: { existing: "preserved" } };
    invoke(event, context);
    assert.deepEqual(event.headers, { existing: "preserved" });
  }
  context.model.baseUrl = "http://127.0.0.1:19876/pi/v1";
  for (const id of [
    undefined,
    "",
    "has space",
    "has\ncontrol",
    "x".repeat(257),
  ]) {
    context.sessionManager.getSessionId = () => id;
    const event = { headers: {} };
    invoke(event, context);
    assert.deepEqual(event.headers, {});
  }
  context.sessionManager.getSessionId = () => {
    throw Error("synthetic failure");
  };
  assert.doesNotThrow(() => invoke({ headers: {} }, context));
});
