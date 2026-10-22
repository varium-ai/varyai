import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "./notifications";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  getStatus: vi.fn(),
  request: vi.fn(),
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("./notificationPermissions", () => ({
  notificationPermissionsApi: mocks,
}));
vi.mock("@tauri-apps/plugin-notification", () => mocks);

function status(authorization: string, extra = {}) {
  return {
    authorization,
    platform: "macos",
    isDev: false,
    canRequest: authorization === "notDetermined",
    ...extra,
  };
}

describe("system notification transport", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.getStatus.mockResolvedValue(status("authorized"));
    mocks.invoke.mockResolvedValue(undefined);
  });

  it("awaits backend submission instead of the legacy fire-and-forget sender", async () => {
    let resolve!: () => void;
    mocks.invoke.mockReturnValue(
      new Promise<void>((done) => {
        resolve = done;
      }),
    );
    const finished = vi.fn();
    const sending = sendNotification({ title: "Title", body: "Body" }).then(
      finished,
    );
    await Promise.resolve();
    expect(finished).not.toHaveBeenCalled();
    expect(mocks.invoke).toHaveBeenCalledWith("send_system_notification", {
      title: "Title",
      body: "Body",
    });
    expect(mocks.sendNotification).not.toHaveBeenCalled();
    resolve();
    await sending;
    expect(finished).toHaveBeenCalledOnce();
  });

  it("propagates native submission failures to the caller", async () => {
    mocks.invoke.mockRejectedValue("notification submission failed");
    await expect(
      sendNotification({ title: "Title", body: "Body" }),
    ).rejects.toBe("notification submission failed");
  });

  it.each(["denied", "notDetermined", "unknown"])(
    "does not trust the plugin's granted status for macOS %s",
    async (authorization) => {
      mocks.getStatus.mockResolvedValue(status(authorization));
      mocks.isPermissionGranted.mockResolvedValue(true);
      expect(await isPermissionGranted()).toBe(false);
      expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
    },
  );

  it.each(["authorized", "provisional", "ephemeral"])(
    "accepts native %s authorization",
    async (authorization) => {
      mocks.getStatus.mockResolvedValue(status(authorization));
      expect(await isPermissionGranted()).toBe(true);
    },
  );

  it("requests native authorization and re-queries the result", async () => {
    mocks.getStatus
      .mockResolvedValueOnce(status("notDetermined"))
      .mockResolvedValue(status("authorized"));
    expect(await requestPermission()).toBe("granted");
    expect(mocks.request).toHaveBeenCalledOnce();
    expect(mocks.requestPermission).not.toHaveBeenCalled();
  });

  it("never retries denied native authorization through the legacy API", async () => {
    mocks.getStatus.mockResolvedValue(status("denied"));
    expect(await requestPermission()).toBe("denied");
    expect(mocks.request).not.toHaveBeenCalled();
    expect(mocks.requestPermission).not.toHaveBeenCalled();
  });

  it.each([
    { platform: "windows", isDev: false },
    { platform: "linux", isDev: false },
  ])("preserves plugin permissions for $platform dev=$isDev", async (extra) => {
    mocks.getStatus.mockResolvedValue(status("unknown", extra));
    mocks.isPermissionGranted.mockResolvedValue(true);
    mocks.requestPermission.mockResolvedValue("granted");
    expect(await isPermissionGranted()).toBe(true);
    expect(await requestPermission()).toBe("granted");
    expect(mocks.request).not.toHaveBeenCalled();
  });

  it("uses native permissions in macOS development, including denial and requests", async () => {
    mocks.getStatus.mockResolvedValue(status("denied", { isDev: true }));
    mocks.isPermissionGranted.mockResolvedValue(true);
    expect(await isPermissionGranted()).toBe(false);
    expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
    mocks.getStatus
      .mockResolvedValueOnce(status("notDetermined", { isDev: true }))
      .mockResolvedValue(status("authorized", { isDev: true }));
    expect(await requestPermission()).toBe("granted");
    expect(mocks.request).toHaveBeenCalledOnce();
    expect(mocks.requestPermission).not.toHaveBeenCalled();
  });

  it("does not fall back to legacy APIs when native permission queries fail", async () => {
    mocks.getStatus.mockRejectedValue(new Error("query failed"));
    await expect(isPermissionGranted()).rejects.toThrow("query failed");
    expect(mocks.isPermissionGranted).not.toHaveBeenCalled();
  });
});
