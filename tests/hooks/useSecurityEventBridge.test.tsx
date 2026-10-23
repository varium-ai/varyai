import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useSecurityEventBridge } from "@/hooks/useSecurityEventBridge";
import { securityKeys } from "@/hooks/useSecurityAudit";

const listenMock = vi.fn();
const logMock = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listenMock(...args) }));
vi.mock("@/lib/frontendLogger", () => ({ reportFrontendError: (...args: unknown[]) => logMock(...args) }));

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
    return { promise, resolve, reject };
}

function Bridge({ onRecorded }: { onRecorded: () => void }) {
    useSecurityEventBridge(onRecorded);
    return null;
}

function setup(onRecorded = vi.fn()) {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const view = render(<QueryClientProvider client={client}><Bridge onRecorded={onRecorded} /></QueryClientProvider>);
    return { ...view, client, invalidate, onRecorded };
}

describe("useSecurityEventBridge", () => {
    beforeEach(() => { listenMock.mockReset(); logMock.mockReset(); });

    it("订阅成功后首读，纯事件 invalidate 全部安全查询并补读；回调更新不重复订阅", async () => {
        const off = vi.fn();
        let emit!: () => void;
        listenMock.mockImplementation(async (_name, cb) => { emit = cb; return off; });
        const first = vi.fn();
const { client, invalidate, rerender, unmount } = setup(first);
        await waitFor(() => expect(first).toHaveBeenCalledTimes(1));
        expect(listenMock).toHaveBeenCalledWith("security-audit-recorded", expect.any(Function));
        expect(invalidate).toHaveBeenCalledWith({ queryKey: securityKeys.all });
        const second = vi.fn();
        rerender(<QueryClientProvider client={client}><Bridge onRecorded={second} /></QueryClientProvider>);
        expect(listenMock).toHaveBeenCalledTimes(1);
        act(() => emit());
        await waitFor(() => expect(second).toHaveBeenCalledTimes(1));
        expect(invalidate).toHaveBeenCalledWith({ queryKey: securityKeys.all });
        unmount();
        expect(off).toHaveBeenCalled();
    });

    it("订阅完成时失效安全缓存，弥补订阅前丢失的日志页信号", async () => {
        const pending = deferred<() => void>();
        listenMock.mockReturnValue(pending.promise);
        const { client, invalidate, onRecorded } = setup();
        client.setQueryData([...securityKeys.all, "logs", "open"], { items: [], total: 0 });
        expect(invalidate).not.toHaveBeenCalled();
        await act(async () => pending.resolve(vi.fn()));
        await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(1));
        expect(invalidate).toHaveBeenCalledWith({ queryKey: securityKeys.all });
    });

    it("卸载时未完成的订阅会立即 off，不补读", async () => {
        const pending = deferred<() => void>();
        const off = vi.fn();
        listenMock.mockReturnValue(pending.promise);
        const { unmount, onRecorded } = setup();
        unmount();
        await act(async () => pending.resolve(off));
        expect(off).toHaveBeenCalledTimes(1);
        expect(onRecorded).not.toHaveBeenCalled();
    });

    it("订阅失败仅记固定安全日志，不产生 unhandled rejection", async () => {
        const secret = "private-custom-fragment-987";
        listenMock.mockRejectedValue(new Error(secret));
        const { onRecorded } = setup();
        await waitFor(() => expect(logMock).toHaveBeenCalledWith("security_audit_listen", "failed"));
        expect(onRecorded).not.toHaveBeenCalled();
        expect(JSON.stringify(logMock.mock.calls)).not.toContain(secret);
    });

    it("invalidate 失败仍触发补读，日志不包含异常内容", async () => {
        let emit!: () => void;
        listenMock.mockImplementation(async (_name, cb) => { emit = cb; return vi.fn(); });
        const { client, onRecorded } = setup();
        vi.spyOn(client, "invalidateQueries").mockRejectedValue(new Error("secret-invalidating-123"));
        await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(1));
        act(() => emit());
        await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(2));
        await waitFor(() => expect(logMock).toHaveBeenCalledWith("security_audit_invalidate", "failed"));
        expect(JSON.stringify(logMock.mock.calls)).not.toContain("secret-invalidating-123");
    });
});

describe("unified security event cache bridge", () => {
  beforeEach(() => {
    listenMock.mockReset();
    logMock.mockReset();
  });

  it("both first read and recorded signals refetch active summaries but never deep pages", async () => {
    let emit!: () => void;
    listenMock.mockImplementation(async (_name, cb) => {
      emit = cb;
      return vi.fn();
    });
    const { client, invalidate, onRecorded } = setup();
    await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(1));
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["securityEvents", "summary"],
      refetchType: "active",
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["securityEvents", "page"],
      refetchType: "none",
    });
    invalidate.mockClear();
    act(() => emit());
    await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(2));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: securityKeys.all });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["securityEvents", "summary"],
      refetchType: "active",
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["securityEvents", "page"],
      refetchType: "none",
    });
    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: ["securityEvents"],
    });
    client.clear();
  });

  it.each(["legacy", "summary", "page"])(
    "a synchronous %s invalidation failure does not block the other domains or notifier",
    async (domain) => {
      let emit!: () => void;
      listenMock.mockImplementation(async (_name, cb) => {
        emit = cb;
        return vi.fn();
      });
      const { invalidate, onRecorded } = setup();
      await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(1));
      invalidate.mockClear().mockImplementation((options) => {
        const key = options?.queryKey;
        if (
          (domain === "legacy" && key?.[0] === "security") ||
          key?.[1] === domain
        )
          throw new Error("secret-bridge-error");
        return Promise.resolve();
      });
      act(() => emit());
      await waitFor(() => expect(onRecorded).toHaveBeenCalledTimes(2));
      expect(invalidate).toHaveBeenCalledTimes(3);
      await waitFor(() => expect(logMock).toHaveBeenCalled());
      expect(JSON.stringify(logMock.mock.calls)).not.toContain(
        "secret-bridge-error",
      );
    },
  );
});
