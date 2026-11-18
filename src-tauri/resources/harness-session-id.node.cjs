const test = require("node:test");
const assert = require("node:assert/strict");

async function fixture() {
  const { apply } = await import("./harness-session-id.mjs");
  let callback;
  apply({
    on(event, handler) {
      assert.equal(event, "llm-pi-ai/request-headers");
      callback = handler;
    },
  });
  return callback;
}

test("native session identity changes with sessions across the three local protocol routes", async () => {
  const callback = await fixture();
  for (const baseURL of [
    "http://127.0.0.1:19876/connections/deepseek-harness",
    "http://localhost:19876/connections/deepseek-harness/v1",
    "http://[::1]:19876/connections/deepseek-harness/v1/",
  ]) {
    for (const sessionId of [
      "synthetic-session-one",
      "synthetic-session-two",
    ]) {
      const request = Object.freeze({ baseURL, sessionId });
      const headers = new Headers({ existing: "preserved" });
      let continued = 0;
      assert.equal(
        callback(request, headers, () => {
          continued++;
          return headers;
        }),
        headers,
      );
      assert.equal(continued, 1);
      assert.equal(headers.get("x-vary-harness-session-id"), sessionId);
      assert.equal(headers.get("existing"), "preserved");
    }
  }
});

test("foreign routes, invalid IDs and missing identity stay untouched", async () => {
  const callback = await fixture();
  for (const baseURL of [
    "https://example.test/connections/deepseek-harness/v1",
    "http://127.0.0.1:19876/connections/pi/v1",
    "http://127.0.0.1:19876/v1",
    "http://user:pass@127.0.0.1:19876/connections/deepseek-harness/v1",
    "invalid",
  ]) {
    const headers = new Headers({ existing: "preserved" });
    callback(
      { baseURL, sessionId: "synthetic-session" },
      headers,
      () => headers,
    );
    assert.equal(headers.get("x-vary-harness-session-id"), null);
  }
  for (const sessionId of [
    undefined,
    "",
    "has space",
    "has\ncontrol",
    "非ASCII",
    "x".repeat(201),
  ]) {
    const headers = new Headers();
    callback(
      {
        baseURL: "http://127.0.0.1:19876/connections/deepseek-harness/v1",
        sessionId,
      },
      headers,
      () => headers,
    );
    assert.equal(headers.get("x-vary-harness-session-id"), null);
  }
});

test("other header contributors keep their output and intentional errors", async () => {
  const callback = await fixture();
  const request = {
    baseURL: "http://127.0.0.1:19876/connections/deepseek-harness/v1",
    sessionId: "synthetic-session",
  };
  const prepared = new Headers({ contributed: "preserved" });
  assert.equal(
    callback(request, new Headers(), () => prepared),
    prepared,
  );
  assert.equal(prepared.get("contributed"), "preserved");
  const error = Error("synthetic upstream contributor error");
  assert.throws(
    () =>
      callback(request, prepared, () => {
        throw error;
      }),
    (candidate) => candidate === error,
  );
});
