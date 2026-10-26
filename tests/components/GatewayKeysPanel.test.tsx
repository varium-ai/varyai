import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { GatewayKeysPanel } from "@/components/proxy/GatewayKeysPanel";
import { gatewayApi } from "@/lib/api/gateway";
import { APP_IDS } from "@/config/appConfig";
import { providersApi } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
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
    update: vi.fn(),
    revoke: vi.fn(),
    delete: vi.fn(),
    catalog: vi.fn(),
    bind: vi.fn(),
    usage: vi.fn(),
  },
}));
const policy = {
  name: "Synthetic",
  models: ["allowed"],
  tokenLimit: 100,
  period: "daily" as const,
};
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
  vi.mocked(gatewayApi.list).mockResolvedValue([]);
  vi.mocked(gatewayApi.catalog).mockResolvedValue({
    agents: APP_IDS.map((id) => ({
      id,
      localRouting: !["opencode", "openclaw", "hermes"].includes(id),
    })),
    models: [
      { agent: "claude-desktop", provider: "Synthetic", model: "allowed" },
    ],
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
it("creates an exact allowlist and shows the secret only until dismissed", async () => {
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
  await screen.findByText("synthetic-secret");
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.dismiss" }));
  expect(screen.queryByText("synthetic-secret")).not.toBeInTheDocument();
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
  expect(screen.getAllByText("gatewayKeys.manualAgent")).toHaveLength(3);
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
