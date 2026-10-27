// @vitest-environment jsdom
import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { useSecurityEventBridge } from "./useSecurityEventBridge";
import { useToolRiskEventBridge } from "./useToolRisk";

const handlers = vi.hoisted(() => new Map<string, () => void>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, callback: () => void) => {
    handlers.set(name, callback);
    return () => handlers.delete(name);
  }),
}));
vi.mock("@/lib/api", () => ({ toolRiskApi: {} }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("security recorded event bridges", () => {
  it.each([
    ["security-audit-recorded", useSecurityEventBridge],
    ["tool-risk-recorded", useToolRiskEventBridge],
  ])(
    "%s marks unified summaries stale while preserving legacy notifications",
    async (name, bridge) => {
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      client.setQueryData(
        ["securityEvents", "summary", { view: "security_events" }],
        { total: 1 },
      );
      const callback = vi.fn();
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      );
      const { unmount } = renderHook(() => bridge(callback), { wrapper });
      await waitFor(() => expect(handlers.has(name)).toBe(true));
      await act(async () => handlers.get(name)?.());
      expect(
        client.getQueryState([
          "securityEvents",
          "summary",
          { view: "security_events" },
        ])?.isInvalidated,
      ).toBe(true);
      expect(callback).toHaveBeenCalled();
      unmount();
      client.clear();
    },
  );
});
