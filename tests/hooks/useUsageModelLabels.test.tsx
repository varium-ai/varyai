import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import { useUsageModelLabels } from "@/lib/query/usage";

vi.mock("@/lib/api/usage", () => ({ usageApi: { getModelLabels: vi.fn() } }));
const wrap = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
};

describe("local usage model name lookup", () => {
  beforeEach(() => {
    vi.mocked(usageApi.getModelLabels).mockReset();
  });
  it("does not query names for ordinary/custom models", () => {
    const { unmount } = renderHook(
      () =>
        useUsageModelLabels(["ordinary-model", "vary-provider/model-custom"], {
          refetchInterval: false,
        }),
      { wrapper: wrap() },
    );
    expect(usageApi.getModelLabels).not.toHaveBeenCalled();
    unmount();
  });
  it("deduplicates both forms and sorts the request independently of usage filters", async () => {
    const id = `model-${"a".repeat(64)}`;
    vi.mocked(usageApi.getModelLabels).mockResolvedValue([]);
    const { result, unmount } = renderHook(
      () =>
        useUsageModelLabels([id, `vary-provider/${id}`], {
          refetchInterval: false,
        }),
      { wrapper: wrap() },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(usageApi.getModelLabels).toHaveBeenCalledExactlyOnceWith([id]);
    unmount();
  });
  it("keeps optional lookup errors separate and does not retry continuously", async () => {
    vi.mocked(usageApi.getModelLabels).mockRejectedValue(
      new Error("synthetic metadata failure"),
    );
    const { result, unmount } = renderHook(
      () =>
        useUsageModelLabels([`model-${"b".repeat(64)}`], {
          refetchInterval: false,
        }),
      { wrapper: wrap() },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(usageApi.getModelLabels).toHaveBeenCalledTimes(1);
    expect(result.current.data).toEqual([]);
    unmount();
  });
  it("batches large historical menus within the backend request limit", async () => {
    vi.mocked(usageApi.getModelLabels).mockResolvedValue([]);
    const ids = Array.from(
      { length: 513 },
      (_, i) => `model-${i.toString(16).padStart(64, "0")}`,
    );
    const { result, unmount } = renderHook(
      () => useUsageModelLabels(ids, { refetchInterval: false }),
      { wrapper: wrap() },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(usageApi.getModelLabels).toHaveBeenCalledTimes(2);
    expect(
      vi
        .mocked(usageApi.getModelLabels)
        .mock.calls.map(([batch]) => batch.length),
    ).toEqual([512, 1]);
    unmount();
  });
});
