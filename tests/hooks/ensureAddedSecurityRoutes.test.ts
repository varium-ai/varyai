import { beforeEach, describe, expect, it, vi } from "vitest";
import { ensureAddedSecurityRoutes } from "@/hooks/useSecurityProtection";
import { settingsApi } from "@/lib/api/settings";
import { proxyApi } from "@/lib/api/proxy";
import { providersApi } from "@/lib/api/providers";
vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getAll: vi.fn(),
    getCurrent: vi.fn(),
    getClaudeDesktopStatus: vi.fn(),
  },
}));

vi.mock("@/lib/api/settings", () => ({ settingsApi: { get: vi.fn() } }));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getAgentInstallations: vi.fn(),
    getProxyStatus: vi.fn(),
    getProxyTakeoverStatus: vi.fn(),
    setProxyTakeoverForApp: vi.fn(),
  },
}));

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(providersApi.getCurrent).mockResolvedValue("configured");
  vi.mocked(providersApi.getAll).mockResolvedValue({
    configured: { id: "configured", settingsConfig: {} },
  } as any);
  vi.mocked(settingsApi.get).mockResolvedValue({
    visibleApps: { codex: true, workbuddy: true, qoder: false },
  } as any);
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    codex: true,
    workbuddy: false,
    qoder: true,
  });
  vi.mocked(proxyApi.getProxyStatus).mockResolvedValue({
    running: true,
  } as any);
  const ready: Record<string, boolean> = {};
  vi.mocked(proxyApi.getProxyTakeoverStatus).mockImplementation(
    async () => ({ ...ready }) as any,
  );
  vi.mocked(proxyApi.setProxyTakeoverForApp).mockImplementation(async (app) => {
    ready[app] = true;
  });
});

describe("global agent protection", () => {
  it("skips unconfigured Qwen and still activates the following agent", async () => {
    vi.mocked(settingsApi.get).mockResolvedValue({
      visibleApps: { qwen: true, zcode: true },
    } as any);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      qwen: true,
      zcode: true,
    });
    vi.mocked(proxyApi.setProxyTakeoverForApp).mockImplementation(
      async (app) => {
        if (app === "qwen") throw "homeAudit.qwenSetup";
        vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
          zcode: true,
        } as any);
      },
    );
    await expect(ensureAddedSecurityRoutes()).resolves.toEqual({
      activated: ["zcode"],
      skipped: ["qwen"],
      failed: [],
    });
  });
  it("skips uninstalled and unselected agents without activating a certificate route", async () => {
    await ensureAddedSecurityRoutes();
    expect(vi.mocked(proxyApi.setProxyTakeoverForApp).mock.calls).toEqual([
      ["codex", true],
    ]);
  });
  it("activates every installed and added agent, including opted-in Qoder", async () => {
    vi.mocked(settingsApi.get).mockResolvedValue({
      visibleApps: { codex: true, workbuddy: true, qoder: true },
    } as any);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      codex: true,
      workbuddy: true,
      qoder: true,
    });
    await ensureAddedSecurityRoutes();
    expect(vi.mocked(proxyApi.setProxyTakeoverForApp).mock.calls).toEqual([
      ["codex", true],
      ["workbuddy", true],
      ["qoder", true],
    ]);
  });
  it("does not treat absence of all clients as an error", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({});
    await expect(ensureAddedSecurityRoutes()).resolves.toEqual({
      activated: [],
      failed: [],
      skipped: [],
    });
    expect(proxyApi.setProxyTakeoverForApp).not.toHaveBeenCalled();
  });
  it("continues to other agents after a real activation failure", async () => {
    vi.mocked(settingsApi.get).mockResolvedValue({
      visibleApps: { codex: true, qoder: true },
    } as any);
    vi.mocked(proxyApi.setProxyTakeoverForApp).mockImplementation(
      async (app) => {
        if (app === "codex") throw new Error("port busy");
        vi.mocked(proxyApi.getProxyTakeoverStatus).mockResolvedValue({
          qoder: true,
        } as any);
      },
    );
    await expect(ensureAddedSecurityRoutes()).resolves.toEqual({
      activated: ["qoder"],
      failed: ["codex"],
      skipped: [],
    });
    expect(proxyApi.setProxyTakeoverForApp).toHaveBeenCalledWith("qoder", true);
  });
  it("skips installed agents that have no selected configuration", async () => {
    vi.mocked(providersApi.getCurrent).mockResolvedValue("");
    await expect(ensureAddedSecurityRoutes()).resolves.toEqual({
      activated: [],
      failed: [],
      skipped: ["codex"],
    });
    expect(proxyApi.setProxyTakeoverForApp).not.toHaveBeenCalled();
  });
});
