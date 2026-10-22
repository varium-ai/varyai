import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationPermissionBanner } from "./NotificationPermissionBanner";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  request: vi.fn(),
  openSettings: vi.fn(),
  error: vi.fn(),
  onFocusChanged: vi.fn(),
  unlisten: vi.fn(),
}));
vi.mock("@/lib/api/notificationPermissions", () => ({
  notificationPermissionsApi: mocks,
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => mocks }));
vi.mock("sonner", () => ({ toast: { error: mocks.error } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const status = (authorization: string, extra = {}) => ({
  authorization,
  platform: "macos",
  isDev: false,
  canRequest: authorization === "notDetermined",
  canOpenSettings: true,
  alertsEnabled: null,
  soundEnabled: null,
  ...extra,
});
const enable = () =>
  screen.getByRole("button", { name: "settings.permissions.banner.enable" });

describe("NotificationPermissionBanner", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.getStatus.mockResolvedValue(status("denied"));
    mocks.onFocusChanged.mockResolvedValue(mocks.unlisten);
    mocks.request.mockResolvedValue(undefined);
    mocks.openSettings.mockResolvedValue(undefined);
  });

  it("warns when denied and opens system settings only on click", async () => {
    render(<NotificationPermissionBanner />);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "settings.permissions.banner.title",
    );
    expect(mocks.openSettings).not.toHaveBeenCalled();
    expect(mocks.request).not.toHaveBeenCalled();
    fireEvent.click(enable());
    await waitFor(() => expect(mocks.openSettings).toHaveBeenCalledOnce());
    expect(mocks.request).not.toHaveBeenCalled();
  });

  it("requests undetermined permission on click and disappears after approval", async () => {
    mocks.getStatus
      .mockResolvedValueOnce(status("notDetermined"))
      .mockResolvedValue(status("authorized"));
    render(<NotificationPermissionBanner />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.permissions.banner.enable",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("status")).not.toBeInTheDocument(),
    );
    expect(mocks.request).toHaveBeenCalledOnce();
    expect(mocks.openSettings).not.toHaveBeenCalled();
  });

  it.each(["authorized", "ephemeral", "provisional", "unknown"])(
    "does not invent a missing permission for %s",
    async (authorization) => {
      render(<NotificationPermissionBanner />);
      await screen.findByRole("status");
      mocks.getStatus.mockResolvedValue(status(authorization));
      fireEvent.focus(window);
      await waitFor(() =>
        expect(screen.queryByRole("status")).not.toBeInTheDocument(),
      );
    },
  );

  it("warns about disabled alerts even with authorization, and opens settings", async () => {
    mocks.getStatus.mockResolvedValue(
      status("authorized", { alertsEnabled: false }),
    );
    render(<NotificationPermissionBanner />);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "settings.permissions.banner.alertsOff",
    );
    fireEvent.click(enable());
    await waitFor(() => expect(mocks.openSettings).toHaveBeenCalledOnce());
  });

  it("refreshes on native focus and unregisters its listener", async () => {
    const { unmount } = render(<NotificationPermissionBanner />);
    await screen.findByRole("status");
    mocks.getStatus.mockResolvedValue(status("authorized"));
    mocks.onFocusChanged.mock.calls[0][0]({ payload: true });
    await waitFor(() =>
      expect(screen.queryByRole("status")).not.toBeInTheDocument(),
    );
    unmount();
    await waitFor(() => expect(mocks.unlisten).toHaveBeenCalledOnce());
  });

  it("can be dismissed without changing permissions and warns again after a new revocation", async () => {
    render(<NotificationPermissionBanner />);
    fireEvent.click(
      await screen.findByRole("button", { name: "common.close" }),
    );
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    fireEvent.focus(window);
    await waitFor(() => expect(mocks.getStatus).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    mocks.getStatus.mockResolvedValue(status("authorized"));
    fireEvent.focus(window);
    await waitFor(() => expect(mocks.getStatus).toHaveBeenCalledTimes(3));
    mocks.getStatus.mockResolvedValue(status("denied"));
    fireEvent.focus(window);
    expect(await screen.findByRole("status")).toBeInTheDocument();
    expect(mocks.request).not.toHaveBeenCalled();
  });

  it("keeps failures recoverable without claiming permissions were enabled", async () => {
    mocks.openSettings.mockRejectedValue(new Error("failed"));
    render(<NotificationPermissionBanner />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.permissions.banner.enable",
      }),
    );
    await waitFor(() =>
      expect(mocks.error).toHaveBeenCalledWith(
        "settings.permissions.actionFailed",
      ),
    );
    expect(enable()).toBeEnabled();
    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("hides uncertain status after a query failure", async () => {
    mocks.getStatus.mockRejectedValue(new Error("unavailable"));
    render(<NotificationPermissionBanner />);
    await waitFor(() => expect(mocks.getStatus).toHaveBeenCalledOnce());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
