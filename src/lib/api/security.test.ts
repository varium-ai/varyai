import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { securityApi } from "./security";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("securityApi.updateBuiltinRuleAction", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it.each([
    "redact",
    "redact_restore",
    "block",
    "audit",
    "allow",
    null,
  ] as const)("sends %s to the builtin rule action command", async (action) => {
    await securityApi.updateBuiltinRuleAction("builtin-1", action);
    expect(invoke).toHaveBeenCalledWith("update_builtin_security_rule_action", {
      id: "builtin-1",
      action,
    });
  });
});

describe("securityApi.updateCustomRuleAction", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it.each(["redact", "redact_restore", "block", "audit", "allow"] as const)(
    "sends %s to the custom rule action command",
    async (action) => {
      await securityApi.updateCustomRuleAction("custom-1", action);
      expect(invoke).toHaveBeenCalledWith(
        "update_custom_security_rule_action",
        {
          id: "custom-1",
          action,
        },
      );
    },
  );
});
