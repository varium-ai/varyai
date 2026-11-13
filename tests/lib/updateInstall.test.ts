import { expect, it, vi } from "vitest";
import { Channel, invoke } from "@tauri-apps/api/core";
import { settingsApi } from "@/lib/api/settings";
import type { UpdateInstallProgress } from "@/lib/updater";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage?: (value: UpdateInstallProgress) => void;
  },
}));

it("routes progress through the matching install channel and preserves cancellation results", async () => {
  vi.mocked(invoke).mockResolvedValue("cancelled");
  const onProgress = vi.fn();
  expect(
    await settingsApi.installUpdateAndRestart("synthetic-attempt", onProgress),
  ).toBe("cancelled");
  expect(invoke).toHaveBeenCalledWith("install_update_and_restart", {
    requestId: "synthetic-attempt",
    onProgress: expect.any(Channel),
  });
  const args = vi.mocked(invoke).mock.calls.at(-1)?.[1] as {
    onProgress: Channel<UpdateInstallProgress>;
  };
  args.onProgress.onmessage({
    phase: "downloading",
    downloaded: 42,
    total: null,
  });
  expect(onProgress).toHaveBeenCalledWith({
    phase: "downloading",
    downloaded: 42,
    total: null,
  });
  vi.mocked(invoke).mockResolvedValue(true);
  expect(await settingsApi.cancelUpdateDownload("synthetic-attempt")).toBe(
    true,
  );
  expect(invoke).toHaveBeenLastCalledWith("cancel_update_download", {
    requestId: "synthetic-attempt",
  });
});
