"use strict";
// Attach only to user-launched TRAE. The local helper never starts or closes the application.
const fs = require("node:fs");
const path = require("node:path");
const net = require("node:net");
const crypto = require("node:crypto");
const { PassThrough } = require("node:stream");
const { createBridge, workbenchServices } = require("./renderer.cjs");
const { createStartingModelAdapter } = require("./starting-model.cjs");
const { createTraeRewards } = require("./rewards.cjs");
const SLOT = "__varyaiTraeNativeBridge";
const REVISION = 11;
// Keep the renderer API compatible so an older resident helper cannot replace the new hook.
const RENDERER_REVISION = 2;
// Older helpers remain resident; their disabled audit callbacks must not receive new events.
const AUDIT_BINDING = `__varyaiTraeAuditV${REVISION}`;
function safeModelError(message) {
  return typeof message === "string" &&
    (/^model_(add|update|remove|restore)_failed(?::-?\d{1,16})?$/.test(
      message,
    ) ||
      message === "model_recovery_unavailable")
    ? message
    : "TRAE native bridge unavailable";
}
function isChatTarget(target, appRoot) {
  if (target.type !== "page") return false;
  try {
    const url = new URL(target.url);
    const normalize = (value) => {
      const normalized = value.replace(/\\/g, "/");
      if (process.platform === "win32")
        return normalized.replace(/^\/?([a-z]):/i, "/$1:").toLowerCase();
      return process.platform === "darwin"
        ? normalized.toLowerCase()
        : normalized;
    };
    return (
      url.protocol === "vscode-file:" &&
      url.hostname === "vscode-app" &&
      [
        appRoot + "/out/vs/code/electron-browser/solo/solo-lite.html",
        appRoot + "/out/vs/code/electron-browser/workbench/workbench.html",
      ]
        .map(normalize)
        .includes(normalize(decodeURIComponent(url.pathname)))
    );
  } catch {
    return false;
  }
}
class PipeClient {
  constructor(input, output, onEvent) {
    this.input = input;
    this.pending = new Map();
    this.nextId = 0;
    let buffer = "";
    output.setEncoding("utf8");
    output.on("data", (chunk) => {
      buffer += chunk;
      if (buffer.length > 16 * 1024 * 1024) {
        buffer = "";
        this.close();
        return;
      }
      let end;
      while ((end = buffer.indexOf("\0")) >= 0) {
        const line = buffer.slice(0, end);
        buffer = buffer.slice(end + 1);
        try {
          const value = JSON.parse(line),
            task = this.pending.get(value.id);
          if (task) {
            clearTimeout(task.timer);
            this.pending.delete(value.id);
            value.error
              ? task.reject(Error("native_command_failed"))
              : task.resolve(value.result);
          } else onEvent(value);
        } catch {}
      }
    });
    output.on("end", () => this.close());
    output.on("error", () => this.close());
    input.on("error", () => this.close());
  }
  call(method, params = {}, sessionId) {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(Error("native_command_timeout"));
      }, 8000);
      this.pending.set(id, { resolve, reject, timer });
      this.input.write(
        JSON.stringify({
          id,
          method,
          params,
          ...(sessionId ? { sessionId } : {}),
        }) + "\0",
        (e) => {
          if (e) {
            clearTimeout(timer);
            this.pending.delete(id);
            reject(Error("native_pipe_closed"));
          }
        },
      );
    });
  }
  close() {
    for (const task of this.pending.values()) {
      clearTimeout(task.timer);
      task.reject(Error("native_pipe_closed"));
    }
    this.pending.clear();
  }
}
function nativeModelService(connection) {
  const request = async (method, data) =>
    (await connection.getDefaultClient()).request({
      service: "model",
      method,
      data,
    });
  return {
    _clientManagerService: { getClient: () => connection.getDefaultClient() },
    async getModelList() {
      const result = await request("model_list", {});
      if (result.code !== 0 || !Array.isArray(result.data?.model_list))
        throw Error("model_list_unavailable");
      return result.data;
    },
    addCustomModel: (data) => request("add_custom_model", data),
    updateCustomModel: (data) => request("update_custom_model", data),
  };
}
function bootstrapSource() {
  return `(async()=>{
    const previous=globalThis.${SLOT};
    if(previous?.revision===${RENDERER_REVISION}&&typeof previous.bridge?.replace==='function'){
      // Resident helpers must not downgrade a newer compatible renderer.
      if(previous.hookRevision>${REVISION})return true;
      if(previous.hookRevision===${REVISION}&&previous.startingModel===1&&previous.modelVerification===1&&previous.auditBinding===${JSON.stringify(AUDIT_BINDING)})return true;
    }
    await previous?.bridge?.disable();
    const solo=globalThis.__varyaiTraeSoloDI;
    let service,refresh,req;
    if(solo?.isInitialized()){
      const nativeModelService=${nativeModelService.toString()};
      service=nativeModelService(solo.resolve('IICubeAiChatConnectionService'));
      req=globalThis.__varyaiTraeSoloRequire;
      refresh=()=>solo.resolve('iICubeModelManagementService').notifyModelChanges({source:'varyai'});
    }else{
      const mod=await import(location.href.split('/out/')[0]+'/node_modules/@byted-icube/ai-modules-chat/dist/index.mjs');
      req=mod.__webpack_require__;
      const workbenchServices=${workbenchServices.toString()};
      ({service,refresh}=workbenchServices(req));
    }
    const pending=new Map(),createBridge=${createBridge.toString()},createStartingModelAdapter=${createStartingModelAdapter.toString()},createTraeRewards=${createTraeRewards.toString()};
    const bridge=createBridge(service,value=>new Promise((resolve,reject)=>{
      const id=crypto.randomUUID(),timer=setTimeout(()=>{pending.delete(id);reject(Error('audit_unavailable'));},5000);
      pending.set(id,{resolve,reject,timer});globalThis.${AUDIT_BINDING}(JSON.stringify({id,value}));
    }),refresh,globalThis.localStorage,createStartingModelAdapter(req),createTraeRewards());
    // An installed IDE extension may still be present. This helper now owns the window's hook.
    const legacy=document.querySelector('varyai-trae-bridge-v1');
    if(legacy)await legacy.receive({op:'disable',request:'native-pipe-takeover'});
    globalThis.${SLOT}={revision:${RENDERER_REVISION},hookRevision:${REVISION},modelVerification:1,startingModel:1,auditBinding:${JSON.stringify(AUDIT_BINDING)},bridge,resolve(id,value,error){const task=pending.get(id);if(!task)return;clearTimeout(task.timer);pending.delete(id);error?task.reject(Error('audit_unavailable')):task.resolve(value);}};
    return true;
  })()`;
}
// Solo bundles its own container. Obtain its loaded runtime without patching app files,
// pausing execution or exporting account credentials.
async function initializeSolo(pipe, sessionId) {
  const cached = await pipe.call(
    "Runtime.evaluate",
    {
      expression:
        "!!globalThis.__varyaiTraeSoloDI?.isInitialized() && !!globalThis.__varyaiTraeSoloRequire",
      returnByValue: true,
    },
    sessionId,
  );
  if (cached.result?.value) return;
  const group = "varyai-solo-container";
  try {
    const fn = await pipe.call(
      "Runtime.evaluate",
      {
        expression:
          "(async()=> (await import(location.href.split('/out/')[0]+'/node_modules/@byted-icube/solo-lite/dist/index.mjs')).bootstrapSoloLite)()",
        awaitPromise: true,
        objectGroup: group,
      },
      sessionId,
    );
    if (!fn.result?.objectId) throw Error("unsupported_agent_version");
    const properties = await pipe.call(
      "Runtime.getProperties",
      { objectId: fn.result.objectId, ownProperties: true },
      sessionId,
    );
    const scopes = properties.internalProperties?.find(
      (p) => p.name === "[[Scopes]]",
    );
    if (!scopes?.value?.objectId) throw Error("unsupported_agent_version");
    const list = await pipe.call(
      "Runtime.getProperties",
      { objectId: scopes.value.objectId, ownProperties: true },
      sessionId,
    );
    for (const scope of list.result) {
      if (!scope.value?.objectId) continue;
      const variables = await pipe.call(
        "Runtime.getProperties",
        { objectId: scope.value.objectId, ownProperties: true },
        sessionId,
      );
      for (const variable of variables.result) {
        if (variable.value?.type !== "function") continue;
        const found = await pipe.call(
          "Runtime.callFunctionOn",
          {
            objectId: variable.value.objectId,
            functionDeclaration: function () {
              if (!this.m) return false;
              // Module IDs change between builds; select the unique container export.
              const ids = Object.keys(this.m).filter((id) => {
                const factory = this.m[id];
                return (
                  typeof factory === "function" &&
                  /\bmcf\s*:/.test(Function.prototype.toString.call(factory))
                );
              });
              if (ids.length !== 1) return false;
              const container = this(ids[0]).mcf;
              if (typeof container?.getInstance !== "function") return false;
              const di = container.getInstance();
              if (
                typeof di?.isInitialized !== "function" ||
                !di.isInitialized() ||
                typeof di.resolve !== "function"
              )
                return false;
              globalThis.__varyaiTraeSoloDI = di;
              globalThis.__varyaiTraeSoloRequire = this;
              return true;
            }.toString(),
            returnByValue: true,
          },
          sessionId,
        );
        if (found.result?.value) return;
      }
    }
    throw Error("unsupported_agent_version");
  } finally {
    await pipe
      .call("Runtime.releaseObjectGroup", { objectGroup: group }, sessionId)
      .catch(() => {});
  }
}
function deduplicateUsage(send) {
  const reported = new Map();
  return async (value) => {
    const key =
      value.usage && typeof value.requestId === "string"
        ? JSON.stringify([value.sessionId, value.requestId])
        : undefined;
    if (key && reported.has(key)) return reported.get(key);
    const result = Promise.resolve().then(() => send(value));
    if (key) {
      reported.set(key, result);
      if (reported.size > 512) reported.delete(reported.keys().next().value);
    }
    try {
      return await result;
    } catch (error) {
      if (key) reported.delete(key);
      throw error;
    }
  };
}
async function connectEndpoint(endpoint, onEvent, WebSocketClass = WebSocket) {
  const url = new URL(endpoint);
  if (
    url.protocol !== "ws:" ||
    url.hostname !== "127.0.0.1" ||
    !url.port ||
    !/^\/devtools\/browser\/[a-zA-Z0-9-]+$/.test(url.pathname) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  )
    throw Error("invalid_endpoint");
  const socket = new WebSocketClass(endpoint);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      socket.close();
      reject(Error("connection_timeout"));
    }, 5000);
    socket.addEventListener(
      "open",
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
    socket.addEventListener(
      "error",
      () => {
        clearTimeout(timer);
        reject(Error("connection_failed"));
      },
      { once: true },
    );
  });
  const input = new PassThrough(),
    output = new PassThrough();
  const pipe = new PipeClient(input, output, onEvent);
  input.on("data", (chunk) => socket.send(chunk.toString().replace(/\0$/, "")));
  socket.addEventListener("message", (event) =>
    output.write(event.data + "\0"),
  );
  socket.addEventListener("close", () => pipe.close());
  try {
    // Native Electron windows are not listed until target discovery is enabled.
    await pipe.call("Target.setDiscoverTargets", { discover: true });
  } catch (error) {
    socket.close();
    pipe.close();
    throw error;
  }
  return { pipe, socket };
}
async function run(endpoint, directory) {
  // The official bundle path, not the endpoint, defines which windows may be attached.
  const executable = process.execPath;
  const appRoot =
    process.platform === "darwin"
      ? path.resolve(executable, "../../Resources/app")
      : path.join(path.dirname(executable), "resources/app");
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  fs.chmodSync(directory, 0o700);
  const socketPath = path.join(
    directory,
    `bridge-agent-v${REVISION}-selection-${process.pid}.sock`,
  );
  const pipeAuth =
    process.platform === "win32"
      ? crypto.randomBytes(32).toString("hex")
      : null;
  const listenPath = pipeAuth
    ? String.raw`\\.\pipe\varyai-trae-${crypto.createHash("sha256").update(directory).digest("hex")}-${process.pid}`
    : socketPath;
  let config,
    pipe,
    scanning,
    expected = 0;
  const sessions = new Map(),
    targets = new Map();
  const evaluate = async (sessionId, expression) => {
    const value = await pipe.call(
      "Runtime.evaluate",
      { expression, awaitPromise: true, returnByValue: true },
      sessionId,
    );
    if (value.exceptionDetails)
      throw Error(
        safeModelError(
          value.exceptionDetails.exception?.description
            ?.split("\n")[0]
            ?.replace(/^Error: /, ""),
        ),
      );
    return value.result?.value;
  };
  const invoke = (sessionId, op, ...args) =>
    evaluate(
      sessionId,
      `globalThis.${SLOT}.bridge[${JSON.stringify(op)}](...${JSON.stringify(args)})`,
    );
  async function disable() {
    config = undefined;
    await Promise.allSettled(
      [...sessions.keys()].map((s) => invoke(s, "disable")),
    );
  }
  const inspect = deduplicateUsage(async (value) => {
    const current = config;
    if (!current) throw Error("audit_disabled");
    const response = await fetch(current.url, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: "Bearer " + current.token,
      },
      body: JSON.stringify(value),
      signal: AbortSignal.timeout(4000),
    });
    if (!response.ok) throw Error("audit_unavailable");
    return response.json();
  });
  const connection = await connectEndpoint(endpoint, (event) => {
    if (event.method === "Target.detachedFromTarget") {
      sessions.delete(event.params.sessionId);
      return;
    }
    if (
      event.method !== "Runtime.bindingCalled" ||
      event.params?.name !== AUDIT_BINDING ||
      !sessions.has(event.sessionId)
    )
      return;
    (async () => {
      const payload = JSON.parse(event.params.payload);
      if (typeof payload.id !== "string") return;
      try {
        const value = await inspect(payload.value);
        await evaluate(
          event.sessionId,
          `globalThis.${SLOT}.resolve(${JSON.stringify(payload.id)},${JSON.stringify(value)},false)`,
        );
      } catch {
        await evaluate(
          event.sessionId,
          `globalThis.${SLOT}.resolve(${JSON.stringify(payload.id)},null,true)`,
        ).catch(() => {});
        await disable();
      }
    })().catch(() => {});
  });
  pipe = connection.pipe;
  async function scan() {
    if (scanning) return scanning;
    scanning = (async () => {
      try {
        const result = await pipe.call("Target.getTargets");
        const alive = new Set();
        const windows = result.targetInfos.filter((t) =>
          isChatTarget(t, appRoot),
        );
        expected = windows.length;
        for (const info of windows) {
          alive.add(info.targetId);
          let sessionId = targets.get(info.targetId);
          try {
            if (!sessionId) {
              ({ sessionId } = await pipe.call("Target.attachToTarget", {
                targetId: info.targetId,
                flatten: true,
              }));
              targets.set(info.targetId, sessionId);
              await pipe.call("Runtime.enable", {}, sessionId);
              await pipe.call(
                "Runtime.addBinding",
                { name: AUDIT_BINDING },
                sessionId,
              );
            }
            if (new URL(info.url).pathname.endsWith("/solo/solo-lite.html"))
              await initializeSolo(pipe, sessionId);
            await evaluate(sessionId, bootstrapSource());
            sessions.set(sessionId, info.targetId);
            if (config) await invoke(sessionId, "enable");
          } catch {
            sessions.delete(sessionId);
          }
        }
        for (const [id, sid] of targets)
          if (!alive.has(id)) {
            targets.delete(id);
            sessions.delete(sid);
          }
      } catch {
        expected = 0;
      } finally {
        scanning = undefined;
      }
    })();
    return scanning;
  }
  function validConfig(command) {
    try {
      const url = new URL(command.url);
      return (
        url.protocol === "http:" &&
        url.hostname === "127.0.0.1" &&
        url.pathname === "/private/trae/audit" &&
        !url.search &&
        !url.hash &&
        !url.username &&
        !url.password &&
        typeof command.token === "string" &&
        command.token.length >= 32
      );
    } catch {
      return false;
    }
  }
  async function control(command) {
    if (command.op === "disable") {
      await disable();
      return true;
    }
    if (command.op === "enable") {
      if (!validConfig(command)) throw Error("invalid_endpoint");
      config = { url: command.url, token: command.token };
      try {
        await inspect({ health: true });
        await scan();
        if (!sessions.size || sessions.size !== expected)
          throw Error("chat_window_unavailable");
        await Promise.all([...sessions.keys()].map((s) => invoke(s, "enable")));
        return true;
      } catch (e) {
        await disable();
        throw e;
      }
    }
    await scan();
    if (command.op === "health") {
      const states = await Promise.all(
        [...sessions.keys()].map((s) =>
          evaluate(s, `!!globalThis.${SLOT}?.bridge.enabled`).catch(
            () => false,
          ),
        ),
      );
      const ready = states.length > 0 && states.length === expected;
      const modelReplacement =
        ready &&
        (
          await Promise.all(
            [...sessions.keys()].map((s) =>
              evaluate(
                s,
                `typeof globalThis.${SLOT}?.bridge.replace === 'function'`,
              ).catch(() => false),
            ),
          )
        ).every(Boolean);
      return {
        ready,
        revision: REVISION,
        modelVerification: 1,
        startingModel: 1,
        modelReplacement,
        enabled: !!config && ready && states.every(Boolean),
        scope: "ide-and-agent",
      };
    }
    const first = sessions.keys().next().value;
    if (!first) throw Error("no_chat_window");
    if (command.op === "rewards")
      return invoke(first, "rewards", command.input);
    if (command.op === "list") return invoke(first, "list");
    if (command.op === "matches")
      return invoke(first, "matches", command.id, command.config);
    if (command.op === "apply")
      return invoke(first, "apply", command.id, command.name, command.config);
    if (command.op === "select") {
      if (sessions.size !== expected) throw Error("chat_window_unavailable");
      await Promise.all(
        [...sessions.keys()].map((s) =>
          invoke(s, "select", command.id, command.model),
        ),
      );
      return true;
    }
    if (command.op === "replace")
      return invoke(first, "replace", command.id, command.name, command.config);
    if (command.op === "remove") {
      await invoke(first, "remove", command.id);
      return true;
    }
    throw Error("invalid_operation");
  }
  const server = net.createServer((socket) => {
    socket.setEncoding("utf8");
    socket.setTimeout(20000, () => socket.destroy());
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk;
      if (Buffer.byteLength(buffer) > 2 * 1024 * 1024) {
        socket.destroy();
        return;
      }
      const end = buffer.indexOf("\n");
      if (end < 0) return;
      socket.removeAllListeners("data");
      Promise.resolve()
        .then(() => {
          const command = JSON.parse(buffer.slice(0, end));
          if (
            pipeAuth &&
            (typeof command.auth !== "string" ||
              command.auth.length !== pipeAuth.length ||
              !crypto.timingSafeEqual(
                Buffer.from(command.auth),
                Buffer.from(pipeAuth),
              ))
          )
            throw Error("unauthorized");
          return control(command);
        })
        .then(
          (result) => socket.end(JSON.stringify({ result }) + "\n"),
          (error) =>
            socket.end(
              JSON.stringify({ error: safeModelError(error?.message) }) + "\n",
            ),
        );
    });
    socket.on("error", () => {});
  });
  server.on("error", () => {
    disable().catch(() => {});
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(listenPath, resolve);
  });
  if (pipeAuth)
    fs.writeFileSync(
      socketPath,
      JSON.stringify({ pipe: listenPath, auth: pipeAuth }),
      { mode: 0o600, flag: "wx" },
    );
  else fs.chmodSync(socketPath, 0o600);
  const timer = setInterval(() => {
    scan().catch(() => {});
    if (config) inspect({ health: true }).catch(() => disable());
  }, 2000);
  const cleanup = () => {
    clearInterval(timer);
    server.close();
    try {
      fs.unlinkSync(socketPath);
    } catch {}
    pipe.close();
  };
  connection.socket.addEventListener("close", () => {
    cleanup();
    process.exit(0);
  });
  // Recoverable helper faults release audit hooks without closing TRAE.
  process.on("uncaughtException", () => {
    disable().catch(() => {});
  });
  process.on("unhandledRejection", () => {
    disable().catch(() => {});
  });
  await scan();
}
module.exports = {
  safeModelError,
  isChatTarget,
  bootstrapSource,
  initializeSolo,
  nativeModelService,
  workbenchServices,
  deduplicateUsage,
  PipeClient,
  connectEndpoint,
  run,
};
if (require.main === module) {
  run(process.argv[2], process.argv[3]).catch(() => process.exit(1));
}
