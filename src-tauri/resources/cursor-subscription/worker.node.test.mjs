import assert from "node:assert/strict";
import { once } from "node:events";
import { afterEach, test } from "node:test";
import { createCursorWorker } from "./worker.mjs";

const workers = [];
afterEach(async () => {
  await Promise.all(workers.splice(0).map((worker) => worker.close()));
});
const secret = "synthetic-private-worker-key";
const auth = {
  type: "oauth",
  access: "synthetic-account-token",
  refresh: "none",
  expires: Date.now() + 60_000,
};
async function fixture(overrides = {}) {
  const calls = [];
  const hooks = {
    auth: {
      methods: [
        {
          authorize: async () => ({
            url: "https://cursor.com/loginDeepControl?challenge=synthetic",
            callback: async () => ({
              ...auth,
              type: "success",
              accountId: "fixture@example.invalid",
            }),
          }),
        },
      ],
      usage: async () => ({
        windows: [
          { name: "Weekly", used: 25, amount: 5, limit: 20, unit: "USD" },
        ],
      }),
      loader: async (getAuth) => ({
        fetch: async (_url, init) => {
          calls.push({
            auth: await getAuth(),
            body: JSON.parse(init.body),
            session: new Headers(init.headers).get("x-magpie-cursor-session"),
          });
          return new Response(
            'data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-fixture","function":{"name":"echo","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":3}}\n\ndata: [DONE]\n\n',
            { headers: { "Content-Type": "text/event-stream" } },
          );
        },
      }),
    },
    provider: {
      models: async () => ({
        exact: {
          id: "exact@200k",
          name: "Exact model",
          limit: { context: 200000, output: 4096 },
          capabilities: { attachment: true, reasoning: true, toolcall: true },
        },
      }),
    },
    ...overrides,
  };
  const worker = await createCursorWorker(hooks, secret, {
    identity: async (value) => ({
      ...value,
      accountId: "fixture@example.invalid",
    }),
  });
  workers.push(worker);
  const post = (path, body = {}, key = secret, signal) =>
    fetch(`http://127.0.0.1:${worker.port}${path}`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${key}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
      signal,
    });
  return { worker, post, calls, hooks };
}

test("worker requires its private bearer before parsing or dispatching requests", async () => {
  const f = await fixture();
  const response = await f.post(
    "/chat",
    { auth, chat: { messages: [] } },
    "wrong-key",
  );
  assert.equal(response.status, 401);
  assert.equal(f.calls.length, 0);
  assert.equal((await f.post("/unknown")).status, 404);
});

test("model catalog preserves exact IDs, context limits and capabilities", async () => {
  const f = await fixture();
  const response = await f.post("/models", { auth });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), [
    {
      model: "exact@200k",
      displayName: "Exact model",
      contextWindow: 200000,
      maxOutputTokens: 4096,
      images: true,
      reasoning: true,
      tools: true,
      reasoningLevels: [],
    },
  ]);
});

test("tool calls, reasoning parameters and usage survive the local worker stream", async () => {
  const f = await fixture();
  const chat = {
    model: "exact@200k",
    messages: [{ role: "user", content: "synthetic" }],
    tools: [
      {
        type: "function",
        function: { name: "echo", parameters: { type: "object" } },
      },
    ],
    reasoning_effort: "high",
    temperature: 0.3,
  };
  const response = await f.post("/chat", {
    auth,
    chat,
    session: "synthetic-session",
  });
  const text = await response.text();
  assert.match(text, /call-fixture/);
  assert.match(text, /"completion_tokens":3/);
  assert.match(text, /\[DONE\]/);
  assert.deepEqual(f.calls[0].auth, auth);
  assert.equal(f.calls[0].session, "synthetic-session");
  assert.deepEqual(f.calls[0].body, {
    ...chat,
    stream: true,
    stream_options: { include_usage: true },
  });
});

test("quota errors remain failures rather than invented zero usage", async () => {
  const f = await fixture();
  const quota = await f.post("/quota", { auth });
  assert.equal((await quota.json()).windows[0].used, 25);
  f.hooks.auth.usage = async () => ({
    error: "synthetic-private-upstream-error",
    windows: [],
  });
  const response = await f.post("/quota", { auth });
  assert.equal(response.status, 502);
  assert.doesNotMatch(await response.text(), /synthetic-private/);
});

test("browser login finishes on the backend and cancellation discards its result", async () => {
  const f = await fixture();
  const started = await (await f.post("/login/start")).json();
  assert.match(started.url, /^https:\/\/cursor.com\/loginDeepControl/);
  let finished;
  for (let i = 0; i < 10; i++) {
    finished = await (await f.post("/login/poll", { id: started.id })).json();
    if (finished.state === "done") break;
  }
  assert.equal(finished.auth.accountId, "fixture@example.invalid");
  assert.equal(finished.auth.access, auth.access);
  await f.post("/login/cancel", { id: started.id });
  assert.equal((await f.post("/login/poll", { id: started.id })).status, 404);
});

test("a disconnected inference request cancels upstream work", async () => {
  const cancelled = new AbortController();
  const f = await fixture({
    auth: {
      loader: async () => ({
        fetch: async (_url, init) => {
          init.signal.addEventListener("abort", () => cancelled.abort(), {
            once: true,
          });
          return new Response(
            new ReadableStream({
              start(controller) {
                controller.enqueue(new TextEncoder().encode("data: {}\n\n"));
              },
            }),
          );
        },
      }),
    },
  });
  const controller = new AbortController();
  const response = await f.post(
    "/chat",
    { auth, chat: { messages: [] } },
    secret,
    controller.signal,
  );
  await response.body.getReader().read();
  controller.abort();
  if (!cancelled.signal.aborted) await once(cancelled.signal, "abort");
  assert.equal(cancelled.signal.aborted, true);
});

test("the packaged helper starts with stdin credentials and exits when its parent closes stdin", async () => {
  const { spawn } = await import("node:child_process");
  const { createInterface } = await import("node:readline");
  const { fileURLToPath } = await import("node:url");
  const { mkdtemp, copyFile, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const directory = await mkdtemp(join(tmpdir(), "vary cursor fixture "));
  for (const name of ["worker.mjs", "index.mjs", "native-session.mjs"]) {
    await copyFile(
      fileURLToPath(new URL(name, import.meta.url)),
      join(directory, name),
    );
  }
  const child = spawn(process.execPath, [join(directory, "worker.mjs")], {
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, NODE_OPTIONS: "", XDG_DATA_HOME: directory },
  });
  const exited = once(child, "exit");
  try {
    const lines = createInterface({ input: child.stdout });
    const ready = once(lines, "line");
    child.stdin.write(JSON.stringify({ secret }) + "\n");
    const [line] = await ready;
    const { port, version } = JSON.parse(line);
    assert.equal(version, "0.2.44");
    const response = await fetch(`http://127.0.0.1:${port}/unknown`, {
      method: "POST",
      headers: { Authorization: `Bearer ${secret}` },
      body: "{}",
    });
    assert.equal(response.status, 404);
    child.stdin.end();
    const [code] = await exited;
    assert.equal(code, 0);
    lines.close();
  } finally {
    child.kill();
    await rm(directory, { recursive: true, force: true });
  }
});

test("API-key imports verify the exchanged token against Cursor identity and sanitize rejection", async () => {
  const f = await fixture();
  const worker = await createCursorWorker(f.hooks, secret);
  workers.push(worker);
  const original = globalThis.fetch;
  const seen = [];
  let rejected = false;
  globalThis.fetch = async (input, init) => {
    const url = new URL(input);
    if (url.hostname === "127.0.0.1") return original(input, init);
    assert.equal(url.hostname, "api2.cursor.sh");
    seen.push([url.pathname, new Headers(init.headers).get("authorization")]);
    if (url.pathname === "/auth/exchange_user_api_key")
      return Response.json({ accessToken: "synthetic-exchanged-token" });
    if (url.pathname.endsWith("/GetMe"))
      return rejected
        ? Response.json({ error: "synthetic-private-error" }, { status: 401 })
        : Response.json({ email: "verified@example.invalid" });
    if (url.pathname.endsWith("/GetPlanInfo"))
      return Response.json({ planInfo: { planName: "pro" } });
    throw new Error("unexpected remote request in fixture");
  };
  try {
    const post = () =>
      fetch(`http://127.0.0.1:${worker.port}/identity`, {
        method: "POST",
        headers: { Authorization: `Bearer ${secret}` },
        body: JSON.stringify({
          auth: { type: "api", key: "synthetic-api-key" },
        }),
      });
    const account = await (await post()).json();
    assert.equal(account.accountId, "verified@example.invalid");
    assert.equal(account.plan, "pro");
    assert.equal(seen[0][1], "Bearer synthetic-api-key");
    assert.equal(seen[1][1], "Bearer synthetic-exchanged-token");
    rejected = true;
    const response = await post();
    assert.equal(response.status, 401);
    assert.doesNotMatch(await response.text(), /synthetic-private/);
  } finally {
    globalThis.fetch = original;
  }
});

test("cancelling a pending browser authorization prevents a late credential result", async () => {
  const f = await fixture();
  let finish;
  f.hooks.auth.methods[0].authorize = async () => ({
    url: "https://cursor.com/loginDeepControl?challenge=synthetic",
    callback: () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  });
  const flow = await (await f.post("/login/start")).json();
  await f.post("/login/cancel", { id: flow.id });
  finish({ ...auth, type: "success", accountId: "cancelled@example.invalid" });
  await Promise.resolve();
  assert.equal((await f.post("/login/poll", { id: flow.id })).status, 404);
});

test("mutable CLI markers and control characters cannot reach inference", async () => {
  const f = await fixture();
  for (const bad of [
    { ...auth, refresh: "cursor-agent" },
    { ...auth, access: "synthetic\ninvalid" },
  ]) {
    assert.equal((await f.post("/chat", { auth: bad, chat: {} })).status, 401);
  }
  assert.equal(f.calls.length, 0);
});

for (const status of [402, 429, 504])
  test(`inference preserves upstream HTTP ${status} without exposing its error body`, async () => {
    const f = await fixture({
      auth: {
        loader: async () => ({
          fetch: async () =>
            new Response("synthetic-private-upstream-body", { status }),
        }),
      },
    });
    const response = await f.post("/chat", { auth, chat: {} });
    assert.equal(response.status, status);
    assert.doesNotMatch(await response.text(), /synthetic-private/);
  });
