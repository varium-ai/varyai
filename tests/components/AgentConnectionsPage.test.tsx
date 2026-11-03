import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentConnectionsPage } from "@/components/agents/AgentConnectionsPage";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
import { proxyApi } from "@/lib/api/proxy";
import { settingsApi } from "@/lib/api/settings";
import { intentRoutingApi } from "@/lib/api/intentRouting";
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    contextSettings: vi.fn(async () => ({ fullWindow: false })),
    setContextSettings: vi.fn(async () => {}),
  },
}));
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    list: vi.fn(),
    models: vi.fn(),
    nativeModels: vi.fn(),
    set: vi.fn(),
    reapply: vi.fn(),
    restart: vi.fn(),
    modelVisibility: vi.fn(),
    setModelVisibility: vi.fn(),
  },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getAgentInstallations: vi.fn(async () => ({
      claude: true,
      codex: true,
      pi: true,
    })),
    getProxyStatus: vi.fn(async () => ({ running: true })),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getToolVersions: vi.fn(async () => []),
    getAgentVersions: vi.fn(async () => ({})),
    openExternal: vi.fn(async () => {}),
  },
}));
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <AgentConnectionsPage />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    claude: true,
    codex: true,
    pi: true,
  } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([
    {
      app: "claude",
      enabled: false,
      configured: false,
      model: null,
      recoveryPending: false,
      restartRequired: false,
    },
  ]);
  vi.mocked(agentConnectionsApi.models).mockResolvedValue([
    { id: "vary-auto/shared", name: "Shared group", contextWindow: 1000000 },
  ]);
  vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
    { app: "claude", model: null, available: true },
    { app: "codex", model: null, available: true },
    { app: "pi", model: null, available: true },
  ]);
  vi.mocked(agentConnectionsApi.set).mockResolvedValue();
  vi.mocked(agentConnectionsApi.reapply).mockResolvedValue();
  vi.mocked(agentConnectionsApi.restart).mockResolvedValue();
  vi.mocked(agentConnectionsApi.modelVisibility).mockResolvedValue({
    only: false,
    ids: [],
  });
  vi.mocked(agentConnectionsApi.setModelVisibility).mockResolvedValue();
});
describe("Agent connections", () => {
  it("confirms a stale Desktop restart and never restarts from rendering or cancel", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: true,
        desktopState: "restart_required",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    const restart = within(row).getByRole("button", {
      name: "agentConnections.restart",
    });
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.restartRequired",
    );
    expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    fireEvent.click(restart);
    let dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("agentConnections.restartConfirmHint");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    fireEvent.click(restart);
    dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "agentConnections.restart" }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.restart).toHaveBeenCalledWith(
        "claude-desktop",
        true,
      ),
    );
    expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it.each(["current", "stopped", "unknown", "unsupported"] as const)(
    "does not offer a restart when Desktop state is %s",
    async (desktopState) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "claude-desktop",
          enabled: true,
          configured: true,
          recoveryPending: false,
          model: "vary-auto/shared",
          restartRequired: false,
          desktopState,
        },
      ]);
      mount();
      const row = await screen.findByRole("article", {
        name: "apps.claude-desktop",
      });
      expect(
        within(row).queryByRole("button", { name: "agentConnections.restart" }),
      ).not.toBeInTheDocument();
      expect(agentConnectionsApi.restart).not.toHaveBeenCalled();
    },
  );

  it("keeps configuration drift distinct from a stale running Desktop", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: false,
        recoveryPending: true,
        recoveryReason: "configuration_changed",
        model: "vary-auto/shared",
        restartRequired: true,
        desktopState: "restart_required",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    expect(
      within(row).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
    expect(
      within(row).queryByRole("button", { name: "agentConnections.restart" }),
    ).not.toBeInTheDocument();
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.recovery",
    );
  });

  it("keeps other Agent switches and model pickers independent while connections overlap", async () => {
    let finishClaude!: () => void;
    let finishCodex!: () => void;
    vi.mocked(agentConnectionsApi.set).mockImplementation(
      (app) =>
        new Promise<void>((resolve) => {
          if (app === "claude") finishClaude = resolve;
          if (app === "codex") finishCodex = resolve;
        }),
    );
    mount();
    const claude = await screen.findByRole("switch", { name: "apps.claude" });
    const codex = screen.getByRole("switch", { name: "apps.codex" });
    const pi = screen.getByRole("switch", { name: "apps.pi" });
    await waitFor(() => expect(claude).toBeEnabled());
    fireEvent.click(claude);
    expect(claude).toBeDisabled();
    expect(claude).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getByRole("article", { name: "apps.claude" }),
    ).toHaveTextContent("agentConnections.states.connecting");
    expect(codex).toBeEnabled();
    expect(pi).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "agentConnections.model apps.pi" }),
    ).toBeEnabled();
    fireEvent.click(codex);
    expect(codex).toBeDisabled();
    expect(agentConnectionsApi.set).toHaveBeenCalledTimes(2);
    await act(async () => finishClaude());
    await waitFor(() => expect(claude).toBeEnabled());
    expect(codex).toBeDisabled();
    expect(pi).toBeEnabled();
    await act(async () => finishCodex());
    await waitFor(() => expect(codex).toBeEnabled());
  });

  it("does not hold a completed connection behind a slow native-model refresh", async () => {
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    await waitFor(() =>
      expect(
        screen.getByRole("button", {
          name: "agentConnections.model Claude Code",
        }),
      ).toHaveTextContent("agentConnections.nativeDefault"),
    );
    vi.mocked(agentConnectionsApi.nativeModels).mockImplementationOnce(
      () => new Promise(() => {}),
    );
    fireEvent.click(toggle);
    await waitFor(() => expect(agentConnectionsApi.set).toHaveBeenCalledOnce());
    await waitFor(() => expect(toggle).toBeEnabled());
  });

  it("does not close another Agent's open setup when an earlier connection completes", async () => {
    let finish!: () => void;
    vi.mocked(agentConnectionsApi.set).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        restartRequired: true,
      },
    ]);
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole("article", { name: "apps.codex" }));
    fireEvent.click(
      screen.getByRole("button", { name: "agentConnections.individual" }),
    );
    const dialog = screen.getByRole("dialog");
    await act(async () => finish());
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(dialog).toBeInTheDocument();
  });

  it("leaves unrelated switches usable after a failed connection", async () => {
    vi.mocked(agentConnectionsApi.set).mockRejectedValueOnce(
      "synthetic-private-failure",
    );
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "apps.codex" })).toBeEnabled();
    expect(
      screen.queryByText("synthetic-private-failure"),
    ).not.toBeInTheDocument();
  });
  it("offers shared long-conversation controls only in the relevant tools' details", async () => {
    mount();
    const codex = await screen.findByRole("article", { name: "apps.codex" });
    fireEvent.click(codex);
    expect(
      await within(codex).findByRole("radiogroup", {
        name: "longConversations.title",
      }),
    ).toBeInTheDocument();
    expect(intentRoutingApi.contextSettings).toHaveBeenCalled();
    const pi = screen.getByRole("article", { name: "apps.pi" });
    fireEvent.click(pi);
    expect(
      within(pi).queryByRole("radiogroup", { name: "longConversations.title" }),
    ).not.toBeInTheDocument();
    expect(intentRoutingApi.setContextSettings).not.toHaveBeenCalled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it("shows the native default when a disconnected tool has no explicit model", async () => {
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model Claude Code",
    });
    await waitFor(() =>
      expect(button).toHaveTextContent("agentConnections.nativeDefault"),
    );
    expect(button).not.toHaveTextContent("agentConnections.pickModel");
    fireEvent.click(button);
    const native = await screen.findByRole("radio", {
      name: /agentConnections.nativeDefault/,
    });
    expect(native).toBeChecked();
    expect(native).toBeDisabled();
    expect(screen.getByRole("dialog")).toHaveClass("mp-model-picker-popover");
    expect(native.closest(".mp-shared-picker .mp-am-body")).not.toBeNull();
    expect(screen.getByRole("searchbox")).toHaveFocus();
    fireEvent.scroll(native);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.scroll(document);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(button);
    expect(await screen.findByRole("searchbox")).toHaveFocus();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "Shared" },
    });
    fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("shows a disconnected tool's configured native model instead of a shared default", async () => {
    vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
      { app: "codex", model: "native-sol", available: true },
    ]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model apps.codex",
    });
    await waitFor(() => expect(button).toHaveTextContent("native-sol"));
    fireEvent.click(button);
    expect(
      await screen.findByRole("radio", { name: /native-sol/ }),
    ).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /Shared group/ }),
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: /Shared group/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "codex",
        true,
        "vary-auto/shared",
      ),
    );
  });

  it("does not label an unreadable native config as Default", async () => {
    vi.mocked(agentConnectionsApi.nativeModels).mockResolvedValue([
      { app: "claude", model: null, available: false },
    ]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model Claude Code",
    });
    await waitFor(() =>
      expect(button).toHaveTextContent("agentConnections.nativeUnavailable"),
    );
    expect(button).not.toHaveTextContent("agentConnections.nativeDefault");
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("keeps native defaults visible even when there are no shared models", async () => {
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model Claude Code",
    });
    await waitFor(() =>
      expect(button).toHaveTextContent("agentConnections.nativeDefault"),
    );
    fireEvent.click(button);
    expect(
      await screen.findByText("agentConnections.noModels"),
    ).toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  async function desktop() {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: true,
        recoveryPending: false,
        model: "vary-auto/shared",
        restartRequired: true,
      },
    ]);
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([
      { id: "vary-auto/shared", name: "Shared group", contextWindow: 1000000 },
      { id: "direct-model", name: "Direct model", contextWindow: 200000 },
    ]);
    mount();
    return screen.findByRole("article", { name: "apps.claude-desktop" });
  }

  async function openDesktopList(row: HTMLElement) {
    const trigger = within(row).getByRole("button", {
      name: "agentConnections.modelList apps.claude-desktop",
    });
    await waitFor(() => expect(trigger).toBeEnabled());
    fireEvent.click(trigger);
    return screen.findByRole("dialog");
  }

  it("applies Desktop visibility immediately without setting a starting model", async () => {
    const row = await desktop();
    const dialog = await openDesktopList(row);
    const shared = within(dialog).getByRole("menuitemcheckbox", {
      name: "Shared group",
    });
    fireEvent.click(shared);
    expect(shared).toHaveAttribute("aria-checked", "false");
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
        "claude-desktop",
        { only: false, ids: ["vary-auto/shared"] },
      ),
    );
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Direct model" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      within(dialog).queryByRole("button", { name: "common.save" }),
    ).not.toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("keeps disconnected Desktop visibility independent from its connection switch", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: false,
        configured: false,
        recoveryPending: false,
        model: null,
        restartRequired: false,
      },
    ]);
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      "claude-desktop": true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    const dialog = await openDesktopList(row);
    fireEvent.click(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
        "claude-desktop",
        { only: false, ids: ["vary-auto/shared"] },
      ),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    expect(within(row).getByRole("switch")).not.toBeChecked();
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent.click(within(row).getByRole("switch"));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude-desktop",
        true,
        "vary-auto/shared",
      ),
    );
  });

  it("saves an explicit Desktop allowlist and confirms bulk hiding", async () => {
    const dialog = await openDesktopList(await desktop());
    fireEvent.click(
      within(dialog).getByRole("switch", {
        name: "agentConnections.onlyPicked",
      }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "agentConnections.hideAll" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.confirmHideAll",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
        "claude-desktop",
        { only: true, ids: [] },
      ),
    );
  });

  it("keeps failed Desktop edits open with a safe error", async () => {
    vi.mocked(agentConnectionsApi.setModelVisibility).mockRejectedValue(
      "synthetic-private-error",
    );
    const dialog = await openDesktopList(await desktop());
    fireEvent.click(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(dialog).not.toHaveTextContent("synthetic-private-error");
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("does not expose a Desktop starting model in expanded details", async () => {
    const row = await desktop();
    fireEvent.click(within(row).getByRole("heading"));
    expect(within(row).getByRole("region")).not.toHaveTextContent(
      "vary-auto/shared",
    );
  });

  it("preserves the visible subset when switching to only-picked mode", async () => {
    vi.mocked(agentConnectionsApi.modelVisibility).mockResolvedValue({
      only: false,
      ids: ["vary-auto/shared"],
    });
    const row = await desktop();
    const dialog = await openDesktopList(row);
    fireEvent.click(
      within(dialog).getByRole("switch", {
        name: "agentConnections.onlyPicked",
      }),
    );
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Shared group" }),
    ).toHaveAttribute("aria-checked", "false");
    expect(
      within(dialog).getByRole("menuitemcheckbox", { name: "Direct model" }),
    ).toHaveAttribute("aria-checked", "true");
    await waitFor(() =>
      expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
        "claude-desktop",
        { only: true, ids: ["direct-model"] },
      ),
    );
  });

  it("closes an unchanged Desktop list without saving or changing another agent", async () => {
    const dialog = await openDesktopList(await desktop());
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("disables Desktop visibility changes during connection recovery", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude-desktop",
        enabled: true,
        configured: false,
        recoveryPending: true,
        model: "vary-auto/shared",
        restartRequired: false,
      },
    ]);
    mount();
    const row = await screen.findByRole("article", {
      name: "apps.claude-desktop",
    });
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.modelList apps.claude-desktop",
      }),
    ).toBeDisabled();
  });

  it.each(["off", "connected", "recovery"] as const)(
    "expands a %s row from its name or status without changing its connection",
    async (status) => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        {
          app: "claude",
          enabled: status !== "off",
          configured: status === "connected",
          recoveryPending: status === "recovery",
          restartRequired: false,
          model: status === "off" ? null : "vary-auto/shared",
        },
      ]);
      mount();
      const row = await screen.findByRole("article", { name: "apps.claude" });
      expect(
        within(row).queryByRole("button", {
          name: "agentConnections.more apps.claude",
        }),
      ).not.toBeInTheDocument();
      fireEvent.click(within(row).getByRole("status"));
      const panel = within(row).getByRole("region", {
        name: "agentConnections.details Claude Code",
      });
      const details = within(row).getByRole("button", {
        name: "agentConnections.details Claude Code",
      });
      expect(details).toHaveAttribute("aria-expanded", "true");
      expect(details).toHaveAttribute("aria-controls", panel.id);
      expect(panel).toHaveTextContent(
        status === "off"
          ? "agentConnections.connectHint"
          : status === "recovery"
            ? "agentConnections.reapplyHint"
            : "agentConnections.sessionHint",
      );
      if (status !== "connected")
        expect(panel).not.toHaveTextContent("agentConnections.sessionHint");
      fireEvent.click(panel);
      expect(panel).toBeInTheDocument();
      fireEvent.click(
        within(row).getByRole("heading", { name: "Claude Code" }),
      );
      expect(panel).not.toBeInTheDocument();
      expect(details).toHaveAttribute("aria-expanded", "false");
      fireEvent.click(details);
      expect(
        within(row).getByRole("region", {
          name: "agentConnections.details Claude Code",
        }),
      ).toBeInTheDocument();
      expect(agentConnectionsApi.set).not.toHaveBeenCalled();
      expect(agentConnectionsApi.reapply).not.toHaveBeenCalled();
    },
  );

  it("opens one row at a time and keeps inline recovery actions available", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: false,
        recoveryPending: true,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const claude = await screen.findByRole("article", { name: "apps.claude" });
    const codex = screen.getByRole("article", { name: "apps.codex" });
    fireEvent.click(claude);
    const first = within(claude).getByRole("region");
    fireEvent.click(codex);
    expect(first).not.toBeInTheDocument();
    const panel = within(codex).getByRole("region", {
      name: "agentConnections.details apps.codex",
    });
    expect(panel).toHaveTextContent("agentConnections.reapplyHint");
    expect(
      within(codex).getByRole("button", { name: "agentConnections.reapply" }),
    ).toBeEnabled();
    fireEvent.click(
      within(panel).getByRole("button", { name: "agentConnections.website" }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(
        "https://developers.openai.com/codex/",
      ),
    );
    expect(panel).toBeInTheDocument();
    fireEvent.click(
      within(panel).getByRole("button", { name: "agentConnections.restore" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "codex",
        false,
        null,
      ),
    );
  });

  it("keeps individual settings in connected details without collapsing the row", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        recoveryPending: false,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const row = await screen.findByRole("article", { name: "apps.claude" });
    fireEvent.click(row);
    const panel = within(row).getByRole("region");
    fireEvent.click(
      within(panel).getByRole("button", {
        name: "agentConnections.individual",
      }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("agentConnections.setupHint");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(panel).toBeInTheDocument();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });

  it("does not toggle a row while its text is being selected", async () => {
    mount();
    const row = await screen.findByRole("article", { name: "apps.claude" });
    const selection = vi.spyOn(window, "getSelection").mockReturnValue({
      isCollapsed: false,
    } as Selection);
    try {
      fireEvent.click(
        within(row).getByRole("heading", { name: "Claude Code" }),
      );
      expect(within(row).queryByRole("region")).not.toBeInTheDocument();
    } finally {
      selection.mockRestore();
    }
  });

  it("keeps the installation disclosure separate from the installed tool card", async () => {
    const { container } = mount();
    await screen.findByRole("article", { name: "apps.claude" });
    const disclosure = screen
      .getByText("agentConnections.moreTools")
      .closest("details");
    expect(disclosure).toHaveClass("mp-agent-more");
    expect(disclosure?.closest(".mp-list")).toBeNull();
    expect(container.querySelector(".mp-list")).toContainElement(
      screen.getByRole("article", { name: "apps.claude" }),
    );
  });
  it("shows installed native adapters in the main list and distinguishes audit-only tools", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      gemini: true,
      grokbuild: true,
      openclaw: true,
      hermes: true,
      qoder: true,
      kimi: true,
      qwen: true,
      zcode: true,
      trae: true,
      doubaowork: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    mount();
    for (const app of [
      "gemini",
      "grokbuild",
      "openclaw",
      "hermes",
      "qoder",
      "kimi",
      "qwen",
      "zcode",
      "trae",
      "doubaowork",
    ]) {
      const row = await screen.findByRole("article", { name: `apps.${app}` });
      expect(
        within(row).getByRole("switch", { name: `apps.${app}` }),
      ).toBeEnabled();
      if (["trae", "qoder", "doubaowork"].includes(app)) {
        expect(
          within(row).queryByRole("button", {
            name: `agentConnections.model apps.${app}`,
          }),
        ).not.toBeInTheDocument();
      }
    }
    expect(screen.getByText("agentConnections.auditOnly")).toBeInTheDocument();
  });
  it("connects the audit-only client without requiring a shared catalog", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      doubaowork: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    mount();
    fireEvent.click(
      await screen.findByRole("switch", { name: "apps.doubaowork" }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "doubaowork",
        true,
        null,
      ),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("can connect by picking a remembered model while the native adapter is disconnected", async () => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      kimi: true,
    } as Awaited<ReturnType<typeof proxyApi.getAgentInstallations>>);
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "kimi",
        enabled: false,
        configured: false,
        recoveryPending: false,
        restartRequired: false,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const button = await screen.findByRole("button", {
      name: "agentConnections.model apps.kimi",
    });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    const model = await screen.findByRole("radio", { name: /Shared group/ });
    expect(model).not.toBeChecked();
    fireEvent.click(model);
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "kimi",
        true,
        "vary-auto/shared",
      ),
    );
  });
  it("offers Apply again directly on a drifted Codex row and refreshes its status", async () => {
    const pending = {
      app: "codex",
      enabled: true,
      configured: false,
      model: "vary-auto/shared",
      recoveryPending: true,
      recoveryReason: "configuration_changed" as const,
      restartRequired: false,
    };
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([pending]);
    vi.mocked(agentConnectionsApi.reapply).mockImplementation(async () => {
      vi.mocked(agentConnectionsApi.list).mockResolvedValue([
        { ...pending, configured: true, recoveryPending: false },
      ]);
    });
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    const retry = await within(row).findByRole("button", {
      name: "agentConnections.reapply",
    });
    expect(retry.closest(".mp-agent-name")).not.toBeNull();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.model apps.codex",
      }),
    ).toHaveTextContent("Shared group");
    fireEvent.click(retry);
    await waitFor(() =>
      expect(agentConnectionsApi.reapply).toHaveBeenCalledWith("codex"),
    );
    await waitFor(() =>
      expect(
        within(row).queryByRole("button", {
          name: "agentConnections.reapply",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it("distinguishes inactive routing from changed configuration and sanitizes reapply failures", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "codex",
        enabled: true,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        recoveryReason: "connection_inactive",
        restartRequired: false,
      },
    ]);
    vi.mocked(agentConnectionsApi.reapply).mockRejectedValue(
      "synthetic-private-reapply-error",
    );
    mount();
    const row = await screen.findByRole("article", { name: "apps.codex" });
    expect(within(row).getByRole("status")).toHaveTextContent(
      "agentConnections.states.inactive",
    );
    fireEvent.click(
      within(row).getByRole("button", {
        name: "agentConnections.reapply",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(
      screen.queryByText("synthetic-private-reapply-error"),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", {
        name: "agentConnections.reapply",
      }),
    ).toBeEnabled();
  });
  it("connects a disconnected tool immediately when a starting model is picked", async () => {
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    );
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("radio", { name: /Shared group/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        true,
        "vary-auto/shared",
      ),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("does not reconnect through the model picker while configuration recovery is pending", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: false,
        model: "vary-auto/shared",
        recoveryPending: true,
        restartRequired: false,
      },
    ]);
    mount();
    expect(
      await screen.findByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    ).toBeDisabled();
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
  });
  it("changes a connected tool's starting model from its row and expands details inline", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        model: "vary-auto/shared",
        recoveryPending: false,
        restartRequired: false,
      },
    ]);
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([
      { id: "vary-auto/shared", name: "Shared group", contextWindow: 1000000 },
      {
        id: "vary-provider/second",
        name: "Second model",
        contextWindow: 128000,
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.details Claude Code",
      }),
    );
    expect(
      screen.getByRole("region", {
        name: "agentConnections.details Claude Code",
      }),
    ).toHaveTextContent("agentConnections.sessionHint");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "agentConnections.model Claude Code",
      }),
    );
    fireEvent.click(await screen.findByRole("radio", { name: /Second model/ }));
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        true,
        "vary-provider/second",
      ),
    );
  });
  it("connects an installed Agent directly from its switch without a model or provider dialog", async () => {
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        true,
        "vary-auto/shared",
      ),
    );
  });
  it("opens the shared provider flow when no model is available", async () => {
    vi.mocked(agentConnectionsApi.models).mockResolvedValue([]);
    const add = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <AgentConnectionsPage onAddProvider={add} />
      </QueryClientProvider>,
    );
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", { name: "modelProviders.add" }),
    );
    expect(add).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("disconnects independently of global providers and security settings", async () => {
    vi.mocked(agentConnectionsApi.list).mockResolvedValue([
      {
        app: "claude",
        enabled: true,
        configured: true,
        recoveryPending: false,
        restartRequired: true,
        model: "vary-auto/shared",
      },
    ]);
    mount();
    const toggle = await screen.findByRole("switch", { name: "apps.claude" });
    await waitFor(() => expect(toggle).toBeChecked());
    fireEvent.click(toggle);
    expect(agentConnectionsApi.set).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "agentConnections.disconnectRestore",
      }),
    );
    await waitFor(() =>
      expect(agentConnectionsApi.set).toHaveBeenCalledWith(
        "claude",
        false,
        null,
      ),
    );
  });
  it("reports failed status reads and never presents them as connected", async () => {
    vi.mocked(agentConnectionsApi.list).mockRejectedValue(
      new Error("synthetic-private-configuration"),
    );
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "agentConnections.failed",
    );
    expect(screen.getByRole("switch", { name: "apps.claude" })).toBeDisabled();
    expect(
      screen.queryByText(/synthetic-private-configuration/),
    ).not.toBeInTheDocument();
  });
});
