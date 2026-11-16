import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();

describe("desktop analytics security boundary", () => {
  it("does not grant the Google tag native plugin capabilities", () => {
    const folder = path.join(root, "src-tauri/capabilities");
    for (const file of readdirSync(folder).filter((name) =>
      name.endsWith(".json"),
    )) {
      const capability = JSON.parse(
        readFileSync(path.join(folder, file), "utf8"),
      );
      const patterns = [
        ...(capability.windows ?? []),
        ...(capability.webviews ?? []),
      ] as string[];
      for (const pattern of patterns) {
        const regex = new RegExp(
          `^${pattern.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*")}$`,
        );
        expect(
          regex.test("analytics"),
          `${file} must not authorize the analytics WebView`,
        ).toBe(false);
      }
    }
  });

  it("keeps the main application script policy restricted to bundled code", () => {
    const config = JSON.parse(
      readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8"),
    );
    const script = (config.app.security.csp as string)
      .split(";")
      .find((part) => part.trim().startsWith("script-src"))!;
    expect(script).toContain("'self'");
    expect(script).not.toContain("googletagmanager");
    expect(script).not.toContain("https:");
    expect(
      readFileSync(path.join(root, "src/index.html"), "utf8"),
    ).not.toContain("gtag");
  });
});
