import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { toast } from "sonner";
import { getVersion } from "@tauri-apps/api/app";
import { AboutSection } from "@/components/settings/AboutSection";
import { APP_DISPLAY_NAME, OFFICIAL_WEBSITE_URL } from "@/config/brand";
import { settingsApi } from "@/lib/api";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

const mocks = vi.hoisted(() => ({
  checkUpdate: vi.fn(),
  resetDismiss: vi.fn(),
  isWindows: vi.fn(() => false),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getToolVersions: vi.fn(async () => []),
    getAgentVersions: vi.fn(async () => ({})),
    runToolLifecycleAction: vi.fn(),
    probeToolInstallations: vi.fn(),
    openExternal: vi.fn(),
    updateDesktopAgent: vi.fn(),
  },
}));
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));
vi.mock("@/lib/platform", () => ({
  isMac: () => !mocks.isWindows(),
  isWindows: mocks.isWindows,
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    checkUpdate: mocks.checkUpdate,
    resetDismiss: mocks.resetDismiss,
    isChecking: false,
  }),
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.0.0-test"),
}));

const locales = { zh, en, ja, "zh-TW": zhTW };

describe("AboutSection", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    for (const [language, resources] of Object.entries(locales)) {
      i18n.addResourceBundle(language, "translation", resources, true, true);
    }
    await i18n.changeLanguage("zh");
    mocks.checkUpdate.mockResolvedValue(false);
  });

  it.each(
    Object.entries(locales).flatMap(([language, resources]) =>
      [false, true].map((windows) => ({ language, resources, windows })),
    ),
  )(
    "removes client version and installation controls ($language, Windows=$windows)",
    async ({ language, resources, windows }) => {
      await i18n.changeLanguage(language);
      mocks.isWindows.mockReturnValue(windows);
      const { container, unmount } = render(
        <AboutSection isPortable={false} />,
      );

      expect(await screen.findByText("v0.0.0-test")).toBeInTheDocument();
      expect(screen.getByText(APP_DISPLAY_NAME)).toBeInTheDocument();
      expect(screen.queryByText(resources.settings.localEnvCheck)).toBeNull();
      expect(screen.queryByText(resources.settings.toolDiagnose)).toBeNull();
      expect(
        screen.queryByText(resources.settings.manualInstallCommands),
      ).toBeNull();
      expect(container.querySelector("article")).toBeNull();
      expect(screen.getAllByRole("button")).toHaveLength(2);
      expect(settingsApi.getToolVersions).not.toHaveBeenCalled();
      expect(settingsApi.getAgentVersions).not.toHaveBeenCalled();
      expect(settingsApi.probeToolInstallations).not.toHaveBeenCalled();
      expect(settingsApi.runToolLifecycleAction).not.toHaveBeenCalled();
      expect(settingsApi.updateDesktopAgent).not.toHaveBeenCalled();

      unmount();
      render(<AboutSection isPortable={false} />);
      await waitFor(() => expect(getVersion).toHaveBeenCalledTimes(2));
      expect(settingsApi.getToolVersions).not.toHaveBeenCalled();
      expect(settingsApi.getAgentVersions).not.toHaveBeenCalled();
    },
  );

  it("preserves the app's portable badge and official website", async () => {
    render(<AboutSection isPortable />);
    expect(await screen.findByText("v0.0.0-test")).toBeInTheDocument();
    expect(screen.getByText(zh.settings.portableMode)).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: zh.settings.officialWebsite }),
    );
    expect(settingsApi.openExternal).toHaveBeenCalledExactlyOnceWith(
      OFFICIAL_WEBSITE_URL,
    );
  });

  it("preserves the app update check and up-to-date feedback", async () => {
    render(<AboutSection isPortable={false} />);
    fireEvent.click(
      screen.getByRole("button", { name: zh.settings.checkForUpdates }),
    );
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(zh.settings.upToDate),
    );
    expect(mocks.resetDismiss).toHaveBeenCalledOnce();
    expect(mocks.checkUpdate).toHaveBeenCalledOnce();
  });

  it("does not report up-to-date when an app update is available", async () => {
    mocks.checkUpdate.mockResolvedValue(true);
    render(<AboutSection isPortable={false} />);
    fireEvent.click(
      screen.getByRole("button", { name: zh.settings.checkForUpdates }),
    );
    await waitFor(() => expect(mocks.checkUpdate).toHaveBeenCalledOnce());
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("handles an app update check failure", async () => {
    mocks.checkUpdate.mockRejectedValue(new Error("Synthetic update failure"));
    render(<AboutSection isPortable={false} />);
    fireEvent.click(
      screen.getByRole("button", { name: zh.settings.checkForUpdates }),
    );
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(zh.settings.checkUpdateFailed),
    );
  });
});
