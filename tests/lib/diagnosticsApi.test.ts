import { expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { settingsApi } from "@/lib/api/settings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

it.each([null, { sourceBytes: 200, redactedLines: 1 }])(
  "exports only the application log without source paths",
  async (result) => {
    vi.mocked(invoke).mockResolvedValueOnce(result);
    expect(await settingsApi.exportApplicationLog()).toEqual(result);
    expect(invoke).toHaveBeenCalledWith("export_application_log");
  },
);

it("requires an updated backend without falling back to broad ZIP export", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(
    "Command export_application_log not found",
  );
  await expect(settingsApi.exportApplicationLog()).rejects.toBe(
    "diagnostics.backendUpdateRequired",
  );
  expect(invoke).toHaveBeenCalledOnce();
  expect(invoke).not.toHaveBeenCalledWith("export_diagnostic_logs");
});
