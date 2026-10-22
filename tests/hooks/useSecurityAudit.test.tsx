// @vitest-environment jsdom
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, type ReactNode } from "react";
import { securityKeys, useSecuritySettings } from "@/hooks/useSecurityAudit";

const updateSettings = vi.fn().mockResolvedValue(undefined);
vi.mock("@/lib/api", () => ({ securityApi: { getSettings: vi.fn().mockResolvedValue({ enabled: true }), updateSettings: (...args: unknown[]) => updateSettings(...args) } }));
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
