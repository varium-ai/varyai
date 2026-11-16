import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import { useRequestLogs } from "@/lib/query/usage";
import type { PaginatedLogs } from "@/types/usage";

vi.mock("@/lib/api/usage", () => ({
  usageApi: { getRequestLogs: vi.fn() },
}));

describe("request log pagination", () => {
  it("keeps the table and record count while loading the last page, but clears them for a different filter", async () => {
    const firstPage: PaginatedLogs = {
      data: [],
      total: 48,
      page: 0,
      pageSize: 20,
    };
    const lastPage = { ...firstPage, page: 2 };
    let finishLastPage!: (value: PaginatedLogs) => void;
    vi.mocked(usageApi.getRequestLogs)
      .mockResolvedValueOnce(firstPage)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishLastPage = resolve;
          }),
      )
      .mockImplementationOnce(() => new Promise(() => {}));
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result, rerender, unmount } = renderHook(
      ({ page, model }: { page: number; model?: string }) =>
        useRequestLogs({
          filters: { model },
          range: { preset: "today" },
          page,
          options: { refetchInterval: false },
        }),
      {
        initialProps: { page: 0 } as { page: number; model?: string },
        wrapper,
      },
    );
    await waitFor(() => expect(result.current.data).toEqual(firstPage));
    rerender({ page: 2 });
    await waitFor(() =>
      expect(usageApi.getRequestLogs).toHaveBeenCalledTimes(2),
    );
    expect(result.current.data).toEqual(firstPage);
    expect(result.current.isLoading).toBe(false);
    expect(result.current.isPlaceholderData).toBe(true);
    await act(async () => finishLastPage(lastPage));
    await waitFor(() => expect(result.current.data?.page).toBe(2));
    expect(result.current.isPlaceholderData).toBe(false);
    rerender({ page: 0, model: "different-model" });
    expect(result.current.data).toBeUndefined();
    expect(result.current.isLoading).toBe(true);
    unmount();
    client.clear();
  });
});
