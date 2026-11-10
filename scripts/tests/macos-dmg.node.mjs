import assert from "node:assert/strict";
import {
  mkdtemp,
  mkdir,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

const root = new URL("../../", import.meta.url);

test("the native DMG has a background and a left-to-right installation layout", async () => {
  const config = JSON.parse(
    await readFile(new URL("src-tauri/tauri.conf.json", root)),
  );
  const dmg = config.bundle.macOS.dmg;
  assert.ok(dmg?.background, "The installation background must be bundled");
  const png = await readFile(new URL(`src-tauri/${dmg.background}`, root));
  assert.equal(png.readUInt32BE(16), dmg.windowSize.width * 2);
  assert.equal(png.readUInt32BE(20), (dmg.windowSize.height - 28) * 2);
  assert.ok(dmg.appPosition.x < dmg.applicationFolderPosition.x);
  assert.equal(dmg.appPosition.y, dmg.applicationFolderPosition.y);
  assert.ok(dmg.applicationFolderPosition.x + 64 < dmg.windowSize.width);
});

async function fixture(t) {
  const dir = await mkdtemp(path.join(tmpdir(), "varyai dmg test "));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const bundle = path.join(dir, "bundle");
  const app = path.join(bundle, "macos", "Vary AI.app");
  await mkdir(app, { recursive: true });
  await mkdir(path.join(bundle, "dmg"));
  await writeFile(
    path.join(bundle, "dmg", "bundle_dmg.sh"),
    "# Synthetic bundler",
  );
  const output = path.join(dir, "output", "Vary AI.dmg");
  await mkdir(path.dirname(output));
  await writeFile(output, "previous image");
  const calls = [];
  const run = async (command, args) => {
    calls.push({ command, args });
    if (command === "bash") await writeFile(args.at(-2), "new image");
  };
  return { dir, app, output, calls, run };
}

test("repackaging preserves the source app and shares the native Finder layout", async (t) => {
  const { createMacosDmg } = await import("../create-macos-dmg.mjs");
  const f = await fixture(t);
  const marker = path.join(f.app, "signature-marker");
  await writeFile(marker, "synthetic stapled signature");
  await createMacosDmg({ app: f.app, output: f.output, run: f.run });
  const build = f.calls.find((call) => call.command === "bash");
  const option = (name, count) => {
    const start = build.args.indexOf(name);
    assert.ok(start >= 0, `Missing ${name}`);
    return build.args.slice(start + 1, start + 1 + count);
  };
  const config = JSON.parse(
    await readFile(new URL("src-tauri/tauri.conf.json", root)),
  );
  const dmg = config.bundle.macOS.dmg;
  assert.deepEqual(
    option("--window-size", 2),
    [dmg.windowSize.width, dmg.windowSize.height].map(String),
  );
  assert.deepEqual(option("--icon", 3), [
    "Vary AI.app",
    ...[dmg.appPosition.x, dmg.appPosition.y].map(String),
  ]);
  assert.deepEqual(
    option("--app-drop-link", 2),
    [dmg.applicationFolderPosition.x, dmg.applicationFolderPosition.y].map(
      String,
    ),
  );
  assert.equal(
    path.basename(option("--background", 1)[0]),
    path.basename(dmg.background),
  );
  assert.equal(await readFile(marker, "utf8"), "synthetic stapled signature");
  assert.equal(await readFile(f.output, "utf8"), "new image");
  assert.deepEqual(await readdir(path.dirname(f.output)), ["Vary AI.dmg"]);
  assert.ok(
    f.calls.some((call) => call.command === "ditto" && call.args[0] === f.app),
  );
  assert.ok(
    f.calls.some(
      (call) => call.command === "hdiutil" && call.args[0] === "verify",
    ),
  );
});

test("a failed image verification leaves the previous artifact intact and removes staging", async (t) => {
  const { createMacosDmg } = await import("../create-macos-dmg.mjs");
  const f = await fixture(t);
  await assert.rejects(
    createMacosDmg({
      app: f.app,
      output: f.output,
      async run(command, args) {
        await f.run(command, args);
        if (command === "hdiutil")
          throw new Error("Synthetic verification failure");
      },
    }),
    /Synthetic verification failure/,
  );
  assert.equal(await readFile(f.output, "utf8"), "previous image");
  assert.deepEqual(await readdir(path.dirname(f.output)), ["Vary AI.dmg"]);
});

test("missing Tauri bundler fails before touching an existing artifact", async (t) => {
  const { createMacosDmg } = await import("../create-macos-dmg.mjs");
  const f = await fixture(t);
  await rm(path.join(f.dir, "bundle", "dmg", "bundle_dmg.sh"));
  await assert.rejects(
    createMacosDmg({ app: f.app, output: f.output, run: f.run }),
    /build:desktop/,
  );
  assert.equal(f.calls.length, 0);
  assert.equal(await readFile(f.output, "utf8"), "previous image");
});

test("both repackaging entry points use the shared DMG creator", async () => {
  for (const file of ["package-macos.sh", "scripts/notarize-macos.sh"]) {
    const source = await readFile(new URL(file, root), "utf8");
    assert.match(source, /node scripts\/create-macos-dmg\.mjs/);
    assert.doesNotMatch(source, /hdiutil create/);
  }
});
