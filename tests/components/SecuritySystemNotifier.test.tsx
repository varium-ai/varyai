import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StrictMode } from "react";
import { act, render, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SecuritySystemNotifier } from "@/components/SecuritySystemNotifier";
import type { AuditLogRecord } from "@/types/security";

const { toastMock } = vi.hoisted(() => ({
  toastMock: { warning: vi.fn(), info: vi.fn(), error: vi.fn() },
}));
vi.mock("sonner", () => ({ toast: toastMock }));
const listenMock = vi.fn();
const logMock = vi.fn();
const sendMock = vi.fn();
const permissionMock = vi.fn();
const requestMock = vi.fn();
const batchMock = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));
vi.mock("@/lib/api/notifications", () => ({
  isPermissionGranted: () => permissionMock(),
  requestPermission: () => requestMock(),
  sendNotification: (opts: unknown) => sendMock(opts),
}));
vi.mock("@/lib/api/security", () => ({
  securityApi: {
    getAuditNotificationBatch: (...args: unknown[]) => batchMock(...args),
  },
}));
vi.mock("@/lib/frontendLogger", () => ({
  reportFrontendError: (...args: unknown[]) => logMock(...args),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
function record(
  id: number,
  overrides: Partial<AuditLogRecord> = {},
): AuditLogRecord {
  return {
    id,
    timestamp: Date.now(),
    appType: "claude",
    providerId: "test",
    ruleId: `rule-${id}`,
    ruleName: `rule-${id}`,
    ruleCategory: "secret",
    riskLevel: "HIGH",
    actionTaken: "block",
    matchedSnippetMasked: "masked",
    ...overrides,
  };
}
function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const view = render(
    <QueryClientProvider client={client}>
      <SecuritySystemNotifier />
    </QueryClientProvider>,
  );
  return { ...view, invalidate };
}
let emit: () => void;
function event() {
  act(() => emit());
}

describe("SecuritySystemNotifier", () => {
  it("notifies tokenization without claiming response restoration", async () => {
    batchMock
      .mockResolvedValueOnce([record(1, { actionTaken: "redact_restore" })])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(sendMock.mock.calls[0][0].title).toBe(
      "Sensitive Information Tokenized",
    );
    expect(sendMock.mock.calls[0][0].body).toContain(
      "response restoration is not confirmed",
    );
  });

  beforeEach(() => {
    vi.spyOn(Date, "now").mockReturnValue(123456);
    listenMock.mockReset();
    logMock.mockReset();
    sendMock.mockReset();
    permissionMock.mockReset();
    requestMock.mockReset();
    batchMock.mockReset();
    toastMock.warning.mockReset();
    toastMock.info.mockReset();
    toastMock.error.mockReset();
    listenMock.mockImplementation(async (_event, cb) => {
      emit = cb;
      return vi.fn();
    });
    permissionMock.mockResolvedValue(true);
    requestMock.mockResolvedValue("granted");
    batchMock.mockResolvedValue([]);
  });
  // restore Date.now before the next test file
  afterEach(() => vi.restoreAllMocks());

  it("先成功订阅再首读，挂载毫秒边界包含；事件使日志刷新", async () => {
    const subscribe = deferred<() => void>();
    listenMock.mockImplementation((_event, cb) => {
      emit = cb;
      return subscribe.promise;
    });
    batchMock
      .mockResolvedValueOnce([record(1, { timestamp: 123456 })])
      .mockResolvedValue([]);
    const { invalidate } = setup();
    expect(batchMock).not.toHaveBeenCalled();
    await act(async () => subscribe.resolve(vi.fn()));
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(batchMock).toHaveBeenCalledWith(0, 123456, 100);
    event();
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["security"] }),
    );
    expect(sendMock).toHaveBeenCalledTimes(1);
  });
  it("StrictMode effect 重放仍可完成当前会话首读", async () => {
    batchMock.mockResolvedValueOnce([record(1)]).mockResolvedValue([]);
    const client = new QueryClient();
    render(
      <StrictMode>
        <QueryClientProvider client={client}>
          <SecuritySystemNotifier />
        </QueryClientProvider>
      </StrictMode>,
    );
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
  });

  it("StrictMode 权限拒绝只记录当前会话一次", async () => {
    permissionMock.mockResolvedValue(false);
    requestMock.mockResolvedValue("denied");
    const client = new QueryClient();
    render(
      <StrictMode>
        <QueryClientProvider client={client}>
          <SecuritySystemNotifier />
        </QueryClientProvider>
      </StrictMode>,
    );
    await waitFor(() => expect(requestMock).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(logMock).toHaveBeenCalledWith(
        "security_audit_permission_denied",
        "denied",
      ),
    );
    expect(
      logMock.mock.calls.filter(
        ([context]) => context === "security_audit_permission_denied",
      ),
    ).toHaveLength(1);
  });

  it("100 条一页逐页补到尾；不同类别及同一行高危优先，只通知一次", async () => {
    const first = Array.from({ length: 100 }, (_, index) =>
      record(index + 1, { riskLevel: "LOW", actionTaken: "redact" }),
    );
    first[0] = record(1, { actionTaken: "audit" });
    first[1] = record(2, { riskLevel: "MEDIUM", actionTaken: "audit" });
    batchMock
      .mockResolvedValueOnce(first)
      .mockResolvedValueOnce([
        record(101, { riskLevel: "CRITICAL", actionTaken: "audit" }),
      ]);
    setup();
    await waitFor(() => expect(batchMock).toHaveBeenCalledTimes(2));
    expect(batchMock.mock.calls).toEqual([
      [0, 123456, 100],
      [100, 123456, 100],
    ]);
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(101));
    expect(sendMock.mock.calls.slice(0, 3).map(([opts]) => opts.title)).toEqual(
      [
        "Sensitive Information Detected",
        "Sensitive Information Detected",
        "Sensitive Information Hidden",
      ],
    );
    expect(sendMock.mock.calls[100][0].title).toBe(
      "Sensitive Information Detected",
    );
    event();
    await waitFor(() =>
      expect(batchMock).toHaveBeenCalledWith(101, 123456, 100),
    );
    expect(sendMock).toHaveBeenCalledTimes(101);
  });

  it("按真实动作通知所有风险等级，脱敏和仅记录也各弹一次", async () => {
    batchMock
      .mockResolvedValueOnce([
        record(1, { riskLevel: "MEDIUM", actionTaken: "redact" }),
        record(2, { riskLevel: "LOW", actionTaken: "audit" }),
        record(3, { riskLevel: "HIGH", actionTaken: "redact" }),
        record(4, { riskLevel: "LOW", actionTaken: "block" }),
      ])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(4));
    expect(sendMock.mock.calls.map(([opts]) => opts.title)).toEqual([
      "Sensitive Information Hidden",
      "Sensitive Information Detected",
      "Sensitive Information Hidden",
      "Request Blocked",
    ]);
  });

  it("不处理策略不产生应用内或系统通知，但仍推进事件游标", async () => {
    batchMock
      .mockResolvedValueOnce([
        record(1, { actionTaken: "allow" }),
        record(2, { actionTaken: "audit" }),
      ])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(toastMock.error).not.toHaveBeenCalled();
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
    expect(toastMock.info).not.toHaveBeenCalled();
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(2, 123456, 100));
    expect(sendMock).toHaveBeenCalledTimes(1);
  });

  it("不同动作的事件分别提示，应用内和系统通知均不重复处理同一事件", async () => {
    batchMock
      .mockResolvedValueOnce([
        record(1, {
          riskLevel: "MEDIUM",
          actionTaken: "redact",
          matchedSnippetMasked: "138****8000",
        }),
        record(2, {
          riskLevel: "HIGH",
          actionTaken: "block",
          appType: "codex",
          matchedSnippetMasked: "sk-****",
        }),
        record(3, {
          riskLevel: "LOW",
          actionTaken: "audit",
          matchedSnippetMasked: "192.168.*.*",
        }),
      ])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(3));
    expect(toastMock.warning).toHaveBeenCalledWith(
      "Sensitive Information Hidden",
      expect.objectContaining({
        description:
          "Claude request detected “rule-1 (MEDIUM)”. Hidden before sending.",
        id: expect.any(String),
      }),
    );
    expect(toastMock.error).toHaveBeenCalledWith(
      "Request Blocked",
      expect.objectContaining({
        description:
          "Codex request detected “rule-2 (HIGH)”. Request not sent.",
        id: expect.any(String),
      }),
    );
    expect(toastMock.warning).toHaveBeenCalledWith(
      "Sensitive Information Detected",
      {
        description:
          "Claude request detected “rule-3 (LOW)”. Request sent unchanged.",
        id: expect.any(String),
        closeButton: true,
      },
    );
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(3, 123456, 100));
    expect(toastMock.warning).toHaveBeenCalledTimes(2);
    expect(toastMock.error).toHaveBeenCalledTimes(1);
    expect(toastMock.info).not.toHaveBeenCalled();
  });

  it("系统通知权限被拒也显示应用内提示，且不会卡住游标", async () => {
    permissionMock.mockResolvedValue(false);
    requestMock.mockResolvedValue("denied");
    batchMock
      .mockResolvedValueOnce([record(1, { actionTaken: "redact" })])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    expect(sendMock).not.toHaveBeenCalled();
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(1, 123456, 100));
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
  });

  it("系统权限还在等待时，应用内提示不等权限就显示", async () => {
    const pending = deferred<boolean>();
    permissionMock.mockReturnValue(pending.promise);
    batchMock.mockResolvedValueOnce([record(1, { actionTaken: "redact" })]);
    setup();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    expect(sendMock).not.toHaveBeenCalled();
    await act(async () => pending.resolve(true));
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
  });

  it("系统权限等待期间同一批次及后续事件仍逐条显示应用内提示", async () => {
    const pending = deferred<boolean>();
    permissionMock.mockReturnValue(pending.promise);
    batchMock
      .mockResolvedValueOnce([
        record(1, { actionTaken: "redact" }),
        record(2, { actionTaken: "audit" }),
      ])
      .mockResolvedValueOnce([record(3, { actionTaken: "block" })])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(2));
    event();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(toastMock.error).toHaveBeenCalledTimes(1));
    expect(sendMock).not.toHaveBeenCalled();
    await act(async () => pending.resolve(true));
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(3));
  });

  it("系统通知发送未完成也不拦截后续事件的应用内提示", async () => {
    const pending = deferred<void>();
    sendMock.mockReturnValueOnce(pending.promise);
    batchMock
      .mockResolvedValueOnce([record(1), record(2, { actionTaken: "redact" })])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    expect(sendMock).toHaveBeenCalledTimes(2);
    await act(async () => pending.resolve());
    expect(sendMock).toHaveBeenCalledTimes(2);
  });

  it("应用内提示抛错只记脱敏错误，后续事件及系统通知仍正常", async () => {
    toastMock.warning.mockImplementationOnce(() => {
      throw new Error("secret-toast-value");
    });
    batchMock.mockResolvedValueOnce([
      record(1, { actionTaken: "redact" }),
      record(2, { actionTaken: "audit" }),
    ]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(2));
    expect(toastMock.warning).toHaveBeenCalledTimes(2);
    expect(toastMock.info).not.toHaveBeenCalled();
    expect(logMock).toHaveBeenCalledWith(
      "security_audit_toast",
      "failed",
      "id=1",
    );
    expect(JSON.stringify(logMock.mock.calls)).not.toContain(
      "secret-toast-value",
    );
  });

  it("读页时信号到达只标 dirty，末页后从已处理 ID 再读", async () => {
    const page = deferred<AuditLogRecord[]>();
    batchMock
      .mockReturnValueOnce(page.promise)
      .mockResolvedValueOnce([record(2)])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(batchMock).toHaveBeenCalledTimes(1));
    event();
    event();
    expect(batchMock).toHaveBeenCalledTimes(1);
    await act(async () => page.resolve([record(1)]));
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(1, 123456, 100));
    expect(sendMock).toHaveBeenCalledTimes(2);
  });

  it("失败不前进也不自旋，下一个事件重试；日志不含异常秘密或 matchedSnippetMasked", async () => {
    batchMock
      .mockRejectedValueOnce(new Error("secret-fetch-custom"))
      .mockResolvedValueOnce([
        record(1, { matchedSnippetMasked: "secret-masked-custom" }),
      ]);
    setup();
    await waitFor(() =>
      expect(logMock).toHaveBeenCalledWith(
        "security_audit_notification_fetch",
        "failed",
        "sinceId=0 limit=100",
      ),
    );
    expect(batchMock).toHaveBeenCalledTimes(1);
    event();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(batchMock).toHaveBeenNthCalledWith(2, 0, 123456, 100);
    expect(JSON.stringify(logMock.mock.calls)).not.toMatch(
      /secret-fetch-custom|secret-masked-custom/,
    );
  });

  it("通知同步异常或 Promise 拒绝不拦截后续行且不重新发该行", async () => {
    sendMock
      .mockImplementationOnce(() => {
        throw new Error("secret-send-custom");
      })
      .mockRejectedValueOnce(new Error("secret-rejection-custom"));
    batchMock
      .mockResolvedValueOnce([record(1), record(2), record(3)])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(logMock).toHaveBeenCalledTimes(2));
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(3, 123456, 100));
    expect(sendMock).toHaveBeenCalledTimes(3);
    expect(JSON.stringify(logMock.mock.calls)).not.toMatch(
      /secret-send-custom|secret-rejection-custom/,
    );
  });

  it("拒绝系统通知权限时仍显示应用内提示，记录一次并推进游标", async () => {
    const permission = deferred<boolean>();
    permissionMock.mockReturnValue(permission.promise);
    requestMock.mockResolvedValue("denied");
    batchMock.mockResolvedValueOnce([record(1)]).mockResolvedValue([]);
    setup();
    await waitFor(() => expect(batchMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(toastMock.error).toHaveBeenCalledTimes(1));
    expect(sendMock).not.toHaveBeenCalled();
    await act(async () => permission.resolve(false));
    await waitFor(() =>
      expect(logMock).toHaveBeenCalledWith(
        "security_audit_permission_denied",
        "denied",
      ),
    );
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(1, 123456, 100));
    expect(sendMock).not.toHaveBeenCalled();
    expect(toastMock.error).toHaveBeenCalledTimes(1);
    expect(requestMock).toHaveBeenCalledTimes(1);
  });

  it("权限延迟授予时先显示应用内提示，授予后再发送系统通知", async () => {
    const pending = deferred<boolean>();
    permissionMock.mockReturnValue(pending.promise);
    batchMock.mockResolvedValueOnce([
      record(1, { riskLevel: "MEDIUM", actionTaken: "audit" }),
    ]);
    setup();
    await waitFor(() => expect(batchMock).toHaveBeenCalledTimes(1));
    expect(sendMock).not.toHaveBeenCalled();
    await act(async () => pending.resolve(true));
    await waitFor(() =>
      expect(sendMock).toHaveBeenCalledWith({
        title: "Sensitive Information Detected",
        body: "Claude request detected “rule-1 (MEDIUM)”. Request sent unchanged.",
      }),
    );
    expect(requestMock).not.toHaveBeenCalled();
  });

  it("权限请求失败固定日志，仍刷新查询；卸载后过期请求不能通知", async () => {
    permissionMock.mockResolvedValue(false);
    requestMock.mockRejectedValue(new Error("secret-permission-custom"));
    const page = deferred<AuditLogRecord[]>();
    batchMock.mockReturnValueOnce(page.promise);
    const { invalidate, unmount } = setup();
    await waitFor(() =>
      expect(logMock).toHaveBeenCalledWith(
        "security_audit_permission",
        "failed",
      ),
    );
    event();
    await waitFor(() => expect(invalidate).toHaveBeenCalled());
    unmount();
    await act(async () => page.resolve([record(1)]));
    expect(sendMock).not.toHaveBeenCalled();
    expect(JSON.stringify(logMock.mock.calls)).not.toContain(
      "secret-permission-custom",
    );
  });
  it("同应用同规则的不同号码只发一条应用内和系统提醒，游标仍处理全部事件", async () => {
    batchMock
      .mockResolvedValueOnce([
        record(1, {
          appType: "codex",
          ruleId: "chinese_phone_number",
          riskLevel: "MEDIUM",
          actionTaken: "audit",
          matchedSnippetMasked: "first-masked-value",
        }),
        record(2, {
          appType: "codex",
          ruleId: "chinese_phone_number",
          riskLevel: "MEDIUM",
          actionTaken: "audit",
          matchedSnippetMasked: "second-masked-value",
        }),
      ])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(2, 123456, 100));
    expect(sendMock).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(sendMock.mock.calls)).not.toMatch(
      /first-masked-value|second-masked-value/,
    );
  });

  it("同类提醒跨批次限频，满一分钟再次提醒且复用旧提示位置", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const phone = {
      appType: "codex",
      ruleId: "chinese_phone_number",
      riskLevel: "MEDIUM",
      actionTaken: "audit",
    } as const;
    batchMock.mockResolvedValueOnce([record(1, phone)]).mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    const toastId = toastMock.warning.mock.calls[0][1].id;
    now = 59_999;
    batchMock.mockResolvedValueOnce([record(2, phone)]);
    event();
    await waitFor(() => expect(batchMock).toHaveBeenCalledWith(1, 123456, 100));
    expect(sendMock).toHaveBeenCalledTimes(1);
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
    now = 60_000;
    batchMock.mockResolvedValueOnce([record(3, phone)]);
    event();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(2));
    expect(batchMock).toHaveBeenCalledWith(2, 123456, 100);
    expect(toastMock.warning.mock.calls[1][1].id).toBe(toastId);
  });

  it("同类提醒跨分页仍只提醒一次，分页游标推进到末尾", async () => {
    const phone = {
      ruleId: "chinese_phone_number",
      actionTaken: "audit",
    } as const;
    batchMock
      .mockResolvedValueOnce(
        Array.from({ length: 100 }, (_, i) => record(i + 1, phone)),
      )
      .mockResolvedValueOnce([record(101, phone)])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(batchMock).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
    expect(toastMock.warning).toHaveBeenCalledTimes(1);
    event();
    await waitFor(() =>
      expect(batchMock).toHaveBeenCalledWith(101, 123456, 100),
    );
  });

  it("动作或风险等级改变立即提醒，不被同类限频吞掉", async () => {
    const phone = {
      ruleId: "chinese_phone_number",
      actionTaken: "audit",
      riskLevel: "MEDIUM",
    } as const;
    batchMock
      .mockResolvedValueOnce([
        record(1, phone),
        record(2, { ...phone, actionTaken: "block" }),
        record(3, { ...phone, riskLevel: "HIGH" }),
        record(4, { ...phone, appType: "pi" }),
        record(5, { ...phone, ruleId: "other-rule" }),
      ])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(5));
    expect(toastMock.error).toHaveBeenCalledTimes(1);
    expect(toastMock.warning).toHaveBeenCalledTimes(4);
  });

  it("权限等待期间已合并同类提醒，授予后只发送一条系统通知", async () => {
    const pending = deferred<boolean>();
    permissionMock.mockReturnValue(pending.promise);
    const phone = {
      ruleId: "chinese_phone_number",
      actionTaken: "audit",
    } as const;
    batchMock
      .mockResolvedValueOnce([record(1, phone), record(2, phone)])
      .mockResolvedValue([]);
    setup();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    expect(sendMock).not.toHaveBeenCalled();
    await act(async () => pending.resolve(true));
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
  });

  it("权限跨过限频窗口才授予时，不集中补发已过期的同类通知", async () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    const pending = deferred<boolean>();
    permissionMock.mockReturnValue(pending.promise);
    const phone = {
      ruleId: "chinese_phone_number",
      actionTaken: "audit",
    } as const;
    batchMock.mockResolvedValueOnce([record(1, phone)]).mockResolvedValue([]);
    setup();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(1));
    now = 60_000;
    batchMock.mockResolvedValueOnce([record(2, phone)]);
    event();
    await waitFor(() => expect(toastMock.warning).toHaveBeenCalledTimes(2));
    await act(async () => pending.resolve(true));
    await waitFor(() => expect(sendMock).toHaveBeenCalledTimes(1));
  });
});
