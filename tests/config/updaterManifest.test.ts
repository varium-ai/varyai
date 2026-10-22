import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

function runModule(source: string) {
  return spawnSync(process.execPath, ["--input-type=module", "-e", source], {
    encoding: "utf8",
  });
}

describe("public updater manifest", () => {
  it("uses one universal macOS archive for both macOS architectures", () => {
    const directory = mkdtempSync(join(tmpdir(), "varyai-macos-manifest-"));
    try {
      const archive = join(
        directory,
        "Vary AI-v0.2.2-macOS-Universal.app.tar.gz",
      );
      writeFileSync(archive, "fixture archive");
      writeFileSync(`${archive}.sig`, "signature\n");
      const result = runModule(`
        import { buildUpdaterManifest } from './scripts/create-updater-manifest.mjs';
        console.log(JSON.stringify(buildUpdaterManifest({
          releaseAssetsDir:${JSON.stringify(directory)},
          repository:'example/varyai-releases', tag:'v0.2.2', platforms:['macos'],
        })));
      `);
      expect(result.status, result.stderr).toBe(0);
      const manifest = JSON.parse(result.stdout);
      expect(Object.keys(manifest.platforms)).toEqual([
        "darwin-aarch64",
        "darwin-x86_64",
      ]);
      expect(manifest.platforms["darwin-aarch64"]).toEqual(
        manifest.platforms["darwin-x86_64"],
      );
      expect(manifest.platforms["darwin-aarch64"].signature).toBe("signature");
      expect(manifest.platforms["darwin-aarch64"].url).toBe(
        "https://github.com/example/varyai-releases/releases/download/v0.2.2/Vary.AI-v0.2.2-macOS-Universal.app.tar.gz",
      );
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it.each([{ platforms: [] }, { platforms: ["unknown"] }])(
    "rejects invalid platform selection %j before looking for artifacts",
    ({ platforms }) => {
      const result = runModule(`
        import { buildUpdaterManifest } from './scripts/create-updater-manifest.mjs';
        buildUpdaterManifest({releaseAssetsDir:'/not-used',
          repository:'example/varyai-releases', tag:'v0.2.2',
          platforms:${JSON.stringify(platforms)}});
      `);
      expect(result.status).not.toBe(0);
      expect(result.stderr).toContain("platform");
      expect(result.stderr).not.toContain("Missing updater artifact");
    },
  );

  it("requires the selected artifact's signature", () => {
    const directory = mkdtempSync(join(tmpdir(), "varyai-updater-signature-"));
    try {
      writeFileSync(
        join(directory, "Vary AI-v0.2.2-macOS-Universal.app.tar.gz"),
        "fixture archive",
      );
      const result = runModule(`
        import { buildUpdaterManifest } from './scripts/create-updater-manifest.mjs';
        buildUpdaterManifest({releaseAssetsDir:${JSON.stringify(directory)},
          repository:'example/varyai-releases',tag:'v0.2.2',platforms:['macos']});
      `);
      expect(result.status).not.toBe(0);
      expect(result.stderr).toContain("Missing updater signature");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it("can generate a manifest for Windows x64 without its unselected sibling", () => {
    const directory = mkdtempSync(join(tmpdir(), "varyai-selected-updater-"));
    try {
      const archive = join(directory, "Vary AI-v0.2.2-Windows.msi");
      writeFileSync(archive, "fixture installer");
      writeFileSync(`${archive}.sig`, "signature");
      const result = runModule(`
        import { buildUpdaterManifest } from './scripts/create-updater-manifest.mjs';
        const manifest=buildUpdaterManifest({releaseAssetsDir:${JSON.stringify(directory)},
          repository:'example/varyai-releases',tag:'v0.2.2',targets:['windows-x86_64']});
        console.log(JSON.stringify(manifest));
      `);
      expect(result.status, result.stderr).toBe(0);
      const manifest = JSON.parse(result.stdout);
      expect(Object.keys(manifest.platforms)).toEqual(["windows-x86_64"]);
      expect(manifest.platforms["windows-x86_64"].url).toMatch(/Windows\.msi$/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it("uses the NSIS installer and its matching signature when explicitly selected", () => {
    const directory = mkdtempSync(join(tmpdir(), "varyai-nsis-updater-"));
    try {
      const installer = join(directory, "Vary AI-v0.2.3-Windows-setup.exe");
      writeFileSync(installer, "fixture NSIS installer");
      writeFileSync(`${installer}.sig`, "nsis-signature\n");
      const result = runModule(`
        import { buildUpdaterManifest } from './scripts/create-updater-manifest.mjs';
        console.log(JSON.stringify(buildUpdaterManifest({
          releaseAssetsDir:${JSON.stringify(directory)},repository:'example/varyai-releases',
          tag:'v0.2.3',targets:['windows-x86_64'],windowsInstallers:{'windows-x86_64':'nsis'},
        })));
      `);
      expect(result.status, result.stderr).toBe(0);
      const manifest = JSON.parse(result.stdout);
      expect(Object.keys(manifest.platforms)).toEqual(["windows-x86_64"]);
      expect(manifest.platforms["windows-x86_64"]).toEqual({
        url: "https://github.com/example/varyai-releases/releases/download/v0.2.3/Vary.AI-v0.2.3-Windows-setup.exe",
        signature: "nsis-signature",
      });
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
