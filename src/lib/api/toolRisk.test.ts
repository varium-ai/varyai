import { beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue("id-1");
});
import { invoke } from "@tauri-apps/api/core";
import { toolRiskApi } from "./toolRisk";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue("id-1"),
}));

describe("command list IPC", () => {
  it("uses the command list protocol and preserves revisions", async () => {
    const input = {
      id: "entry-1",
      expectedRevision: 8,
      listType: "blacklist" as const,
      matchKind: "prefix" as const,
      pattern: "git push",
      enabled: false,
      severity: "high" as const,
      riskCategory: "data_exfiltration" as const,
    };
    await toolRiskApi.getCommandLists();
    expect(invoke).toHaveBeenLastCalledWith("get_tool_risk_command_lists");
    await toolRiskApi.saveCommandList(input);
    expect(invoke).toHaveBeenLastCalledWith("save_tool_risk_command_list", {
      input,
    });
    await toolRiskApi.deleteCommandList("entry-1", 8);
    expect(invoke).toHaveBeenLastCalledWith("delete_tool_risk_command_list", {
      id: "entry-1",
      expectedRevision: 8,
    });
    await toolRiskApi.previewCommandPattern("regex", "^git");
    expect(invoke).toHaveBeenLastCalledWith(
      "preview_tool_risk_command_pattern",
      { matchKind: "regex", pattern: "^git" },
    );
  });
});

describe("event verdict query IPC", () => {
  it("preserves the unfiltered legacy arguments", async () => {
    await toolRiskApi.getEvents(50, 100);
    expect(invoke).toHaveBeenLastCalledWith("get_tool_risk_events", {
      limit: 50,
      offset: 100,
    });
  });

  it.each(["risky", "unchecked", "uncertain", "low_risk"] as const)(
    "passes the optional %s filter before paging",
    async (verdict) => {
      await toolRiskApi.getEvents(50, 100, verdict);
      expect(invoke).toHaveBeenLastCalledWith("get_tool_risk_events", {
        limit: 50,
        offset: 100,
        verdict,
      });
    },
  );

  it("leaves notification ID-cursor reads unchanged", async () => {
    await toolRiskApi.getEventsAfter(101, 50, 8);
    expect(invoke).toHaveBeenLastCalledWith("get_tool_risk_events_after", {
      afterId: 101,
      limit: 50,
      policyVersion: 8,
    });
  });
});

const policyChange = {
  id: 1,
  at: 1_700_000_000_000,
  policyVersion: 8,
  actor: "local_ui",
  kind: "rule_update",
  targetId: "custom-12345678-1234-4234-8234-123456789abc",
  changedFields: ["enabled", "conditions"],
  auditStatus: "recorded",
};

describe("P2-9 policy history IPC", () => {
  it("reads metadata using the single paginated read command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([policyChange]);
    expect(await toolRiskApi.getPolicyChanges(50, 100)).toEqual([policyChange]);
    expect(invoke).toHaveBeenLastCalledWith("get_tool_risk_policy_changes", {
      limit: 50,
      offset: 100,
    });
  });
  it("clears local history without parameters", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(42);
    expect(await toolRiskApi.clearPolicyChanges()).toBe(42);
    expect(invoke).toHaveBeenLastCalledWith("clear_tool_risk_policy_changes");
  });
  it("projects arbitrary database text to fixed metadata without extra values", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        ...policyChange,
        actor: "secret actor",
        kind: "secret kind",
        targetId: "builtin.secret",
        auditStatus: "secret status",
        changedFields: ["pattern", "secret field", "pattern"],
        before: "secret before",
        after: "secret after",
      },
    ]);
    const rows = await toolRiskApi.getPolicyChanges(50, 0);
    expect(rows).toEqual([
      {
        ...policyChange,
        actor: "unknown",
        kind: "unknown",
        targetId: "<unknown_target>",
        auditStatus: "unknown",
        changedFields: ["pattern"],
      },
    ]);
    expect(JSON.stringify(rows)).not.toContain("secret");
  });
  it.each([
    "builtin.destructive_change",
    "builtin.sensitive_read",
    "builtin.data_exfiltration",
    "builtin.remote_code_execution",
    "builtin.privilege_change",
    "builtin.security_config_change",
    "builtin.destructive_database_change",
    "custom-12345678-1234-4234-8234-123456789abc",
    "shell-list-12345678-1234-4234-8234-123456789abc",
    null,
  ])("accepts only canonical target %s", async (targetId) => {
    vi.mocked(invoke).mockResolvedValueOnce([{ ...policyChange, targetId }]);
    expect((await toolRiskApi.getPolicyChanges(50, 0))[0].targetId).toBe(
      targetId,
    );
  });
  it.each([
    "custom-old",
    "custom-12345678-1234-4234-8234-123456789ABC",
    "shell-list-12345678-1234-4234-8234-123456789abc-secret",
    "builtin.unknown",
    "<secret>",
    9,
  ])("rejects noncanonical target %s", async (targetId) => {
    vi.mocked(invoke).mockResolvedValueOnce([{ ...policyChange, targetId }]);
    expect((await toolRiskApi.getPolicyChanges(50, 0))[0].targetId).toBe(
      "<unknown_target>",
    );
  });
  it("ignores malformed rows and non-array fields", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      null,
      "secret",
      { ...policyChange, id: "secret" },
      { ...policyChange, at: "secret" },
      { ...policyChange, policyVersion: "secret" },
      { ...policyChange, changedFields: "secret" },
    ]);
    expect(await toolRiskApi.getPolicyChanges(50, 0)).toEqual([
      { ...policyChange, changedFields: [] },
    ]);
    vi.mocked(invoke).mockResolvedValueOnce("secret");
    expect(await toolRiskApi.getPolicyChanges(50, 0)).toEqual([]);
  });
});

describe("P2-9 structured write receipts", () => {
  it("keeps all seven write command names and input objects unchanged", async () => {
    const rule = {
      name: "rule",
      enabled: false,
      selector: {
        target: { kind: "operation" as const, types: ["shell_exec" as const] },
      },
      conditions: [],
      riskCategory: "sensitive_read" as const,
      severity: "high" as const,
      reasonCode: "sensitive_target",
      localReview: "off" as const,
    };
    const list = {
      listType: "whitelist" as const,
      matchKind: "prefix" as const,
      pattern: "git status",
      enabled: true,
      severity: "none" as const,
      riskCategory: "sensitive_read" as const,
    };
    const settings = { enabled: true, policyVersion: 8 };
    const operations = [
      [
        () => toolRiskApi.saveCustomRule(rule),
        "save_custom_tool_risk_rule",
        { input: rule },
        "custom-1",
      ],
      [
        () => toolRiskApi.saveCommandList(list),
        "save_tool_risk_command_list",
        { input: list },
        "entry-1",
      ],
      [
        () => toolRiskApi.deleteCustomRule("custom-1", 3),
        "delete_custom_tool_risk_rule",
        { id: "custom-1", expectedRevision: 3 },
        true,
      ],
      [
        () => toolRiskApi.deleteCommandList("entry-1", 4),
        "delete_tool_risk_command_list",
        { id: "entry-1", expectedRevision: 4 },
        false,
      ],
      [
        () => toolRiskApi.toggleRule("custom-1", false, 3),
        "toggle_tool_risk_rule",
        { id: "custom-1", enabled: false, expectedRevision: 3 },
        null,
      ],
      [
        () => toolRiskApi.resetBuiltinRule("builtin.sensitive_read", 7),
        "reset_builtin_tool_risk_rule",
        { id: "builtin.sensitive_read", expectedRevision: 7 },
        null,
      ],
      [
        () => toolRiskApi.updateSettings(settings),
        "update_tool_risk_settings",
        { settings },
        null,
      ],
    ] as const;
    for (const [run, command, args, value] of operations) {
      const receipt = {
        value,
        persistedPolicyVersion: 9,
        auditStatus: "audit_unavailable",
        runtimeStatus: "apply_failed",
      };
      vi.mocked(invoke).mockResolvedValueOnce(receipt);
      expect(await run()).toEqual(receipt);
      expect(invoke).toHaveBeenLastCalledWith(command, args);
    }
  });
});
