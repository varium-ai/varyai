import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

const argv = process.argv.slice(2).filter((arg) => arg !== "--");

if (argv.includes("--help") || argv.includes("-h")) {
  console.log(`Usage: pnpm build:desktop [-- --updater] [--arch arm64|x64]

Builds the desktop installer for the current operating system:
  macOS   universal Apple Silicon + Intel (.app and .dmg)
  Windows native x64 or ARM64 (.msi)
  Linux   native architecture (.AppImage, .deb and .rpm)

Options:
  --updater       also create Tauri updater artifacts (release CI only)
  --arch <value>  Windows architecture; defaults to WINDOWS_RELEASE_ARCH or x64
  --notarize      notarize on macOS using an explicitly configured Keychain profile

macOS signs only when APPLE_SIGNING_IDENTITY is explicitly set.
Notarization additionally requires APPLE_TEAM_ID and NOTARY_KEYCHAIN_PROFILE.
`);
  process.exit(0);
}

function optionValue(name) {
  const inline = argv.find((arg) => arg.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);

  const index = argv.indexOf(name);
  return index >= 0 ? argv[index + 1] : undefined;
}

function run(command, args, env = process.env) {
  console.log(`> ${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, {
    cwd: process.cwd(),
    env,
    shell: process.platform === "win32",
    stdio: "inherit",
  });

  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const createUpdaterArtifacts = argv.includes("--updater");
const notarize = argv.includes("--notarize");
const tauriArgs = ["tauri", "build"];
const signingIdentity = process.env.APPLE_SIGNING_IDENTITY || undefined;

if (notarize && process.platform !== "darwin") {
  throw new Error("--notarize is supported only on macOS");
}
if (notarize) {
  for (const key of [
    "APPLE_SIGNING_IDENTITY",
    "APPLE_TEAM_ID",
    "NOTARY_KEYCHAIN_PROFILE",
  ]) {
    const value = process.env[key];
    if (!value || value !== value.trim() || /[\r\n]/.test(value)) {
      throw new Error(
        `${key} must be explicitly configured without surrounding whitespace for notarization`,
      );
    }
  }
  if (!/^[A-Z0-9]{10}$/.test(process.env.APPLE_TEAM_ID)) {
    throw new Error(
      "APPLE_TEAM_ID must be a ten-character Apple developer team identifier",
    );
  }
}
if (
  process.platform === "darwin" &&
  signingIdentity &&
  (signingIdentity !== signingIdentity.trim() ||
    !signingIdentity.startsWith("Developer ID Application: ") ||
    /[\r\n]/.test(signingIdentity))
) {
  throw new Error(
    "APPLE_SIGNING_IDENTITY must be a Developer ID Application identity without surrounding whitespace",
  );
}

switch (process.platform) {
  case "darwin":
    run("rustup", [
      "target",
      "add",
      "aarch64-apple-darwin",
      "x86_64-apple-darwin",
    ]);
    tauriArgs.push(
      "--target",
      "universal-apple-darwin",
      "--bundles",
      "app,dmg",
    );
    if (!signingIdentity) tauriArgs.push("--no-sign");
    break;

  case "win32": {
    const arch =
      optionValue("--arch") ?? process.env.WINDOWS_RELEASE_ARCH ?? "x64";
    if (arch === "arm64") {
      run("rustup", ["target", "add", "aarch64-pc-windows-msvc"]);
      tauriArgs.push("--target", "aarch64-pc-windows-msvc");
    } else if (arch !== "x64" && arch !== "x86_64") {
      throw new Error(`Unsupported Windows architecture: ${arch}`);
    }
    tauriArgs.push("--bundles", "msi");
    break;
  }

  case "linux":
    tauriArgs.push("--bundles", "appimage,deb,rpm");
    break;

  default:
    throw new Error(`Unsupported operating system: ${process.platform}`);
}

if (!createUpdaterArtifacts) {
  tauriArgs.push("--config", "src-tauri/tauri.no-updater.conf.json");
}

const buildEnv = { ...process.env };
if (process.platform === "darwin" && signingIdentity) {
  buildEnv.APPLE_SIGNING_IDENTITY = signingIdentity;
} else if (process.platform === "darwin") {
  delete buildEnv.APPLE_SIGNING_IDENTITY;
}
if (process.platform === "darwin") {
  // Notarization is a separate, profile-based step; never let Tauri infer it.
  for (const key of [
    "APPLE_ID",
    "APPLE_PASSWORD",
    "APPLE_API_KEY",
    "APPLE_API_KEY_PATH",
    "APPLE_API_ISSUER",
    "APPLE_NOTARIZATION_KEYCHAIN_PROFILE",
    "NOTARY_KEYCHAIN_PROFILE",
  ]) {
    delete buildEnv[key];
  }
}
run(pnpm, tauriArgs, buildEnv);

if (process.platform === "darwin" && signingIdentity) {
  const version = JSON.parse(readFileSync("package.json", "utf8")).version;
  const bundle = "src-tauri/target/universal-apple-darwin/release/bundle";
  const app = `${bundle}/macos/Vary AI.app`;
  const dmg = `${bundle}/dmg/Vary AI_${version}_universal.dmg`;
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]);
  const signature = spawnSync("codesign", ["-dv", "--verbose=4", app], {
    encoding: "utf8",
  });
  if (
    signature.status !== 0 ||
    !signature.stderr.split(/\r?\n/).includes(`Authority=${signingIdentity}`)
  ) {
    throw new Error(
      "App was not signed with the selected Developer ID identity",
    );
  }
  run("codesign", ["--force", "--sign", signingIdentity, "--timestamp", dmg]);
  run("codesign", ["--verify", "--verbose=2", dmg]);
  run("hdiutil", ["verify", dmg]);
  console.log(`Signed build ready: ${dmg}`);
} else if (process.platform === "darwin" && !signingIdentity) {
  console.warn(
    "Development packages built with --no-sign; no Developer ID identity was selected.",
  );
}

if (notarize) run("bash", ["scripts/notarize-macos.sh"]);
