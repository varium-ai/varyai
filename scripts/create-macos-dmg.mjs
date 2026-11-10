import { spawnSync } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, rename, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));

function runCommand(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.signal ?? result.status})`);
  }
}

// Reuse Tauri's create-dmg tooling without rebuilding or re-signing the app.
export async function createMacosDmg({ app, output, run = runCommand }) {
  app = path.resolve(app);
  output = path.resolve(output);
  const config = JSON.parse(
    await readFile(path.join(root, "src-tauri/tauri.conf.json"), "utf8"),
  );
  const { dmg } = config.bundle.macOS;
  const bundler = path.resolve(path.dirname(app), "../dmg/bundle_dmg.sh");
  try {
    await access(bundler);
  } catch {
    throw new Error(
      "Tauri DMG tooling is missing; run pnpm build:desktop before repackaging.",
    );
  }
  await access(app);
  await access(path.join(root, "src-tauri", dmg.background));
  await mkdir(path.dirname(output), { recursive: true });
  const work = await mkdtemp(path.join(path.dirname(output), ".varyai-dmg-"));
  try {
    const stage = path.join(work, "stage");
    const image = path.join(work, path.basename(output));
    await mkdir(stage);
    await run("ditto", [app, path.join(stage, path.basename(app))]);
    await run("bash", [
      bundler,
      "--volname",
      config.productName,
      "--volicon",
      path.join(root, "src-tauri/icons/icon.icns"),
      "--background",
      path.join(root, "src-tauri", dmg.background),
      "--window-size",
      String(dmg.windowSize.width),
      String(dmg.windowSize.height),
      "--icon-size",
      "128",
      "--text-size",
      "16",
      "--icon",
      path.basename(app),
      String(dmg.appPosition.x),
      String(dmg.appPosition.y),
      "--hide-extension",
      path.basename(app),
      "--app-drop-link",
      String(dmg.applicationFolderPosition.x),
      String(dmg.applicationFolderPosition.y),
      image,
      stage,
    ]);
    await run("hdiutil", ["verify", image]);
    // Keep an existing artifact until its replacement has been verified.
    await rename(image, output);
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  if (process.platform !== "darwin" || process.argv.length !== 4) {
    console.error(
      "Usage (macOS): node scripts/create-macos-dmg.mjs <app-path> <output.dmg>",
    );
    process.exitCode = 1;
  } else {
    try {
      await createMacosDmg({ app: process.argv[2], output: process.argv[3] });
    } catch (error) {
      console.error(error.message);
      process.exitCode = 1;
    }
  }
}
