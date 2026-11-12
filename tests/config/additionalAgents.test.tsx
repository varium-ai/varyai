import { describe, expect, it } from "vitest";
import {
  APP_IDS,
  APP_ICON_MAP,
  DEFAULT_VISIBLE_APPS,
  MCP_APP_IDS,
  SKILLS_APP_IDS,
} from "@/config/appConfig";

describe("additional agent entries", () => {
  it.each(["cursor", "t3code"])(
    "registers %s without advertising unsupported legacy integrations",
    (app) => {
      expect(APP_IDS).toContain(app);
      expect(APP_ICON_MAP["t3code" as keyof typeof APP_ICON_MAP]?.label).toBe(
        "T3 Code",
      );
      expect(DEFAULT_VISIBLE_APPS).toHaveProperty(app, false);
      expect(MCP_APP_IDS).not.toContain(app);
      expect(SKILLS_APP_IDS).not.toContain(app);
    },
  );
  it("groups Cursor desktop and CLI under one public entry", () => {
    expect(APP_IDS).toContain("cursor");
    expect(APP_IDS).not.toContain("cursor-cli");
    expect(APP_ICON_MAP.cursor.label).toBe("Cursor");
  });
});
