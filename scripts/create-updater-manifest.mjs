import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const releaseArtifacts = {
  macos: `Vary AI-{tag}-macOS-Universal.app.tar.gz`,
  windowsX64: `Vary AI-{tag}-Windows.msi`,
  windowsArm64: `Vary AI-{tag}-Windows-arm64.msi`,
  linuxX64: `Vary AI-{tag}-Linux-x86_64.AppImage`,
  linuxArm64: `Vary AI-{tag}-Linux-arm64.AppImage`,
};

const targetArtifacts = {
  "darwin-aarch64": releaseArtifacts.macos,
  "darwin-x86_64": releaseArtifacts.macos,
  "windows-x86_64": releaseArtifacts.windowsX64,
  "windows-aarch64": releaseArtifacts.windowsArm64,
  "linux-x86_64": releaseArtifacts.linuxX64,
  "linux-aarch64": releaseArtifacts.linuxArm64,
};

function updaterArtifact({ releaseAssetsDir, repository, tag, assetTemplate }) {
  const assetName = assetTemplate.replaceAll("{tag}", tag);
  const assetPath = path.join(releaseAssetsDir, assetName);
  const signaturePath = `${assetPath}.sig`;

  if (!existsSync(assetPath)) {
    throw new Error(`Missing updater artifact: ${assetPath}`);
  }
  if (!existsSync(signaturePath)) {
    throw new Error(`Missing updater signature: ${signaturePath}`);
  }

  const signature = readFileSync(signaturePath, "utf8").trim();
  if (!signature) {
    throw new Error(`Updater signature is empty: ${signaturePath}`);
  }

  const encodedTag = encodeURIComponent(tag);
  // GitHub normalizes spaces in uploaded asset names to dots.
  // Keep the local filename for reading the artifact and signature above.
  const encodedAssetName = encodeURIComponent(assetName.replaceAll(" ", "."));
  return {
    url: `https://github.com/${repository}/releases/download/${encodedTag}/${encodedAssetName}`,
    signature,
  };
}

export function buildUpdaterManifest({
  releaseAssetsDir,
  repository,
  tag,
  pubDate = new Date().toISOString(),
  platforms = ["macos", "windows", "linux"],
  targets: selectedTargets,
  windowsInstallers = {},
}) {
  if (
    Object.entries(windowsInstallers).some(
      ([target, format]) =>
        !["windows-x86_64", "windows-aarch64"].includes(target) ||
        !["msi", "nsis"].includes(format),
    )
  )
    throw new Error("Unsupported Windows updater installer format");
  if (
    !Array.isArray(platforms) ||
    platforms.length === 0 ||
    platforms.some(
      (platform) => !["macos", "windows", "linux"].includes(platform),
    )
  ) {
    throw new Error(
      "Select at least one supported platform: macos, windows, linux",
    );
  }
  if (
    selectedTargets !== undefined &&
    (!Array.isArray(selectedTargets) ||
      selectedTargets.length === 0 ||
      selectedTargets.some((target) => !Object.hasOwn(targetArtifacts, target)))
  ) {
    throw new Error("Select at least one supported updater target");
  }
  if (!/^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(tag)) {
    throw new Error(`Release tag must be a version tag such as v1.2.3: ${tag}`);
  }
  if (!/^[^/]+\/[^/]+$/.test(repository)) {
    throw new Error(
      `GitHub repository must use owner/name format: ${repository}`,
    );
  }

  const targets = {};
  const artifact = (assetTemplate) =>
    updaterArtifact({
      releaseAssetsDir,
      repository,
      tag,
      assetTemplate,
    });
  const artifactForTarget = (target) =>
    artifact(
      windowsInstallers[target] === "nsis"
        ? targetArtifacts[target].replace(/\.msi$/, "-setup.exe")
        : targetArtifacts[target],
    );
  if (selectedTargets) {
    for (const target of selectedTargets)
      targets[target] = artifactForTarget(target);
  } else {
    if (platforms.includes("macos")) {
      const macos = artifact(releaseArtifacts.macos);
      targets["darwin-aarch64"] = macos;
      targets["darwin-x86_64"] = macos;
    }
    if (platforms.includes("windows")) {
      targets["windows-x86_64"] = artifactForTarget("windows-x86_64");
      targets["windows-aarch64"] = artifactForTarget("windows-aarch64");
    }
    if (platforms.includes("linux")) {
      targets["linux-x86_64"] = artifact(releaseArtifacts.linuxX64);
      targets["linux-aarch64"] = artifact(releaseArtifacts.linuxArm64);
    }
  }

  return {
    version: tag.slice(1),
    notes: "See the GitHub release page for release notes.",
    pub_date: pubDate,
    platforms: targets,
  };
}

export function writeUpdaterManifest(options) {
  const manifest = buildUpdaterManifest(options);
  const manifestPath = path.join(options.releaseAssetsDir, "latest.json");
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  return manifestPath;
}

const invokedPath = process.argv[1]
  ? pathToFileURL(path.resolve(process.argv[1])).href
  : "";
if (import.meta.url === invokedPath) {
  const releaseAssetsDir = process.argv[2] ?? "release-assets";
  const tag = process.argv[3] ?? process.env.GITHUB_REF_NAME;
  const repository = process.argv[4] ?? process.env.GITHUB_REPOSITORY;

  if (!tag || !repository) {
    throw new Error(
      "Usage: node scripts/create-updater-manifest.mjs <assets-dir> <tag> <owner/repo> [macos,windows,linux]",
    );
  }

  const manifestPath = writeUpdaterManifest({
    releaseAssetsDir,
    repository,
    tag,
    ...(process.argv[5] ? { platforms: process.argv[5].split(",") } : {}),
  });
  console.log(`Created updater manifest: ${manifestPath}`);
}
