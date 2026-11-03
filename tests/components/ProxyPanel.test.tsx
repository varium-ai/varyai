import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import { toast } from "sonner";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

vi.mock("@/components/routing/RoutingClient", () => ({
  RoutingClient: () => <span>Codex</span>,
}));
vi.mock("@/components/proxy/GatewayKeysPanel", () => ({
  GatewayKeysPanel: () => <div />,
}));

const { state, setTakeover, stopProxyServer } = vi.hoisted(() => ({
  state: { takeoverStatus: {} as Record<string, boolean> },
  setTakeover: vi.fn(),
  stopProxyServer: vi.fn(),
}));
vi.mock("@/lib/query/proxy", () => ({
  useProxyStatusQuery: () => ({
    data: {
      running: true,
      address: "127.0.0.1",
      port: 15721,
      active_connections: 0,
      total_requests: 0,
      success_rate: 0,
    },
  }),
  useProxyTakeoverStatus: () => ({ data: state.takeoverStatus }),
  useSetProxyTakeoverForApp: () => ({
    mutateAsync: setTakeover,
    isPending: false,
  }),
  useGlobalProxyConfig: () => ({ data: undefined }),
  useUpdateGlobalProxyConfig: () => ({ isPending: false }),
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: true,
    takeoverStatus: state.takeoverStatus,
    stopProxyServer,
    setTakeoverForApp: setTakeover,
    isPending: false,
    isInitialStatusPending: false,
  }),
}));
vi.mock("@/lib/query/failover", () => ({
  useFailoverQueue: () => ({ data: [] }),
  useProviderHealth: () => ({ data: [] }),
}));

describe("ProxyPanel routing applications", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.takeoverStatus = {};
    setTakeover.mockResolvedValue(undefined);
    stopProxyServer.mockResolvedValue(undefined);
  });

  it("lists Desktop after Claude Code and uses the existing gateway control", async () => {
    render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending={false} />);
    const routing = screen.getByText("应用接管").parentElement!;
    expect(
      within(routing)
        .getAllByText(
          /^(Claude|Claude Desktop|Codex|Gemini|Grok Build|WorkBuddy)$/,
        )
        .map((el) => el.textContent),
    ).toEqual([
      "Claude",
      "Claude Desktop",
      "Codex",
      "Gemini",
      "Grok Build",
      "WorkBuddy",
    ]);
    const desktop = within(routing).getByRole("switch", {
      name: "Claude Desktop 本地路由",
    });
    expect(desktop).not.toBeChecked();
    fireEvent.click(desktop);
    await waitFor(() =>
      expect(setTakeover).toHaveBeenCalledWith({
        appType: "claude-desktop",
        enabled: true,
      }),
    );
    expect(stopProxyServer).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "shows Pi routing state %s and uses the shared application control",
    async (enabled) => {
      state.takeoverStatus = { pi: enabled };
      render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending={false} />);
      const pi = screen.getByText("Pi").parentElement!;
      const toggle = within(pi).getByRole("switch");
      expect(toggle).toHaveAttribute("aria-checked", String(enabled));
      fireEvent.click(toggle);
      await waitFor(() =>
        expect(setTakeover).toHaveBeenCalledWith({
          appType: "pi",
          enabled: !enabled,
        }),
      );
      expect(screen.getByText(/Pi.*现有供应商/)).toBeInTheDocument();
    },
  );

  it("disables Pi while the master switch is changing applications", () => {
    render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending />);
    expect(
      within(screen.getByText("Pi").parentElement!).getByRole("switch"),
    ).toBeDisabled();
  });

  it.each(["trae", "doubaowork"])(
    "keeps the %s routing intention checked while waiting and allows cancellation",
    async (app) => {
      state.takeoverStatus = { [app]: false, [`${app}_waiting`]: true };
      render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending={false} />);
      const toggle = screen.getByRole("switch", {
        name: app === "trae" ? "TRAE CN" : "豆包工作",
      });
      expect(toggle).toBeChecked();
      expect(
        screen.getByText(
          app === "trae" ? "homeAudit.traeWaiting" : "homeAudit.doubaoWaiting",
        ),
      ).toBeInTheDocument();
      fireEvent.click(toggle);
      await waitFor(() =>
        expect(setTakeover).toHaveBeenCalledWith({
          appType: app,
          enabled: false,
        }),
      );
    },
  );

  it("reports pending TRAE connection instead of claiming routing is active", async () => {
    setTakeover.mockResolvedValue({ trae: false, trae_waiting: true });
    render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending={false} />);
    fireEvent.click(screen.getByRole("switch", { name: "TRAE CN" }));
    await waitFor(() =>
      expect(toast.info).toHaveBeenCalledWith("homeAudit.traeWaiting", {
        closeButton: true,
      }),
    );
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("disables Desktop while the master switch is changing applications", () => {
    render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending />);
    expect(
      screen.getByRole("switch", { name: "Claude Desktop 本地路由" }),
    ).toBeDisabled();
  });

  it("explains how to initialize a client when enabling routing fails", async () => {
    i18n.addResourceBundle(
      "zh",
      "translation",
      { proxy: { takeover: zh.proxy.takeover } },
      true,
      true,
    );
    setTakeover.mockRejectedValue("proxy.takeover.setupRequired");
    render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending={false} />);
    fireEvent.click(
      within(screen.getByText("Claude").parentElement!).getByRole("switch"),
    );
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        expect.stringContaining("Claude 配置尚未初始化"),
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
    i18n.removeResourceBundle("zh", "translation");
  });
});
