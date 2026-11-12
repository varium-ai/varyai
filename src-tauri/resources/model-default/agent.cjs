"use strict";
const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");
const { pathToFileURL } = require("node:url");
const { applyDefault } = require("./renderer.cjs");

function isTarget(target, resources, app) {
  if (target.type !== "page") return false;
  try {
    const url = new URL(target.url);
    if (app === "qoder" && url.protocol === "qoder-cn-app:") {
      return (
        url.hostname === "renderer" &&
        url.pathname === "/index.html" &&
        url.searchParams.get("workbenchScope") === "primary"
      );
    }
    url.search = "";
    url.hash = "";
    const relative =
      app === "qoder" ? "out/renderer/index.html" : "renderer/index.html";
    return (
      url.href ===
      pathToFileURL(path.join(resources, "app.asar", relative)).href
    );
  } catch {
    return false;
  }
}

class Pipe {
  constructor(input, output) {
    this.input = input;
    this.pending = new Map();
    this.next = 0;
    let buffer = "";
    output.setEncoding("utf8");
    output.on("data", (chunk) => {
      buffer += chunk;
      if (buffer.length > 1024 * 1024) return this.close();
      let end;
      while ((end = buffer.indexOf("\0")) >= 0) {
        const message = buffer.slice(0, end);
        buffer = buffer.slice(end + 1);
        try {
          const value = JSON.parse(message),
            task = this.pending.get(value.id);
          if (!task) continue;
          this.pending.delete(value.id);
          clearTimeout(task.timer);
          value.error
            ? task.reject(Error("model_default_unavailable"))
            : task.resolve(value.result);
        } catch {
          /* Protocol events have no model data to forward. */
        }
      }
    });
    for (const stream of [input, output])
      stream.on("error", () => this.close());
    output.on("end", () => this.close());
  }
  call(method, params = {}, sessionId) {
    const id = ++this.next;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(Error("model_default_unavailable"));
      }, 5000);
      this.pending.set(id, { resolve, reject, timer });
      this.input.write(
        JSON.stringify({
          id,
          method,
          params,
          ...(sessionId ? { sessionId } : {}),
        }) + "\0",
      );
    });
  }
  close() {
    for (const task of this.pending.values()) {
      clearTimeout(task.timer);
      task.reject(Error("model_default_unavailable"));
    }
    this.pending.clear();
  }
}
function readPlan(directory) {
  const text = fs.readFileSync(path.join(directory, "plan.json"), "utf8");
  if (text.length > 8192) throw Error("model_default_unavailable");
  const plan = JSON.parse(text);
  if (
    !["workbuddy", "workbuddy-ai", "qoder"].includes(plan.app) ||
    !/^[a-f0-9]{64}$/.test(plan.revision) ||
    typeof plan.restore !== "boolean" ||
    typeof plan.model !== "string" ||
    plan.model.length > 512
  )
    throw Error("model_default_unavailable");
  return plan;
}
function resourcesFor(executable) {
  return process.platform === "darwin"
    ? path.resolve(executable, "../../Resources")
    : path.join(path.dirname(executable), "resources");
}
async function nativeKey(pipe, sessionId, model) {
  // Return only one verified managed-provider key, never the native catalog or account state.
  const result = await pipe.call(
    "Runtime.evaluate",
    {
      expression: `(async()=>{const api=globalThis.qoderDesktop;if(!api?.listChatComposerModels)return null;const rows=await api.listChatComposerModels({fetchStrategy:"live",workspaceDirectories:[]});const own=rows.filter(m=>m.enabled&&m.byok?.providerKey==="varyai"&&m.byok.modelKey===${JSON.stringify(model)}&&typeof m.key==="string"&&m.key.startsWith("byok:"));return own.length===1?own[0].key:null})()`,
      awaitPromise: true,
      returnByValue: true,
    },
    sessionId,
  );
  return result.result?.value;
}
async function apply(pipe, sessionId, plan, verifiedKey) {
  const key =
    plan.app === "qoder" && !plan.restore
      ? (verifiedKey ?? (await nativeKey(pipe, sessionId, plan.model)))
      : undefined;
  if (plan.app === "qoder" && !plan.restore && !key)
    throw Error("model_default_unavailable");
  const result = await pipe.call(
    "Runtime.evaluate",
    {
      expression: `(${applyDefault.toString()})(globalThis,${JSON.stringify({ ...plan, ...(key ? { nativeKey: key } : {}) })})`,
      returnByValue: true,
    },
    sessionId,
  );
  if (result.exceptionDetails || result.result?.value?.applied !== true)
    throw Error("model_default_unavailable");
  return key;
}

async function run(executable, directory) {
  const initial = readPlan(directory),
    resources = resourcesFor(executable);
  if (
    !path.isAbsolute(executable) ||
    !fs.statSync(path.join(resources, "app.asar")).isFile()
  )
    throw Error("model_default_unavailable");
  // No TCP inspector. Keep these inherited private pipes alive independently of Vary's lifetime.
  const child = spawn(executable, ["--remote-debugging-pipe"], {
    stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"],
    env: { ...process.env, ELECTRON_RUN_AS_NODE: "", NODE_OPTIONS: "" },
    windowsHide: false,
  });
  let alive = true;
  child.on("exit", () => {
    alive = false;
  });
  child.on("error", () => {
    alive = false;
  });
  const pipe = new Pipe(child.stdio[3], child.stdio[4]);
  const sessions = new Map();
  const applied = new Map();
  const preloads = new Map();
  let persistedRevision = null;
  let writtenStatus = null;
  const status = (plan, ready) => {
    if (!alive) return;
    if (ready) persistedRevision = plan.revision;
    else if (persistedRevision === plan.revision) return;
    const marker = `${plan.revision}:${ready}`;
    if (writtenStatus === marker) return;
    const destination = path.join(directory, "status.json"),
      temporary = destination + `.${process.pid}.tmp`;
    fs.writeFileSync(
      temporary,
      JSON.stringify({ revision: plan.revision, ready, updatedAt: Date.now() }),
      { mode: 0o600 },
    );
    fs.renameSync(temporary, destination);
    writtenStatus = marker;
  };
  while (alive) {
    let plan = initial;
    try {
      plan = readPlan(directory);
      if (plan.app !== initial.app) throw Error("model_default_unavailable");
      // Changed disk catalogs need a fresh native launch before claiming the new default.
      if (plan.revision !== initial.revision && !plan.restore) {
        status(plan, false);
      } else {
        const { targetInfos } = await pipe.call("Target.getTargets");
        const targets = targetInfos.filter((target) =>
          isTarget(target, resources, plan.app),
        );
        let ready = targets.length > 0;
        for (const target of targets) {
          let session = sessions.get(target.targetId);
          if (!session) {
            session = (
              await pipe.call("Target.attachToTarget", {
                targetId: target.targetId,
                flatten: true,
              })
            ).sessionId;
            sessions.set(target.targetId, session);
          }
          const marker = `${target.targetId}:${plan.revision}`;
          try {
            if (plan.restore && preloads.has(target.targetId)) {
              await pipe.call(
                "Page.removeScriptToEvaluateOnNewDocument",
                { identifier: preloads.get(target.targetId) },
                session,
              );
              preloads.delete(target.targetId);
            }
            // Recheck the renderer even when its target ID survives a reload.
            const key = await apply(pipe, session, plan, applied.get(marker));
            applied.set(marker, key);
            if (!plan.restore && !preloads.has(target.targetId)) {
              const source = `(${applyDefault.toString()})(globalThis,${JSON.stringify({ ...plan, ...(key ? { nativeKey: key } : {}) })})`;
              const { identifier } = await pipe.call(
                "Page.addScriptToEvaluateOnNewDocument",
                { source },
                session,
              );
              preloads.set(target.targetId, identifier);
              // Recreate the initial empty composer after the choice is persisted.
              await pipe.call("Page.reload", {}, session);
              ready = false;
            }
          } catch {
            ready = false;
            applied.delete(marker);
            if (sessions.delete(target.targetId)) {
              try {
                await pipe.call("Target.detachFromTarget", {
                  sessionId: session,
                });
              } catch {}
            }
          }
        }
        status(plan, ready);
      }
    } catch {
      try {
        status(plan, false);
      } catch {}
    }
    await new Promise((resolve) => setTimeout(resolve, 1500));
  }
  pipe.close();
}
if (require.main === module)
  run(process.argv[2], process.argv[3]).catch(() => {
    process.exitCode = 1;
  });
module.exports = { isTarget, Pipe, readPlan, apply, resourcesFor };
