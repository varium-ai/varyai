import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { LogConfigPanel } from "@/components/settings/LogConfigPanel";
import { settingsApi } from "@/lib/api/settings";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getLogConfig: vi.fn(),
    setLogConfig: vi.fn(),
    exportApplicationLog: vi.fn(),
  },
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

beforeEach(() => {
  vi.mocked(settingsApi.getLogConfig).mockResolvedValue({
    enabled: true,
    level: "info",
  });
  vi.mocked(settingsApi.exportApplicationLog).mockResolvedValue({
    sourceBytes: 200,
  });
});

it.each([true, false])(
  "exports diagnostics when logging enabled=%s",
  async (enabled) => {
    vi.mocked(settingsApi.getLogConfig).mockResolvedValue({
      enabled,
      level: "info",
    });
    render(<LogConfigPanel />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.advanced.logConfig.export",
      }),
    );
    await waitFor(() =>
      expect(settingsApi.exportApplicationLog).toHaveBeenCalledOnce(),
    );
    expect(toast.success).toHaveBeenCalledWith(
      "settings.advanced.logConfig.exportSuccess",
    );
  },
);

it("locks export while the native save dialog or log writer is pending", async () => {
  let finish!: (value: null) => void;
  vi.mocked(settingsApi.exportApplicationLog).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  render(<LogConfigPanel />);
  const button = await screen.findByRole("button", {
    name: "settings.advanced.logConfig.export",
  });
  fireEvent.click(button);
  expect(button).toBeDisabled();
  expect(button).toHaveTextContent("settings.advanced.logConfig.exporting");
  fireEvent.click(button);
  expect(settingsApi.exportApplicationLog).toHaveBeenCalledOnce();
  finish(null);
  await waitFor(() => expect(button).toBeEnabled());
  expect(toast.success).not.toHaveBeenCalled();
  expect(toast.error).not.toHaveBeenCalled();
});

it("uses a fixed localized failure message and permits retry", async () => {
  vi.mocked(settingsApi.exportApplicationLog).mockRejectedValueOnce(
    new Error("synthetic private path and token"),
  );
  render(<LogConfigPanel />);
  const button = await screen.findByRole("button", {
    name: "settings.advanced.logConfig.export",
  });
  fireEvent.click(button);
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith(
      "settings.advanced.logConfig.exportFailed",
    ),
  );
  expect(button).toBeEnabled();
  expect(document.body).not.toHaveTextContent("synthetic private");
});

it("keeps export available when log settings cannot be read", async () => {
  vi.mocked(settingsApi.getLogConfig).mockRejectedValueOnce(
    new Error("synthetic settings failure"),
  );
  render(<LogConfigPanel />);
  expect(
    await screen.findByRole("button", {
      name: "settings.advanced.logConfig.export",
    }),
  ).toBeEnabled();
});

it("keeps export independent of a pending settings query", async () => {
  vi.mocked(settingsApi.getLogConfig).mockImplementationOnce(
    () => new Promise(() => {}),
  );
  render(<LogConfigPanel />);
  const button = screen.getByRole("button", {
    name: "settings.advanced.logConfig.export",
  });
  expect(button).toBeEnabled();
  expect(screen.getByRole("switch")).toBeDisabled();
  fireEvent.click(button);
  await waitFor(() =>
    expect(settingsApi.exportApplicationLog).toHaveBeenCalledOnce(),
  );
});

it.each([
  ["en", en],
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const)(
  "provides translated export controls and results in %s",
  async (language, tree) => {
    const instance = createInstance();
    await instance.init({
      lng: language,
      fallbackLng: false,
      resources: { [language]: { translation: tree } },
    });
    for (const key of [
      "export",
      "exporting",
      "exportDescription",
      "exportSuccess",
      "exportMissing",
      "exportEmpty",
      "exportSameSource",
      "exportBackendUpdate",
      "exportFailed",
    ]) {
      expect(instance.exists(`settings.advanced.logConfig.${key}`)).toBe(true);
    }
    expect(tree.settings.advanced.logConfig.enabledDescription).toContain(
      "varyai.log",
    );
    expect(tree.settings.advanced.logConfig.enabledDescription).not.toContain(
      "cc-switch",
    );
    render(
      <I18nextProvider i18n={instance}>
        <LogConfigPanel />
      </I18nextProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: tree.settings.advanced.logConfig.export,
      }),
    );
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(
        tree.settings.advanced.logConfig.exportSuccess,
      ),
    );
  },
);

it.each([
  ["diagnostics.logMissing", "exportMissing"],
  ["diagnostics.logEmpty", "exportEmpty"],
  ["diagnostics.sameSource", "exportSameSource"],
  ["diagnostics.backendUpdateRequired", "exportBackendUpdate"],
])("shows a fixed message for %s", async (error, key) => {
  vi.mocked(settingsApi.exportApplicationLog).mockRejectedValueOnce(error);
  render(<LogConfigPanel />);
  fireEvent.click(
    screen.getByRole("button", { name: "settings.advanced.logConfig.export" }),
  );
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith(
      `settings.advanced.logConfig.${key}`,
    ),
  );
  expect(toast.success).not.toHaveBeenCalled();
});

it.each(["__proto__", "constructor", "toString"])(
  "keeps unrecognized backend errors private: %s",
  async (error) => {
    vi.mocked(settingsApi.exportApplicationLog).mockRejectedValueOnce(error);
    render(<LogConfigPanel />);
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.logConfig.export",
      }),
    );
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "settings.advanced.logConfig.exportFailed",
      ),
    );
  },
);
