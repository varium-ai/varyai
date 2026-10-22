import { homedir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { command } from "./lib/command.mjs";

const target = "x86_64-pc-windows-msvc";
const xwinVersion = "0.23.1";

function requireMac(os) {
  if (os !== "darwin")
    throw new Error("Windows cross compilation requires macOS");
}

export function verifyWindowsRuntime({
  env = process.env,
  run = command,
} = {}) {
  const executable = `src-tauri/target/${target}/release/varyai.exe`;
  const result = run("llvm-readobj", ["--coff-imports", executable], {
    env,
    capture: true,
  });
  const imports = [
    ...(result.stdout ?? "").matchAll(/Name:\s*(\S+\.dll)\b/gi),
  ].map((match) => match[1]);
  if (result.status !== 0 || imports.length === 0)
    throw new Error(
      "Unable to inspect Windows executable runtime dependencies",
    );
  const externalRuntime = imports.filter((name) =>
    /^(vcruntime|msvcp|concrt)\d.*\.dll$/i.test(name),
  );
  if (externalRuntime.length)
    throw new Error(
      `Windows executable still depends on external Visual C++ runtime: ${externalRuntime.join(", ")}; rebuild the application with STATIC_VCRUNTIME=true before distributing it`,
    );
}

export function setupWindowsCross({
  os = process.platform,
  env = process.env,
  run = command,
} = {}) {
  requireMac(os);
  const missing = ["llvm", "lld", "nsis", "cmake", "ninja"].filter(
    (formula) =>
      run("brew", ["list", "--versions", formula], {
        env,
        capture: true,
        allowFailure: true,
      }).status !== 0,
  );
  if (missing.length) run("brew", ["install", ...missing], { env });
  if (
    run("cargo-xwin", ["--version"], { env, capture: true, allowFailure: true })
      .status !== 0
  ) {
    run(
      "cargo",
      ["install", "--locked", "cargo-xwin", "--version", xwinVersion],
      { env },
    );
  }
  run("rustup", ["target", "add", target], { env });
}

export function buildWindowsCross({
  os = process.platform,
  env = process.env,
  run = command,
  updater = false,
} = {}) {
  requireMac(os);
  const prefixes = ["llvm", "lld"].map((formula) =>
    run("brew", ["--prefix", formula], { env, capture: true }).stdout.trim(),
  );
  const buildEnv = {
    ...env,
    PATH: [
      ...prefixes.map((prefix) => path.join(prefix, "bin")),
      env.PATH ?? "",
    ].join(path.delimiter),
    XWIN_CACHE_DIR:
      env.XWIN_CACHE_DIR ||
      path.join(env.HOME || homedir(), ".cache", "varyai", "xwin"),
    XWIN_ARCH: env.XWIN_ARCH || "x86_64",
    // macOS does not provide C.UTF-8; NSIS can crash while converting Unicode in C locale.
    LANG: "en_US.UTF-8",
    LC_ALL: "en_US.UTF-8",
    LC_CTYPE: "en_US.UTF-8",
    // tauri-build 2.5 requires this explicitly, even with newer Tauri CLIs.
    STATIC_VCRUNTIME: "true",
  };
  for (const key of Object.keys(buildEnv)) {
    if (key.startsWith("APPLE_")) delete buildEnv[key];
  }
  for (const [program, args] of [
    ["cargo-xwin", ["--version"]],
    ["clang-cl", ["--version"]],
    ["lld-link", ["--version"]],
    ["llvm-rc", ["/?"]],
    ["llvm-readobj", ["--version"]],
    ["makensis", ["-VERSION"]],
    ["cmake", ["--version"]],
    ["ninja", ["--version"]],
  ]) {
    const result = run(program, args, {
      env: buildEnv,
      capture: true,
      allowFailure: true,
    });
    if (result.error || result.status !== 0) {
      throw new Error(
        `${program} is unavailable; run pnpm build:windows setup to install the Windows cross-compilation tools`,
      );
    }
  }
  run(
    "pnpm",
    [
      "tauri",
      "build",
      "--runner",
      "cargo-xwin",
      "--target",
      target,
      // The macOS CLI restricts --bundles to Apple formats before reading --target.
      // Inline configuration selects NSIS when using the macOS CLI.
      "--config",
      JSON.stringify({
        bundle: { targets: ["nsis"], createUpdaterArtifacts: updater },
      }),
    ],
    { env: buildEnv },
  );
  verifyWindowsRuntime({ env: buildEnv, run });
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href
) {
  try {
    const args = process.argv.slice(2).filter((arg) => arg !== "--");
    if (args[0] === "setup" && args.length === 1) setupWindowsCross();
    else if (args.every((arg) => arg === "--updater"))
      buildWindowsCross({ updater: args.includes("--updater") });
    else
      throw new Error(
        "Usage: node scripts/build-windows-cross.mjs [setup|--updater]",
      );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
