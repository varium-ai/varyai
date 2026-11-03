import { spawn, spawnSync } from "node:child_process";
import { constants } from "node:os";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const script = fileURLToPath(import.meta.url);
const projectRoot = fileURLToPath(new URL("..", import.meta.url));
const tauriCli = require.resolve("@tauri-apps/cli/tauri.js");

function spawnTauri(args, options) {
  return spawn(process.execPath, [tauriCli, ...args], options);
}

function stopGroup(child, signal) {
  if (!child?.pid) return;
  if (process.platform === "win32") {
    if (child.exitCode === null && child.signalCode === null) {
      spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
        timeout: 5000,
      });
    }
    return;
  }
  try {
    process.kill(-child.pid, signal);
  } catch (error) {
    if (error.code === "ESRCH") return;
    if (error.code === "EPERM") {
      if (child.exitCode === null && child.signalCode === null)
        child.kill(signal);
      return;
    }
    throw error;
  }
}

function childResult(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
}

function requestRendererStop(child) {
  if (child?.connected) child.send({ type: "stop" }, () => {});
}

async function closeRenderer(child, result) {
  if (!child) return;
  requestRendererStop(child);
  let timer;
  try {
    await Promise.race([
      result,
      new Promise((resolve) => {
        timer = setTimeout(resolve, 4000);
      }),
    ]);
  } finally {
    clearTimeout(timer);
    stopGroup(child, "SIGKILL");
  }
}

// Keep Vite and esbuild out of the terminal's foreground process group.
// IPC shutdown lets in-flight transforms finish before their workers exit.
async function runRenderer(root) {
  process.chdir(root);
  let server;
  let stopping = false;
  let closing = false;
  const stop = () => {
    stopping = true;
    if (!server || closing) return;
    closing = true;
    setTimeout(() => process.exit(0), 3000).unref();
    server.close().then(
      () => process.exit(0),
      (error) => {
        console.error(`[dev] ${error.message}`);
        process.exit(1);
      },
    );
  };
  process.on("message", (message) => {
    if (message?.type === "stop") stop();
  });
  process.once("disconnect", stop);
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  try {
    const { createServer } = await import("vite");
    server = await createServer({
      configFile: path.join(root, "vite.config.ts"),
      server: { strictPort: false },
    });
    if (stopping) return stop();
    await server.listen();
    if (stopping) return stop();
    const url = server.resolvedUrls?.local[0];
    if (!url)
      throw new Error("The renderer did not publish a local development URL.");
    process.send({ type: "ready", url });
  } catch (error) {
    console.error(`[dev] ${error.message}`);
    process.exit(1);
  }
}

export async function runDev({
  root = projectRoot,
  args = process.argv.slice(2),
  spawnTauri: launchTauri = spawnTauri,
} = {}) {
  process.chdir(root);
  let renderer;
  let rendererResult;
  let tauri;
  let interrupted = false;
  let killTimer;
  const stop = (signal) => {
    if (interrupted) return;
    interrupted = true;
    requestRendererStop(renderer);
    stopGroup(tauri, signal);
    if (tauri?.pid) {
      killTimer = setTimeout(() => stopGroup(tauri, "SIGKILL"), 3000);
      killTimer.unref();
    }
  };
  const onInterrupt = () => stop("SIGINT");
  const onTerminate = () => stop("SIGTERM");
  process.on("SIGINT", onInterrupt);
  process.on("SIGTERM", onTerminate);

  try {
    const separator = args.indexOf("--");
    const cliArgs = separator < 0 ? args : args.slice(0, separator);
    const runnerArgs = separator < 0 ? [] : args.slice(separator);
    const tauriArgs = ["dev", ...cliArgs];
    if (
      !cliArgs.some((arg) => ["-h", "--help", "-V", "--version"].includes(arg))
    ) {
      renderer = spawn(process.execPath, [script, "--renderer-worker", root], {
        cwd: root,
        detached: process.platform !== "win32",
        stdio: ["ignore", "inherit", "inherit", "ipc"],
      });
      rendererResult = childResult(renderer);
      const devUrl = await Promise.race([
        new Promise((resolve) =>
          renderer.once("message", (message) => resolve(message.url)),
        ),
        rendererResult.then(() => {
          throw new Error("The renderer exited before it was ready.");
        }),
      ]);
      if (interrupted) return 0;
      console.log(`[dev] Renderer ready at ${devUrl}`);
      // Tauri must use this renderer, without launching a second frontend hook.
      tauriArgs.push(
        "--config",
        JSON.stringify({ build: { devUrl, beforeDevCommand: "" } }),
      );
    }
    if (interrupted) return 0;
    tauri = launchTauri([...tauriArgs, ...runnerArgs], {
      cwd: root,
      stdio: "inherit",
      detached: process.platform !== "win32",
    });
    const tauriResult = childResult(tauri);
    const result = await Promise.race([
      tauriResult,
      ...(rendererResult
        ? [
            rendererResult.then(() => {
              if (interrupted) return tauriResult;
              throw new Error("The renderer stopped while Tauri was running.");
            }),
          ]
        : []),
    ]);
    return interrupted
      ? 0
      : (result.code ?? 128 + (constants.signals[result.signal] ?? 1));
  } catch (error) {
    if (interrupted) return 0;
    throw error;
  } finally {
    clearTimeout(killTimer);
    try {
      stopGroup(tauri, "SIGTERM");
      stopGroup(tauri, "SIGKILL");
    } finally {
      try {
        await closeRenderer(renderer, rendererResult);
      } finally {
        process.off("SIGINT", onInterrupt);
        process.off("SIGTERM", onTerminate);
      }
    }
  }
}

if (
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
) {
  if (process.argv[2] === "--renderer-worker" && process.send) {
    await runRenderer(process.argv[3]);
  } else {
    let code;
    try {
      code = await runDev();
    } catch (error) {
      console.error(`[dev] ${error.message}`);
      code = 1;
    }
    process.exit(code);
  }
}
