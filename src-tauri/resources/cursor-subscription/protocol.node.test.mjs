import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import http2 from "node:http2";
import { _internal } from "./index.mjs";
import { NativeSessionPool } from "./native-session.mjs";
import {
  message,
  tool,
  update,
  uint,
  text,
  readResult,
  readWire,
} from "./native-test-wire.mjs";

// These protocol fixtures must never consult a real account or remote service.
globalThis.fetch = () => {
  throw new Error("offline protocol fixture");
};
http2.connect = () => {
  throw new Error("offline protocol fixture");
};
const pools = [];
afterEach(async () => {
  await Promise.all(pools.splice(0).map((pool) => pool.clear()));
});
const tools = [
  {
    name: "audit_echo",
    description: "synthetic",
    schema: { type: "object" },
    schemaText: '{"type":"object"}',
  },
];
function owner() {
  const pool = new NativeSessionPool();
  pools.push(pool);
  const chat = {
    model: "synthetic-model",
    messages: [{ role: "user", content: "synthetic" }],
  };
  const run = pool.create(
    pool.prepare("synthetic-token", chat, "synthetic-session", tools),
    { id: chat.model, maxMode: false, params: [] },
    _internal,
  );
  run.chatTools = tools;
  const writes = [];
  run.bind(
    { send: (b) => writes.push(Buffer.from(b)), close() {} },
    new Map(),
    11,
  );
  return { pool, run, chat, writes };
}
const frames = (wire) =>
  wire.map((packet) => ({ data: packet.subarray(5), end: !!(packet[0] & 2) }));
const until = async (predicate) => {
  const deadline = Date.now() + 2000;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error("synthetic protocol timeout");
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
};

test("native dynamic tools return to the caller and exact results resume the retained run", async () => {
  const { pool, run, chat, writes } = owner();
  let release;
  const held = new Promise((resolve) => {
    release = resolve;
  });
  const pumping = run.pump(
    (async function* () {
      yield* frames([tool(7, "synthetic-call"), update(27, uint(1, 1))]);
      await held;
    })(),
  );
  await until(() => run.state === "awaiting-results" || run.error);
  assert.equal(run.error, undefined);
  const first = run.response();
  const events = [];
  for await (const event of first.events) events.push(event);
  first.delivered();
  const call = events.find((event) => event.tool).tool;
  assert.equal(call.name, "audit_echo");
  assert.deepEqual(JSON.parse(call.args), { value: "seed" });
  assert.equal(events.at(-1).stop, "tool_calls");
  assert.equal(
    writes.filter((packet) => readWire(packet).some((f) => f.no === 2)).length,
    0,
  );
  const next = {
    ...chat,
    messages: [
      ...chat.messages,
      {
        role: "assistant",
        tool_calls: [
          {
            id: call.id,
            type: "function",
            function: { name: call.name, arguments: call.args },
          },
        ],
      },
      {
        role: "tool",
        tool_call_id: call.id,
        content: "synthetic caller result",
        is_error: true,
      },
    ],
  };
  const prepared = pool.prepare(
    "synthetic-token",
    next,
    "synthetic-session",
    tools,
  );
  assert.equal(prepared.owner, run);
  assert.equal(
    pool.prepare("another-account", next, "synthetic-session", tools).fallback,
    "owner incompatible or expired",
  );
  run.resume(prepared);
  const result = writes.find((packet) =>
    readWire(packet).some((f) => f.no === 2),
  );
  assert.deepEqual(readResult(result), {
    id: 7,
    execID: "exec-7",
    content: [{ type: "text", text: "synthetic caller result" }],
    isError: true,
  });
  assert.equal(
    pool.prepare("synthetic-token", next, "synthetic-session", tools).fallback,
    "result already submitted or busy",
  );
  await run.cancel("fixture complete");
  release();
  await pumping;
});

test("stateless tool history preserves call links and explicit errors", () => {
  const chat = {
    messages: [
      {
        role: "assistant",
        tool_calls: [
          {
            id: "call-fixture",
            type: "function",
            function: { name: "audit_echo", arguments: "{}" },
          },
        ],
      },
      {
        role: "tool",
        tool_call_id: "call-fixture",
        content: "synthetic result",
        is_error: true,
      },
    ],
  };
  const messages = _internal
    .conversation(chat, tools)
    .msgs.map((b) => JSON.parse(b.toString()));
  const result = messages
    .filter((m) => m.role === "tool")
    .flatMap((m) => m.content)[0];
  assert.equal(result.toolCallId, "call-fixture");
  assert.equal(result.isError, true);
});

test("native tool result encoding preserves image bytes and text without interpreting error-looking text", () => {
  const { run } = owner();
  const image = Buffer.from("89504e470d0a1a0a", "hex");
  const encoded = run.encodeResult({
    role: "tool",
    content: [
      { type: "text", text: '{"error":"ordinary output"}' },
      {
        type: "image_url",
        image_url: { url: `data:image/png;base64,${image.toString("base64")}` },
      },
    ],
  });
  const wire = _internal
    .pb()
    .bytes(2, _internal.pb().varint(1, 3).bytes(11, encoded))
    .done();
  const result = readResult(wire);
  assert.equal(result.isError, false);
  assert.deepEqual(result.content[1].data, image);
});

test("changed duplicate exec identities fail before exposing a tool call", async () => {
  const { run } = owner();
  await run.pump(
    frames([
      tool(7, "synthetic-call", "first"),
      tool(7, "synthetic-call", "changed"),
    ]),
  );
  assert.equal(run.error.status, 502);
  assert.equal(
    run.segment.events.some((e) => e.tool),
    false,
  );
});

test("server shell tools are refused locally and never executed", () => {
  const writes = [];
  const body = message(
    uint(1, 7),
    text(15, "synthetic-exec"),
    _internal.pb().bytes(3, _internal.pb().str(1, "synthetic command")).done(),
  );
  const result = _internal.exec(
    body,
    tools,
    (packet) => writes.push(packet),
    () => {},
    null,
    true,
  );
  assert.equal(result, null);
  assert.ok(writes[0].includes(Buffer.from("not available")));
});

test("the vendored protocol and independent wire fixture match the pinned upstream revision", async () => {
  const { readFile } = await import("node:fs/promises");
  const { createHash } = await import("node:crypto");
  for (const [name, digest] of [
    [
      "index.mjs",
      "cc9627245c34d2a8e18cd1015424dc820c754a519a37a56beca5650589481aa0",
    ],
    [
      "native-session.mjs",
      "c7cb24c612b8cc3f5b94843b9b1b6783a9bffb1f04597920451d74b04232bedc",
    ],
    [
      "native-test-wire.mjs",
      "28e3297fb24747150a49935055ef3e1b5ec8a0e39c1fa60e1970840dc1dccdb5",
    ],
  ])
    assert.equal(
      createHash("sha256")
        .update(await readFile(new URL(name, import.meta.url)))
        .digest("hex"),
      digest,
    );
});
