import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import {
  focusManager,
  onlineManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  securityEventsKeys,
  useSecurityEvents,
  useSecurityEventsLiveRefresh,
  useSecurityEventsSummary,
} from "./useSecurityEvents";
import type {
  SecurityEventFilter,
  SecurityEventPage,
  SecurityEventSummary,
} from "@/types/securityEvents";

const mocks = vi.hoisted(() => ({
  getEvents: vi.fn(),
  getSummary: vi.fn(),
  clearEvents: vi.fn(),
  listen: vi.fn(),
  log: vi.fn(),
}));
vi.mock("@/lib/api", () => ({ securityEventsApi: mocks }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/frontendLogger", () => ({ reportFrontendError: mocks.log }));

const filter: SecurityEventFilter = { view: "security_events" };
const summary: SecurityEventSummary = {
  total: 12,
  dlpCount: 8,
  toolRiskCount: 4,
  redacted: 2,
  warnings: 5,
  protectiveBlocked: 1,
  other: 4,
};
const makePage = (page = 1, total = 12): SecurityEventPage => ({
  items: [],
  total,
  page,
  pageSize: 50,
});
const receipt = { deletedTotal: 3, deletedDlp: 1, deletedToolRisk: 2 };
const clients: QueryClient[] = [];
function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  clients.push(client);
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}> {children} </QueryClientProvider>
  );
  return { client, wrapper };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
const handlers = new Map<string, () => void>();
const off = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  mocks.getEvents.mockReset().mockResolvedValue(makePage());
  mocks.getSummary.mockReset().mockResolvedValue(summary);
  mocks.clearEvents.mockReset().mockResolvedValue(receipt);
  handlers.clear();
  mocks.listen
    .mockReset()
    .mockImplementation(async (name: string, cb: () => void) => {
      handlers.set(name, cb);
      return off;
    });
});
afterEach(() => {
  clients.splice(0).forEach((client) => client.clear());
  focusManager.setFocused(undefined);
  onlineManager.setOnline(true);
});

describe("unified event queries", () => {
  it("normalizes optional DLP whitespace for both exact spec keys and IPC without dropping incompatible conditions", async () => {
    const { client, wrapper } = setup();
    const input: SecurityEventFilter = {
      ...filter,
      sessionId: " session_% ",
      keyword: "  ",
      appType: undefined,
      toolVerdict: "uncertain",
    };
    const normalized = {
      ...filter,
      sessionId: "session_%",
      toolVerdict: "uncertain",
    };
    const { result } = renderHook(
      () => ({
        page: useSecurityEvents(input, 2, 50),
        summary: useSecurityEventsSummary(input),
      }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.page.isLoading).toBe(false));
    expect(mocks.getEvents).toHaveBeenCalledWith(normalized, 2, 50);
    expect(mocks.getSummary).toHaveBeenCalledWith(normalized);
    expect(securityEventsKeys.page(input, 2, 50)).toEqual([
      "securityEvents",
      "page",
      normalized,
      2,
      50,
    ]);
    expect(securityEventsKeys.summary(input)).toEqual([
      "securityEvents",
      "summary",
      normalized,
    ]);
    expect(
      client.getQueryData(["securityEvents", "page", normalized, 2, 50]),
    ).toEqual(makePage());
    expect(
      client.getQueryCache().findAll({ queryKey: ["security"] }),
    ).toHaveLength(0);
  });

  it("keeps view/filter/page/pageSize in separate entries and never presents an old count under a new filter", async () => {
    const pending = deferred<SecurityEventPage>();
    const { wrapper } = setup();
    const { result, rerender } = renderHook(
      ({ currentFilter, page, size }) =>
        useSecurityEvents(currentFilter, page, size),
      { wrapper, initialProps: { currentFilter: filter, page: 1, size: 50 } },
    );
    await waitFor(() => expect(result.current.eventsResult?.total).toBe(12));
    mocks.getEvents.mockReturnValueOnce(pending.promise);
    rerender({
      currentFilter: { view: "tool_calls", toolVerdict: "uncertain" },
      page: 2,
      size: 25,
    });
    expect(result.current.eventsResult).toBeUndefined();
    expect(result.current.isLoading).toBe(true);
    await act(async () => pending.resolve({ ...makePage(2, 2), pageSize: 25 }));
    await waitFor(() => expect(result.current.eventsResult?.total).toBe(2));
    expect(mocks.getEvents).toHaveBeenLastCalledWith(
      { view: "tool_calls", toolVerdict: "uncertain" },
      2,
      25,
    );
  });

  it("ignores a late old-filter response after a new query has succeeded", async () => {
    const old = deferred<SecurityEventPage>();
    mocks.getEvents
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce(makePage(1, 7));
    const { wrapper } = setup();
    const { result, rerender } = renderHook(
      ({ appType }) => useSecurityEvents({ ...filter, appType }, 1, 50),
      { wrapper, initialProps: { appType: "claude" } },
    );
    rerender({ appType: "codex" });
    await waitFor(() => expect(result.current.eventsResult?.total).toBe(7));
    await act(async () => old.resolve(makePage(1, 999)));
    expect(result.current.eventsResult?.total).toBe(7);
  });

  it("cancels superseded refreshes so a late response cannot replace a newer snapshot", async () => {
    const { wrapper } = setup();
    const { result } = renderHook(() => useSecurityEvents(filter, 1, 50), {
      wrapper,
    });
    await waitFor(() => expect(result.current.eventsResult?.total).toBe(12));
    const old = deferred<SecurityEventPage>();
    mocks.getEvents
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce(makePage(1, 7));
    let oldRefresh!: Promise<unknown>;
    act(() => {
      oldRefresh = result.current.refetch();
    });
    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() => expect(result.current.eventsResult?.total).toBe(7));
    await act(async () => {
      old.resolve(makePage(1, 999));
      await oldRefresh;
    });
    expect(result.current.eventsResult?.total).toBe(7);
  });

  it("does not refetch lists on focus/reconnect or install polling; explicit refresh still forces a read", async () => {
    const { client, wrapper } = setup();
    const { result } = renderHook(() => useSecurityEvents(filter, 3, 50), {
      wrapper,
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    const options = client
      .getQueryCache()
      .find({ queryKey: securityEventsKeys.page(filter, 3, 50) })
      ?.options as Record<string, unknown>;
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBeFalsy();
    act(() => {
      focusManager.setFocused(false);
      focusManager.setFocused(true);
      onlineManager.setOnline(false);
      onlineManager.setOnline(true);
    });
    expect(mocks.getEvents).toHaveBeenCalledTimes(1);
    await act(async () => {
      await result.current.refetch();
    });
    expect(mocks.getEvents).toHaveBeenCalledTimes(2);
  });

  it("exposes query failures without inventing zero counts, and permits retry", async () => {
    mocks.getEvents.mockRejectedValueOnce(
      new Error("security_events_query_failed"),
    );
    mocks.getSummary.mockRejectedValueOnce(
      new Error("security_events_query_failed"),
    );
    const { wrapper } = setup();
    const { result } = renderHook(
      () => ({
        page: useSecurityEvents(filter, 1, 50),
        summary: useSecurityEventsSummary(filter),
      }),
      { wrapper },
    );
    await waitFor(() =>
      expect(
        result.current.page.isError && result.current.summary.isError,
      ).toBe(true),
    );
    expect(result.current.page.eventsResult).toBeUndefined();
    expect(result.current.summary.data).toBeUndefined();
    await act(async () => {
      await Promise.all([
        result.current.page.refetch(),
        result.current.summary.refetch(),
      ]);
    });
    await waitFor(() => expect(result.current.summary.data?.total).toBe(12));
  });

  it("supports opt-in 30s foreground-only summary polling without polling by default", async () => {
    const { client, wrapper } = setup();
    const { result } = renderHook(
      () => [
        useSecurityEventsSummary(filter, { refetchInterval: 30000 }),
        useSecurityEventsSummary({ ...filter, appType: "codex" }),
      ],
      { wrapper },
    );
    await waitFor(() =>
      expect(result.current.every((query) => !query.isLoading)).toBe(true),
    );
    const options = client
      .getQueryCache()
      .find({ queryKey: securityEventsKeys.summary(filter) })
      ?.options as Record<string, unknown>;
    expect(options.refetchInterval).toBe(30000);
    expect(options.refetchIntervalInBackground).toBe(false);
    const otherOptions = client.getQueryCache().find({
      queryKey: securityEventsKeys.summary({ ...filter, appType: "codex" }),
    })?.options as Record<string, unknown>;
    expect(otherOptions.refetchInterval).toBeFalsy();
  });

  it("clears the supplied frozen filter, returns receipt, refreshes summaries and legacy domains but only marks pages stale", async () => {
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(
      () => ({
        page: useSecurityEvents(filter, 3, 50),
        summary: useSecurityEventsSummary(filter),
      }),
      { wrapper },
    );
    await waitFor(() =>
      expect(
        result.current.page.isLoading || result.current.summary.isLoading,
      ).toBe(false),
    );
    const frozen = { ...filter, endTimestamp: 123 };
    let actual;
    await act(async () => {
      actual = await result.current.page.clearEvents(frozen);
    });
    expect(actual).toEqual(receipt);
    expect(mocks.clearEvents).toHaveBeenCalledWith(frozen);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["security"] });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["toolRisk"] });
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
    expect(mocks.getEvents).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(mocks.getSummary).toHaveBeenCalledTimes(2));
  });

  it("keeps a failed clear as failure without invalidating caches", async () => {
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useSecurityEvents(filter, 1, 50), {
      wrapper,
    });
    mocks.clearEvents.mockRejectedValueOnce(
      new Error("security_events_clear_failed"),
    );
    await act(async () => {
      await expect(result.current.clearEvents(filter)).rejects.toThrow(
        "security_events_clear_failed",
      );
    });
    expect(invalidate).not.toHaveBeenCalled();
  });
});

describe("live reading mode", () => {
  it("performs one first read after both subscriptions, then refreshes page one for either payload-free event", async () => {
    const refresh = vi.fn();
    const { wrapper } = setup();
    const { result } = renderHook(
      () => useSecurityEventsLiveRefresh(1, refresh),
      { wrapper },
    );
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    expect(mocks.listen).toHaveBeenCalledTimes(2);
    act(() => {
      handlers.get("security-audit-recorded")?.();
      handlers.get("tool-risk-recorded")?.();
    });
    expect(refresh).toHaveBeenCalledTimes(3);
    expect(result.current.hasUpdates).toBe(false);
  });

  it("keeps deep-page items/count and only marks possible updates, including low-risk/unrelated signals", async () => {
    const { wrapper } = setup();
    const { result } = renderHook(
      () => {
        const query = useSecurityEvents(
          { ...filter, eventType: "dlp", endTimestamp: 123 },
          4,
          50,
        );
        const live = useSecurityEventsLiveRefresh(4, query.refetch);
        return { query, live };
      },
      { wrapper },
    );
    await waitFor(() => expect(result.current.live.hasUpdates).toBe(true));
    await waitFor(() => expect(result.current.query.isLoading).toBe(false));
    const snapshot = result.current.query.eventsResult;
    act(() => {
      result.current.live.resetUpdates();
    });
    expect(result.current.live.hasUpdates).toBe(false);
    act(() => {
      for (let i = 0; i < 5; i++) handlers.get("tool-risk-recorded")?.();
      handlers.get("security-audit-recorded")?.();
    });
    expect(result.current.live.hasUpdates).toBe(true);
    expect(result.current.query.eventsResult).toBe(snapshot);
    expect(mocks.getEvents).toHaveBeenCalledTimes(1);
  });

  it("uses the latest page/callback without resubscribing, and reset clears the marker", async () => {
    const { wrapper } = setup();
    const first = vi.fn();
    const second = vi.fn();
    const { result, rerender } = renderHook(
      ({ page, refresh }) => useSecurityEventsLiveRefresh(page, refresh),
      { wrapper, initialProps: { page: 3, refresh: first } },
    );
    await waitFor(() => expect(result.current.hasUpdates).toBe(true));
    rerender({ page: 1, refresh: second });
    act(() => {
      result.current.resetUpdates();
      handlers.get("tool-risk-recorded")?.();
    });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    expect(mocks.listen).toHaveBeenCalledTimes(2);
    expect(result.current.hasUpdates).toBe(false);
  });

  it("marks other open deep pages changed when clear invalidates pages without a recorded signal", async () => {
    const { client, wrapper } = setup();
    client.setQueryData(securityEventsKeys.page(filter, 3, 50), makePage(3));
    const { result } = renderHook(
      () => useSecurityEventsLiveRefresh(3, vi.fn()),
      { wrapper },
    );
    await waitFor(() => expect(result.current.hasUpdates).toBe(true));
    act(() => {
      result.current.resetUpdates();
    });
    await act(async () => {
      await client.invalidateQueries({
        queryKey: ["securityEvents", "page"],
        refetchType: "none",
      });
    });
    expect(result.current.hasUpdates).toBe(true);
  });

  it("cleans up both subscriptions, including a subscription that resolves after unmount", async () => {
    const pending = deferred<() => void>();
    mocks.listen.mockReturnValueOnce(pending.promise);
    const { wrapper } = setup();
    const refresh = vi.fn();
    const { unmount } = renderHook(
      () => useSecurityEventsLiveRefresh(1, refresh),
      { wrapper },
    );
    await waitFor(() => expect(handlers.has("tool-risk-recorded")).toBe(true));
    unmount();
    const lateOff = vi.fn();
    await act(async () => pending.resolve(lateOff));
    expect(off).toHaveBeenCalledTimes(1);
    expect(lateOff).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
    act(() => handlers.get("tool-risk-recorded")?.());
    expect(refresh).not.toHaveBeenCalled();
  });

  it("logs fixed safe subscription errors and still reads through the surviving subscription", async () => {
    mocks.listen.mockRejectedValueOnce(new Error("secret-session-command"));
    const { wrapper } = setup();
    const refresh = vi.fn();
    renderHook(() => useSecurityEventsLiveRefresh(1, refresh), { wrapper });
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    expect(mocks.log).toHaveBeenCalledWith("security_events_listen", "failed");
    expect(JSON.stringify(mocks.log.mock.calls)).not.toContain(
      "secret-session-command",
    );
    act(() => handlers.get("tool-risk-recorded")?.());
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it("isolates synchronous and asynchronous refresh errors without leaking payload or unhandled rejection", async () => {
    const { wrapper } = setup();
    const refresh = vi
      .fn()
      .mockImplementationOnce(() => {
        throw new Error("secret-sync");
      })
      .mockRejectedValue(new Error("secret-async"));
    renderHook(() => useSecurityEventsLiveRefresh(1, refresh), { wrapper });
    await waitFor(() =>
      expect(mocks.log).toHaveBeenCalledWith(
        "security_events_refresh",
        "failed",
      ),
    );
    act(() => handlers.get("tool-risk-recorded")?.());
    await waitFor(() => expect(mocks.log).toHaveBeenCalledTimes(2));
    expect(JSON.stringify(mocks.log.mock.calls)).not.toContain("secret");
  });
});

describe("clear and controlled refresh integration", () => {
  it("a successful clear marks other deep readers without changing their snapshot or emitting a recorded signal", async () => {
    const { wrapper } = setup();
    const refresh = vi.fn();
    const { result } = renderHook(
      () => {
        const query = useSecurityEvents(filter, 4, 50);
        const live = useSecurityEventsLiveRefresh(4, refresh);
        return { query, live };
      },
      { wrapper },
    );
    await waitFor(() => expect(result.current.query.isLoading).toBe(false));
    await waitFor(() => expect(result.current.live.hasUpdates).toBe(true));
    act(() => result.current.live.resetUpdates());
    const snapshot = result.current.query.eventsResult;
    await act(async () => {
      await result.current.query.clearEvents({ ...filter, endTimestamp: 123 });
    });
    expect(result.current.live.hasUpdates).toBe(true);
    expect(result.current.query.eventsResult).toBe(snapshot);
    expect(mocks.getEvents).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("a controlled manual refresh can return to a cached first page and still force a new snapshot", async () => {
    const { client, wrapper } = setup();
    client.setQueryData(
      securityEventsKeys.page(filter, 1, 50),
      makePage(1, 99),
    );
    const { result, rerender } = renderHook(
      ({ page }) => {
        const query = useSecurityEvents(filter, page, 50);
        const live = useSecurityEventsLiveRefresh(page, query.refetch);
        return { query, live };
      },
      { wrapper, initialProps: { page: 3 } },
    );
    await waitFor(() => expect(result.current.live.hasUpdates).toBe(true));
    await waitFor(() => expect(result.current.query.isLoading).toBe(false));
    // The component controls pagination: return to page one, refetch the current key, then clear the marker.
    rerender({ page: 1 });
    await waitFor(() =>
      expect(result.current.query.eventsResult?.total).toBe(12),
    );
    mocks.getEvents.mockResolvedValueOnce(makePage(1, 7));
    await act(async () => {
      await result.current.query.refetch();
      result.current.live.resetUpdates();
    });
    expect(mocks.getEvents).toHaveBeenLastCalledWith(filter, 1, 50);
    expect(result.current.query.eventsResult?.total).toBe(7);
    expect(result.current.live.hasUpdates).toBe(false);
  });

  it("clear cache failures remain isolated and do not turn a completed deletion into a false failure", async () => {
    const { client, wrapper } = setup();
    const { result } = renderHook(() => useSecurityEvents(filter, 1, 50), {
      wrapper,
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    const invalidate = vi
      .spyOn(client, "invalidateQueries")
      .mockRejectedValue(new Error("secret-cache-input"));
    await act(async () => {
      expect(await result.current.clearEvents(filter)).toEqual(receipt);
    });
    expect(invalidate).toHaveBeenCalledTimes(4);
    expect(mocks.log).toHaveBeenCalledWith(
      "security_events_invalidate",
      "failed",
    );
    expect(JSON.stringify(mocks.log.mock.calls)).not.toContain(
      "secret-cache-input",
    );
  });
});

describe("repeated clear while already stale", () => {
  it("marks deep readers after every successful clear even when the page was already invalidated", async () => {
    const { wrapper } = setup();
    const { result } = renderHook(
      () => {
        const query = useSecurityEvents(filter, 4, 50);
        const live = useSecurityEventsLiveRefresh(4, vi.fn());
        return { query, live };
      },
      { wrapper },
    );
    await waitFor(() => expect(result.current.query.isLoading).toBe(false));
    await waitFor(() => expect(result.current.live.hasUpdates).toBe(true));
    await act(async () => {
      await result.current.query.clearEvents({ ...filter, endTimestamp: 123 });
    });
    act(() => result.current.live.resetUpdates());
    expect(result.current.live.hasUpdates).toBe(false);
    await act(async () => {
      await result.current.query.clearEvents({ ...filter, endTimestamp: 456 });
    });
    expect(result.current.live.hasUpdates).toBe(true);
    expect(mocks.getEvents).toHaveBeenCalledTimes(1);
  });
});
