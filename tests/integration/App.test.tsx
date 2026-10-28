import i18n from "i18next";
import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  screen,
  waitFor,
  fireEvent,
  within,
} from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  resetProviderState,
  getCurrentProviderId,
  getProviders,
  setProviders,
  setCurrentProviderId,
  setSettings,
  getSettings,
} from "../msw/state";
import { emitTauriEvent } from "../msw/tauriMocks";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const skillsPanelMocks = vi.hoisted(() => ({
  checkUpdates: vi.fn(),
  openDiscovery: vi.fn(),
}));
const securityProtectionMock = vi.hoisted(() => ({
  settings: { enabled: true },
  connected: true,
  isLoading: false,
  setEnabled: vi.fn(),
  setAppEnabled: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/security/SecurityCenterPage", () => ({
  SecurityCenterPage: () => <div data-testid="security-center" />,
}));

vi.mock("@/components/statistics/StatisticsPage", () => ({
  StatisticsPage: () => <div data-testid="statistics-page" />,
}));

vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: ({ initialTab }: any) => (
    <div data-testid="settings-page">settings:{initialTab}</div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open, provider, onSubmit, onOpenChange }: any) =>
    open ? (
      <div data-testid="edit-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              provider: {
                ...provider,
                name: `${provider.name}-edited`,
              },
              originalId: provider.id,
            })
          }
        >
          confirm-edit
        </button>
        <button onClick={() => onOpenChange(false)}>close-edit</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, message, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <div data-testid="confirm-message">{message}</div>
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/skills/UnifiedSkillsPanel", async () => {
  const React = await import("react");
  const MockUnifiedSkillsPanel = React.forwardRef(
    ({ onCheckUpdatesStateChange }: any, ref) => {
      React.useEffect(() => {
        onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: true });
        return () =>
          onCheckUpdatesStateChange?.({
            isChecking: false,
            hasSkills: false,
          });
      }, [onCheckUpdatesStateChange]);
      React.useImperativeHandle(ref, () => ({
        openDiscovery: skillsPanelMocks.openDiscovery,
        openImport: vi.fn(),
        openInstallFromZip: vi.fn(),
        openRestoreFromBackup: vi.fn(),
        checkUpdates: skillsPanelMocks.checkUpdates,
      }));
      return <div data-testid="unified-skills-panel" />;
    },
  );
  MockUnifiedSkillsPanel.displayName = "MockUnifiedSkillsPanel";
  return { default: MockUnifiedSkillsPanel };
});

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

vi.mock("@/hooks/useSecurityAudit", () => ({
  useSecurityAuditLogs: () => ({
    logsResult: { total: 0 },
    isError: false,
    isLoading: false,
  }),
}));
vi.mock("@/hooks/useSecurityProtection", () => ({
  useSecurityProtection: () => securityProtectionMock,
}));
vi.mock("@/components/security/SecurityAuditToggle", () => ({
  SecurityAuditToggle: () => <div data-testid="home-audit-toggle" />,
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient();
  const result = render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
  return { ...result, client };
};

describe("App integration with MSW", () => {
  beforeEach(() => {
    resetProviderState();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    skillsPanelMocks.checkUpdates.mockReset();
    skillsPanelMocks.openDiscovery.mockReset();
    securityProtectionMock.connected = true;
    securityProtectionMock.setAppEnabled
      .mockReset()
      .mockResolvedValue(undefined);
    localStorage.removeItem("cc-switch-last-view");
    localStorage.removeItem("cc-switch-last-app");
  });

  it.each([true, false])(
    "exposes the project switcher on the home page only when enabled=%s",
    async (showProfileSwitcher) => {
      setSettings({ showProfileSwitcher, firstRunNoticeConfirmed: true });
      const { default: App } = await import("@/App");
      const { client } = renderApp(App);
      await screen.findByTestId("home-dashboard");
      await waitFor(() =>
        expect(client.getQueryState(["settings"])?.status).toBe("success"),
      );
      const switcher = screen.queryByRole("combobox", {
        name: i18n.t("profiles.switcherAriaLabel", {
          name: i18n.t("profiles.none"),
        }),
      });
      if (showProfileSwitcher) expect(switcher).toBeInTheDocument();
      else expect(switcher).not.toBeInTheDocument();
    },
  );

  it("does not offer unsupported provider configuration for Doubao Work", async () => {
    setSettings({ visibleApps: { ...DEFAULT_VISIBLE_APPS, doubaowork: true } });
    localStorage.setItem("cc-switch-last-app", "doubaowork");
    const { default: App } = await import("@/App");
    renderApp(App);
    await screen.findByText("homeAudit.doubaoAccount");
    expect(
      screen.queryByRole("button", { name: "provider.addNewProvider" }),
    ).not.toBeInTheDocument();
  });

  it.each(["zh", "zh-TW", "ja", "en"] as const)(
    "restores persisted %s language on the home page across restarts despite a stale WebView cache",
    async (language) => {
      const staleLanguage = language === "en" ? "zh" : "en";
      setSettings({ language });
      localStorage.setItem("language", staleLanguage);
      await i18n.changeLanguage(staleLanguage);
      const { default: App } = await import("@/App");
      const first = renderApp(App);
      await waitFor(() => expect(i18n.language).toBe(language));
      expect(localStorage.getItem("language")).toBe(language);
      expect(screen.getByTestId("home-dashboard")).toBeInTheDocument();
      first.unmount();
      localStorage.removeItem("language");
      await i18n.changeLanguage(staleLanguage);
      renderApp(App);
      await waitFor(() => expect(i18n.language).toBe(language));
      expect(localStorage.getItem("language")).toBe(language);
    },
  );

  it.each([
    { running: true, takeover: false, expected: "Jochen的中转站" },
    { running: false, takeover: true, expected: "Jochen的中转站" },
    { running: false, takeover: false, expected: "Jochen的中转站" },
    { running: true, takeover: true, expected: "OpenAI Official" },
  ])(
    "shows the current Codex provider with proxy running=$running and takeover=$takeover",
    async ({ running, takeover, expected }) => {
      localStorage.setItem("cc-switch-last-app", "codex");
      setProviders("codex", {
        jochen: { id: "jochen", name: "Jochen的中转站", settingsConfig: {} },
        official: {
          id: "official",
          name: "OpenAI Official",
          settingsConfig: {},
        },
      });
      setCurrentProviderId("codex", "jochen");
      server.use(
        http.post("http://tauri.local/get_proxy_status", () =>
          HttpResponse.json({
            running,
            active_targets: [{ app_type: "codex", provider_id: "official" }],
          }),
        ),
        http.post("http://tauri.local/get_proxy_takeover_status", () =>
          HttpResponse.json({ codex: takeover }),
        ),
      );
      const { default: App } = await import("@/App");
      const { client } = renderApp(App);
      fireEvent.click(
        await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
      );
      await waitFor(() => {
        expect(client.getQueryState(["proxyStatus"])?.status).toBe("success");
        expect(client.getQueryState(["proxyTakeoverStatus"])?.status).toBe(
          "success",
        );
        const currentSection = screen
          .getByText("provider.inUse")
          .closest("section");
        expect(currentSection).not.toBeNull();
        expect(within(currentSection!).getByText(expected)).toBeInTheDocument();
      });
    },
    15_000,
  );

  it.each([false, true])(
    "shows the selected provider immediately after switching with takeover=%s",
    async (takenOverAfterSwitch) => {
      localStorage.setItem("cc-switch-last-app", "codex");
      setProviders("codex", {
        jochen: { id: "jochen", name: "Jochen的中转站", settingsConfig: {} },
        official: {
          id: "official",
          name: "OpenAI Official",
          category: "official",
          settingsConfig: { auth: {} },
        },
      });
      setCurrentProviderId("codex", "jochen");
      let switched = false;
      server.use(
        http.post("http://tauri.local/get_proxy_status", () =>
          HttpResponse.json({
            running: true,
            active_targets: [
              {
                app_type: "codex",
                provider_id:
                  switched && takenOverAfterSwitch ? "official" : "jochen",
              },
            ],
          }),
        ),
        http.post("http://tauri.local/get_proxy_takeover_status", () =>
          HttpResponse.json({ codex: switched ? takenOverAfterSwitch : true }),
        ),
        http.post("http://tauri.local/switch_provider", async ({ request }) => {
          const { id } = (await request.json()) as { id: string };
          setCurrentProviderId("codex", id);
          switched = true;
          return HttpResponse.json({ warnings: [] });
        }),
      );
      const { default: App } = await import("@/App");
      const { client, unmount } = renderApp(App);
      try {
        fireEvent.click(
          await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
        );
        await waitFor(() => {
          expect(client.getQueryData(["proxyTakeoverStatus"])).toEqual({
            codex: true,
          });
          expect(client.getQueryState(["proxyStatus"])?.status).toBe("success");
        });
        await screen.findByText("OpenAI Official");
        fireEvent.click(
          screen.getByRole("button", { name: "homeAudit.switchTo" }),
        );

        // A normal switch can leave a stale proxy target after CC Switch has
        // reset takeover. A routed switch must also refresh its new target.
        await waitFor(() => {
          expect(toastSuccessMock).toHaveBeenCalled();
          const currentSection = screen
            .getByText("provider.inUse")
            .closest("section");
          expect(
            within(currentSection!).getByText("OpenAI Official"),
          ).toBeInTheDocument();
          expect(
            within(currentSection!).queryByText("Jochen的中转站"),
          ).not.toBeInTheDocument();
        });
        expect(getCurrentProviderId("codex")).toBe("official");
      } finally {
        unmount();
        client.clear();
      }
    },
    15_000,
  );

  it.each(["codex", "workbuddy"] as const)(
    "duplicates a %s card into alternatives without switching",
    async (appId) => {
      setSettings({
        visibleApps: {
          ...DEFAULT_VISIBLE_APPS,
          ...getSettings().visibleApps,
          [appId]: true,
        },
      });
      localStorage.setItem("cc-switch-last-app", appId);
      const source = {
        id: "source",
        name: "Source configuration",
        category: "custom" as const,
        settingsConfig: {
          id: "same-model",
          url: "https://api.example/v1/chat/completions",
          apiKey: "synthetic-copy-key",
        },
      };
      setProviders(appId, { source });
      setCurrentProviderId(appId, source.id);
      const { default: App } = await import("@/App");
      renderApp(App);
      fireEvent.click(
        await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
      );
      await screen.findByText(source.name);
      fireEvent.click(
        await screen.findByRole("button", { name: "provider.duplicate" }),
      );
      await waitFor(() =>
        expect(Object.keys(getProviders(appId))).toHaveLength(2),
      );
      expect(getCurrentProviderId(appId)).toBe(source.id);
      expect(getProviders(appId).source).toEqual(source);
      expect(
        screen.queryByTestId("add-provider-dialog"),
      ).not.toBeInTheDocument();
      const copy = Object.values(getProviders(appId)).find(
        (provider) => provider.id !== source.id,
      )!;
      expect(copy.settingsConfig).toEqual(source.settingsConfig);
      expect(
        await screen.findByRole("button", {
          name: `provider.duplicate ${copy.name}`,
        }),
      ).toBeInTheDocument();
      expect(screen.queryByText("synthetic-copy-key")).not.toBeInTheDocument();
    },
    15_000,
  );

  it("shows Pi membership after enabling and removing without selecting a current provider", async () => {
    localStorage.setItem("cc-switch-last-app", "pi");
    const provider = {
      id: "company",
      name: "Company Pi",
      settingsConfig: {
        baseUrl: "https://pi.example/v1",
        models: [{ id: "model-a" }],
      },
    };
    setProviders("pi", { [provider.id]: provider });
    let enabled = false;
    let changes = 0;
    server.use(
      http.post("http://tauri.local/get_pi_current_state", () =>
        HttpResponse.json({
          enabledProviderIds: enabled ? [provider.id] : [],
          defaultProviderId: "native-default",
        }),
      ),
      http.post("http://tauri.local/switch_provider", async ({ request }) => {
        expect(await request.json()).toEqual({ id: provider.id, app: "pi" });
        enabled = true;
        changes += 1;
        return HttpResponse.json({ warnings: [] });
      }),
      http.post(
        "http://tauri.local/remove_provider_from_live_config",
        async ({ request }) => {
          expect(await request.json()).toEqual({ id: provider.id, app: "pi" });
          enabled = false;
          changes += 1;
          return HttpResponse.json(true);
        },
      ),
    );
    const { default: App } = await import("@/App");
    renderApp(App);
    fireEvent.click(
      await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
    );
    expect(
      await screen.findByRole("heading", { name: provider.name }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("homeAudit.noCurrentProviderForApp"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("homeAudit.selectProviderHint"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "homeAudit.switchTo" }),
    ).not.toBeInTheDocument();

    const enable = await screen.findByRole("button", { name: "启用" });
    await waitFor(() => expect(enable).toBeEnabled());
    fireEvent.click(enable);
    const remove = await screen.findByRole("button", { name: "移除" });
    expect(
      screen.getByRole("heading", { name: provider.name }).closest(".group"),
    ).toHaveClass("border-blue-600");
    expect(changes).toBe(1);
    expect(getCurrentProviderId("pi")).toBe("");
    expect(screen.queryByText("provider.inUse")).not.toBeInTheDocument();

    fireEvent.click(remove);
    fireEvent.click(
      await screen.findByRole("button", { name: "confirm-delete" }),
    );
    await waitFor(() => expect(changes).toBe(2));
    expect(await screen.findByRole("button", { name: "启用" })).toBeEnabled();
    expect(
      screen.getByRole("heading", { name: provider.name }).closest(".group"),
    ).not.toHaveClass("border-blue-600");
    expect(getProviders("pi")[provider.id]).toEqual(provider);
    expect(getCurrentProviderId("pi")).toBe("");
  }, 15_000);

  it("renders the redesigned home dashboard with primary actions", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);
    const dashboard = await screen.findByTestId("home-dashboard");
    expect(dashboard.closest("main")).not.toBeNull();
    expect(dashboard).toHaveClass("max-w-none");
    expect(dashboard).not.toHaveClass("max-w-4xl", "max-w-6xl", "max-w-7xl");
    expect(screen.getByTestId("home-audit-toggle")).toBeInTheDocument();
    expect(screen.getByText("homeAudit.alternatives")).toBeInTheDocument();
    expect(await screen.findByText("Claude Custom")).toBeInTheDocument();
    expect(screen.queryByText("sk-l••••123")).not.toBeInTheDocument();
    expect(
      screen.queryByText("sk-live-test-secret-123"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("homeAudit.advancedManagement"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("primaryNav.providers")).not.toBeInTheDocument();
  }, 15_000);

  it("starts on the home page instead of restoring the previous view", async () => {
    localStorage.setItem("cc-switch-last-view", "settings");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(await screen.findByTestId("home-dashboard")).toBeInTheDocument();
    expect(screen.queryByText("settings.title")).not.toBeInTheDocument();
  }, 15_000);

  it("scrolls the selected app tab into view after switching", async () => {
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    const scrollIntoViewMock = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoViewMock,
    });

    try {
      const { default: App } = await import("@/App");
      renderApp(App);
      const dashboard = await screen.findByTestId("home-dashboard");
      const firstRunConfirm = await screen.findByRole("button", {
        name: "firstRunNotice.confirm",
      });
      fireEvent.click(firstRunConfirm);
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
      );
      scrollIntoViewMock.mockClear();

      fireEvent.click(screen.getByRole("button", { name: "apps.pi" }));

      await waitFor(() =>
        expect(screen.getByRole("button", { name: "apps.pi" })).toHaveAttribute(
          "aria-current",
          "page",
        ),
      );
      expect(screen.getByTestId("home-dashboard")).toBe(dashboard);
      expect(scrollIntoViewMock).toHaveBeenCalledWith({
        behavior: "smooth",
        block: "nearest",
        inline: "nearest",
      });
    } finally {
      if (originalScrollIntoView) {
        Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
          configurable: true,
          value: originalScrollIntoView,
        });
      } else {
        delete (HTMLElement.prototype as any).scrollIntoView;
      }
    }
  }, 15_000);

  it("keeps the primary actions visible while switching top tabs", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await screen.findByTestId("home-dashboard");
    expect(screen.getByTestId("global-status-bar")).toBeInTheDocument();
    const firstRunConfirm = await screen.findByRole("button", {
      name: "firstRunNotice.confirm",
    });
    fireEvent.click(firstRunConfirm);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );

    for (const tab of ["security", "stats", "settings"] as const) {
      fireEvent.click(
        screen.getByRole("button", { name: `primaryNav.${tab}` }),
      );
      expect(
        screen.getByRole("button", { name: `primaryNav.${tab}` }),
      ).toHaveAttribute("aria-current", "page");
      expect(
        screen.getByRole("button", { name: "provider.addNewProvider" }),
      ).toBeInTheDocument();
      expect(screen.getByTestId("global-status-bar")).toBeInTheDocument();
      if (tab === "security") {
        expect(
          await screen.findByTestId("security-center"),
        ).toBeInTheDocument();
      }
      if (tab === "stats") {
        expect(
          await screen.findByTestId("statistics-page"),
        ).toBeInTheDocument();
      }
      expect(
        screen.queryByRole("button", { name: "common.back" }),
      ).not.toBeInTheDocument();
    }

    fireEvent.click(
      screen.getByRole("button", { name: "provider.addNewProvider" }),
    );
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
  }, 15_000);

  it("shows WorkBuddy audit counts and the shared home configuration layout", async () => {
    setSettings({
      visibleApps: {
        ...DEFAULT_VISIBLE_APPS,
        ...getSettings().visibleApps,
        workbuddy: true,
      },
    });
    setProviders("workbuddy", {
      "workbuddy-official": {
        id: "workbuddy-official",
        name: "WorkBuddy Official",
        category: "official",
        settingsConfig: {},
      },
      "workbuddy-api": {
        id: "workbuddy-api",
        name: "WorkBuddy API",
        settingsConfig: {
          id: "custom-model",
          apiKey: "synthetic-key",
          url: "https://example.com/v1/chat/completions",
        },
      },
    });
    setCurrentProviderId("workbuddy", "workbuddy-official");
    const { default: App } = await import("@/App");
    renderApp(App);
    await screen.findByTestId("home-dashboard");
    fireEvent.click(
      await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "apps.workbuddy" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "apps.workbuddy" }),
      ).toHaveAttribute("aria-current", "page"),
    );
    await screen.findByText("WorkBuddy Official");
    expect(
      screen.queryByRole("button", { name: "workbuddy.importModels" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("homeAudit.alternatives")).toBeInTheDocument();
    expect(screen.getByText("homeAudit.auditSummary")).toBeInTheDocument();
    expect(screen.getByText("homeAudit.activeSet")).toBeInTheDocument();
    expect(screen.getByText("WorkBuddy Official")).toBeInTheDocument();
    expect(screen.getByText("provider.inUse")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "common.edit" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("homeAudit.noProvidersForApp"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "homeAudit.configureRouting" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "homeAudit.switchTo" }));
    await waitFor(() =>
      expect(getCurrentProviderId("workbuddy")).toBe("workbuddy-api"),
    );
    await screen.findByRole("button", {
      name: "common.edit WorkBuddy Official",
    });
    fireEvent.click(screen.getByRole("button", { name: "homeAudit.switchTo" }));
    await waitFor(() =>
      expect(getCurrentProviderId("workbuddy")).toBe("workbuddy-official"),
    );
    await screen.findByRole("button", { name: "common.edit WorkBuddy API" });
    fireEvent.click(
      screen.getByRole("button", { name: "homeAudit.addAlternative" }),
    );
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
  }, 15_000);

  it("enables the current app directly from the unrouted protection warning", async () => {
    securityProtectionMock.connected = false;
    const { default: App } = await import("@/App");
    renderApp(App);

    await screen.findByTestId("home-dashboard");
    fireEvent.click(
      screen.getByRole("button", { name: "homeAudit.enableRouting" }),
    );

    expect(securityProtectionMock.setAppEnabled).toHaveBeenCalledWith(true);
    expect(screen.queryByTestId("settings-page")).not.toBeInTheDocument();
  }, 15_000);

  it("resets provider view scroll when switching apps", async () => {
    const { default: App } = await import("@/App");
    const { container } = renderApp(App);

    await screen.findByTestId("home-dashboard");

    const mainScrollContainer = container.querySelector("main") as HTMLElement;
    const providerScrollContainer = Array.from(
      container.querySelectorAll<HTMLElement>(".overflow-y-auto"),
    ).find(
      (element) =>
        element !== mainScrollContainer && element.className.includes("pb-12"),
    );

    expect(mainScrollContainer).not.toBeNull();
    expect(providerScrollContainer).toBeDefined();

    mainScrollContainer.scrollTop = 320;
    mainScrollContainer.scrollLeft = 12;
    providerScrollContainer!.scrollTop = 640;
    providerScrollContainer!.scrollLeft = 24;

    fireEvent.click(screen.getByText("switch-codex"));
    await screen.findByTestId("home-dashboard");

    expect(mainScrollContainer.scrollTop).toBe(0);
    expect(mainScrollContainer.scrollLeft).toBe(0);
    expect(providerScrollContainer!.scrollTop).toBe(0);
    expect(providerScrollContainer!.scrollLeft).toBe(0);
  }, 10_000);

  it("shows toast when auto sync fails in background", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await screen.findByTestId("home-dashboard");

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });

    toastErrorMock.mockReset();
    expect(() => {
      emitTauriEvent("s3-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });

  it("hosts the Skills check-update action in the App toolbar", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(
      await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(await screen.findByTitle("skills.manage"));

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    const checkUpdatesButton = await screen.findByRole("button", {
      name: "skills.checkUpdates",
    });
    await waitFor(() => expect(checkUpdatesButton).toBeEnabled());

    fireEvent.click(checkUpdatesButton);
    expect(skillsPanelMocks.checkUpdates).toHaveBeenCalledTimes(1);
  });

  it("routes the Skills discover toolbar action through the panel guard", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(
      await screen.findByRole("button", { name: "firstRunNotice.confirm" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(await screen.findByTitle("skills.manage"));

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "skills.discover",
      }),
    );

    expect(skillsPanelMocks.openDiscovery).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("unified-skills-panel")).toBeInTheDocument();
  });
});
