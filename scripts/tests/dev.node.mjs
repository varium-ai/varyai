import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

const devModule = new URL("../dev.mjs", import.meta.url).href;

async function listen(server, port = 0) {
  server.listen(port, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

async function fixture(t, port) {
  const root = await mkdtemp(path.join(tmpdir(), "varyai dev test "));
  t.after(() => rm(root, { recursive: true, force: true }));
  await writeFile(
    path.join(root, "vite.config.ts"),
    `export default { server: { host: "127.0.0.1", port: ${port}, strictPort: true } };`,
  );
  await writeFile(path.join(root, "index.html"), "<p>Synthetic renderer</p>");
  await writeFile(
    path.join(root, "run.mjs"),
    `import { spawn } from "node:child_process";
import { runDev } from ${JSON.stringify(devModule)};
const mode = process.argv[2];
try {
  process.exitCode = await runDev({
    root: ${JSON.stringify(root)},
    args: ["--no-watch", "--", "--offline"],
    spawnTauri(args, options) {
      console.log("TEST_ARGS " + JSON.stringify(args));
      if (mode === "spawn-error") return spawn("varyai-nonexistent-test-command", [], options);
      const config = JSON.parse(args[args.indexOf("--config") + 1]);
      if (config.build.beforeDevCommand !== "") throw new Error("Duplicate frontend hook");
      const code = \
        'const url = process.argv[1];' +
        'const response = await fetch(url);' +
        'if (!(await response.text()).includes("Synthetic renderer")) process.exit(9);' +
        'console.log("TEST_CHILD_PID " + process.pid);' +
        (mode === "ignore-stop" ? 'process.on("SIGTERM", () => {});' : '') +
        'console.log("TEST_READY " + url);' +
        (mode === "failure" ? 'process.exit(7);' : 'setInterval(() => {}, 1000);');
      return spawn(process.execPath, ["--input-type=module", "-e", code, config.build.devUrl], options);
    },
  });
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
`,
  );
  return root;
}

function launch(t, root, mode = "wait") {
  const child = spawn(process.execPath, [path.join(root, "run.mjs"), mode], {
    cwd: root,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  let readyResolve;
  let readyReject;
  const ready = new Promise((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });
  // Failure tests intentionally exit before announcing readiness.
  ready.catch(() => {});
  const collect = (data) => {
    output += data.toString();
    const match = output.match(/TEST_READY (http:\/\/[^\s]+)/);
    if (match) readyResolve(match[1]);
  };
  child.stdout.on("data", collect);
  child.stderr.on("data", collect);
  const exited = once(child, "exit").then(([code, signal]) => {
    readyReject(new Error(`Runner exited (${code}, ${signal}): ${output}`));
    return { code, signal };
  });
  t.after(async () => {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await exited;
    }
  });
  return { child, ready, exited, output: () => output };
}

async function assertReleased(port) {
  const server = createServer();
  try {
    await listen(server, port);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

test(
  "occupied ports are skipped and Tauri receives the actual renderer URL",
  { timeout: 20000 },
  async (t) => {
    const occupied = createServer((_, response) =>
      response.end("Other application"),
    );
    const port = await listen(occupied);
    t.after(() => new Promise((resolve) => occupied.close(resolve)));
    const root = await fixture(t, port);
    const runner = launch(t, root, "failure");
    const url = await runner.ready;
    const selected = Number(new URL(url).port);
    assert.notEqual(selected, port);
    assert.equal(
      await (await fetch(`http://127.0.0.1:${port}`)).text(),
      "Other application",
    );
    const args = JSON.parse(runner.output().match(/TEST_ARGS (.+)/)[1]);
    assert.deepEqual(args.slice(0, 2), ["dev", "--no-watch"]);
    assert.ok(args.indexOf("--config") < args.indexOf("--"));
    assert.deepEqual(args.slice(-2), ["--", "--offline"]);
    assert.deepEqual(await runner.exited, { code: 7, signal: null });
    await assertReleased(selected);
    assert.equal(
      await (await fetch(`http://127.0.0.1:${port}`)).text(),
      "Other application",
    );
  },
);

test(
  "Tauri failure releases the renderer port and the next run succeeds",
  { timeout: 20000 },
  async (t) => {
    const reservation = createServer();
    const port = await listen(reservation);
    await new Promise((resolve) => reservation.close(resolve));
    const root = await fixture(t, port);
    const failed = launch(t, root, "failure");
    assert.deepEqual(await failed.exited, { code: 7, signal: null });
    assert.match(failed.output(), /TEST_READY /);
    await assertReleased(port);
    const restarted = launch(t, root, "failure");
    assert.equal(Number(new URL(await restarted.ready).port), port);
    assert.deepEqual(await restarted.exited, { code: 7, signal: null });
    await assertReleased(port);
  },
);

test(
  "failure to spawn Tauri closes the renderer",
  { timeout: 20000 },
  async (t) => {
    const reservation = createServer();
    const port = await listen(reservation);
    await new Promise((resolve) => reservation.close(resolve));
    const runner = launch(t, await fixture(t, port), "spawn-error");
    assert.deepEqual(await runner.exited, { code: 1, signal: null });
    assert.match(runner.output(), /ENOENT/);
    await assertReleased(port);
  },
);

test(
  "a stalled Vite shutdown cannot hold the launcher open",
  { timeout: 20000 },
  async (t) => {
    const reservation = createServer();
    const port = await listen(reservation);
    await new Promise((resolve) => reservation.close(resolve));
    const root = await fixture(t, port);
    await writeFile(
      path.join(root, "vite.config.ts"),
      `export default {
    server: { host: "127.0.0.1", port: ${port}, strictPort: true },
    plugins: [{ name: "synthetic-stalled-close", closeBundle() { return new Promise(() => {}); } }],
  };`,
    );
    const runner = launch(t, root, "failure");
    assert.deepEqual(await runner.exited, { code: 7, signal: null });
    await assertReleased(port);
  },
);

test(
  "concurrent runs use separate ports and shutting one down leaves the other running",
  {
    timeout: 20000,
    skip: process.platform === "win32" && "Requires POSIX signal delivery",
  },
  async (t) => {
    const reservation = createServer();
    const port = await listen(reservation);
    await new Promise((resolve) => reservation.close(resolve));
    const root = await fixture(t, port);
    const first = launch(t, root);
    const firstUrl = await first.ready;
    const second = launch(t, root);
    const secondUrl = await second.ready;
    assert.notEqual(firstUrl, secondUrl);
    second.child.kill("SIGTERM");
    assert.deepEqual(await second.exited, { code: 0, signal: null });
    assert.equal((await fetch(firstUrl)).status, 200);
    await assertReleased(Number(new URL(secondUrl).port));
    first.child.kill("SIGTERM");
    await first.exited;
    await assertReleased(Number(new URL(firstUrl).port));
  },
);

test(
  "the renderer releases its port if the launcher is killed",
  {
    timeout: 20000,
    skip: process.platform === "win32" && "Requires POSIX signal delivery",
  },
  async (t) => {
    const reservation = createServer();
    const port = await listen(reservation);
    await new Promise((resolve) => reservation.close(resolve));
    const runner = launch(t, await fixture(t, port));
    const selected = Number(new URL(await runner.ready).port);
    const childPid = Number(runner.output().match(/TEST_CHILD_PID (\d+)/)[1]);
    t.after(() => {
      try {
        process.kill(-childPid, "SIGKILL");
      } catch (error) {
        if (error.code !== "ESRCH") throw error;
      }
    });
    runner.child.kill("SIGKILL");
    assert.deepEqual(await runner.exited, { code: null, signal: "SIGKILL" });
    const deadline = Date.now() + 5000;
    while (true) {
      try {
        await assertReleased(selected);
        break;
      } catch (error) {
        if (error.code !== "EADDRINUSE" || Date.now() >= deadline) throw error;
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
    }
  },
);

for (const [signal, mode] of [
  ["SIGINT", "wait"],
  ["SIGTERM", "ignore-stop"],
]) {
  test(
    `shutdown with ${signal} releases the renderer and stops Tauri (${mode})`,
    {
      timeout: 20000,
      skip: process.platform === "win32" && "Requires POSIX signal delivery",
    },
    async (t) => {
      const reservation = createServer();
      const port = await listen(reservation);
      await new Promise((resolve) => reservation.close(resolve));
      const runner = launch(t, await fixture(t, port), mode);
      const selected = Number(new URL(await runner.ready).port);
      const childPid = Number(runner.output().match(/TEST_CHILD_PID (\d+)/)[1]);
      runner.child.kill(signal);
      if (mode === "ignore-stop") {
        await new Promise((resolve) => setTimeout(resolve, 25));
        runner.child.kill(signal);
      }
      assert.deepEqual(await runner.exited, { code: 0, signal: null });
      await assertReleased(selected);
      assert.throws(() => process.kill(childPid, 0), { code: "ESRCH" });
    },
  );
}
