import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { useAgentInstallations } from "@/hooks/useAgentInstallations";
import { proxyApi } from "@/lib/api/proxy";

const native = vi.hoisted(() => ({
  onFocusChanged: vi.fn(),
  unlisten: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => native }));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { getAgentInstallations: vi.fn() },
}));

beforeEach(() => {
  vi.resetAllMocks();
  native.onFocusChanged.mockResolvedValue(native.unlisten);
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    workbuddy: false,
    qwen: false,
  } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
});

it("discovers newly installed desktop and CLI agents on native focus without connecting them", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  client.setQueryData(["agent-desktop-versions"], {
    workbuddy: { installed: false },
  });
  const { result, unmount } = renderHook(() => useAgentInstallations(), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() => expect(result.current.data?.workbuddy).toBe(false));
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    workbuddy: true,
    qwen: true,
  } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
  act(() => native.onFocusChanged.mock.calls[0][0]({ payload: false }));
  expect(proxyApi.getAgentInstallations).toHaveBeenCalledTimes(1);
  act(() => native.onFocusChanged.mock.calls[0][0]({ payload: true }));
  await waitFor(() => expect(result.current.data?.workbuddy).toBe(true));
  expect(result.current.data?.qwen).toBe(true);
  expect(client.getQueryState(["agent-desktop-versions"])?.isInvalidated).toBe(
    true,
  );
  unmount();
  await waitFor(() => expect(native.unlisten).toHaveBeenCalledOnce());
});

it("refreshes on browser focus and unregisters fallback listeners", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const { result, unmount } = renderHook(() => useAgentInstallations(), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() => expect(result.current.data).toBeDefined());
  act(() => window.dispatchEvent(new Event("focus")));
  await waitFor(() =>
    expect(proxyApi.getAgentInstallations).toHaveBeenCalledTimes(2),
  );
  unmount();
  act(() => window.dispatchEvent(new Event("focus")));
  expect(proxyApi.getAgentInstallations).toHaveBeenCalledTimes(2);
});

it("refreshes cached desktop versions when periodic presence results change", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  client.setQueryData(["agent-desktop-versions"], {
    workbuddy: { installed: false },
  });
  const { result } = renderHook(() => useAgentInstallations(), {
    wrapper: ({ children }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
  await waitFor(() => expect(result.current.data?.workbuddy).toBe(false));
  act(() =>
    client.setQueryData(["agent-installations"], {
      workbuddy: true,
      qwen: false,
    }),
  );
  await waitFor(() =>
    expect(
      client.getQueryState(["agent-desktop-versions"])?.isInvalidated,
    ).toBe(true),
  );
  expect(proxyApi.getAgentInstallations).toHaveBeenCalledOnce();
});
