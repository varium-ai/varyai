import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { RELEASES_LATEST_URL } from "../../src/config/brand";

const releasePage = "https://github.com/varium-ai/varyai/releases/latest";

describe("official update distribution", () => {
  it("links the renderer to the official release repository", () => {
    expect(RELEASES_LATEST_URL).toBe(releasePage);
  });

  it("checks the updater manifest in the official release repository", () => {
    const config = JSON.parse(
      readFileSync("src-tauri/tauri.conf.json", "utf8"),
    );
    expect(config.plugins.updater.endpoints).toEqual([
      `${releasePage}/download/latest.json`,
    ]);
  });

  it("opens the same repository for manual update checks", () => {
    const source = readFileSync("src-tauri/src/commands/misc.rs", "utf8");
    const command = source.slice(
      source.indexOf("pub async fn check_for_updates("),
      source.indexOf("pub async fn is_portable_mode("),
    );
    expect(command).toContain(`"${releasePage}"`);
  });
});
