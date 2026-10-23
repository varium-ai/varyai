// @vitest-environment jsdom
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, type ReactNode } from "react";
import { securityKeys, useSecuritySettings, useSecurityAuditLogs } from "@/hooks/useSecurityAudit";

const updateSettings = vi.fn().mockResolvedValue(undefined);
const auditApi = vi.hoisted(() => ({
  getAuditLogs: vi.fn().mockResolvedValue({ items: [], total: 0, page: 1, pageSize: 50 }),
  clearAuditLogs: vi.fn().mockResolvedValue(2),
}));
vi.mock("@/lib/api", () => ({ securityApi: { ...auditApi, getSettings: vi.fn().mockResolvedValue({ enabled: true }), updateSettings: (...args: unknown[]) => updateSettings(...args) } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

describe("useSecuritySettings", () => {
  beforeEach(() => updateSettings.mockClear());
  it("updates audit enablement", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryData(securityKeys.rules(), [{ id: "github_token", action: "block", }]);
    const wrapper = ({ children }: { children: ReactNode }) => createElement(QueryClientProvider, { client }, children);
    const { result } = renderHook(() => useSecuritySettings(), { wrapper });
    await act(async () => {
      await result.current.updateSettings({ enabled: true });
    });
    await waitFor(() => expect(client.getQueryState(securityKeys.rules())?.isInvalidated).toBe(true));
  });
});

describe("legacy DLP clear cache compatibility", () => {
  it("preserves the old clear IPC and invalidates both legacy domains plus unified summaries/pages", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client }, children);
    const { result, unmount } = renderHook(
      () => useSecurityAuditLogs({}, 1, 50),
      { wrapper },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      expect(await result.current.clearLogs(123)).toBe(2);
    });
    expect(auditApi.clearAuditLogs).toHaveBeenLastCalledWith(123);
    expect(client.getMutationCache().getAll()[0]?.options.mutationKey).toEqual([
      "security", "clearLogs",
    ]);
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
    unmount();
    client.clear();
  });
});
