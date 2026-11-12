import { describe, expect, it } from "vitest";
import { isTrayPanel, panelTab, sharedQueryRoot } from "@/lib/trayPanel";

describe("tray panel boundaries", () => {
  it("selects the independent renderer only for the tray entry point", () => {
    expect(isTrayPanel("?panel=tray")).toBe(true);
    expect(isTrayPanel("?panel=other")).toBe(false);
    expect(isTrayPanel("")).toBe(false);
  });
  it("only restores the three supported tabs", () => {
    expect(panelTab("routing")).toBe("routing");
    expect(panelTab("security")).toBe("security");
    expect(panelTab("providers")).toBe("agents");
    expect(panelTab(null)).toBe("agents");
  });
  it("only shares invalidation keys, never arbitrary payloads", () => {
    expect(sharedQueryRoot(["security", "settings"])).toBe("security");
    expect(sharedQueryRoot(["securityEvents", "page"])).toBe("securityEvents");
    expect(sharedQueryRoot(["agent-connections"])).toBe("agent-connections");
    expect(sharedQueryRoot(["unrelated", { private: "fixture" }])).toBeNull();
    expect(sharedQueryRoot([{ private: "fixture" }])).toBeNull();
  });
});
