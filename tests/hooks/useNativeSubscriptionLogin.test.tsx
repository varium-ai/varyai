import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { useNativeSubscriptionLogin } from "@/components/providers/forms/hooks/useNativeSubscriptionLogin";
import {
  nativeSubscriptionApi,
  type NativeSubscriptionProvider,
} from "@/lib/api/nativeSubscription";

vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: {
    start: vi.fn(),
    poll: vi.fn(),
    cancel: vi.fn().mockResolvedValue(undefined),
  },
}));

function mount(provider: NativeSubscriptionProvider) {
  vi.clearAllMocks();
  const client = new QueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return renderHook(() => useNativeSubscriptionLogin(provider), { wrapper });
}

it.each([
  "claude",
  "chatgpt_api",
  "gemini_cli",
  "antigravity",
  "workbuddy",
  "workbuddy-ai",
] as const)(
  "uses an already-completed %s login without polling",
  async (provider) => {
    const { result } = mount(provider);
    vi.mocked(nativeSubscriptionApi.start).mockResolvedValue({
      id: "fixture-flow",
      state: "done",
      account: { id: "fixture-account", login: "", project: "" },
    });
    await act(async () => result.current.start());
    expect(result.current.completion).toBe("fixture-account");
    expect(nativeSubscriptionApi.poll).not.toHaveBeenCalled();
  },
);

it("surfaces an immediate native login failure without opening a stale URL", async () => {
  const { result } = mount("claude");
  vi.mocked(nativeSubscriptionApi.start).mockResolvedValue({
    id: "fixture-flow",
    state: "failed",
    error: "nativeSubscription.claudeMissing",
  });
  await act(async () => result.current.start());
  expect(result.current.error).toBe("nativeSubscription.claudeMissing");
  expect(result.current.starting).toBe(false);
  expect(nativeSubscriptionApi.poll).not.toHaveBeenCalled();
});
