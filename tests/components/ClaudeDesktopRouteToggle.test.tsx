import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";

const useProxyStatusMock = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: useProxyStatusMock,
}));
vi.mock("sonner", () => ({ toast: { warning: vi.fn() } }));

describe("ClaudeDesktopRouteToggle in settings", () => {
  let state: {
    isRunning: boolean;
    takeoverStatus: Record<string, boolean> | undefined;
    startProxyServer: ReturnType<typeof vi.fn>;
    stopProxyServer: ReturnType<typeof vi.fn>;
    isPending: boolean;
    isInitialStatusPending: boolean;
  };

  beforeEach(() => {
    state = {
      isRunning: false,
      takeoverStatus: {},
      startProxyServer: vi.fn().mockResolvedValue(undefined),
      stopProxyServer: vi.fn().mockResolvedValue(undefined),
      isPending: false,
      isInitialStatusPending: false,
    };
    useProxyStatusMock.mockImplementation(() => state);
  });

  it("starts the existing gateway and follows its running state", async () => {
    const { rerender } = render(
      <ClaudeDesktopRouteToggle variant="settings" />,
    );
    const control = screen.getByRole("switch", {
      name: "Claude Desktop 本地路由",
    });
    expect(control).not.toBeChecked();
    fireEvent.click(control);
    await waitFor(() => expect(state.startProxyServer).toHaveBeenCalledOnce());
    state.isRunning = true;
    rerender(<ClaudeDesktopRouteToggle variant="settings" />);
    expect(control).toBeChecked();
  });

  it("stops the gateway when no application uses takeover", async () => {
    state.isRunning = true;
    render(<ClaudeDesktopRouteToggle variant="settings" />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(state.stopProxyServer).toHaveBeenCalledOnce());
  });

  it.each(["claude", "codex", "gemini", "grokbuild", "workbuddy", "pi"])(
    "preserves the shared gateway while %s is using it",
    async (app) => {
      state.isRunning = true;
      state.takeoverStatus = { [app]: true };
      render(<ClaudeDesktopRouteToggle variant="settings" />);
      fireEvent.click(screen.getByRole("switch"));
      await waitFor(() => expect(toast.warning).toHaveBeenCalledOnce());
      expect(state.stopProxyServer).not.toHaveBeenCalled();
      expect(screen.getByRole("switch")).toBeChecked();
    },
  );

  it("waits for status and disables controls during routing changes", () => {
    state.isInitialStatusPending = true;
    const { rerender } = render(
      <ClaudeDesktopRouteToggle variant="settings" />,
    );
    expect(screen.getByRole("switch")).toBeDisabled();
    state.isInitialStatusPending = false;
    state.isPending = true;
    rerender(<ClaudeDesktopRouteToggle variant="settings" />);
    expect(screen.getByRole("switch")).toBeDisabled();
    state.isPending = false;
    rerender(<ClaudeDesktopRouteToggle variant="settings" disabled />);
    expect(screen.getByRole("switch")).toBeDisabled();
    expect(state.startProxyServer).not.toHaveBeenCalled();
    expect(state.stopProxyServer).not.toHaveBeenCalled();
  });
});
