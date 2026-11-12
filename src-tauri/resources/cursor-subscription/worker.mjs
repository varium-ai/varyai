import http from "node:http";
import http2 from "node:http2";
import { createHash } from "node:crypto";
import { randomUUID, timingSafeEqual } from "node:crypto";
import { AsyncLocalStorage } from "node:async_hooks";
import { once } from "node:events";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { realpathSync } from "node:fs";

const MAX_BODY = 16 * 1024 * 1024;
const contexts = new AsyncLocalStorage();

function failure(status = 502) {
  return Object.assign(new Error("Cursor subscription unavailable"), {
    status,
  });
}
function json(response, status, value) {
  response.writeHead(status, { "Content-Type": "application/json" });
  response.end(JSON.stringify(value));
}
async function body(request) {
  let size = 0;
  const parts = [];
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_BODY) throw failure(413);
    parts.push(chunk);
  }
  try {
    const value = JSON.parse(Buffer.concat(parts).toString("utf8"));
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw failure(400);
    return value;
  } catch {
    throw failure(400);
  }
}
function checkedAuth(auth) {
  if (!auth || (auth.type !== "oauth" && auth.type !== "api"))
    throw failure(401);
  const token = auth.type === "api" ? auth.key : auth.access;
  if (
    typeof token !== "string" ||
    !token ||
    token.length > 32768 ||
    /[\x00-\x1f\x7f]/.test(token) ||
    auth.refresh === "cursor-agent"
  )
    throw failure(401);
  return auth;
}

async function identity(auth) {
  checkedAuth(auth);
  let token = auth.access;
  const headers = {
    "Content-Type": "application/json",
    "Connect-Protocol-Version": "1",
    "x-cursor-client-type": "cli",
    "x-cursor-client-version": "2026.09.23-86fc751",
    "x-ghost-mode": "true",
  };
  if (auth.type === "api") {
    const response = await fetch(
      "https://api2.cursor.sh/auth/exchange_user_api_key",
      {
        method: "POST",
        headers: { ...headers, Authorization: `Bearer ${auth.key}` },
        body: "{}",
      },
    );
    if (!response.ok) throw failure(response.status);
    token = (await response.json()).accessToken;
    if (typeof token !== "string" || !token) throw failure(401);
  }
  const call = async (method) => {
    const response = await fetch(
      `https://api2.cursor.sh/aiserver.v1.DashboardService/${method}`,
      {
        method: "POST",
        headers: { ...headers, Authorization: `Bearer ${token}` },
        body: "{}",
      },
    );
    if (!response.ok) throw failure(response.status);
    return response.json();
  };
  const me = await call("GetMe");
  const plan = await call("GetPlanInfo").catch(() => ({}));
  if (typeof me.email !== "string" || !me.email.trim()) throw failure(401);
  return {
    ...auth,
    accountId: me.email.trim(),
    plan: plan.planInfo?.planName ?? null,
  };
}

export async function createCursorWorker(hooks, secret, options = {}) {
  if (typeof secret !== "string" || secret.length < 24) throw failure();
  const key = Buffer.from(`Bearer ${secret}`);
  const logins = new Map();
  const active = new Set();
  const inspectIdentity = options.identity ?? identity;
  const server = http.createServer((request, response) => {
    const supplied = Buffer.from(request.headers.authorization ?? "");
    if (supplied.length !== key.length || !timingSafeEqual(supplied, key))
      return json(response, 401, { error: "unauthorized" });
    if (request.method !== "POST")
      return json(response, 405, { error: "method" });
    if (active.size >= 32) return json(response, 429, { error: "busy" });
    const controller = new AbortController();
    active.add(controller);
    const timer = setTimeout(
      () => controller.abort(),
      request.url === "/chat" ? 900_000 : 90_000,
    );
    timer.unref();
    request.once("aborted", () => controller.abort());
    response.once("close", () => {
      if (!response.writableFinished) controller.abort();
    });
    contexts.run(controller.signal, async () => {
      try {
        const value = await body(request);
        if (request.url === "/login/start") {
          if (logins.size >= 8) throw failure(429);
          const login = await hooks.auth.methods[0].authorize();
          const url = new URL(login.url);
          if (
            url.protocol !== "https:" ||
            url.hostname !== "cursor.com" ||
            url.pathname !== "/loginDeepControl"
          )
            throw failure();
          const id = randomUUID();
          const pending = {
            state: "waiting",
            controller: new AbortController(),
          };
          logins.set(id, pending);
          pending.timer = setTimeout(() => {
            pending.controller.abort();
            logins.delete(id);
          }, 300_000);
          pending.timer.unref();
          contexts.run(pending.controller.signal, async () => {
            try {
              const result = await login.callback();
              if (result.type !== "success" || !result.accountId)
                throw failure(401);
              if (logins.get(id) === pending)
                Object.assign(pending, {
                  state: "done",
                  auth: checkedAuth({ ...result, type: "oauth" }),
                });
            } catch {
              if (logins.get(id) === pending) pending.state = "failed";
            }
          });
          return json(response, 200, { id, url: login.url });
        }
        if (request.url === "/login/poll" || request.url === "/login/cancel") {
          const pending = logins.get(value.id);
          if (!pending) throw failure(404);
          if (request.url === "/login/cancel") {
            pending.controller.abort();
            clearTimeout(pending.timer);
            logins.delete(value.id);
            return json(response, 200, {});
          }
          return json(response, 200, {
            state: pending.state,
            ...(pending.state === "done" ? { auth: pending.auth } : {}),
          });
        }
        if (
          !["/identity", "/models", "/quota", "/chat", "/forget"].includes(
            request.url,
          )
        )
          throw failure(404);
        const auth = checkedAuth(value.auth);
        if (request.url === "/forget") {
          await options.forget?.(auth);
          return json(response, 200, {});
        }
        if (request.url === "/identity")
          return json(response, 200, await inspectIdentity(auth));
        if (request.url === "/models") {
          const models = await hooks.provider.models({}, { auth });
          const rows = Object.values(models).map((model) => ({
            model: model.id,
            displayName: model.name,
            contextWindow: model.limit?.context ?? null,
            maxOutputTokens: model.limit?.output || null,
            images: model.capabilities?.attachment === true,
            reasoning: model.capabilities?.reasoning === true,
            tools: model.capabilities?.toolcall !== false,
            reasoningLevels: Object.keys(model.variants ?? {}),
          }));
          return json(response, 200, rows);
        }
        if (request.url === "/quota") {
          const quota = await hooks.auth.usage(async () => auth, {});
          if (quota.error || !Array.isArray(quota.windows)) {
            await inspectIdentity(auth);
            throw failure();
          }
          return json(response, 200, quota);
        }
        const loader = await hooks.auth.loader(async () => auth);
        const upstream = await loader.fetch(
          "http://varyai.invalid/chat/completions",
          {
            method: "POST",
            signal: controller.signal,
            headers: {
              "Content-Type": "application/json",
              ...(value.session
                ? { "x-magpie-cursor-session": value.session }
                : {}),
            },
            body: JSON.stringify({
              ...value.chat,
              stream: true,
              stream_options: {
                ...value.chat?.stream_options,
                include_usage: true,
              },
            }),
          },
        );
        if (!upstream.ok || !upstream.body) throw failure(upstream.status);
        response.writeHead(200, {
          "Content-Type": "text/event-stream",
          "Cache-Control": "no-cache",
        });
        const reader = upstream.body.getReader();
        const cancel = () => {
          void reader.cancel().catch(() => {});
        };
        controller.signal.addEventListener("abort", cancel, { once: true });
        try {
          while (!controller.signal.aborted) {
            const chunk = await reader.read();
            if (chunk.done) break;
            if (!response.write(chunk.value)) {
              await new Promise((resolve) => {
                const finish = () => {
                  response.off("drain", finish);
                  response.off("close", finish);
                  resolve();
                };
                response.once("drain", finish);
                response.once("close", finish);
              });
            }
          }
          response.end();
        } finally {
          controller.signal.removeEventListener("abort", cancel);
          reader.releaseLock();
        }
      } catch (error) {
        if (response.headersSent) response.destroy();
        else
          json(
            response,
            Number.isInteger(error?.status) &&
              error.status >= 400 &&
              error.status <= 599
              ? error.status
              : 502,
            { error: "Cursor subscription unavailable" },
          );
      } finally {
        clearTimeout(timer);
        active.delete(controller);
      }
    });
  });
  server.headersTimeout = 10_000;
  server.requestTimeout = 30_000;
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    port: server.address().port,
    close: async () => {
      for (const controller of active) controller.abort();
      for (const pending of logins.values()) {
        pending.controller.abort();
        clearTimeout(pending.timer);
      }
      logins.clear();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
      await options.clearSessions?.();
    },
  };
}

async function main() {
  const [major, minor] = process.versions.node.split(".").map(Number);
  if (major < 20 || (major === 20 && minor < 19)) throw failure();
  const lines = createInterface({ input: process.stdin });
  const [line] = await once(lines, "line");
  lines.close();
  if (line.length > 4096) throw failure();
  const { secret } = JSON.parse(line);
  const allowed = (value) => {
    const url = new URL(value);
    if (
      url.protocol !== "https:" ||
      !(
        url.hostname === "cursor.com" ||
        url.hostname.endsWith(".cursor.com") ||
        url.hostname.endsWith(".cursor.sh")
      )
    )
      throw failure();
  };
  const originalConnect = http2.connect;
  http2.connect = (authority, ...args) => {
    allowed(authority);
    return originalConnect(authority, ...args);
  };
  const exchanged = new Map();
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init = {}) => {
    const url = new URL(
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.href
          : input.url,
    );
    allowed(url);
    const context = contexts.getStore();
    const response = await originalFetch(input, {
      ...init,
      redirect: "error",
      ...(context
        ? {
            signal: init.signal
              ? AbortSignal.any([context, init.signal])
              : context,
          }
        : {}),
    });
    if (url.pathname === "/auth/exchange_user_api_key" && response.ok) {
      const authorization =
        new Headers(init.headers).get("authorization") ?? "";
      const token = (
        await response
          .clone()
          .json()
          .catch(() => ({}))
      ).accessToken;
      if (token && authorization.startsWith("Bearer ")) {
        const hash = (s) => createHash("sha256").update(s).digest("hex");
        exchanged.set(hash(authorization.slice(7)), hash(token));
      }
    }
    return response;
  };
  const { CursorAuthPlugin } = await import("./index.mjs");
  const { nativeSessions } = await import("./native-session.mjs");
  const hooks = await CursorAuthPlugin({ magpie: { option: () => false } });
  const worker = await createCursorWorker(hooks, secret, {
    clearSessions: () => nativeSessions.clear(),
    forget: async (auth) => {
      const hash = createHash("sha256")
        .update(auth.access ?? auth.key)
        .digest("hex");
      const account = auth.type === "api" ? exchanged.get(hash) : hash;
      exchanged.delete(hash);
      for (const entry of [...nativeSessions.entries])
        if (entry.account === account) {
          await entry.cancel("account removed");
          nativeSessions.forget(entry);
        }
    },
  });
  process.stdout.write(
    JSON.stringify({ port: worker.port, version: "0.2.44" }) + "\n",
  );
  process.stdin.resume();
  process.stdin.once("end", () => {
    void worker.close().finally(() => process.exit(0));
  });
}
if (
  process.argv[1] &&
  realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
) {
  main().catch(() => process.exit(1));
}
