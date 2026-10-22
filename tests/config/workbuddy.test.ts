import { describe, expect, it } from "vitest";
import {
  APP_IDS,
  DEFAULT_VISIBLE_APPS,
  isAdditiveAppId,
  isMcpAppId,
  isProxyAppId,
  SKILLS_APP_IDS,
} from "@/config/appConfig";

describe("WorkBuddy application capabilities", () => {
  it("exposes native model configuration without advertising unsupported integrations", () => {
    expect(APP_IDS).toContain("workbuddy");
    expect(DEFAULT_VISIBLE_APPS).toHaveProperty("workbuddy", false);
    expect(DEFAULT_VISIBLE_APPS).toHaveProperty("qoder", false);
    expect(isAdditiveAppId("workbuddy")).toBe(true);
    expect(isProxyAppId("workbuddy")).toBe(false);
    expect(isMcpAppId("workbuddy")).toBe(false);
    expect(SKILLS_APP_IDS).not.toContain("workbuddy");
  });
});
