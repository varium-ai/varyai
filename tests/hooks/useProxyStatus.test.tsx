import { proxyKeys } from "@/lib/query/proxy";
import type { ReactNode } from "react";
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { createTestQueryClient } from "../utils/testQueryClient";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const invokeMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === "proxy.server.started") {
        return `代理服务已启动 - ${options?.address}:${options?.port}`;
      }

      if (typeof options?.defaultValue === "string") {
        return options.defaultValue;
      }

      return key;
    },
  }),
}));

interface WrapperProps {
  children: ReactNode;
}

function createWrapper() {
  const queryClient = createTestQueryClient();

  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, queryClient };
}

describe("useProxyStatus", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();

    invokeMock.mockImplementation((command: string) => {
      if (command === "get_proxy_status") {
        return Promise.resolve({
          running: false,
          address: "127.0.0.1",
          port: 15721,
          active_connections: 0,
          total_requests: 0,
          success_requests: 0,
          failed_requests: 0,
          success_rate: 0,
          uptime_seconds: 0,
          current_provider: null,
          current_provider_id: null,
          last_request_at: null,
          last_error: null,
          failover_count: 0,
        });
      }

      if (command === "get_proxy_takeover_status") {
        return Promise.resolve({
          claude: false,
          codex: false,
          gemini: false,
          grokbuild: false,
          opencode: false,
          openclaw: false,
        });
      }

      if (command === "start_proxy_server") {
        return Promise.resolve({
          address: "127.0.0.1",
          port: 15721,
          started_at: "2026-03-10T00:00:00Z",
        });
      }

      return Promise.resolve(null);
    });
  });

  it("keeps the established proxy query key shapes", () => {
    expect(proxyKeys.status).toEqual(["proxyStatus"]);
    expect(proxyKeys.takeoverStatus).toEqual(["proxyTakeoverStatus"]);
    expect(proxyKeys.globalConfig).toEqual(["globalProxyConfig"]);
  });

  it("detects an external takeover reset while the proxy keeps running", async () => {
    let takenOver = true;
    invokeMock.mockImplementation((command: string) =>
      Promise.resolve(
        command === "get_proxy_status"
          ? { running: true }
          : { codex: takenOver },
      ),
    );
    const { wrapper, queryClient } = createWrapper();
    const { result, unmount } = renderHook(() => useProxyStatus(), { wrapper });

    try {
      await waitFor(() => {
        expect(result.current.isRunning).toBe(true);
        expect(result.current.takeoverStatus?.codex).toBe(true);
      });

      // Another app restores the shared native configuration without emitting
      // a Tauri event to this process. The proxy itself is still alive.
      takenOver = false;
      await waitFor(
        () => {
          expect(result.current.isRunning).toBe(true);
          expect(result.current.takeoverStatus?.codex).toBe(false);
        },
        { timeout: 3500 },
      );
    } finally {
      unmount();
      queryClient.clear();
    }
  });

  it("shows interpolated address and port after proxy server starts", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useProxyStatus(), { wrapper });

    await waitFor(() => {
      expect(result.current.status).toBeDefined();
    });

    await act(async () => {
      await result.current.startProxyServer();
    });

    expect(toastSuccessMock).toHaveBeenCalledWith(
      "代理服务已启动 - 127.0.0.1:15721",
      { closeButton: true },
    );

    await act(async () => {
      await result.current.stopProxyServer();
    });

    expect(invokeMock).toHaveBeenCalledWith("start_proxy_server");
    expect(invokeMock).toHaveBeenCalledWith("stop_proxy_server");
  });
  it.each(["stopProxyServer", "stopWithRestore"] as const)(
    "%s refreshes global audit and per-agent connection state",
    async (method) => {
      const { wrapper, queryClient } = createWrapper();
      const invalidate = vi.spyOn(queryClient, "invalidateQueries");
      const { result } = renderHook(() => useProxyStatus(), { wrapper });
      await act(async () => {
        await result.current[method]();
      });
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["security"] });
      expect(invalidate).toHaveBeenCalledWith({ queryKey: ["security-route"] });
    },
  );
});
