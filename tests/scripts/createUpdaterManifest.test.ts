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
  it("embeds the supplied Markdown release notes instead of a placeholder", async () => {
    artifactNames.forEach((name) => {
      writeFileSync(path.join(assetsDir, name), "fixture");
      writeFileSync(path.join(assetsDir, `${name}.sig`), "signature");
    });
    const scriptUrl = pathToFileURL(
      path.resolve(process.cwd(), "scripts/create-updater-manifest.mjs"),
    ).href;
    const { buildUpdaterManifest } = await import(scriptUrl);
    const notes =
      "## New features\n\n- Add routing groups.\n\n## Fixes\n\n- Keep saved accounts.";
    expect(
      buildUpdaterManifest({
        releaseAssetsDir: assetsDir,
        repository: "example/varyai-releases",
        tag,
        notes,
      }).notes,
    ).toBe(notes);
  });

  it("includes a staged release-notes.md file in the generated manifest", async () => {
    artifactNames.forEach((name) => {
      writeFileSync(path.join(assetsDir, name), "fixture");
      writeFileSync(path.join(assetsDir, `${name}.sig`), "signature");
    });
    const notes = "## Improvements\n\n- Display release notes.";
    writeFileSync(path.join(assetsDir, "release-notes.md"), notes);
    const scriptUrl = pathToFileURL(
      path.resolve(process.cwd(), "scripts/create-updater-manifest.mjs"),
    ).href;
    const { buildUpdaterManifest } = await import(scriptUrl);
    expect(
      buildUpdaterManifest({
        releaseAssetsDir: assetsDir,
        repository: "example/varyai-releases",
        tag,
      }).notes,
    ).toBe(notes);
  });
  it("uses an explicitly selected notes file and rejects ambiguous input", async () => {
    artifactNames.forEach((name) => {
      writeFileSync(path.join(assetsDir, name), "fixture");
      writeFileSync(path.join(assetsDir, `${name}.sig`), "signature");
    });
    const notesFile = path.join(assetsDir, "selected-notes.md");
    writeFileSync(notesFile, "## Fixes\n\n- Keep update actions available.");
    const scriptUrl = pathToFileURL(
      path.resolve(process.cwd(), "scripts/create-updater-manifest.mjs"),
    ).href;
    const { buildUpdaterManifest } = await import(scriptUrl);
    const options = {
      releaseAssetsDir: assetsDir,
      repository: "example/varyai-releases",
      tag,
      notesFile,
    };
    expect(buildUpdaterManifest(options).notes).toContain(
      "Keep update actions available.",
    );
    expect(() =>
      buildUpdaterManifest({ ...options, notes: "Other notes" }),
    ).toThrow("not both");
  });
});
