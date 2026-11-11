import { describe, expect, it } from "vitest";
import {
  APP_IDS,
  APP_ICON_MAP,
  DEFAULT_VISIBLE_APPS,
  MCP_APP_IDS,
  SKILLS_APP_IDS,
} from "@/config/appConfig";

describe("additional agent entries", () => {
  it.each(["cursor", "cursor-cli", "t3code"])(
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
});
