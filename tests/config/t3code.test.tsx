import { expect, it } from "vitest";
import {
  APP_IDS,
  APP_ICON_MAP,
  DEFAULT_VISIBLE_APPS,
  MCP_APP_IDS,
  SKILLS_APP_IDS,
} from "@/config/appConfig";
import { subscriptionProviders } from "@/config/subscriptionProviders";

it("keeps T3 Code available after removing the Cursor integrations", () => {
  expect(APP_IDS).toContain("t3code");
  expect(APP_ICON_MAP.t3code.label).toBe("T3 Code");
  expect(DEFAULT_VISIBLE_APPS.t3code).toBe(false);
  expect(MCP_APP_IDS).not.toContain("t3code");
  expect(SKILLS_APP_IDS).not.toContain("t3code");
  for (const app of ["cursor", "cursor-cli"]) {
    expect(APP_IDS).not.toContain(app);
    expect(APP_ICON_MAP).not.toHaveProperty(app);
    expect(DEFAULT_VISIBLE_APPS).not.toHaveProperty(app);
  }
  expect(subscriptionProviders.map((provider) => provider.id)).not.toContain(
    "cursor",
  );
});
