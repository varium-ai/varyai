import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DoubaoWorkPanel } from "@/components/DoubaoWorkPanel";
import zh from "@/i18n/locales/zh.json";
import i18n from "i18next";
const state = vi.hoisted(() => ({
  connected: false,
  toggle: vi.fn(),
  launch: vi.fn(),
}));
vi.mock("@/hooks/useSecurityProtection", () => ({
  useSecurityProtection: () => ({
    connected: state.connected,
    setAppEnabled: state.toggle,
    isUpdating: false,
  }),
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { launchDoubaoWorkForAudit: state.launch },
}));
function show() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DoubaoWorkPanel />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  state.connected = false;
  state.toggle.mockReset().mockResolvedValue(undefined);
  state.launch.mockReset().mockResolvedValue(undefined);
  i18n.addResourceBundle("zh", "translation", zh, true, true);
});
describe("Doubao Work connection", () => {
  it("shows the new-conversation recommendation without expanding the details", () => {
    show();
    expect(
      screen.getByText(/首次开启或重新开启审计后，建议新建会话/),
    ).toBeVisible();
  });

  it("starts only after an explicit click and offers collapsed connection details", async () => {
    show();
    expect(state.launch).not.toHaveBeenCalled();
    const details = screen.getByRole("button", {
      name: zh.homeAudit.doubaoNoticeTitle,
    });
    expect(details).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(details);
    expect(screen.getByText(zh.homeAudit.doubaoRestore)).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: zh.homeAudit.doubaoLaunch }),
    );
    await waitFor(() => expect(state.launch).toHaveBeenCalledOnce());
    expect(state.toggle).toHaveBeenCalledWith(true);
  });
  it("disconnects an active client without relaunching it", () => {
    state.connected = true;
    show();
    fireEvent.click(
      screen.getByRole("button", { name: zh.homeAudit.doubaoDisconnect }),
    );
    expect(state.toggle).toHaveBeenCalledWith(false);
    expect(state.launch).not.toHaveBeenCalled();
  });
});
