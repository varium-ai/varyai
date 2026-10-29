import { describe, expect, it } from "vitest";
import { getCacheWriteAvailability } from "@/types/usage";

describe("getCacheWriteAvailability", () => {
  it("keeps reported cache creation visible for clients using other upstream protocols", () => {
    expect(getCacheWriteAvailability(["codex"], 1200)).toBe("partial");
    expect(getCacheWriteAvailability(["gemini"], 1200)).toBe("partial");
    expect(getCacheWriteAvailability(["claude"], 1200)).toBe("ok");
  });
  it("distinguishes cache-write support across fixed protocols", () => {
    expect(getCacheWriteAvailability(["claude"])).toBe("ok");
    expect(getCacheWriteAvailability(["pi"])).toBe("partial");
    expect(getCacheWriteAvailability(["codex", "gemini"])).toBe("na");
    expect(getCacheWriteAvailability(["claude", "codex"])).toBe("partial");
    expect(getCacheWriteAvailability([])).toBe("ok");
  });
});
