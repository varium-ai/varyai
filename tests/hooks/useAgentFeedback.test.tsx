import type { PropsWithChildren } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, expect, it, vi } from "vitest";
import { useAgentFeedback } from "@/lib/agentFeedback";

const wrapper = (client = new QueryClient()) =>
  function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };

afterEach(() => vi.useRealTimers());
it("keeps Agent messages and their eight-second lifetimes independent", () => {
  vi.useFakeTimers();
  const feedback = renderHook(() => useAgentFeedback(), { wrapper: wrapper() });
  act(() => feedback.result.current.show("codex", "Synthetic reopen failed"));
  act(() => vi.advanceTimersByTime(4000));
  act(() =>
    feedback.result.current.show("claude-desktop", "Synthetic quit refused"),
  );
  expect(feedback.result.current.messages).toEqual({
    codex: "Synthetic reopen failed",
    "claude-desktop": "Synthetic quit refused",
  });
  act(() => vi.advanceTimersByTime(4000));
  expect(feedback.result.current.messages).toEqual({
    "claude-desktop": "Synthetic quit refused",
  });
  act(() => vi.advanceTimersByTime(4000));
  expect(feedback.result.current.messages).toEqual({});
});
it("retains feedback across navigation and clears only the Agent being retried", () => {
  vi.useFakeTimers();
  const client = new QueryClient();
  const first = renderHook(() => useAgentFeedback(), {
    wrapper: wrapper(client),
  });
  act(() => first.result.current.show("codex", "Synthetic retry needed"));
  act(() => first.result.current.show("kimi", "Synthetic app open"));
  first.unmount();
  const next = renderHook(() => useAgentFeedback(), {
    wrapper: wrapper(client),
  });
  expect(next.result.current.messages.codex).toBe("Synthetic retry needed");
  act(() => next.result.current.clear("codex"));
  expect(next.result.current.messages).toEqual({ kimi: "Synthetic app open" });
});
it("isolates independent query sessions", () => {
  vi.useFakeTimers();
  const first = renderHook(() => useAgentFeedback(), { wrapper: wrapper() });
  const second = renderHook(() => useAgentFeedback(), { wrapper: wrapper() });
  act(() => first.result.current.show("codex", "Synthetic session feedback"));
  expect(second.result.current.messages).toEqual({});
});
