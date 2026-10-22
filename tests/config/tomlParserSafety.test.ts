import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

describe("TOML parser safety", () => {
  it("rejects an unterminated array comment without hanging the caller", () => {
    const result = spawnSync(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        `import { parse } from "smol-toml";
         try { parse("a=[1 #"); process.exitCode = 2; }
         catch { process.exitCode = 0; }`,
      ],
      { encoding: "utf8", timeout: 3000 },
    );
    expect(result.error).toBeUndefined();
    expect(result.status).toBe(0);
  });
});
