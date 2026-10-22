import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

let assetsDir: string;
const tag = "v1.2.3";
const artifactNames = [
  `Vary AI-${tag}-macOS-Universal.app.tar.gz`,
  `Vary AI-${tag}-Windows.msi`,
  `Vary AI-${tag}-Windows-arm64.msi`,
  `Vary AI-${tag}-Linux-x86_64.AppImage`,
  `Vary AI-${tag}-Linux-arm64.AppImage`,
];

beforeEach(() => {
  assetsDir = mkdtempSync(path.join(tmpdir(), "varyai-updater-manifest-"));
});

afterEach(() => {
  rmSync(assetsDir, { recursive: true, force: true });
});

describe("updater manifest generation", () => {
  it("maps signed release artifacts to all supported Tauri targets", async () => {
    artifactNames.forEach((name, index) => {
      writeFileSync(path.join(assetsDir, name), `artifact-${index}`);
      writeFileSync(
        path.join(assetsDir, `${name}.sig`),
        `signature-${index}\n`,
      );
    });

    const scriptUrl = pathToFileURL(
      path.resolve(process.cwd(), "scripts/create-updater-manifest.mjs"),
    ).href;
    const { buildUpdaterManifest } = (await import(scriptUrl)) as {
      buildUpdaterManifest: (options: {
        releaseAssetsDir: string;
        repository: string;
        tag: string;
        pubDate: string;
      }) => {
        version: string;
        pub_date: string;
        platforms: Record<string, { url: string; signature: string }>;
      };
    };

    const manifest = buildUpdaterManifest({
      releaseAssetsDir: assetsDir,
      repository: "example/varyai-releases",
      tag,
      pubDate: "2026-10-09T00:00:00.000Z",
    });

    expect(manifest.version).toBe("1.2.3");
    expect(manifest.pub_date).toBe("2026-10-09T00:00:00.000Z");
    expect(Object.keys(manifest.platforms).sort()).toEqual(
      [
        "darwin-aarch64",
        "darwin-x86_64",
        "linux-aarch64",
        "linux-x86_64",
        "windows-aarch64",
        "windows-x86_64",
      ].sort(),
    );
    expect(manifest.platforms["darwin-aarch64"]).toEqual(
      manifest.platforms["darwin-x86_64"],
    );
    expect(manifest.platforms["windows-aarch64"].signature).toBe("signature-2");
    expect(manifest.platforms["linux-x86_64"].signature).toBe("signature-3");
    expect(manifest.platforms["windows-x86_64"].url).toContain(
      "Vary.AI-v1.2.3-Windows.msi",
    );
  });

  it("writes the manifest as the latest.json release asset", async () => {
    artifactNames.forEach((name, index) => {
      writeFileSync(path.join(assetsDir, name), `artifact-${index}`);
      writeFileSync(path.join(assetsDir, `${name}.sig`), `signature-${index}`);
    });
    const scriptUrl = pathToFileURL(
      path.resolve(process.cwd(), "scripts/create-updater-manifest.mjs"),
    ).href;
    const { writeUpdaterManifest } = (await import(scriptUrl)) as {
      writeUpdaterManifest: (options: {
        releaseAssetsDir: string;
        repository: string;
        tag: string;
        pubDate: string;
      }) => string;
    };

    writeUpdaterManifest({
      releaseAssetsDir: assetsDir,
      repository: "example/varyai-releases",
      tag,
      pubDate: "2026-10-09T00:00:00.000Z",
    });

    const manifest = JSON.parse(
      readFileSync(path.join(assetsDir, "latest.json"), "utf8"),
    );
    expect(manifest.version).toBe("1.2.3");
    expect(manifest.platforms["linux-aarch64"].signature).toBe("signature-4");
  });
});
