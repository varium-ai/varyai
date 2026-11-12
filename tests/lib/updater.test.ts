import { beforeEach, expect, it, vi } from "vitest";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { checkForUpdate } from "@/lib/updater";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
beforeEach(() => {
  vi.mocked(getVersion).mockResolvedValue("0.2.1");
});
it("reads release notes from the Tauri Update body field", async () => {
  vi.mocked(check).mockResolvedValue({
    version: "0.3.0",
    body: "## Fixes\n- Keep accounts.",
    date: "2026-11-11T00:00:00Z",
  } as never);
  expect(await checkForUpdate()).toEqual({
    status: "available",
    info: {
      currentVersion: "0.2.1",
      availableVersion: "0.3.0",
      notes: "## Fixes\n- Keep accounts.",
      pubDate: "2026-11-11T00:00:00Z",
    },
  });
});
it("keeps an absent update as up-to-date", async () => {
  vi.mocked(check).mockResolvedValue(null);
  expect(await checkForUpdate()).toEqual({ status: "up-to-date" });
});
