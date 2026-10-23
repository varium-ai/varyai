import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";

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
    expect(desktop).toBeChecked();
    fireEvent.click(desktop);
    await waitFor(() => expect(stopProxyServer).toHaveBeenCalledOnce());
    expect(setTakeover).not.toHaveBeenCalled();
    expect(screen.getByText(/Claude Desktop 需先启用/)).toBeInTheDocument();
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

  it("disables Desktop while the master switch is changing applications", () => {
    render(<ProxyPanel onToggleProxy={vi.fn()} isProxyPending />);
    expect(
      screen.getByRole("switch", { name: "Claude Desktop 本地路由" }),
    ).toBeDisabled();
  });
});
