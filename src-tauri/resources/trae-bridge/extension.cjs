"use strict";
const fs = require("node:fs");
const net = require("node:net");
const path = require("node:path");
const os = require("node:os");
const crypto = require("node:crypto");
const { rendererSource } = require("./renderer.cjs");
let server, widget, socketPath, timer, auditConfig;
const pending = new Map();
function validAuditUrl(value) {
  try {
    const u = new URL(value);
    return (
      u.protocol === "http:" &&
      u.hostname === "127.0.0.1" &&
      u.pathname === "/private/trae/audit" &&
      !u.username &&
      !u.password &&
      !u.search &&
      !u.hash
    );
  } catch {
    return false;
  }
}
async function call(message) {
  const request = crypto.randomUUID();
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(request);
      reject(Error("TRAE bridge timed out"));
    }, 15000);
    pending.set(request, { resolve, reject, timeout });
    widget.postMessage({ ...message, request }).then(
      (ok) => {
        if (!ok) {
          clearTimeout(timeout);
          pending.delete(request);
          reject(Error("TRAE component unavailable"));
        }
      },
      () => {
        clearTimeout(timeout);
        pending.delete(request);
        reject(Error("TRAE component unavailable"));
      },
    );
  });
}
async function inspect(value) {
  if (!auditConfig) throw Error("audit_disabled");
  const response = await fetch(auditConfig.url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${auditConfig.token}`,
    },
    body: JSON.stringify(value),
    signal: AbortSignal.timeout(4000),
  });
  if (!response.ok) throw Error("audit_unavailable");
  return response.json();
}
async function disable() {
  auditConfig = undefined;
  return call({ op: "disable" });
}
async function activate(context) {
  const vscode = require("vscode");
  await vscode.icube.defineComponent(rendererSource());
  widget = vscode.icube.addIcubeComponentInTitleCenter(
    { tag: "varyai-trae-bridge-v1" },
    { position: "right" },
  );
  context.subscriptions.push(
    widget,
    widget.onDidReceiveMessage(async (message) => {
      if (message.reply) {
        const task = pending.get(message.reply);
        if (task) {
          clearTimeout(task.timeout);
          pending.delete(message.reply);
          message.error
            ? task.reject(Error("TRAE native bridge unavailable"))
            : task.resolve(message.result);
        }
      } else if (message.audit) {
        try {
          const value = await inspect(message.value);
          await widget.postMessage({ auditResult: message.audit, value });
        } catch {
          auditConfig = undefined;
          await widget.postMessage({ auditResult: message.audit, error: true });
        }
      }
    }),
  );
  const directory = path.join(os.homedir(), ".cc-switch", "trae-bridge");
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  fs.chmodSync(directory, 0o700);
  socketPath = path.join(directory, `bridge-${process.pid}.sock`);
  server = net.createServer((socket) => {
    socket.setEncoding("utf8");
    socket.setTimeout(20000, () => socket.destroy());
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk.toString("utf8");
      if (Buffer.byteLength(buffer) > 2 * 1024 * 1024) {
        socket.destroy();
        return;
      }
      const end = buffer.indexOf("\n");
      if (end < 0) return;
      socket.removeAllListeners("data");
      (async () => {
        const command = JSON.parse(buffer.slice(0, end));
        if (
          !["health", "enable", "disable", "apply", "remove", "list"].includes(
            command.op,
          )
        )
          throw Error("invalid_operation");
        if (command.op === "enable") {
          if (
            !validAuditUrl(command.url) ||
            typeof command.token !== "string" ||
            command.token.length < 32
          )
            throw Error("invalid_endpoint");
          // Verify VaryAI is present before enabling interception.
          const previous = auditConfig;
          auditConfig = { url: command.url, token: command.token };
          try {
            await inspect({ health: true });
          } catch {
            auditConfig = previous;
            throw Error("audit_unavailable");
          }
        }
        if (command.op === "disable") auditConfig = undefined;
        return call(command);
      })().then(
        (result) => socket.end(JSON.stringify({ result }) + "\n"),
        () =>
          socket.end(
            JSON.stringify({ error: "TRAE native bridge unavailable" }) + "\n",
          ),
      );
    });
    socket.on("error", () => {});
  });
  server.on("error", () => {});
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
  fs.chmodSync(socketPath, 0o600);
  timer = setInterval(async () => {
    if (auditConfig)
      try {
        await inspect({ health: true });
      } catch {
        await disable().catch(() => {});
      }
  }, 2000);
  timer.unref();
  context.subscriptions.push({ dispose: deactivate });
}
function deactivate() {
  clearInterval(timer);
  auditConfig = undefined;
  widget?.postMessage({ op: "disable", request: "shutdown" });
  for (const task of pending.values()) {
    clearTimeout(task.timeout);
    task.reject(Error("bridge_closed"));
  }
  pending.clear();
  server?.close();
  try {
    if (socketPath) fs.unlinkSync(socketPath);
  } catch {}
}
module.exports = { activate, deactivate, validAuditUrl };
