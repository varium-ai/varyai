import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { AboutSection } from "@/components/settings/AboutSection";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { proxyApi, settingsApi } from "@/lib/api";
import { isMac } from "@/lib/platform";
import zh from "@/i18n/locales/zh.json";

vi.mock("@/lib/api", () => ({
  proxyApi: { getAgentInstallations: vi.fn() },
  settingsApi: { getToolVersions: vi.fn() },
}));
vi.mock("@/lib/platform", () => ({
  isMac: vi.fn(() => true),
  isWindows: () => false,
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    checkUpdate: vi.fn(),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.0.0-test"),
}));

function card(name: string) {
  return within(screen.getByText(name).closest("article")!);
}

describe("AboutSection agent environment checks", () => {
  beforeEach(() => {
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    vi.mocked(isMac).mockReturnValue(true);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      "claude-desktop": true,
      workbuddy: false,
      qoder: true,
      doubaowork: false,
      trae: true,
      kimi: false,
      qwen: true,
      zcode: false,
    });
    vi.mocked(settingsApi.getToolVersions).mockImplementation(async (names) =>
      (names ?? []).map((name) => ({
        name,
        version: null,
        latest_version: null,
        installed_but_broken: name === "claude",
        error: null,
        env_type: "macos" as const,
        wsl_distro: null,
      })),
    );
  });

  it("lists every supported agent and distinguishes missing, broken and failed checks", async () => {
    render(<AboutSection isPortable={false} />);

    for (const app of APP_IDS) {
      const label =
        app === "claude"
          ? "Claude Code"
          : app === "gemini"
            ? "Gemini CLI"
            : APP_ICON_MAP[app].label;
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(
      await card("Codex").findByText(zh.common.notInstalled),
    ).toBeInTheDocument();
    expect(
      card("Codex").getByRole("button", { name: zh.settings.toolInstall }),
    ).toBeEnabled();
    expect(
      card("Claude Code").getByText(zh.settings.installedNotRunnable),
    ).toBeInTheDocument();
    expect(
      card("Claude Code").queryByRole("button", {
        name: zh.settings.toolInstall,
      }),
    ).not.toBeInTheDocument();
    expect(await card("Qoder CN").findByText("已安装")).toBeInTheDocument();
    expect(card("WorkBuddy").getByText(zh.common.notInstalled)).toBeInTheDocument();

    vi.mocked(settingsApi.getToolVersions).mockRejectedValue(
      new Error("Synthetic IPC failure"),
    );
    vi.mocked(proxyApi.getAgentInstallations).mockRejectedValue(
      new Error("Synthetic IPC failure"),
    );
    fireEvent.click(screen.getByRole("button", { name: zh.common.refresh }));
    expect(
      await card("Codex").findByText("检测失败，请刷新重试"),
    ).toBeInTheDocument();
    expect(
      card("Codex").queryByRole("button", { name: zh.settings.toolInstall }),
    ).not.toBeInTheDocument();
    expect(
      await card("Qoder CN").findByText("检测失败，请刷新重试"),
    ).toBeInTheDocument();
    expect(
      card("WorkBuddy").queryByText(zh.common.notInstalled),
    ).not.toBeInTheDocument();
  });

  it("does not report desktop clients as missing on platforms without desktop detection", async () => {
    vi.mocked(isMac).mockReturnValue(false);
    render(<AboutSection isPortable={false} />);
    expect(
      await card("Qoder CN").findByText("当前系统暂不支持检测"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(card("Qwen Code").getByText("已安装")).toBeInTheDocument(),
    );
    expect(
      card("Qoder CN").queryByText(zh.common.notInstalled),
    ).not.toBeInTheDocument();
  });
});
