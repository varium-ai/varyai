import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import type { ComponentProps } from "react";
import { GlobalStatusBar } from "@/components/layout/GlobalStatusBar";
import { HomeDashboard } from "@/components/home/HomeDashboard";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import type { Provider } from "@/types";
import zh from "@/i18n/locales/zh.json";
const protection = vi.hoisted(() => ({
  connected: false,
  waiting: false,
  enabled: false,
  busy: false,
  setAppEnabled: vi.fn(),
}));

vi.mock("@/hooks/useSecurityProtection", () => ({
  useSecurityProtection: () => ({
    isLoading: false,
    connected: protection.connected,
    waiting: protection.waiting,
    settings: { enabled: protection.enabled },
    isUpdating: protection.busy,
    setAppEnabled: protection.setAppEnabled,
    setEnabled: vi.fn(),
  }),
}));
vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityAuditLogs: () => ({ isLoading: false, logsResult: { total: 0 } }),
}));
vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: () => <div data-testid="pi-provider-list" />,
}));
vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/QoderCertificatePanel", () => ({
  QoderCertificatePanel: () => null,
}));

const official: Provider = {
  id: "codex-official",
  name: "OpenAI Official",
  category: "official",
  settingsConfig: { auth: {}, config: "" },
};
const custom: Provider = {
  id: "custom",
  name: "Custom gateway",
  settingsConfig: { auth: { OPENAI_API_KEY: "test-key" }, config: "" },
};

function renderDashboard(
  overrides: Partial<ComponentProps<typeof HomeDashboard>> = {},
) {
  const onSwitch = vi.fn();
  render(
    <HomeDashboard
      activeApp="codex"
      visibleApps={DEFAULT_VISIBLE_APPS}
      providers={{ [official.id]: official }}
      currentProviderId=""
      onAppChange={vi.fn()}
      onCreate={vi.fn()}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onDuplicate={vi.fn()}
      onSwitch={onSwitch}
      onOpenSecurity={vi.fn()}
      onOpenRoutingSettings={vi.fn()}
      {...overrides}
    />,
  );
  return { onSwitch };
}

describe("HomeDashboard current provider", () => {
  beforeEach(() => {
    protection.connected = false;
    protection.waiting = false;
    protection.enabled = false;
    protection.busy = false;
    protection.setAppEnabled.mockReset().mockResolvedValue(undefined);
    i18n.addResourceBundle("zh", "translation", zh, true, true);
  });
  it("shows TRAE pending without claiming protection and lets the user cancel", () => {
    protection.waiting = true;
    protection.enabled = true;
    renderDashboard({ activeApp: "trae" });
    expect(screen.getByText(zh.homeAudit.traeWaiting)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: zh.homeAudit.activate }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: zh.homeAudit.traeStopWaiting }),
    );
    expect(protection.setAppEnabled).toHaveBeenCalledWith(false);
  });
  it.each(["codex", "workbuddy", "qoder"] as const)(
    "enables %s protection directly from its home row",
    (app) => {
      renderDashboard({ activeApp: app });
      fireEvent.click(
        screen.getByRole("button", { name: zh.homeAudit.activate }),
      );
      expect(protection.setAppEnabled).toHaveBeenCalledWith(true);
      expect(
        screen.getByRole("button", { name: zh.homeAudit.viewLogs }),
      ).toBeInTheDocument();
    },
  );
  it("explains TRAE IDE and Agent coverage and its managed launch requirement", () => {
    renderDashboard({ activeApp: "trae" });
    const note = screen.getByRole("note", { name: "TRAE 防护说明" });
    const toggle = within(note).getByRole("button", { name: "TRAE 防护说明" });
    const scope = within(note).getByText(zh.homeAudit.traeNoticeScope);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(scope).not.toBeVisible();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(scope).toBeVisible();
    expect(note).toHaveTextContent("IDE 和 Agent 窗口");
    expect(note).toHaveTextContent("完全退出 TRAE");
    expect(note).toHaveTextContent("VaryAI 不会自动打开 TRAE");
    expect(note).not.toHaveTextContent("证书");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(scope).not.toBeVisible();
  });
  it("keeps the TRAE-specific notice off other agents", () => {
    renderDashboard();
    expect(
      screen.queryByRole("note", { name: "TRAE 防护说明" }),
    ).not.toBeInTheDocument();
  });
  it.each(["opencode", "openclaw", "hermes", "pi"] as const)(
    "opens routing settings from the %s home row",
    (app) => {
      protection.enabled = true;
      const onOpenRoutingSettings = vi.fn();
      renderDashboard({ activeApp: app, onOpenRoutingSettings });
      expect(
        screen.queryByRole("button", { name: zh.homeAudit.activate }),
      ).not.toBeInTheDocument();
      fireEvent.click(
        screen.getByRole("button", { name: zh.homeAudit.configureRouting }),
      );
      expect(onOpenRoutingSettings).toHaveBeenCalledOnce();
      expect(protection.setAppEnabled).not.toHaveBeenCalled();
    },
  );
  it.each([
    [false, false, "disabled"],
    [true, false, "notConnected"],
    [true, true, "ready"],
  ] as const)(
    "shows the common Pi status row (audit=%s, route=%s)",
    (enabled, connected, status) => {
      protection.enabled = enabled;
      protection.connected = connected;
      const onOpenSecurity = vi.fn();
      renderDashboard({ activeApp: "pi", providers: {}, onOpenSecurity });
      const row = screen.getByRole("region", { name: zh.homeAudit.label });
      expect(screen.getByTestId("pi-provider-list")).toBeInTheDocument();
      if (status === "ready") {
        expect(
          within(row).getByText(
            zh.homeAudit.auditSummary
              .replace("{{handled}}", "0")
              .replace("{{warned}}", "0"),
          ),
        ).toBeInTheDocument();
        expect(
          within(row).queryByRole("button", { name: zh.homeAudit.activate }),
        ).not.toBeInTheDocument();
      } else {
        expect(within(row).getByText(zh.homeAudit[status])).toBeInTheDocument();
      }
      expect(
        within(row).queryByText(zh.piRouting.description),
      ).not.toBeInTheDocument();
      fireEvent.click(
        within(row).getByRole("button", { name: zh.homeAudit.viewLogs }),
      );
      expect(onOpenSecurity).toHaveBeenCalledOnce();
    },
  );

  it("uses the common audit status in the Pi footer", () => {
    protection.connected = true;
    protection.enabled = true;
    render(<GlobalStatusBar activeApp="pi" isProxyRunning />);
    expect(
      screen.getByText(zh.globalStatus.auditStates.running),
    ).toBeInTheDocument();
  });
  it("shows only the event link when this app is already protected", () => {
    protection.connected = true;
    protection.enabled = true;
    renderDashboard();
    expect(
      screen.queryByRole("button", { name: zh.homeAudit.activate }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: zh.homeAudit.viewLogs }),
    ).toBeInTheDocument();
  });
  it("prevents duplicate clicks while changing protection", () => {
    protection.busy = true;
    renderDashboard();
    expect(
      screen.getByRole("button", { name: zh.homeAudit.activate }),
    ).toBeDisabled();
  });
  it("keeps optional agents off the default tabs and offers the agent manager", () => {
    const onManageAgents = vi.fn();
    renderDashboard({ onManageAgents });
    expect(
      screen.queryByRole("button", { name: "WorkBuddy" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Qoder CN" }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: zh.homeAudit.manageAgents }),
    );
    expect(onManageAgents).toHaveBeenCalledOnce();
  });

  it("does not label the first provider as in use when nothing is selected", () => {
    const { onSwitch } = renderDashboard();
    expect(screen.queryByText("使用中")).not.toBeInTheDocument();
    expect(screen.getByText("OpenAI Official")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "改用这个" }));
    expect(onSwitch).toHaveBeenCalledWith(official);
  });

  it("keeps all providers available when the selected ID is missing", () => {
    renderDashboard({
      currentProviderId: "deleted",
      providers: { [official.id]: official, [custom.id]: custom },
    });
    expect(screen.queryByText("使用中")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "改用这个" })).toHaveLength(2);
    expect(
      screen.queryByText("当前没有为 Codex 添加供应商"),
    ).not.toBeInTheDocument();
  });

  it("labels the selected provider in use even when it is not first", () => {
    renderDashboard({
      currentProviderId: custom.id,
      providers: { [official.id]: official, [custom.id]: custom },
    });
    const activeCard = screen.getByText("使用中").closest("section")!;
    expect(within(activeCard).getByText(custom.name)).toBeInTheDocument();
    expect(
      within(activeCard).queryByText(official.name),
    ).not.toBeInTheDocument();
  });

  it("falls back to the selected provider when a proxy active ID is stale", () => {
    renderDashboard({
      currentProviderId: custom.id,
      activeProviderId: "deleted",
      providers: { [official.id]: official, [custom.id]: custom },
    });
    const activeCard = screen.getByText("使用中").closest("section")!;
    expect(within(activeCard).getByText(custom.name)).toBeInTheDocument();
  });

  it("uses the actual failover provider when the proxy reports a valid ID", () => {
    renderDashboard({
      currentProviderId: official.id,
      activeProviderId: custom.id,
      providers: { [official.id]: official, [custom.id]: custom },
    });
    const activeCard = screen.getByText("使用中").closest("section")!;
    expect(within(activeCard).getByText(custom.name)).toBeInTheDocument();
  });

  it("describes the native Codex official provider as following the current login", () => {
    renderDashboard({ currentProviderId: official.id });
    expect(screen.getByText("跟随 Codex 当前登录")).toBeInTheDocument();
    expect(screen.queryByText("未配置凭证")).not.toBeInTheDocument();
  });

  it("keeps the configured credential label for API key providers", () => {
    const apiKeyOfficial = {
      ...official,
      settingsConfig: { auth: { OPENAI_API_KEY: "test-key" }, config: "" },
    };
    renderDashboard({
      currentProviderId: apiKeyOfficial.id,
      providers: { [apiKeyOfficial.id]: apiKeyOfficial },
    });
    expect(screen.getByText("已配置凭证")).toBeInTheDocument();
    expect(screen.queryByText("跟随 Codex 当前登录")).not.toBeInTheDocument();
  });

  it("keeps the missing credential label for a custom provider", () => {
    const unconfigured = {
      ...custom,
      settingsConfig: { auth: {}, config: "" },
    };
    renderDashboard({
      currentProviderId: unconfigured.id,
      providers: { [unconfigured.id]: unconfigured },
    });
    expect(screen.getByText("未配置凭证")).toBeInTheDocument();
    expect(screen.queryByText("跟随 Codex 当前登录")).not.toBeInTheDocument();
  });

  it("keeps the WorkBuddy official login description", () => {
    const workbuddyOfficial: Provider = {
      id: "workbuddy-official",
      name: "WorkBuddy Official",
      category: "official",
      settingsConfig: {},
    };
    renderDashboard({
      activeApp: "workbuddy",
      currentProviderId: workbuddyOfficial.id,
      providers: { [workbuddyOfficial.id]: workbuddyOfficial },
    });
    expect(screen.getByText("官方登录")).toBeInTheDocument();
    expect(screen.queryByText("未配置凭证")).not.toBeInTheDocument();
    expect(screen.queryByText("跟随 Codex 当前登录")).not.toBeInTheDocument();
  });

  it("keeps WorkBuddy model groups and endpoints visible", () => {
    const workbuddy: Provider = {
      id: "workbuddy-custom",
      name: "WorkBuddy custom",
      settingsConfig: {
        apiKey: "synthetic-key",
        url: "https://example.com/v1/chat/completions",
        models: [{ id: "model-one" }, { id: "model-two" }],
      },
    };
    renderDashboard({
      activeApp: "workbuddy",
      currentProviderId: official.id,
      providers: { [official.id]: official, [workbuddy.id]: workbuddy },
    });
    expect(screen.getByText(/model-one, model-two/)).toBeInTheDocument();
    expect(
      screen.getByText("https://example.com/v1/chat/completions"),
    ).toBeInTheDocument();
    expect(screen.queryByText("synthetic-key")).not.toBeInTheDocument();
  });
});
