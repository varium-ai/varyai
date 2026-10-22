// @vitest-environment jsdom
import { StrictMode } from "react";
import { act, cleanup, render, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useToolRiskRules } from "@/hooks/useToolRisk";
import { ToolRiskNotifier, shouldNotify } from "./ToolRiskNotifier";
import type { ToolRiskEventRecord, ToolRiskSettings } from "@/types/toolRisk";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  fetchAfter: vi.fn(),
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
  toast: vi.fn(),
  report: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/api", async () => ({
  toolRiskApi: (await import("@/lib/api/toolRisk")).toolRiskApi,
}));
vi.mock("@/lib/api/notifications", () => ({
  isPermissionGranted: mocks.isPermissionGranted,
  requestPermission: mocks.requestPermission,
  sendNotification: mocks.sendNotification,
}));
vi.mock("sonner", () => ({
  toast: { error: mocks.toast, warning: mocks.toast, success: vi.fn() },
}));
vi.mock("@/lib/frontendLogger", () => ({ reportFrontendError: mocks.report }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) =>
      values ? `${key} ${JSON.stringify(values)}` : key,
  }),
}));

const NOW = 1_800_000_000_000;
let settings: ToolRiskSettings;
let records: ToolRiskEventRecord[];
let listeners: Set<() => void>;
let legacyReads: number;

function record(
  overrides: Partial<ToolRiskEventRecord> = {},
): ToolRiskEventRecord {
  return {
    id: 1,
    timestamp: NOW,
    source: "model_response",
    stage: "proposed",
    detectionScope: "in_scope",
    integrityStatus: "complete",
    verdict: "risky",
    riskCategory: "destructive_change",
    severity: "high",
    policyVersion: 1,
    ruleIds: [],
    suppressedCount: 0,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

// Flush bounded IPC/permission microtasks without running pagination continuation timers.
async function flush() {
  await act(async () => {
    for (let i = 0; i < 2_000; i += 1) await Promise.resolve();
  });
}

async function mount(strict = false) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const component = (
    <QueryClientProvider client={queryClient}>
      <ToolRiskNotifier />
    </QueryClientProvider>
  );
  const view = render(
    strict ? <StrictMode>{component}</StrictMode> : component,
  );
  await flush();
  return { ...view, queryClient };
}

async function signal(count = 1) {
  act(() => {
    for (let i = 0; i < count; i += 1) {
      for (const listener of listeners) listener();
    }
  });
  await flush();
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
  await flush();
}

function summaryValues() {
  const call = mocks.toast.mock.calls.find(([, options]) =>
    options.description.startsWith("toolRisk.notifySummaryBody "),
  );
  expect(call, "a window summary should be delivered").toBeDefined();
  return JSON.parse(
    call![1].description.slice("toolRisk.notifySummaryBody ".length),
  );
}

function toastIds() {
  return mocks.toast.mock.calls.map(([, options]) => options.id);
}

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ["Date", "performance", "setTimeout", "clearTimeout"],
  });
  vi.setSystemTime(NOW);
  for (const mock of Object.values(mocks)) mock.mockReset();
  settings = { enabled: true, policyVersion: 1 };
  records = [];
  listeners = new Set();
  legacyReads = 0;
  mocks.listen.mockImplementation(
    async (_name: string, listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  );
  mocks.fetchAfter.mockImplementation(
    async ({ afterId, limit, policyVersion }) =>
      records
        .filter(
          (item) => item.id > afterId && item.policyVersion === policyVersion,
        )
        .sort((a, b) => a.id - b.id)
        .slice(0, limit),
  );
  mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
    if (command === "get_tool_risk_settings") return { ...settings };
    if (command === "get_tool_risk_events_after") return mocks.fetchAfter(args);
    if (command === "get_tool_risk_events") {
      // Stop the old first-page spin so RED reports assertions rather than hanging.
      if (++legacyReads > 4) throw new Error("first-page spin");
      return [...records].sort((a, b) => b.id - a.id).slice(0, 50);
    }
    throw new Error(`Unexpected IPC command: ${command}`);
  });
  mocks.isPermissionGranted.mockResolvedValue(true);
  mocks.requestPermission.mockResolvedValue("granted");
  mocks.sendNotification.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("ToolRiskNotifier.shouldNotify", () => {
  it("notifies only in-scope risky calls", () => {
    expect(shouldNotify(record())).toBe(true);
    for (const verdict of ["unchecked", "uncertain", "low_risk", null]) {
      expect(shouldNotify(record({ verdict }))).toBe(false);
    }
    expect(shouldNotify(record({ detectionScope: "out_of_scope" }))).toBe(
      false,
    );
  });
});

describe("ToolRiskNotifier lifecycle", () => {
  it("does not request permission or read records while disabled", async () => {
    settings.enabled = false;
    await mount();
    await signal();
    expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
    expect(mocks.requestPermission).not.toHaveBeenCalled();
    expect(mocks.fetchAfter).not.toHaveBeenCalled();
    expect(mocks.toast).not.toHaveBeenCalled();
  });

  it("catches up once the real event listener is registered", async () => {
    records = [record()];
    await mount();
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.listen).toHaveBeenCalledWith(
      "tool-risk-recorded",
      expect.any(Function),
    );
  });

  it("terminates for exactly 50 records using an advancing ID cursor", async () => {
    await mount();
    mocks.fetchAfter.mockClear();
    records = Array.from({ length: 50 }, (_, index) =>
      record({ id: index + 1, verdict: "unchecked" }),
    );
    await signal();
    expect(legacyReads).toBe(0);
    expect(mocks.fetchAfter.mock.calls.map(([args]) => args)).toEqual([
      { afterId: 0, limit: 50, policyVersion: 1 },
      { afterId: 50, limit: 50, policyVersion: 1 },
    ]);
  });

  it("reads more than 100 records without dropping older unseen pages", async () => {
    await mount();
    mocks.fetchAfter.mockClear();
    records = Array.from({ length: 125 }, (_, index) =>
      record({ id: index + 1, verdict: index === 124 ? "risky" : "unchecked" }),
    );
    await signal();
    expect(mocks.fetchAfter.mock.calls.map(([args]) => args.afterId)).toEqual([
      0, 50, 100,
    ]);
    expect(toastIds()).toEqual(["tool-risk-125"]);
    await signal();
    expect(mocks.fetchAfter.mock.lastCall?.[0].afterId).toBe(125);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("bounds each pagination turn and resumes without losing the cursor", async () => {
    await mount();
    mocks.fetchAfter.mockClear();
    records = Array.from({ length: 251 }, (_, index) =>
      record({ id: index + 1, verdict: index === 250 ? "risky" : "unchecked" }),
    );
    await signal(100);
    expect(mocks.fetchAfter.mock.calls.map(([args]) => args.afterId)).toEqual([
      0, 50, 100, 150,
    ]);
    expect(mocks.toast).not.toHaveBeenCalled();
    await signal(100);
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(4);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    await flush();
    expect(mocks.fetchAfter.mock.calls.map(([args]) => args.afterId)).toEqual([
      0, 50, 100, 150, 200, 250,
    ]);
    expect(toastIds()).toEqual(["tool-risk-251"]);
  });

  it("stops if a full page does not advance the cursor", async () => {
    await mount();
    mocks.fetchAfter.mockClear();
    mocks.fetchAfter.mockResolvedValue(
      Array.from({ length: 50 }, () => record({ verdict: "unchecked" })),
    );
    await signal();
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(2);
  });

  it("does not replay history even when its database ID is newer", async () => {
    await mount();
    records = [
      record({ id: 1, timestamp: 0 }),
      record({ id: 2, timestamp: NOW - 1 }),
      record({ id: 3, timestamp: NOW }),
    ];
    await signal();
    expect(toastIds()).toEqual(["tool-risk-3"]);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("coalesces concurrent signals and deduplicates delivery", async () => {
    await mount();
    mocks.fetchAfter.mockClear();
    const page = deferred<ToolRiskEventRecord[]>();
    mocks.fetchAfter.mockReturnValueOnce(page.promise);
    await signal();
    await signal(100);
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(1);
    page.resolve([record()]);
    await flush();
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(2);
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("rechecks settings after a page read before delivering", async () => {
    await mount();
    const page = deferred<ToolRiskEventRecord[]>();
    mocks.fetchAfter.mockReturnValueOnce(page.promise);
    await signal();
    settings.enabled = false;
    page.resolve([record()]);
    await flush();
    expect(mocks.toast).not.toHaveBeenCalled();
    expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
  });

  it("filters records by the current policy before asking for permission", async () => {
    await mount();
    mocks.fetchAfter.mockResolvedValueOnce([
      record({ id: 1, policyVersion: 0 }),
      record({ id: 2, policyVersion: 1 }),
    ]);
    records = [record({ id: 1, policyVersion: 0 }), record({ id: 2 })];
    await signal();
    expect(toastIds()).toEqual(["tool-risk-2"]);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it.each(["disabled", "new version"])(
    "invalidates pending permission when policy becomes %s",
    async (change) => {
      mocks.isPermissionGranted.mockResolvedValue(false);
      const permission = deferred<string>();
      mocks.requestPermission.mockReturnValue(permission.promise);
      await mount();
      records = [record()];
      await signal();
      expect(mocks.requestPermission).toHaveBeenCalledTimes(1);
      if (change === "disabled") settings.enabled = false;
      else settings.policyVersion = 2;
      permission.resolve("granted");
      await flush();
      expect(mocks.sendNotification).not.toHaveBeenCalled();
    },
  );

  it("rechecks policy for each system delivery, not just once per page", async () => {
    await mount();
    records = [record(), record({ id: 2, riskCategory: "sensitive_read" })];
    mocks.sendNotification.mockImplementationOnce(async () => {
      settings.policyVersion = 2;
    });
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("keeps application notifications when permission lookup fails", async () => {
    mocks.isPermissionGranted.mockRejectedValue(
      new Error("permission failure"),
    );
    await mount();
    records = [record()];
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.sendNotification).not.toHaveBeenCalled();
    expect(mocks.report).toHaveBeenCalledWith("tool_risk_permission", "failed");
  });

  it("keeps application notifications when requesting permission is rejected", async () => {
    mocks.isPermissionGranted.mockResolvedValue(false);
    mocks.requestPermission.mockRejectedValue(new Error("permission failure"));
    await mount();
    records = [record()];
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.sendNotification).not.toHaveBeenCalled();
  });

  it("degrades independently when system notification delivery fails", async () => {
    mocks.sendNotification.mockRejectedValue(new Error("send failure"));
    await mount();
    records = [record(), record({ id: 2, riskCategory: "sensitive_read" })];
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1"]);
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(2);
    expect(mocks.report).toHaveBeenCalledWith(
      "tool_risk_notification_send",
      "failed",
    );
  });

  it("does not deliver or request permission after unmount during a read", async () => {
    const view = await mount();
    const page = deferred<ToolRiskEventRecord[]>();
    mocks.fetchAfter.mockReturnValueOnce(page.promise);
    await signal();
    view.unmount();
    page.resolve([record()]);
    await flush();
    expect(listeners.size).toBe(0);
    expect(mocks.toast).not.toHaveBeenCalled();
    expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
  });

  it("does not send a system notification after unmount during permission", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    const view = await mount();
    records = [record()];
    await signal();
    view.unmount();
    permission.resolve(true);
    await flush();
    expect(mocks.sendNotification).not.toHaveBeenCalled();
  });

  it("cancels the single scheduled pagination continuation on unmount", async () => {
    const view = await mount();
    mocks.fetchAfter.mockClear();
    records = Array.from({ length: 300 }, (_, index) =>
      record({ id: index + 1, verdict: "unchecked" }),
    );
    await signal();
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(4);
    view.unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(4);
  });

  it("does not block catch-up or counting behind slow permission", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = Array.from({ length: 125 }, (_, index) =>
      record({ id: index + 1 }),
    );
    await signal(100);
    expect(mocks.fetchAfter.mock.calls.map(([args]) => args.afterId)).toContain(
      100,
    );
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(summaryValues()).toMatchObject({ count: 125 });
    expect(mocks.isPermissionGranted).toHaveBeenCalledTimes(1);
    permission.resolve(true);
    await flush();
    expect(mocks.sendNotification).toHaveBeenCalledTimes(2);
  });

  it("uses one global window rather than a per-category budget", async () => {
    await mount();
    records = [
      "destructive_change",
      "sensitive_read",
      "data_exfiltration",
      "remote_code_execution",
      "privilege_change",
    ].map((riskCategory, index) => record({ id: index + 1, riskCategory }));
    await signal();
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(2);
    expect(summaryValues()).toMatchObject({
      count: 5,
      category: "toolRisk.category.privilege_change",
    });
  });

  it("fails closed if settings cannot be read", async () => {
    mocks.invoke.mockRejectedValue(new Error("settings unavailable"));
    await mount();
    await signal();
    expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
    expect(mocks.toast).not.toHaveBeenCalled();
  });

  it("survives StrictMode effect replay with one active event listener", async () => {
    await mount(true);
    records = [record()];
    await signal(10);
    expect(listeners.size).toBe(1);
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });
});

describe("tool risk rule mutation IPC", () => {
  it("forwards expected revisions through real hooks and API to every mutation", async () => {
    mocks.invoke.mockImplementation(async (command) => {
      if (command === "get_tool_risk_rules") return [];
      return {
        value: command === "delete_custom_tool_risk_rule" ? true : null,
        persistedPolicyVersion: 2,
        auditStatus: "recorded",
        runtimeStatus: "applied",
      };
    });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(() => useToolRiskRules(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    await act(async () => {
      await result.current.toggleRule("custom-1", false, 3);
      await result.current.resetBuiltinRule("builtin-1", 7);
      await result.current.deleteRule("custom-1", 3);
    });
    expect(mocks.invoke).toHaveBeenCalledWith("toggle_tool_risk_rule", {
      id: "custom-1",
      enabled: false,
      expectedRevision: 3,
    });
    expect(mocks.invoke).toHaveBeenCalledWith("reset_builtin_tool_risk_rule", {
      id: "builtin-1",
      expectedRevision: 7,
    });
    expect(mocks.invoke).toHaveBeenCalledWith("delete_custom_tool_risk_rule", {
      id: "custom-1",
      expectedRevision: 3,
    });
  });
});

describe("P2-11 notification windows", () => {
  it("summarizes five same-category calls without another refresh signal", async () => {
    await mount();
    records = Array.from({ length: 5 }, (_, index) =>
      record({ id: index + 1 }),
    );
    await signal();
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    await advance(59_999);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(summaryValues()).toMatchObject({ count: 5 });
    await advance(120_000);
    await signal(20);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(2);
  });

  it("does not summarize a single call and starts only on the next risky", async () => {
    await mount();
    records = [record()];
    await signal();
    await advance(120_000);
    await signal(20);
    expect(toastIds()).toEqual(["tool-risk-1"]);
    records.push(record({ id: 2, timestamp: Date.now() }));
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-2"]);
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
  });

  it("deduplicates IDs and counts only current in-scope risky records", async () => {
    await mount();
    mocks.fetchAfter.mockResolvedValueOnce([
      record(),
      record(),
      record({ id: 2, verdict: "uncertain" }),
      record({ id: 3, detectionScope: "out_of_scope" }),
      record({ id: 4, policyVersion: 0 }),
      record({ id: 5 }),
    ]);
    await signal(100);
    await advance(60_000);
    expect(summaryValues()).toMatchObject({ count: 2 });
    expect(mocks.toast).toHaveBeenCalledTimes(2);
  });

  it("uses monotonic time despite a wall-clock jump", async () => {
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(20_000);
    vi.setSystemTime(NOW + 3_600_000);
    records.push(record({ id: 3, timestamp: Date.now() }));
    await signal();
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    await advance(39_999);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(summaryValues()).toMatchObject({ count: 3 });
  });

  it.each(["disabled", "new version", "unmount"])(
    "cancels a tail on %s without a refresh signal",
    async (change) => {
      const view = await mount();
      records = [record(), record({ id: 2 })];
      await signal();
      if (change === "disabled") settings.enabled = false;
      else if (change === "new version") settings.policyVersion = 2;
      else view.unmount();
      await advance(60_000);
      expect(mocks.toast).toHaveBeenCalledTimes(1);
      expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
    },
  );

  it("fails closed on an expiry-time settings read failure", async () => {
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    mocks.invoke.mockRejectedValue(new Error("private error text"));
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("restarts counting for a newly active policy", async () => {
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(20_000);
    settings.policyVersion = 2;
    records.push(
      record({ id: 3, policyVersion: 2 }),
      record({ id: 4, policyVersion: 2 }),
    );
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-3"]);
    await advance(40_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    await advance(20_000);
    expect(summaryValues()).toMatchObject({ count: 2 });
  });

  it("never sends arbitrary categories, severity or command summaries", async () => {
    await mount();
    records = [
      record({
        riskCategory: "SECRET-category",
        severity: "SECRET-severity",
        commandSummary: "SECRET-command",
      }),
      record({
        id: 2,
        riskCategory: "SECRET-category",
        severity: "SECRET-severity",
      }),
    ];
    await signal();
    await advance(60_000);
    const payloads = JSON.stringify([
      mocks.toast.mock.calls,
      mocks.sendNotification.mock.calls,
    ]);
    expect(payloads).not.toContain("SECRET");
    expect(payloads).toContain("toolRisk.notifyUnknownCategory");
    expect(payloads).toContain("toolRisk.notifyUnknownSeverity");
  });
});

describe("P2-11 bounded OS queue", () => {
  it("drops expired pending notifications instead of replaying after permission", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(70_001);
    permission.resolve(true);
    await flush();
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(mocks.sendNotification).not.toHaveBeenCalled();
    expect(mocks.isPermissionGranted).toHaveBeenCalledTimes(1);
  });

  it("serializes sends across windows and drops the oldest of more than two pending entries", async () => {
    const sending = deferred<void>();
    mocks.sendNotification.mockReturnValueOnce(sending.promise);
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(60_000);
    records.push(record({ id: 3, timestamp: Date.now() }));
    await signal();
    records.push(record({ id: 4, timestamp: Date.now() }));
    await signal();
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(4);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
    expect(mocks.report).toHaveBeenCalledWith(
      "tool_risk_notification_queue",
      "dropped_oldest",
    );
    sending.resolve();
    await flush();
    expect(mocks.sendNotification).toHaveBeenCalledTimes(3);
  });

  it("keeps a single permission task across delayed windows", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(60_000);
    records.push(record({ id: 3, timestamp: Date.now() }));
    await signal();
    await advance(60_000);
    records.push(record({ id: 4, timestamp: Date.now() }));
    await signal();
    expect(mocks.toast).toHaveBeenCalledTimes(4);
    expect(mocks.isPermissionGranted).toHaveBeenCalledTimes(1);
    permission.resolve(true);
    await flush();
    // Window 2's first delivery is still within its 10-second grace period;
    // window 3's first delivery is also fresh. Only window 1 has expired.
    expect(mocks.sendNotification).toHaveBeenCalledTimes(2);
  });

  it("rechecks policy after a slow send before starting queued deliveries", async () => {
    const sending = deferred<void>();
    mocks.sendNotification.mockReturnValueOnce(sending.promise);
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(60_000);
    settings.enabled = false;
    sending.resolve();
    await flush();
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("silently suppresses OS delivery on permission denial but still summarizes in-app", async () => {
    mocks.isPermissionGranted.mockResolvedValue(false);
    mocks.requestPermission.mockResolvedValue("denied");
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    expect(mocks.sendNotification).not.toHaveBeenCalled();
    expect(mocks.requestPermission).toHaveBeenCalledTimes(1);
  });
});

describe("P2-11 asynchronous boundaries", () => {
  it("cancels immediately on a settings-cache success, without trusting it to enable delivery", async () => {
    const { queryClient } = await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    act(() => {
      queryClient.setQueryData(["toolRisk", "settings"], {
        enabled: false,
        policyVersion: 1,
      });
    });
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    // The IPC policy is still enabled; cache cancellation cannot invent its state.
    records.push(record({ id: 3 }));
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-3"]);
    queryClient.clear();
  });

  it("does not revive an old tail after disable and re-enable within the same version", async () => {
    const { queryClient } = await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    act(() => {
      queryClient.setQueryData(["toolRisk", "settings"], {
        enabled: false,
        policyVersion: 1,
      });
    });
    act(() => {
      queryClient.setQueryData(["toolRisk", "settings"], {
        enabled: true,
        policyVersion: 1,
      });
    });
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    queryClient.clear();
  });

  it("completes the old window snapshot before delivering the next window", async () => {
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    const expirySettings = deferred<ToolRiskSettings>();
    const original = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementationOnce(() => expirySettings.promise);
    await advance(60_000);
    records.push(record({ id: 3, timestamp: Date.now() }));
    await signal();
    expect(mocks.toast).toHaveBeenCalledTimes(1);
    expirySettings.resolve({ ...settings });
    mocks.invoke.mockImplementation(original);
    await flush();
    expect(toastIds()).toEqual([
      "tool-risk-1",
      expect.stringContaining("tool-risk-window"),
      "tool-risk-3",
    ]);
    expect(summaryValues()).toMatchObject({ count: 2 });
  });

  it("uses the new policy after an old permission check completes without reopening permissions", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = [record()];
    await signal();
    settings.policyVersion = 2;
    records.push(record({ id: 2, policyVersion: 2 }));
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-2"]);
    permission.resolve(true);
    await flush();
    expect(mocks.isPermissionGranted).toHaveBeenCalledTimes(1);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("uses fresh queued items rather than the item evicted during OS lookup", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(60_000);
    records.push(record({ id: 3 }));
    await signal();
    await advance(1_000);
    permission.resolve(false);
    await flush();
    expect(mocks.requestPermission).toHaveBeenCalledTimes(1);
    // The evicted first entry is never delivered; its summary and next window
    // remain fresh and share the same mount-wide permission task.
    expect(mocks.sendNotification).toHaveBeenCalledTimes(2);
    expect(mocks.sendNotification.mock.calls[0][0].body).toContain(
      "toolRisk.notifySummaryBody",
    );
  });

  it("clears queued content on unmount during a slow send", async () => {
    const send = deferred<void>();
    mocks.sendNotification.mockReturnValueOnce(send.promise);
    const view = await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(60_000);
    view.unmount();
    send.resolve();
    await flush();
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
  });
});

describe("P2-11 cancelled expiry isolation", () => {
  it("does not let an old expiry IPC block a new policy window", async () => {
    const { queryClient } = await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    const oldExpiry = deferred<ToolRiskSettings>();
    mocks.invoke.mockImplementationOnce(() => oldExpiry.promise);
    await advance(60_000);
    settings.policyVersion = 2;
    act(() => {
      queryClient.setQueryData(["toolRisk", "settings"], { ...settings });
    });
    records.push(record({ id: 3, policyVersion: 2 }));
    await signal();
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-3"]);
    oldExpiry.resolve({ enabled: true, policyVersion: 1 });
    await flush();
    await advance(60_000);
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    queryClient.clear();
  });
});

describe("P2-11 stale settings catch-up", () => {
  it("preserves a new-policy signal received during an invalidated settings read", async () => {
    const { queryClient } = await mount();
    records = [record()];
    await signal();
    const oldSettings = deferred<ToolRiskSettings>();
    mocks.invoke.mockImplementationOnce(() => oldSettings.promise);
    await signal();
    settings.policyVersion = 2;
    act(() => {
      queryClient.setQueryData(["toolRisk", "settings"], { ...settings });
    });
    records.push(record({ id: 2, policyVersion: 2 }));
    await signal();
    oldSettings.resolve({ enabled: true, policyVersion: 1 });
    await flush();
    await advance(250);
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-2"]);
    queryClient.clear();
  });

  it("requests permission for a live new-policy item after the old lookup completes", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = [record()];
    await signal();
    settings.policyVersion = 2;
    records.push(
      record({ id: 2, policyVersion: 2, riskCategory: "sensitive_read" }),
    );
    await signal();
    permission.resolve(false);
    await flush();
    expect(mocks.isPermissionGranted).toHaveBeenCalledTimes(1);
    expect(mocks.requestPermission).toHaveBeenCalledTimes(1);
    expect(mocks.sendNotification).toHaveBeenCalledTimes(1);
    expect(mocks.sendNotification.mock.calls[0][0].body).toContain(
      "toolRisk.category.sensitive_read",
    );
  });
});

describe("P2-11 cancellation wakes pending ingress", () => {
  it("wakes ingress already waiting on an old expiry before a cache cancellation", async () => {
    const { queryClient } = await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    const expiry = deferred<ToolRiskSettings>();
    mocks.invoke.mockImplementationOnce(() => expiry.promise);
    await advance(60_000);
    records.push(record({ id: 3 }));
    await signal();
    settings.policyVersion = 2;
    act(() => {
      queryClient.setQueryData(["toolRisk", "settings"], { ...settings });
    });
    records.push(record({ id: 4, policyVersion: 2 }));
    await signal();
    await advance(250);
    expect(toastIds()).toEqual(["tool-risk-1", "tool-risk-4"]);
    expiry.resolve({ enabled: true, policyVersion: 1 });
    await flush();
    expect(mocks.toast).toHaveBeenCalledTimes(2);
    queryClient.clear();
  });

  it("does not start a permission request after every queued item expires", async () => {
    const permission = deferred<boolean>();
    mocks.isPermissionGranted.mockReturnValue(permission.promise);
    await mount();
    records = [record(), record({ id: 2 })];
    await signal();
    await advance(70_001);
    permission.resolve(false);
    await flush();
    expect(mocks.requestPermission).not.toHaveBeenCalled();
  });
});

describe("P2-11 final application-delivery fence", () => {
  it("rechecks cancellation in the caller after a successful async policy check", async () => {
    const { queryClient } = await mount();
    const original = mocks.invoke.getMockImplementation()!;
    let settingsReads = 0;
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "get_tool_risk_settings" && ++settingsReads === 3) {
        let reads = 0;
        return {
          enabled: true,
          get policyVersion() {
            // readSettings compares once, then isCurrentPolicy checks the version.
            if (++reads === 2)
              void Promise.resolve().then(() => {
                queryClient.setQueryData(["toolRisk", "settings"], {
                  enabled: false,
                  policyVersion: 1,
                });
              });
            return 1;
          },
        };
      }
      return original(command, args);
    });
    records = [record()];
    await signal();
    expect(mocks.toast).not.toHaveBeenCalled();
    expect(mocks.sendNotification).not.toHaveBeenCalled();
    queryClient.clear();
  });
});

describe("P2-11 cancellation listener lifecycle", () => {
  it("releases every expiry cancellation listener after normal completion", async () => {
    const add = vi.spyOn(AbortSignal.prototype, "addEventListener");
    const remove = vi.spyOn(AbortSignal.prototype, "removeEventListener");
    try {
      await mount();
      records = [record(), record({ id: 2 })];
      await signal();
      const expiry = deferred<ToolRiskSettings>();
      mocks.invoke.mockImplementationOnce(() => expiry.promise);
      await advance(60_000);
      const start = add.mock.calls.length;
      records.push(record({ id: 3 }));
      await signal();
      const added = add.mock.calls
        .slice(start)
        .filter(([name]) => name === "abort");
      expect(added).toHaveLength(1);
      expiry.resolve({ ...settings });
      await flush();
      for (const [name, listener] of added) {
        expect(remove).toHaveBeenCalledWith(name, listener);
      }
      expect(toastIds()).toEqual([
        "tool-risk-1",
        expect.stringContaining("tool-risk-window"),
        "tool-risk-3",
      ]);
    } finally {
      add.mockRestore();
      remove.mockRestore();
    }
  });
});

describe("P2-11 concurrent enable signal", () => {
  it("does not consume a new signal behind an old disabled snapshot without cache updates", async () => {
    settings.enabled = false;
    await mount();
    const disabled = deferred<ToolRiskSettings>();
    mocks.invoke.mockImplementationOnce(() => disabled.promise);
    await signal();
    settings = { enabled: true, policyVersion: 2 };
    records = [record({ policyVersion: 2 })];
    await signal();
    disabled.resolve({ enabled: false, policyVersion: 1 });
    await flush();
    await advance(250);
    expect(toastIds()).toEqual(["tool-risk-1"]);
    expect(mocks.fetchAfter).toHaveBeenCalledTimes(1);
  });
});
