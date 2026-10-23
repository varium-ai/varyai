import { toast } from "sonner";
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useSecurityProtection } from "@/hooks/useSecurityProtection";
import { piApi, type PiRoutingStatus } from "@/lib/api/pi";
import { securityApi } from "@/lib/api/security";
import { settingsApi } from "@/lib/api/settings";
vi.mock("@/lib/api/pi", () => ({ piApi: { getRoutingStatus: vi.fn() } }));
vi.mock("@/lib/api/settings", () => ({ settingsApi: { get: vi.fn() } }));
const auditState = vi.hoisted(() => ({ enabled: false }));
vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecuritySettings: () => ({
    settings: auditState,
    isLoading: false,
  }),
}));
vi.mock("@/lib/api/security", () => ({
  securityApi: { getSettings: vi.fn(), updateSettings: vi.fn() },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn(), info: vi.fn() },
}));
const wrapper = ({ children }: { children: ReactNode }) => (
  <QueryClientProvider
    client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
  >
    {children}
  </QueryClientProvider>
);
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ensureSecurityRoute,
  ensureAddedSecurityRoutes,
} from "@/hooks/useSecurityProtection";
import { proxyApi } from "@/lib/api/proxy";
import { providersApi } from "@/lib/api/providers";
import { setCurrentAppProtection } from "@/hooks/useSecurityProtection";
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getProxyStatus: vi.fn(),
    getProxyTakeoverStatus: vi.fn(),
    setProxyTakeoverForApp: vi.fn(),
    startProxyServer: vi.fn(),
    getAgentInstallations: vi.fn(),
  },
}));
vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopStatus: vi.fn(),
    getCurrent: vi.fn(),
    getAll: vi.fn(),
  },
}));
describe("current app protection", () => {
  beforeEach(() => vi.resetAllMocks());
  it("enables the TRAE native bridge from the same home protection control", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: true,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus)
      .mockResolvedValueOnce({ trae: false } as any)
      .mockResolvedValue({ trae: true } as any);
    vi.mocked(securityApi.getSettings).mockResolvedValue({
      enabled: true,
    } as any);
    await setCurrentAppProtection("trae", true);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith("trae", true);
  });
  it("keeps TRAE enabled while waiting for the user to open it, without claiming a connection", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: true,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
      trae: false,
      trae_waiting: true,
    } as any);
    vi.mocked(securityApi.getSettings).mockResolvedValue({
      enabled: true,
    } as any);
    await expect(setCurrentAppProtection("trae", true)).resolves.not.toThrow();
  });
  it("enables only this app and turns on detection without activating other apps", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: true,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus)
      .mockResolvedValueOnce({} as any)
      .mockResolvedValue({ qoder: true } as any);
    vi.mocked(securityApi.getSettings).mockResolvedValue({
      enabled: false,
    } as any);
    await setCurrentAppProtection("qoder", true);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledTimes(1);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith("qoder", true);
    expect(securityApi.updateSettings).toHaveBeenCalledWith({ enabled: true });
    expect(settingsApi.get).not.toHaveBeenCalled();
    expect(proxyApi.getAgentInstallations).not.toHaveBeenCalled();
  });
  it("disabling this app does not disable global detection or other apps", async () => {
    await setCurrentAppProtection("codex", false);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledTimes(1);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith(
      "codex",
      false,
    );
    expect(securityApi.updateSettings).not.toHaveBeenCalled();
  });
  it("keeps detection off when takeover fails", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: false,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({} as any);
    vi.mocked(proxyApi.setProxyTakeoverForApp).mockRejectedValue(
      new Error("cancelled"),
    );
    await expect(setCurrentAppProtection("qoder", true)).rejects.toThrow();
    expect(securityApi.updateSettings).not.toHaveBeenCalled();
  });
});
describe("ensureSecurityRoute", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });
  it("starts takeover for only the requested app and verifies it", async () => {
    vi.mocked(proxyApi.getProxyStatus)
      .mockResolvedValueOnce({ running: false } as any)
      .mockResolvedValue({ running: true } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus)
      .mockResolvedValueOnce({} as any)
      .mockResolvedValue({ codex: true } as any);
    await ensureSecurityRoute("codex");
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith("codex", true);
  });
  it("does not change a ready route", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: true,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
      claude: true,
    } as any);
    await ensureSecurityRoute("claude");
    expect(proxyApi.setProxyTakeoverForApp).not.toHaveBeenCalled();
  });
  it("fails when takeover does not become effective", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: false,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({} as any);
    await expect(ensureSecurityRoute("codex")).rejects.toThrow();
  });
  it("rejects unsupported apps without changing routes", async () => {
    await expect(ensureSecurityRoute("opencode")).rejects.toThrow();
    expect(proxyApi.setProxyTakeoverForApp).not.toHaveBeenCalled();
  });
  it("does not claim Desktop direct mode is protected", async () => {
    vi.mocked(providersApi.getClaudeDesktopStatus).mockResolvedValue({
      configured: true,
      mode: "direct",
    } as any);
    await expect(ensureSecurityRoute("claude-desktop")).rejects.toThrow();
    expect(proxyApi.startProxyServer).not.toHaveBeenCalled();
  });
});

it("activates the official WorkBuddy route before claiming message protection", async () => {
  vi.resetAllMocks();
  vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
    running: true,
  } as any);
  vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
    workbuddy: false,
  } as any);
  vi.mocked(proxyApi.setProxyTakeoverForApp).mockImplementation(async () => {
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
      workbuddy: true,
    } as any);
  });
  await ensureSecurityRoute("workbuddy");
  expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith(
    "workbuddy",
    true,
  );
});

describe("protection mutation", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(providersApi.getCurrent).mockResolvedValue("configured");
    vi.mocked(providersApi.getAll).mockResolvedValue({
      configured: { id: "configured", settingsConfig: {} },
    } as any);
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: false,
    } as any);
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({} as any);
    vi.mocked(settingsApi.get).mockResolvedValue({
      visibleApps: { codex: true },
    } as any);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      codex: true,
    });
  });
  it.each(["codex", "trae", "doubaowork"])(
    "recommends a new conversation when enabling %s, including waiting connections",
    async (app) => {
      vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
        running: true,
      } as any);
      vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
        [app]: app === "codex",
        [`${app}_waiting`]: app !== "codex",
      } as any);
      vi.mocked(securityApi.getSettings).mockResolvedValue({
        enabled: true,
      } as any);
      const { result } = renderHook(() => useSecurityProtection(app), {
        wrapper,
      });
      await act(async () => {
        await result.current.setAppEnabled(true);
      });
      expect(app === "codex" ? toast.success : toast.info).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          description: "homeAudit.newConversationNotice",
        }),
      );
    },
  );

  it.each([
    ["qwen", "homeAudit.qwenSetup", "homeAudit.qwenSetup"],
    ["qwen", new Error("homeAudit.qwenSetup"), "homeAudit.qwenSetup"],
    ["codex", "homeAudit.qwenSetup", "homeAudit.appProtectionFailed"],
    [
      "qwen",
      new Error("synthetic-private-upstream-detail"),
      "homeAudit.appProtectionFailed",
    ],
  ] as const)(
    "uses a safe configuration hint for %s without exposing unknown errors",
    async (app, error, expected) => {
      vi.mocked(proxyApi.setProxyTakeoverForApp).mockRejectedValue(error);
      const { result } = renderHook(() => useSecurityProtection(app), {
        wrapper,
      });
      await act(async () => {
        await expect(result.current.setAppEnabled(true)).rejects.toBe(error);
      });
      if (expected === "homeAudit.qwenSetup") {
        expect(toast.info).toHaveBeenCalledWith(expected);
        expect(toast.error).not.toHaveBeenCalled();
      } else {
        expect(toast.error).toHaveBeenCalledWith(expected);
        expect(toast.info).not.toHaveBeenCalled();
      }
      expect(toast.success).not.toHaveBeenCalled();
      expect(securityApi.updateSettings).not.toHaveBeenCalled();
    },
  );
  it("enables detection and reports an agent failure without blocking the global switch", async () => {
    vi.mocked(proxyApi.setProxyTakeoverForApp).mockRejectedValue(
      new Error("port busy"),
    );
    vi.mocked(securityApi.getSettings).mockResolvedValue({
      enabled: false,
    } as any);
    const { result } = renderHook(() => useSecurityProtection("codex"), {
      wrapper,
    });
    await act(async () => {
      await result.current.setEnabled(true);
    });
    expect(securityApi.updateSettings).toHaveBeenCalledWith({ enabled: true });
    expect(toast.warning).toHaveBeenCalledWith(
      "homeAudit.batchPartial",
      expect.anything(),
    );
  });
  it.each(["codex", "workbuddy"] as const)(
    "disables global audit for %s without altering any route",
    async (app) => {
      vi.mocked(securityApi.getSettings).mockResolvedValue({
        enabled: true,
      });
      const { result } = renderHook(() => useSecurityProtection(app), {
        wrapper,
      });
      await act(async () => {
        await result.current.setEnabled(false);
      });
      expect(proxyApi.setProxyTakeoverForApp).not.toHaveBeenCalled();
      expect(proxyApi.startProxyServer).not.toHaveBeenCalled();
      expect(toast.warning).toHaveBeenCalledWith("homeAudit.disabled", {
        description: "homeAudit.disabledDetail",
      });
      expect(securityApi.updateSettings).toHaveBeenCalledWith({
        enabled: false,
      });
    },
  );
  it.each(["codex", "workbuddy"] as const)(
    "saves audit for %s only after route verification succeeds",
    async (app) => {
      vi.mocked(settingsApi.get).mockResolvedValue({
        visibleApps: { [app]: true },
      } as any);
      vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
        [app]: true,
      });
      vi.mocked(proxyApi.setProxyTakeoverForApp).mockImplementation(
        async () => {
          vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
            running: true,
          } as any);
          vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
            [app]: true,
          } as any);
        },
      );
      vi.mocked(securityApi.getSettings).mockResolvedValue({
        enabled: false,
      } as any);
      const { result } = renderHook(() => useSecurityProtection(app), {
        wrapper,
      });
      await act(async () => {
        await result.current.setEnabled(true);
      });
      expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith(app, true);
      expect(securityApi.updateSettings).toHaveBeenCalledWith({
        enabled: true,
      });
      expect(toast.success).toHaveBeenCalledWith("homeAudit.enabled", {
        description:
          app === "workbuddy"
            ? "homeAudit.protectionDetail homeAudit.appProtectionRestart homeAudit.newConversationNotice"
            : "homeAudit.protectionDetail homeAudit.newConversationNotice",
      });
      expect(
        vi.mocked(proxyApi.setProxyTakeoverForApp).mock.invocationCallOrder[0],
      ).toBeLessThan(
        vi.mocked(securityApi.updateSettings).mock.invocationCallOrder[0],
      );
    },
  );
});

describe("global audit routing prerequisite", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    auditState.enabled = true;
    vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({} as any);
  });
  it("shows audit off when routing is stopped even if the saved flag is on", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: false,
    } as any);
    const { result } = renderHook(() => useSecurityProtection("codex"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.settings?.enabled).toBe(false);
  });
  it("keeps global audit on when routing runs but the selected agent is disconnected", async () => {
    vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
      running: true,
    } as any);
    const { result } = renderHook(() => useSecurityProtection("codex"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.connected).toBe(false);
    expect(result.current.settings?.enabled).toBe(true);
  });
  afterEach(() => {
    auditState.enabled = false;
  });
});

const piReady: PiRoutingStatus = {
  enabled: true,
  configured: true,
  running: true,
  providerIds: ["saved"],
  modelCount: 1,
  issues: [],
  lastRequestAt: null,
  errorCode: null,
};

describe("Pi security route status", () => {
  beforeEach(() => vi.resetAllMocks());
  it.each([
    [{}, true],
    [{ enabled: false }, false],
    [{ configured: false }, false],
    [{ running: false }, false],
    [{ errorCode: "piRouting.conflict" }, false],
  ] as const)(
    "uses actual Pi configuration and runtime status %o",
    async (change, expected) => {
      vi.mocked(piApi.getRoutingStatus).mockResolvedValue({
        ...piReady,
        ...change,
      });
      vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
        running: true,
      } as any);
      vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
        pi: true,
      } as any);
      const { result } = renderHook(() => useSecurityProtection("pi"), {
        wrapper,
      });
      await waitFor(() => expect(result.current.isLoading).toBe(false));
      expect(piApi.getRoutingStatus).toHaveBeenCalled();
      expect(result.current.connected).toBe(expected);
      expect(result.current.supported).toBe(true);
    },
  );
  it("does not report a ready Pi route when its status cannot be read", async () => {
    vi.mocked(piApi.getRoutingStatus).mockRejectedValue(
      new Error("unreadable"),
    );
    const { result } = renderHook(() => useSecurityProtection("pi"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(piApi.getRoutingStatus).toHaveBeenCalled();
    expect(result.current.connected).toBe(false);
  });
  it("verifies the Pi route before enabling audit", async () => {
    vi.mocked(piApi.getRoutingStatus)
      .mockResolvedValueOnce({ ...piReady, configured: false })
      .mockResolvedValue(piReady);
    vi.mocked(securityApi.getSettings).mockResolvedValue({
      enabled: false,
    } as any);
    await setCurrentAppProtection("pi", true);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith("pi", true);
    expect(securityApi.updateSettings).toHaveBeenCalledWith({ enabled: true });
  });
  it("keeps audit disabled when Pi configuration is still invalid", async () => {
    vi.mocked(piApi.getRoutingStatus).mockResolvedValue({
      ...piReady,
      errorCode: "piRouting.conflict",
    });
    await expect(setCurrentAppProtection("pi", true)).rejects.toThrow();
    expect(securityApi.updateSettings).not.toHaveBeenCalled();
  });
  it("activates saved Pi sources without requiring a native default provider", async () => {
    vi.mocked(settingsApi.get).mockResolvedValue({
      visibleApps: { pi: true },
    } as any);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({ pi: true });
    vi.mocked(providersApi.getCurrent).mockResolvedValue("");
    vi.mocked(providersApi.getAll).mockResolvedValue({
      saved: { id: "saved", name: "Saved Pi source", settingsConfig: {} },
    });
    vi.mocked(piApi.getRoutingStatus)
      .mockResolvedValueOnce({ ...piReady, enabled: false })
      .mockResolvedValue(piReady);
    const result = await ensureAddedSecurityRoutes();
    expect(result.activated).toEqual(["pi"]);
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith("pi", true);
    expect(providersApi.getCurrent).not.toHaveBeenCalled();
  });
});
