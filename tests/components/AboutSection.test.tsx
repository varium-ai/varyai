import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { toast } from "sonner";
import { AboutSection } from "@/components/settings/AboutSection";
import { APP_IDS, APP_ICON_MAP } from "@/config/appConfig";
import { settingsApi } from "@/lib/api";
import zh from "@/i18n/locales/zh.json";

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getToolVersions: vi.fn(),
    getAgentVersions: vi.fn(),
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
  let testTime = Date.now();
  afterEach(() => vi.useRealTimers());
  beforeEach(() => {
    vi.clearAllMocks();
    testTime += 601000;
    vi.setSystemTime(testTime);
    i18n.addResourceBundle("zh", "translation", zh, true, true);
    vi.mocked(settingsApi.getAgentVersions).mockResolvedValue({
      "claude-desktop": {
        supported: true,
        installed: true,
        version: "3.5.1",
        latest_version: null,
        source: "desktop",
      },
      workbuddy: {
        supported: true,
        installed: false,
        version: null,
        latest_version: null,
        source: null,
      },
      qoder: {
        supported: true,
        installed: true,
        version: "0.4.8",
        latest_version: null,
        source: "desktop",
      },
      doubaowork: {
        supported: true,
        installed: false,
        version: null,
        latest_version: null,
        source: null,
      },
      trae: {
        supported: true,
        installed: true,
        version: "3.4.9",
        latest_version: null,
        source: "desktop",
      },
      kimi: {
        supported: true,
        installed: true,
        version: null,
        latest_version: null,
        source: "desktop",
      },
      qwen: {
        supported: true,
        installed: true,
        version: "0.25.2",
        latest_version: "0.26.2",
        source: "cli",
      },
      zcode: {
        supported: true,
        installed: false,
        version: null,
        latest_version: null,
        source: null,
      },
    });
    vi.mocked(settingsApi.getToolVersions).mockImplementation(async (names) =>
      (names ?? []).map((name) => ({
        name,
        version: name === "qwen" ? "0.25.2" : null,
        latest_version: name === "qwen" ? "0.26.2" : null,
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
    expect(await card("Qoder CN").findByText("0.4.8")).toBeInTheDocument();
    expect(
      card("WorkBuddy").getByText(zh.common.notInstalled),
    ).toBeInTheDocument();

    vi.mocked(settingsApi.getToolVersions).mockRejectedValue(
      new Error("Synthetic IPC failure"),
    );
    vi.mocked(settingsApi.getAgentVersions).mockRejectedValue(
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
    const versions = await settingsApi.getAgentVersions();
    versions.qoder.supported = false;
    vi.mocked(settingsApi.getAgentVersions).mockResolvedValue(versions);
    render(<AboutSection isPortable={false} />);
    expect(
      await card("Qoder CN").findByText("当前系统暂不支持检测"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(card("Qwen Code").getByText("0.25.2")).toBeInTheDocument(),
    );
    expect(
      card("Qoder CN").queryByText(zh.common.notInstalled),
    ).not.toBeInTheDocument();
  });

  it("shows desktop and CLI versions, update availability and honest unknown states", async () => {
    render(<AboutSection isPortable={false} />);
    expect(await card("Qoder CN").findByText("0.4.8")).toBeInTheDocument();
    expect(
      card("Qoder CN").getByText(zh.settings.agentLatestUnavailable),
    ).toBeInTheDocument();
    expect(
      card("Qoder CN").queryByText(zh.settings.toolReady),
    ).not.toBeInTheDocument();
    expect(card("Qwen Code").getByText("0.25.2")).toBeInTheDocument();
    expect(card("Qwen Code").getByText("0.26.2")).toBeInTheDocument();
    expect(
      card("Qwen Code").getByText(zh.settings.updateAvailableShort),
    ).toBeInTheDocument();
    expect(
      card("Kimi Code").getByText("版本读取失败，请刷新重试"),
    ).toBeInTheDocument();
    expect(
      card("WorkBuddy").getByText(zh.settings.currentVersion),
    ).toBeInTheDocument();
  });
  it("uses the existing upgrade action for Qwen and refreshes its installed version", async () => {
    vi.mocked(settingsApi.probeToolInstallations).mockResolvedValue([]);
    vi.mocked(settingsApi.runToolLifecycleAction).mockResolvedValue(undefined);
    render(<AboutSection isPortable={false} />);
    const button = await card("Qwen Code").findByRole("button", {
      name: zh.settings.toolUpdate,
    });
    vi.mocked(settingsApi.getToolVersions).mockResolvedValue([
      {
        name: "qwen",
        version: "0.26.2",
        latest_version: "0.26.2",
        error: null,
        installed_but_broken: false,
        env_type: "macos",
        wsl_distro: null,
      },
    ]);
    fireEvent.click(button);
    await waitFor(() =>
      expect(settingsApi.runToolLifecycleAction).toHaveBeenCalledWith(
        ["qwen"],
        "update",
        {},
      ),
    );
    expect(
      await card("Qwen Code").findByText(zh.settings.toolReady),
    ).toBeInTheDocument();
  });
  it("upgrades desktop apps inside Vary AI, shares the action lock and refreshes the card", async () => {
    const versions = await settingsApi.getAgentVersions();
    versions.qoder.latest_version = "0.4.11";
    versions.kimi.version = "1.1.0";
    versions.kimi.latest_version = "1.1.4";
    vi.mocked(settingsApi.getAgentVersions).mockResolvedValue(versions);
    let finish!: () => void;
    vi.mocked(settingsApi.updateDesktopAgent).mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    render(<AboutSection isPortable={false} />);
    const upgrade = await card("Qoder CN").findByRole("button", {
      name: zh.settings.toolUpdate,
    });
    expect(card("Qoder CN").getByText("0.4.11")).toBeInTheDocument();
    expect(screen.getByText("Qoder CN").closest("article")).toHaveAttribute(
      "class",
      screen.getByText("Codex").closest("article")!.className,
    );
    fireEvent.click(upgrade);
    await waitFor(() =>
      expect(settingsApi.updateDesktopAgent).toHaveBeenCalledWith("qoder"),
    );
    expect(upgrade).toBeDisabled();
    expect(
      card("Kimi Code").getByRole("button", { name: zh.settings.toolUpdate }),
    ).toBeDisabled();
    expect(
      card("Qwen Code").getByRole("button", { name: zh.settings.toolUpdate }),
    ).toBeDisabled();
    vi.mocked(settingsApi.getAgentVersions).mockResolvedValue({
      ...versions,
      qoder: { ...versions.qoder, version: "0.4.11" },
    });
    finish();
    expect(
      await card("Qoder CN").findByText(zh.settings.toolReady),
    ).toBeInTheDocument();
    expect(
      card("Kimi Code").getByRole("button", { name: zh.settings.toolUpdate }),
    ).toBeEnabled();
    expect(settingsApi.openExternal).not.toHaveBeenCalled();
  });

  it("includes desktop agents in update-all and continues after a failed desktop update", async () => {
    const versions = await settingsApi.getAgentVersions();
    versions.qoder.latest_version = "0.4.11";
    versions.kimi.version = "1.1.0";
    versions.kimi.latest_version = "1.1.4";
    vi.mocked(settingsApi.getAgentVersions).mockResolvedValue(versions);
    vi.mocked(settingsApi.probeToolInstallations).mockResolvedValue([]);
    const order: string[] = [];
    vi.mocked(settingsApi.runToolLifecycleAction).mockImplementation(
      async (tools) => {
        order.push(...tools);
      },
    );
    vi.mocked(settingsApi.updateDesktopAgent).mockImplementation(
      async (app) => {
        order.push(app);
        if (app === "qoder") throw "signature";
        vi.mocked(settingsApi.getAgentVersions).mockResolvedValue({
          ...versions,
          kimi: { ...versions.kimi, version: "1.1.4" },
        });
      },
    );
    render(<AboutSection isPortable={false} />);
    const all = await screen.findByRole("button", {
      name: i18n.t("settings.updateAllTools", { count: 3 }),
    });
    await waitFor(() => expect(all).toBeEnabled());
    vi.mocked(settingsApi.getToolVersions).mockResolvedValue([
      {
        name: "qwen",
        version: "0.26.2",
        latest_version: "0.26.2",
        error: null,
        installed_but_broken: false,
        env_type: "macos",
        wsl_distro: null,
      },
    ]);
    fireEvent.click(all);
    await waitFor(() => expect(order).toEqual(["qwen", "qoder", "kimi"]));
    await waitFor(() => expect(toast.warning).toHaveBeenCalled());
    expect(
      vi.mocked(toast.warning).mock.calls.at(-1)?.[1]?.description,
    ).toContain(zh.settings.agentUpdateErrors.signature);
    expect(
      await card("Kimi Code").findByText(zh.settings.toolReady),
    ).toBeInTheDocument();
    expect(
      card("Qoder CN").getByRole("button", { name: zh.settings.toolUpdate }),
    ).toBeEnabled();
  });
});
