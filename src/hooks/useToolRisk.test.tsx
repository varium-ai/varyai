import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useToolRiskCommandLists,
  useToolRiskEvents,
  useToolRiskSettings,
  useToolRiskSandbox,
  useToolRiskPolicyChanges,
  useToolRiskRules,
  toolRiskKeys,
} from "./useToolRisk";
import { invoke } from "@tauri-apps/api/core";
import { toolRiskApi as realApi } from "@/lib/api/toolRisk";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const api = vi.hoisted(() => ({
  getCommandLists: vi.fn().mockResolvedValue([]),
  saveCommandList: vi.fn().mockResolvedValue({
    value: "entry-1",
    persistedPolicyVersion: 2,
    auditStatus: "recorded",
    runtimeStatus: "applied",
  }),
  deleteCommandList: vi.fn().mockResolvedValue({
    value: true,
    persistedPolicyVersion: 2,
    auditStatus: "recorded",
    runtimeStatus: "applied",
  }),
  previewCommandPattern: vi.fn().mockResolvedValue([]),
  getRules: vi.fn().mockResolvedValue([]),
  saveCustomRule: vi.fn(),
  deleteCustomRule: vi.fn(),
  toggleRule: vi.fn(),
  resetBuiltinRule: vi.fn(),
  getSettings: vi.fn().mockResolvedValue({ enabled: true, policyVersion: 1 }),
  updateSettings: vi.fn(),
  getPolicyChanges: vi.fn().mockResolvedValue([]),
  clearPolicyChanges: vi.fn(),
  getEvents: vi.fn().mockResolvedValue([]),
  clearEvents: vi.fn().mockResolvedValue(0),
  testRuleMatch: vi.fn(),
}));
vi.mock("@/lib/api", () => ({ toolRiskApi: api }));
const notices = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: notices }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("command list cache", () => {
  it("invalidates lists, settings, events and rules after mutations and revision conflicts", async () => {
    const client = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useToolRiskCommandLists(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
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
    await act(async () => {
      await result.current.saveEntry(input);
    });
    expect(api.saveCommandList).toHaveBeenCalledWith(input);
    expect(invalidate).toHaveBeenLastCalledWith({ queryKey: toolRiskKeys.all });
    await act(async () => {
      await result.current.deleteEntry("entry-1", 8);
    });
    expect(api.deleteCommandList).toHaveBeenCalledWith("entry-1", 8);
    expect(invalidate).toHaveBeenCalledTimes(2);
    api.saveCommandList.mockRejectedValueOnce("revision conflict");
    await act(async () => {
      await expect(result.current.saveEntry(input)).rejects.toBe(
        "revision conflict",
      );
    });
    expect(invalidate).toHaveBeenCalledTimes(3);
  });
});

describe("custom rule conflict messages", () => {
  it.each([
    ["tool_risk_builtin_duplicate", "toolRisk.builtinDuplicate"],
    ["tool_risk_risk_severity_required", "toolRisk.riskSeverityRequired"],
  ])("translates the backend policy code %s", async (code, key) => {
    const client = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useToolRiskRules(), { wrapper });
    const input = {
      name: "copy",
      enabled: true,
      selector: {
        target: { kind: "operation" as const, types: ["shell_exec" as const] },
      },
      conditions: [
        { field: "/command", op: "keyword" as const, value: "danger" },
      ],
      riskCategory: "remote_code_execution" as const,
      severity: "high" as const,
      reasonCode: "remote_exec_chain",
      localReview: "off" as const,
    };
    api.saveCustomRule.mockRejectedValueOnce(`database error: ${code}`);
    await act(async () => {
      await expect(result.current.saveRule(input)).rejects.toContain(code);
    });
    expect(notices.error).toHaveBeenLastCalledWith(
      `toolRisk.ruleSaveFailed${key}`,
    );
    client.clear();
  });
});

describe("event verdict query cache", () => {
  function setup() {
    const client = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    return { client, wrapper };
  }

  it("keeps every verdict and unfiltered query in separate cache entries", async () => {
    const { client, wrapper } = setup();
    api.getEvents.mockClear();
    const { result, unmount } = renderHook(
      () => [
        useToolRiskEvents(50, 0),
        useToolRiskEvents(50, 0, "risky"),
        useToolRiskEvents(50, 0, "unchecked"),
        useToolRiskEvents(50, 0, "uncertain"),
        useToolRiskEvents(50, 0, "low_risk"),
      ],
      { wrapper },
    );
    await waitFor(() =>
      expect(result.current.every((query) => !query.isLoading)).toBe(true),
    );
    expect(api.getEvents).toHaveBeenCalledWith(50, 0);
    for (const verdict of [
      "risky",
      "unchecked",
      "uncertain",
      "low_risk",
    ] as const) {
      expect(api.getEvents).toHaveBeenCalledWith(50, 0, verdict);
    }
    expect(api.getEvents).toHaveBeenCalledTimes(5);
    expect(
      client
        .getQueryCache()
        .findAll({ queryKey: [...toolRiskKeys.all, "events"] }),
    ).toHaveLength(5);
    unmount();
    client.clear();
  });

  it("switches the active verdict and refreshes only that query", async () => {
    const { client, wrapper } = setup();
    api.getEvents.mockClear();
    const { result, rerender, unmount } = renderHook(
      ({ verdict }: { verdict: "risky" | "uncertain" }) =>
        useToolRiskEvents(50, 50, verdict),
      { wrapper, initialProps: { verdict: "risky" } },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    rerender({ verdict: "uncertain" });
    await waitFor(() =>
      expect(api.getEvents).toHaveBeenCalledWith(50, 50, "uncertain"),
    );
    api.getEvents.mockClear();
    await act(async () => {
      await result.current.refetch();
    });
    expect(api.getEvents).toHaveBeenCalledTimes(1);
    expect(api.getEvents).toHaveBeenCalledWith(50, 50, "uncertain");
    unmount();
    client.clear();
  });

  it("clears all history and invalidates every filtered event query", async () => {
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result, unmount } = renderHook(
      () => useToolRiskEvents(50, 0, "risky"),
      { wrapper },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      await result.current.clearEvents();
    });
    expect(api.clearEvents).toHaveBeenLastCalledWith(undefined);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: toolRiskKeys.all });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["security"] });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["securityEvents", "summary"],
      refetchType: "active",
    });
    unmount();
    client.clear();
  });
});

const ruleInput = {
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
const listInput = {
  listType: "whitelist" as const,
  matchKind: "prefix" as const,
  pattern: "git status",
  enabled: true,
  severity: "none" as const,
  riskCategory: "sensitive_read" as const,
};
const settingsInput = { enabled: true, policyVersion: 1 };
const writes = [
  [
    "saveCustomRule",
    "save_custom_tool_risk_rule",
    "custom-1",
    { input: ruleInput },
  ],
  [
    "deleteCustomRule",
    "delete_custom_tool_risk_rule",
    false,
    { id: "custom-1", expectedRevision: 3 },
  ],
  [
    "toggleRule",
    "toggle_tool_risk_rule",
    null,
    { id: "custom-1", enabled: false, expectedRevision: 3 },
  ],
  [
    "resetBuiltinRule",
    "reset_builtin_tool_risk_rule",
    null,
    { id: "builtin.sensitive_read", expectedRevision: 4 },
  ],
  [
    "saveCommandList",
    "save_tool_risk_command_list",
    "entry-1",
    { input: listInput },
  ],
  [
    "deleteCommandList",
    "delete_tool_risk_command_list",
    true,
    { id: "entry-1", expectedRevision: 5 },
  ],
  [
    "updateSettings",
    "update_tool_risk_settings",
    null,
    { settings: settingsInput },
  ],
] as const;
function receiptSetup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = renderHook(
    () => ({
      rules: useToolRiskRules(),
      lists: useToolRiskCommandLists(),
      settings: useToolRiskSettings(),
    }),
    { wrapper },
  );
  const run = (name: (typeof writes)[number][0]) => {
    const h = hook.result.current;
    switch (name) {
      case "saveCustomRule":
        return h.rules.saveRule(ruleInput);
      case "deleteCustomRule":
        return h.rules.deleteRule("custom-1", 3);
      case "toggleRule":
        return h.rules.toggleRule("custom-1", false, 3);
      case "resetBuiltinRule":
        return h.rules.resetBuiltinRule("builtin.sensitive_read", 4);
      case "saveCommandList":
        return h.lists.saveEntry(listInput);
      case "deleteCommandList":
        return h.lists.deleteEntry("entry-1", 5);
      case "updateSettings":
        return h.settings.updateSettings(settingsInput);
    }
  };
  return { client, invalidate, hook, run };
}

describe("P2-9 real API receipt consumption", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const [name] of writes)
      api[name].mockImplementation(realApi[name] as never);
  });
  it.each(writes)(
    "unwraps %s and preserves the real IPC inputs",
    async (name, command, value, args) => {
      vi.mocked(invoke).mockResolvedValue({
        value,
        persistedPolicyVersion: 2,
        auditStatus: "not_needed",
        runtimeStatus: "applied",
      });
      const { client, invalidate, hook, run } = receiptSetup();
      await act(async () => {
        expect(await run(name)).toBe(value === null ? undefined : value);
      });
      expect(invoke).toHaveBeenCalledWith(command, args);
      expect(invalidate).toHaveBeenCalledWith({ queryKey: toolRiskKeys.all });
      expect(notices.warning).not.toHaveBeenCalled();
      hook.unmount();
      client.clear();
    },
  );
  it.each(writes)(
    "warns without losing the saved value for %s",
    async (name, _command, value) => {
      vi.mocked(invoke).mockResolvedValue({
        value,
        persistedPolicyVersion: 2,
        auditStatus: "audit_unavailable",
        runtimeStatus: "apply_failed",
        error: "secret token",
      });
      const { client, invalidate, hook, run } = receiptSetup();
      await act(async () => {
        expect(await run(name)).toBe(value === null ? undefined : value);
      });
      expect(notices.warning.mock.calls).toEqual([
        ["toolRisk.savedNotApplied"],
        ["toolRisk.auditUnavailable"],
      ]);
      expect(notices.success).not.toHaveBeenCalled();
      expect(notices.error).not.toHaveBeenCalled();
      expect(invalidate.mock.invocationCallOrder[0]).toBeLessThan(
        notices.warning.mock.invocationCallOrder[0],
      );
      hook.unmount();
      client.clear();
    },
  );
  it.each(["apply_failed", "audit_unavailable"])(
    "reports %s independently",
    async (state) => {
      vi.mocked(invoke).mockResolvedValue({
        value: "custom-1",
        persistedPolicyVersion: 2,
        auditStatus: state === "audit_unavailable" ? state : "recorded",
        runtimeStatus: state === "apply_failed" ? state : "applied",
      });
      const { client, hook, run } = receiptSetup();
      await act(async () => {
        await run("saveCustomRule");
      });
      expect(notices.warning).toHaveBeenCalledTimes(1);
      expect(notices.warning).toHaveBeenCalledWith(
        state === "apply_failed"
          ? "toolRisk.savedNotApplied"
          : "toolRisk.auditUnavailable",
      );
      expect(notices.success).not.toHaveBeenCalled();
      hook.unmount();
      client.clear();
    },
  );
  it.each(writes)(
    "does not echo unknown rejection text for %s",
    async (name) => {
      vi.mocked(invoke).mockRejectedValue(
        new Error("secret token and pattern"),
      );
      const { client, hook, run } = receiptSetup();
      await act(async () => {
        await expect(run(name)).rejects.toThrow("secret token");
      });
      expect(notices.error).toHaveBeenCalled();
      expect(JSON.stringify(notices.error.mock.calls)).not.toMatch(
        /secret|token|pattern/,
      );
      expect(notices.warning).not.toHaveBeenCalled();
      hook.unmount();
      client.clear();
    },
  );
});

describe("P2-9 policy history hook", () => {
  it("pages and clears via real API, exposing loading/error/refresh state", async () => {
    api.getPolicyChanges.mockImplementation(realApi.getPolicyChanges);
    api.clearPolicyChanges.mockImplementation(realApi.clearPolicyChanges);
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "clear_tool_risk_policy_changes" ? 20 : [],
    );
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result, rerender, unmount } = renderHook(
      ({ offset }) => useToolRiskPolicyChanges(50, offset),
      { wrapper, initialProps: { offset: 0 } },
    );
    expect(result.current.isLoading).toBe(true);
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.isError).toBe(false);
    expect(result.current.changes).toEqual([]);
    rerender({ offset: 50 });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_tool_risk_policy_changes", {
        limit: 50,
        offset: 50,
      }),
    );
    await act(async () => {
      await result.current.refetch();
      expect(await result.current.clearChanges()).toBe(20);
    });
    expect(invoke).toHaveBeenCalledWith("clear_tool_risk_policy_changes");
    vi.mocked(invoke).mockRejectedValue(new Error("secret history"));
    await act(async () => {
      await expect(result.current.clearChanges()).rejects.toThrow();
    });
    expect(notices.error).toHaveBeenLastCalledWith(
      "toolRisk.policyChanges.clearFailed",
    );
    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    unmount();
    client.clear();
  });
});

describe("P2-9 safe auxiliary errors", () => {
  it.each(["events", "sandbox"])(
    "does not echo %s error text",
    async (kind) => {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
      const { result, unmount } = renderHook(
        () => ({
          events: useToolRiskEvents(50, 0),
          sandbox: useToolRiskSandbox(),
        }),
        { wrapper },
      );
      api.clearEvents.mockRejectedValueOnce(new Error("secret raw event"));
      api.testRuleMatch.mockRejectedValueOnce(new Error("secret raw sandbox"));
      await act(async () => {
        await expect(
          kind === "events"
            ? result.current.events.clearEvents()
            : result.current.sandbox.test({ toolName: "Bash", input: {} }),
        ).rejects.toThrow("secret");
      });
      expect(notices.error).toHaveBeenLastCalledWith(
        kind === "events"
          ? "toolRisk.eventsClearFailed"
          : "toolRisk.sandboxFailed",
      );
      unmount();
      client.clear();
    },
  );
});
