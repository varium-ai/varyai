import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationPermissionSettings } from "./NotificationPermissionSettings";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  request: vi.fn(),
  openSettings: vi.fn(),
  sendNotification: vi.fn(),
  error: vi.fn(),
  success: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false }));
vi.mock("@/lib/api/notificationPermissions", () => ({
  notificationPermissionsApi: mocks,
}));
vi.mock("@/lib/api/notifications", () => ({
  sendNotification: mocks.sendNotification,
}));
vi.mock("sonner", () => ({
  toast: { error: mocks.error, success: mocks.success },
}));
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

describe("NotificationPermissionSettings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.getStatus.mockResolvedValue(status("denied"));
    mocks.openSettings.mockResolvedValue(undefined);
    mocks.sendNotification.mockResolvedValue(undefined);
  });

  it("shows denied and opens system settings without requesting automatically", async () => {
    render(<NotificationPermissionSettings />);
    expect(
      await screen.findByText("settings.permissions.status.denied"),
    ).toBeInTheDocument();
    expect(mocks.request).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "settings.permissions.request" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.permissions.openSettings" }),
    );
    await waitFor(() => expect(mocks.openSettings).toHaveBeenCalledOnce());
  });

  it("requests only on click and refreshes the real status afterwards", async () => {
    mocks.getStatus
      .mockResolvedValueOnce(status("notDetermined"))
      .mockResolvedValue(status("authorized"));
    mocks.request.mockResolvedValue(undefined);
    render(<NotificationPermissionSettings />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.permissions.request",
      }),
    );
    expect(
      await screen.findByText("settings.permissions.status.authorized"),
    ).toBeInTheDocument();
    expect(mocks.request).toHaveBeenCalledOnce();
  });

  it.each(["authorized", "denied", "unknown"])(
    "shows actual %s authorization in macOS development",
    async (authorization) => {
      mocks.getStatus.mockResolvedValue(status(authorization, { isDev: true }));
      render(<NotificationPermissionSettings />);
      expect(
        await screen.findByText(`settings.permissions.status.${authorization}`),
      ).toBeInTheDocument();
      expect(
        screen.getByText("settings.permissions.devHint"),
      ).toBeInTheDocument();
      expect(
        screen.queryByText("settings.permissions.devMode"),
      ).not.toBeInTheDocument();
    },
  );

  it("refreshes on window focus after changing system permissions", async () => {
    render(<NotificationPermissionSettings />);
    await screen.findByText("settings.permissions.status.denied");
    mocks.getStatus.mockResolvedValue(
      status("authorized", { alertsEnabled: false, soundEnabled: true }),
    );
    fireEvent.focus(window);
    expect(
      await screen.findByText("settings.permissions.status.authorized"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("settings.permissions.disabled"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("settings.permissions.enabled"),
    ).toBeInTheDocument();
  });

  it("sends a localized test notification and says only that it was submitted", async () => {
    render(<NotificationPermissionSettings />);
    await screen.findByText("settings.permissions.status.denied");
    fireEvent.click(
      screen.getByRole("button", { name: "settings.permissions.test" }),
    );
    await waitFor(() =>
      expect(mocks.sendNotification).toHaveBeenCalledWith({
        title: "settings.permissions.testTitle",
        body: "settings.permissions.testBody",
      }),
    );
    expect(mocks.success).toHaveBeenCalledWith(
      "settings.permissions.testSubmitted",
    );
  });

  it("keeps query failures visible and allows retry", async () => {
    mocks.getStatus.mockRejectedValueOnce(new Error("unavailable"));
    render(<NotificationPermissionSettings />);
    expect(
      await screen.findByText("settings.permissions.loadFailed"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.permissions.refresh" }),
    );
    expect(
      await screen.findByText("settings.permissions.status.denied"),
    ).toBeInTheDocument();
  });

  it("reports a rejected submission instead of claiming the test was submitted", async () => {
    mocks.sendNotification.mockRejectedValue(
      new Error("notification submission failed"),
    );
    render(<NotificationPermissionSettings />);
    fireEvent.click(
      await screen.findByRole("button", { name: "settings.permissions.test" }),
    );
    await waitFor(() =>
      expect(mocks.error).toHaveBeenCalledWith(
        "settings.permissions.actionFailed",
      ),
    );
    expect(mocks.success).not.toHaveBeenCalled();
  });

  it("does not offer system settings when unsupported", async () => {
    mocks.getStatus.mockResolvedValue(
      status("unknown", { platform: "linux", canOpenSettings: false }),
    );
    render(<NotificationPermissionSettings />);
    expect(
      await screen.findByText("settings.permissions.unsupportedHint"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "settings.permissions.openSettings",
      }),
    ).not.toBeInTheDocument();
  });
});
