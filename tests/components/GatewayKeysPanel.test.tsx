import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { GatewayKeysPanel } from "@/components/proxy/GatewayKeysPanel";
import { gatewayApi } from "@/lib/api/gateway";
import { APP_IDS, getAppLabel } from "@/config/appConfig";
import { providersApi } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
import { agentConnectionsApi } from "@/lib/api/agentConnections";
vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: { list: vi.fn(), get: vi.fn() },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { startProxyServer: vi.fn(), setProxyTakeoverForApp: vi.fn() },
}));
vi.mock("@/lib/api/providers", () => ({
  providersApi: { getClaudeDesktopStatus: vi.fn() },
}));
vi.mock("@/lib/api/gateway", () => ({
  gatewayApi: {
    list: vi.fn(),
    create: vi.fn(),
    reveal: vi.fn(),
    update: vi.fn(),
    revoke: vi.fn(),
    delete: vi.fn(),
    catalog: vi.fn(),
    bind: vi.fn(),
    usage: vi.fn(),
  },
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
const policy = {
  name: "Synthetic",
  models: ["allowed"],
  tokenLimit: 100,
  period: "daily" as const,
};

it("edits keys in their own cards and focuses the selected usage filter", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    { id: "first", policy, revoked: false, usedTokens: 0, resetsAt: null },
    {
      id: "second",
      policy: { ...policy, name: "Second fixture" },
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  mount();
  const second = (await screen.findByText("Second fixture")).closest(
    "article",
  )!;
  const first = screen.getByText(policy.name).closest("article")!;
  fireEvent.click(
    within(second).getByRole("button", { name: "gatewayKeys.edit" }),
  );
  await waitFor(() =>
    expect(screen.getByLabelText("gatewayKeys.name")).toHaveFocus(),
  );
  expect(within(second).getByLabelText("gatewayKeys.name")).toHaveValue(
    "Second fixture",
  );
  expect(
    within(first).queryByLabelText("gatewayKeys.name"),
  ).not.toBeInTheDocument();
  fireEvent.click(
    within(first).getByRole("button", { name: "gatewayKeys.edit" }),
  );
  expect(within(first).getByLabelText("gatewayKeys.name")).toHaveValue(
    policy.name,
  );
  expect(
    within(second).queryByLabelText("gatewayKeys.name"),
  ).not.toBeInTheDocument();
  fireEvent.click(within(first).getByRole("button", { name: "common.cancel" }));
  expect(screen.queryByLabelText("gatewayKeys.name")).not.toBeInTheDocument();
  fireEvent.click(
    within(second).getByRole("button", { name: "gatewayKeys.viewUsage" }),
  );
  await waitFor(() =>
    expect(screen.getByLabelText("gatewayKeys.filterKey")).toHaveFocus(),
  );
  expect(screen.getByLabelText("gatewayKeys.filterKey")).toHaveValue("second");
});

it("rejects a duplicate key name without submitting and preserves the draft", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    { id: "first", policy, revoked: false, usedTokens: 0, resetsAt: null },
  ]);
  mount();
  await screen.findByText(policy.name);
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.create" }));
  fireEvent.change(screen.getByLabelText("gatewayKeys.name"), {
    target: { value: ` ${policy.name.toUpperCase()} ` },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.add" }));
  expect(await screen.findByText("gatewayKeys.nameExists")).toBeInTheDocument();
  expect(gatewayApi.create).not.toHaveBeenCalled();
  expect(screen.getByLabelText("gatewayKeys.name")).toHaveValue(
    ` ${policy.name.toUpperCase()} `,
  );
});
function mount(
  props: Partial<React.ComponentProps<typeof GatewayKeysPanel>> = {},
) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <GatewayKeysPanel port="15721" {...props} />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(agentConnectionsApi.list).mockResolvedValue([]);
  vi.mocked(agentConnectionsApi.get).mockImplementation(async (app) => ({
    app,
    enabled: false,
    configured: false,
    recoveryPending: false,
    model: null,
    restartRequired: false,
  }));
  vi.mocked(gatewayApi.list).mockResolvedValue([]);
  vi.mocked(gatewayApi.catalog).mockResolvedValue({
    agents: APP_IDS.map((id) => ({
      id,
      localRouting: id !== "openclaw",
    })),
    models: [],
  });
  vi.mocked(gatewayApi.usage).mockResolvedValue([]);
  vi.mocked(providersApi.getClaudeDesktopStatus).mockResolvedValue({
    supported: true,
    configured: true,
    mode: "proxy",
    proxyRunning: true,
    staleRawModels: false,
    missingRouteMappings: false,
    gatewayTokenConfigured: true,
    expectedBaseUrl: "http://127.0.0.1:15721/claude-desktop",
    actualBaseUrl: "http://127.0.0.1:1/old",
  });
  vi.mocked(gatewayApi.create).mockResolvedValue({
    key: {
      id: "synthetic",
      policy,
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
    secret: "synthetic-secret",
  });
});
it("starts with key management and opens creation explicitly", async () => {
  mount();
  expect(screen.queryByLabelText("gatewayKeys.name")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.create" }));
  expect(await screen.findByLabelText("gatewayKeys.name")).toBeInTheDocument();
});
it("creates an exact allowlist without opening the secret dialog", async () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.create" }));
  fireEvent.change(screen.getByLabelText("gatewayKeys.name"), {
    target: { value: "Project" },
  });
  fireEvent.change(screen.getByLabelText("gatewayKeys.models"), {
    target: { value: " a, b, a " },
  });
  fireEvent.change(screen.getByLabelText("gatewayKeys.limit"), {
    target: { value: "50" },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.add" }));
  await waitFor(() =>
    expect(gatewayApi.create).toHaveBeenCalledWith({
      name: "Project",
      models: ["a", "b"],
      allModels: false,
      tokenLimit: 50,
      period: "daily",
    }),
  );
  await waitFor(() =>
    expect(toast.success).toHaveBeenCalledWith("gatewayKeys.created"),
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByText("synthetic-secret")).not.toBeInTheDocument();
});

it("selects one shared route by supplier and preserves a legacy exact ID", async () => {
  const upstreamId = "vendor/shared-model";
  const firstId = "vary-provider/first-route";
  const secondId = "vary-provider/second-route";
  const groupId = "vary-auto/team-route";
  vi.mocked(gatewayApi.catalog).mockResolvedValue({
    agents: [],
    models: [
      { model: firstId, name: `${upstreamId} · Supplier A` },
      { model: secondId, name: `${upstreamId} · Supplier B` },
      { model: groupId, name: "Team route" },
    ],
  });
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "fixture",
      policy: { ...policy, models: [upstreamId] },
      agents: ["gemini", "qoder"],
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "gatewayKeys.edit" }),
  );
  const first = await screen.findByRole("checkbox", {
    name: `${upstreamId} · Supplier A`,
  });
  expect(first).not.toBeChecked();
  fireEvent.change(screen.getByLabelText("gatewayKeys.searchModels"), {
    target: { value: "supplier a" },
  });
  expect(screen.queryByRole("checkbox", { name: /Supplier B/ })).toBeNull();
  fireEvent.click(first);
  fireEvent.change(screen.getByLabelText("gatewayKeys.searchModels"), {
    target: { value: "Team route" },
  });
  fireEvent.click(screen.getByRole("checkbox", { name: "Team route" }));
  expect(screen.getByLabelText("gatewayKeys.models")).toHaveValue(
    `${upstreamId}, ${firstId}, ${groupId}`,
  );
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(gatewayApi.update).toHaveBeenCalledWith("fixture", {
      ...policy,
      allModels: false,
      models: [upstreamId, firstId, groupId],
    }),
  );
  expect(gatewayApi.bind).not.toHaveBeenCalled();
});

it("updates budgets without replacing a credential and revokes by ID", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    { id: "fixture", policy, revoked: false, usedTokens: 42, resetsAt: null },
  ]);
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "gatewayKeys.edit" }),
  );
  fireEvent.change(screen.getByLabelText("gatewayKeys.limit"), {
    target: { value: "10" },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(gatewayApi.update).toHaveBeenCalledWith("fixture", {
      ...policy,
      allModels: false,
      tokenLimit: 10,
    }),
  );
  expect(gatewayApi.create).not.toHaveBeenCalled();
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "gatewayKeys.revoke" }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.revoke" }));
  await waitFor(() =>
    expect(gatewayApi.revoke).toHaveBeenCalledWith("fixture"),
  );
});

it("reveals a saved key on demand and can show it again after hiding", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "fixture",
      policy,
      revoked: false,
      usedTokens: 42,
      resetsAt: null,
      secretAvailable: true,
    },
  ]);
  vi.mocked(gatewayApi.reveal).mockResolvedValue("synthetic-saved-secret");
  mount();
  const reveal = await screen.findByRole("button", {
    name: "gatewayKeys.reveal",
  });
  expect(gatewayApi.reveal).not.toHaveBeenCalled();
  expect(screen.queryByText("synthetic-saved-secret")).not.toBeInTheDocument();
  fireEvent.click(reveal);
  await screen.findByText("synthetic-saved-secret");
  expect(gatewayApi.reveal).toHaveBeenCalledWith("fixture");
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.dismiss" }));
  expect(screen.queryByText("synthetic-saved-secret")).not.toBeInTheDocument();
  fireEvent.click(reveal);
  await screen.findByText("synthetic-saved-secret");
  expect(gatewayApi.reveal).toHaveBeenCalledTimes(2);
});

it("explains that legacy hash-only credentials cannot be recovered", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    { id: "legacy", policy, revoked: false, usedTokens: 42, resetsAt: null },
  ]);
  mount();
  await screen.findByText("gatewayKeys.legacySecret");
  expect(
    screen.queryByRole("button", { name: "gatewayKeys.reveal" }),
  ).not.toBeInTheDocument();
});

it("keeps the revealed key panel mounted while showing the same key again", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "fixture",
      policy,
      revoked: false,
      usedTokens: 42,
      resetsAt: null,
      secretAvailable: true,
    },
  ]);
  vi.mocked(gatewayApi.reveal).mockResolvedValue("synthetic-saved-secret");
  mount();
  const reveal = await screen.findByRole("button", {
    name: "gatewayKeys.reveal",
  });
  fireEvent.click(reveal);
  const shown = await screen.findByText("synthetic-saved-secret");
  expect(screen.getByRole("dialog")).toContainElement(shown);
  let finish!: (value: string) => void;
  vi.mocked(gatewayApi.reveal).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await waitFor(() => expect(reveal).toBeEnabled());
  fireEvent.click(reveal);
  expect(shown).toBeInTheDocument();
  finish("synthetic-saved-secret");
  await waitFor(() => expect(reveal).toBeEnabled());
  expect(screen.getByText("synthetic-saved-secret")).toBe(shown);
});

it("requires confirmation to delete a revoked key and refreshes the list", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "retired",
      policy,
      revoked: true,
      agents: [],
      usedTokens: 42,
      resetsAt: null,
    },
  ]);
  vi.mocked(gatewayApi.delete).mockImplementation(async () => {
    vi.mocked(gatewayApi.list).mockResolvedValue([]);
  });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "common.delete" }));
  expect(screen.getByRole("dialog")).toHaveTextContent(
    "gatewayKeys.deleteConfirm",
  );
  expect(gatewayApi.delete).not.toHaveBeenCalled();
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "common.cancel",
    }),
  );
  expect(gatewayApi.delete).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "common.delete",
    }),
  );
  await waitFor(() =>
    expect(gatewayApi.delete).toHaveBeenCalledWith("retired"),
  );
  await waitFor(() =>
    expect(screen.queryByText("Synthetic")).not.toBeInTheDocument(),
  );
});

it("keeps bound revoked keys visible and requires unbinding before deletion", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "retired",
      policy,
      revoked: true,
      agents: ["claude-desktop"],
      usedTokens: 42,
      resetsAt: null,
    },
  ]);
  mount();
  expect(
    await screen.findByRole("button", { name: "common.delete" }),
  ).toBeDisabled();
  expect(screen.getByText("gatewayKeys.deleteUnbindHint")).toBeInTheDocument();
  expect(gatewayApi.delete).not.toHaveBeenCalled();
});

it("does not delete active keys before revocation", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "active",
      policy,
      revoked: false,
      agents: [],
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  mount();
  await screen.findByText("Synthetic");
  expect(
    screen.queryByRole("button", { name: "common.delete" }),
  ).not.toBeInTheDocument();
});

it("creates a name-only key with an explicit all-model policy", async () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.create" }));
  fireEvent.change(screen.getByLabelText("gatewayKeys.name"), {
    target: { value: "Desktop" },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.add" }));
  await waitFor(() =>
    expect(gatewayApi.create).toHaveBeenCalledWith({
      name: "Desktop",
      models: [],
      allModels: true,
      tokenLimit: null,
      period: "daily",
    }),
  );
});

it("switches all connection examples through the protocol menu", async () => {
  HTMLElement.prototype.scrollIntoView = vi.fn();
  const { container } = mount();
  fireEvent.click(
    screen.getByRole("tab", { name: "gatewayKeys.tabs.connect" }),
  );
  const protocol = screen.getByRole("combobox", {
    name: "gatewayKeys.clientProtocol",
  });
  for (const [name, endpoint] of [
    ["OpenAI Chat Completions", "/v1/chat/completions"],
    ["OpenAI Responses", "/v1/responses"],
    ["Gemini", ":generateContent"],
    ["Anthropic Messages", "/v1/messages"],
  ]) {
    fireEvent.keyDown(protocol, { key: "ArrowDown" });
    fireEvent.click(await screen.findByRole("option", { name }));
    expect(container.querySelector("pre")).toHaveTextContent(endpoint);
  }
  expect(gatewayApi.create).not.toHaveBeenCalled();
  expect(gatewayApi.bind).not.toHaveBeenCalled();
});

it("uses an externally callable alias in connection examples for a shared key", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "shared",
      policy: {
        ...policy,
        models: ["vary-provider/local-only", "vary-auto/published"],
      },
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  const { container } = mount();
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.connect" }));
  await screen.findByRole("option", { name: policy.name });
  fireEvent.change(screen.getByLabelText("gatewayKeys.filterKey"), {
    target: { value: "shared" },
  });
  expect(container.querySelector("pre")).toHaveTextContent("vary-auto/published");
  expect(container.querySelector("pre")).not.toHaveTextContent("vary-provider/local-only");
});

it("offers every agent, binds routed agents and explains direct-only agents", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "key",
      policy,
      agents: [],
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.agents" }));
  const desktop = await screen.findByLabelText("Claude Desktop");
  await waitFor(() =>
    expect(desktop).toHaveAttribute("id", "gateway-agent-claude-desktop"),
  );
  fireEvent.change(desktop, { target: { value: "key" } });
  await waitFor(() =>
    expect(gatewayApi.bind).toHaveBeenCalledWith("claude-desktop", "key"),
  );
  expect(screen.getAllByText("gatewayKeys.manualAgent")).toHaveLength(1);
});

it("shows usage attributed to a key and agent, including missing usage", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "key",
      policy,
      agents: ["claude-desktop"],
      revoked: false,
      usedTokens: 25,
      resetsAt: null,
    },
  ]);
  vi.mocked(gatewayApi.usage).mockResolvedValue([
    {
      keyId: "key",
      window: 0,
      agent: "claude-desktop",
      model: "allowed",
      upstreamModel: "actual-upstream-model",
      tokens: 25,
      requests: 2,
      unmetered: 1,
      lastUsed: 100,
    },
  ]);
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "gatewayKeys.viewUsage" }),
  );
  expect(
    await screen.findByText(
      "Synthetic · gatewayKeys.sourceAgent: Claude Desktop",
    ),
  ).toBeInTheDocument();
  expect(screen.getByText("allowed")).toBeInTheDocument();
  expect(screen.getByText("actual-upstream-model")).toBeInTheDocument();
  expect(screen.getByText("gatewayKeys.upstreamModel")).toBeInTheDocument();
  expect(screen.getByText("gatewayKeys.requestModel")).toBeInTheDocument();
  expect(screen.getByLabelText("gatewayKeys.filterKey")).toHaveValue("key");
});

it("does not infer historical upstream models from the current catalog", async () => {
  vi.mocked(gatewayApi.usage).mockResolvedValue([
    {
      keyId: "old",
      window: 0,
      agent: "claude-desktop",
      model: "allowed",
      upstreamModel: null,
      tokens: 25,
      requests: 1,
      unmetered: 0,
      lastUsed: 100,
    },
  ]);
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.usage" }));
  expect(
    await screen.findByText("gatewayKeys.upstreamMissing"),
  ).toBeInTheDocument();
  expect(screen.getByText("allowed")).toBeInTheDocument();
});

it("shows the catalog label for an opaque request ID on the usage tab", async () => {
  const model = "vary-provider/model-synthetic";
  const name = "Synthetic model · Synthetic supplier";
  vi.mocked(gatewayApi.catalog).mockResolvedValue({
    agents: [],
    models: [{ model, name }],
  });
  vi.mocked(gatewayApi.usage).mockResolvedValue([
    {
      keyId: "key",
      window: 0,
      agent: "codex",
      model,
      upstreamModel: "actual-upstream-model",
      tokens: 25,
      requests: 1,
      unmetered: 0,
      lastUsed: 100,
    },
  ]);
  mount();
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.usage" }));
  expect(await screen.findByText(name)).toHaveAttribute("title", model);
  expect(screen.queryByText(model)).not.toBeInTheDocument();
  expect(screen.getByText("actual-upstream-model")).toBeInTheDocument();
});

it("does not claim Desktop policy activation when its profile points elsewhere", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "key",
      policy,
      agents: ["claude-desktop"],
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  mount({ running: true });
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.agents" }));
  expect(
    await screen.findByText("gatewayKeys.routeRequired"),
  ).toBeInTheDocument();
  expect(
    screen.queryByText("gatewayKeys.bindingActive"),
  ).not.toBeInTheDocument();
});

it("starts the listener without rewriting every agent configuration", async () => {
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "gatewayKeys.startRouting" }),
  );
  await waitFor(() =>
    expect(proxyApi.startProxyServer).toHaveBeenCalledTimes(1),
  );
  expect(proxyApi.setProxyTakeoverForApp).not.toHaveBeenCalled();
});

it.each([false, true])(
  "reports secret copy success only after clipboard succeeds (failure: %s)",
  async (failed) => {
    const writeText = vi.fn();
    if (failed)
      writeText.mockRejectedValue(new Error("Synthetic clipboard failure"));
    else writeText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    vi.mocked(gatewayApi.list).mockResolvedValue([
      {
        id: "fixture",
        policy,
        revoked: false,
        usedTokens: 0,
        resetsAt: null,
        secretAvailable: true,
      },
    ]);
    vi.mocked(gatewayApi.reveal).mockResolvedValue("synthetic-saved-secret");
    mount();
    fireEvent.click(
      await screen.findByRole("button", { name: "gatewayKeys.reveal" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.copy" }),
    );
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("synthetic-saved-secret"),
    );
    await waitFor(() =>
      expect(failed ? toast.error : toast.success).toHaveBeenCalledWith(
        failed ? "gatewayKeys.failed" : "gatewayKeys.copied",
      ),
    );
    if (failed) expect(toast.success).not.toHaveBeenCalled();
    expect(dialog).toBeInTheDocument();
  },
);

it("reports shared connection readiness for every policy-capable Agent", async () => {
  const external = ["openclaw"];
  const routed = APP_IDS.filter((app) => !external.includes(app));
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "key",
      policy,
      agents: routed,
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  vi.mocked(agentConnectionsApi.get).mockImplementation(async (app) => ({
    app,
    enabled: true,
    configured: app !== "claude-desktop",
    recoveryPending: app === "claude-desktop",
    model: null,
    restartRequired: false,
  }));
  mount({ running: true });
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.agents" }));
  await waitFor(() => {
    for (const app of routed) {
      const row = screen.getByLabelText(getAppLabel(app)).parentElement!;
      expect(row).toHaveTextContent(
        app === "claude-desktop"
          ? "gatewayKeys.routeRequired"
          : "gatewayKeys.bindingActive",
      );
    }
  });
  expect(screen.getAllByText("gatewayKeys.manualAgent")).toHaveLength(
    external.length,
  );
});

it("keeps bound routes visible when a separate Agent status fails", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "key",
      policy,
      agents: ["gemini", "codex", "trae", "trae-intl", "qoder"],
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  vi.mocked(agentConnectionsApi.list).mockRejectedValue(
    new Error("synthetic-status-failure"),
  );
  vi.mocked(agentConnectionsApi.get).mockImplementation(async (app) => {
    if (app === "qoder") throw new Error("synthetic-status-failure");
    const selectingStartModel = app === "trae" || app === "trae-intl";
    return {
      app,
      enabled: true,
      configured: true,
      recoveryPending: selectingStartModel,
      recoveryReason: selectingStartModel ? "bridge_selection_pending" : null,
      model: null,
      restartRequired: false,
    };
  });
  mount({ running: true });
  fireEvent.click(screen.getByRole("tab", { name: "gatewayKeys.tabs.agents" }));
  await waitFor(() => {
    for (const app of ["gemini", "codex", "trae", "trae-intl"] as const) {
      expect(
        screen.getByLabelText(getAppLabel(app)).parentElement,
      ).toHaveTextContent("gatewayKeys.bindingActive");
    }
    expect(
      screen.getByLabelText(getAppLabel("qoder")).parentElement,
    ).toHaveTextContent("gatewayKeys.routeStatusUnavailable");
  });
  expect(agentConnectionsApi.get).toHaveBeenCalledTimes(5);
  expect(screen.queryByText("synthetic-status-failure")).toBeNull();
});

it("shows readable catalog names while storing exact route IDs", async () => {
  const model = {
    model: "vary-provider/opaque-fixture",
    name: "Example Model · Synthetic supplier",
  };
  vi.mocked(gatewayApi.catalog).mockResolvedValue({
    agents: [],
    models: [model],
  });
  vi.mocked(gatewayApi.list).mockResolvedValue([
    {
      id: "fixture",
      policy: { ...policy, models: [model.model] },
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
  ]);
  mount();
  expect(await screen.findByText(model.name)).toBeInTheDocument();
  expect(screen.queryByText(model.model)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.edit" }));
  expect(screen.getByRole("checkbox", { name: /Example Model/ })).toBeChecked();
  expect(screen.getByLabelText("gatewayKeys.models")).toHaveValue(model.model);
});
