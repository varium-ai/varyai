import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeDesktopRouteToggle } from "@/components/proxy/ClaudeDesktopRouteToggle";
const useProxyStatusMock = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: useProxyStatusMock,
}));
describe("Claude Desktop independent routing", () => {
  let state: {
    isRunning: boolean;
    takeoverStatus: Record<string, boolean>;
    setTakeoverForApp: ReturnType<typeof vi.fn>;
    isPending: boolean;
    isInitialStatusPending: boolean;
  };
  beforeEach(() => {
    state = {
      isRunning: false,
      takeoverStatus: {},
      setTakeoverForApp: vi.fn().mockResolvedValue(undefined),
      isPending: false,
      isInitialStatusPending: false,
    };
    useProxyStatusMock.mockImplementation(() => state);
  });
  it("enables Desktop takeover and follows its own state", async () => {
    const view = render(<ClaudeDesktopRouteToggle variant="settings" />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(state.setTakeoverForApp).toHaveBeenCalledWith({
        appType: "claude-desktop",
        enabled: true,
      }),
    );
    state.isRunning = true;
    view.rerender(<ClaudeDesktopRouteToggle variant="settings" />);
    expect(screen.getByRole("switch")).not.toBeChecked();
    state.takeoverStatus = { "claude-desktop": true };
    view.rerender(<ClaudeDesktopRouteToggle variant="settings" />);
    expect(screen.getByRole("switch")).toBeChecked();
  });
  it.each([
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "workbuddy",
    "pi",
    "qoder",
    "kimi",
    "qwen",
    "zcode",
    "trae",
    "doubaowork",
    "trae_waiting",
    "doubaowork_waiting",
  ])("disables only Desktop while %s keeps using the gateway", async (app) => {
    state.isRunning = true;
    state.takeoverStatus = { [app]: true, "claude-desktop": true };
    render(<ClaudeDesktopRouteToggle variant="settings" />);
    expect(screen.getByRole("switch")).toBeEnabled();
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(state.setTakeoverForApp).toHaveBeenCalledExactlyOnceWith({
        appType: "claude-desktop",
        enabled: false,
      }),
    );
  });
  it("waits for initial status and pending changes", () => {
    state.isInitialStatusPending = true;
    const view = render(<ClaudeDesktopRouteToggle />);
    expect(screen.getByRole("switch")).toBeDisabled();
    state.isInitialStatusPending = false;
    state.isPending = true;
    view.rerender(<ClaudeDesktopRouteToggle />);
    expect(screen.getByRole("switch")).toBeDisabled();
    state.isPending = false;
    view.rerender(<ClaudeDesktopRouteToggle disabled />);
    expect(screen.getByRole("switch")).toBeDisabled();
  });
});
