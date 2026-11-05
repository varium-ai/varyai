import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { useCodexBrowserLogin } from "@/components/providers/forms/hooks/useCodexBrowserLogin";
import { codexBrowserLoginApi as api } from "@/lib/api/codexBrowserLogin";
import { settingsApi } from "@/lib/api/settings";
vi.mock("@/lib/api/codexBrowserLogin", () => ({
  codexBrowserLoginApi: {
    start: vi.fn(),
    poll: vi.fn(),
    callback: vi.fn(),
    cancel: vi.fn(),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));
const flow = {
  id: "synthetic-flow",
  url: "https://auth.openai.com/oauth/authorize?state=synthetic",
  status: "waiting" as const,
  callbackBlocked: false,
};
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { ...renderHook(() => useCodexBrowserLogin(), { wrapper }), client };
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.start).mockReset().mockResolvedValue(flow);
  vi.mocked(api.poll).mockReset().mockResolvedValue(flow);
  vi.mocked(api.cancel).mockResolvedValue(true);
  vi.mocked(api.callback).mockReset().mockResolvedValue(undefined);
  vi.mocked(settingsApi.openExternal).mockReset().mockResolvedValue(undefined);
});
it("opens the authorization URL and sends pasted callback values to the current flow", async () => {
  const { result } = mount();
  await act(() => result.current.start("fixture-account", "direct"));
  expect(api.start).toHaveBeenCalledWith("fixture-account", "direct");
  expect(settingsApi.openExternal).toHaveBeenCalledWith(flow.url);
  await act(() =>
    result.current.submit(
      "http://localhost:1455/auth/callback?state=synthetic&code=synthetic",
    ),
  );
  expect(api.callback).toHaveBeenCalledWith(
    flow.id,
    "http://localhost:1455/auth/callback?state=synthetic&code=synthetic",
  );
  expect(result.current.submitted).toBe(true);
});
it("keeps an invalid callback editable and reports only a safe localized error", async () => {
  vi.mocked(api.callback).mockRejectedValue("synthetic-private-vendor-message");
  const { result } = mount();
  await act(() => result.current.start());
  await act(() => result.current.submit("invalid"));
  expect(result.current.submitted).toBe(false);
  expect(result.current.error).toBe("codexBrowser.failed");
  expect(result.current.isPending).toBe(true);
});
it("preserves a browser-opening failure for recovery without stopping authorization", async () => {
  vi.mocked(settingsApi.openExternal).mockRejectedValue(
    new Error("synthetic-private-error"),
  );
  const { result } = mount();
  await act(() => result.current.start());
  expect(result.current.openFailed).toBe(true);
  expect(result.current.flow?.url).toBe(flow.url);
});
it("cancels a late start after the editor unmounts and never opens its URL", async () => {
  let finish!: (value: typeof flow) => void;
  vi.mocked(api.start).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const { result, unmount } = mount();
  act(() => {
    void result.current.start();
  });
  await waitFor(() => expect(api.start).toHaveBeenCalled());
  unmount();
  await act(async () => finish(flow));
  await waitFor(() => expect(api.cancel).toHaveBeenCalledWith(flow.id));
  expect(settingsApi.openExternal).not.toHaveBeenCalled();
});
it("refreshes managed account status and publishes completion only for the current flow", async () => {
  vi.mocked(api.poll).mockResolvedValue({
    ...flow,
    status: "done",
    account: { id: "fixture-account", login: "fixture@example.test" },
  });
  const { result, client } = mount();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await act(() => result.current.start());
  await waitFor(
    () => expect(result.current.completion?.accountId).toBe("fixture-account"),
    { timeout: 3000 },
  );
  expect(invalidate).toHaveBeenCalledWith({
    queryKey: ["managed-auth-status", "codex_oauth"],
  });
  expect(result.current.isPending).toBe(false);
});
