import { describe, expect, it } from "vitest";
import {
  APP_IDS,
  DEFAULT_VISIBLE_APPS,
  isTakeoverAppId,
  isManagedAgentApp,
  MCP_APP_IDS,
  SKILLS_APP_IDS,
} from "@/config/appConfig";

describe("Doubao Work capabilities", () => {
  it("offers opt-in official message auditing without unsupported model or tool configuration", () => {
    expect(APP_IDS).toContain("doubaowork");
    expect(DEFAULT_VISIBLE_APPS).toHaveProperty("doubaowork", false);
    expect(isTakeoverAppId("doubaowork")).toBe(true);
    expect(isManagedAgentApp("doubaowork")).toBe(false);
    expect(MCP_APP_IDS).not.toContain("doubaowork");
    expect(SKILLS_APP_IDS).not.toContain("doubaowork");
  });
});
