import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    editor: vi.fn(),
    saveCodex: vi.fn(),
    codexModels: vi.fn(),
    codexQuota: vi.fn(),
    testCodexModel: vi.fn(),
    save: vi.fn(),
    remove: vi.fn(),
  },
}));
vi.mock("@/components/providers/forms/hooks/useCodexOauth", () => ({
  useCodexOauth: () => ({
    accounts: [],
    pollingState: "idle",
    addAccount: vi.fn(),
    cancelAuth: vi.fn(),
    refetchStatus: vi.fn(),
    isAddingAccount: false,
  }),
}));

const upstream = {
  schemaVersion: 1 as const,
  id: "fixture",
  revision: 2,
  name: "Synthetic Codex",
  hasApiKey: false,
  source: {
    kind: "codex" as const,
  },
};
const codex = {
  settings: {
    accounts: ["native"],
    mode: "picked",
    models: [{ model: "model-a", reasoningLevels: ["low", "high"] }],
    groupsOnly: false,
  },
  accounts: [
    {
      id: "native",
      login: "person@example.test",
      plan: "ChatGPT Plus",
      native: true,
      ready: true,
    },
  ],
  models: [
    { model: "model-a", reasoningLevels: ["low", "high"] },
    { model: "model-b" },
  ],
};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([upstream]);
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex,
  } as never);
});
async function open() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  return screen.findByRole("dialog", { name: "Synthetic Codex" });
}
it("loads a missing official model catalog for the selected signed-in account automatically", async () => {
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: {
      ...codex,
      models: [],
      settings: { ...codex.settings, mode: "vendor", models: [] },
    },
  } as never);
  vi.mocked(routingUpstreamsApi.codexModels).mockResolvedValue([
    { model: "account-model", contextWindow: 1000000 },
  ]);
  await open();
  await waitFor(() =>
    expect(routingUpstreamsApi.codexModels).toHaveBeenCalledWith(
      "fixture",
      2,
      "native",
    ),
  );
  expect(
    await screen.findByRole("button", { name: /^account-model$/ }),
  ).toBeVisible();
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.testCodexModel).not.toHaveBeenCalled();
});

it("distinguishes an unavailable login from a successfully empty official catalog", async () => {
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: {
      ...codex,
      models: [],
      accounts: codex.accounts.map((account) => ({ ...account, ready: false })),
    },
  } as never);
  await open();
  expect(
    await screen.findByText("codexProvider.catalogLoginRequired"),
  ).toBeVisible();
  expect(routingUpstreamsApi.codexModels).not.toHaveBeenCalled();
});
it("reports a successfully empty official catalog without retrying or probing models", async () => {
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: { ...codex, models: [] },
  } as never);
  vi.mocked(routingUpstreamsApi.codexModels).mockResolvedValue([]);
  await open();
  expect(await screen.findByText("codexProvider.catalogEmpty")).toBeVisible();
  await waitFor(() =>
    expect(routingUpstreamsApi.codexModels).toHaveBeenCalledOnce(),
  );
  expect(routingUpstreamsApi.testCodexModel).not.toHaveBeenCalled();
});

it("keeps official catalog failures safe and offers an explicit retry", async () => {
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: { ...codex, models: [] },
  } as never);
  vi.mocked(routingUpstreamsApi.codexModels).mockRejectedValue(
    "synthetic-private-error",
  );
  await open();
  expect(await screen.findByText("codexProvider.catalogFailed")).toBeVisible();
  expect(screen.queryByText("synthetic-private-error")).not.toBeInTheDocument();
  vi.mocked(routingUpstreamsApi.codexModels).mockResolvedValue([
    { model: "retry-model" },
  ]);
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.fetchModels" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "codexProvider.showOthers" }),
    ).toBeEnabled(),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.showOthers" }),
  );
  expect(
    await screen.findByRole("button", { name: /^retry-model$/ }),
  ).toBeVisible();
  expect(routingUpstreamsApi.codexModels).toHaveBeenCalledTimes(2);
});

it("opens a real account/model editor instead of the imported connection placeholder without vendor requests", async () => {
  await open();
  expect(await screen.findByText("person@example.test")).toBeVisible();
  expect(screen.getByText("ChatGPT Plus")).toBeVisible();
  expect(
    screen.getByRole("button", { name: "codexProvider.modeVendor" }),
  ).toBeVisible();
  expect(
    screen.getByRole("button", { name: "codexProvider.modePicked" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(screen.queryByText("providerEditor.importedAccountHint")).toBeNull();
  expect(screen.queryByRole("textbox", { name: "ID" })).toBeNull();
  expect(routingUpstreamsApi.codexModels).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.codexQuota).not.toHaveBeenCalled();
});
it("stages usage-only and saves account policy without converting OAuth into an API key", async () => {
  vi.mocked(routingUpstreamsApi.saveCodex).mockResolvedValue(upstream);
  await open();
  await screen.findByText("person@example.test");
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.modeUsage" }),
  );
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(routingUpstreamsApi.saveCodex).toHaveBeenCalledWith(
      expect.objectContaining({ id: "fixture", revision: 2 }),
      expect.objectContaining({ accounts: ["native"], mode: "usage" }),
    ),
  );
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
});

it("keeps model discovery explicit and stages a newly chosen model with its metadata", async () => {
  vi.mocked(routingUpstreamsApi.codexModels).mockResolvedValue([
    { model: "model-c", reasoningLevels: ["high"] },
  ]);
  vi.mocked(routingUpstreamsApi.saveCodex).mockResolvedValue(upstream);
  await open();
  await screen.findByText("person@example.test");
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.fetchModels" }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.codexModels).toHaveBeenCalledWith(
      "fixture",
      2,
      "native",
    ),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.showOthers" }),
  );
  fireEvent.click(screen.getByRole("button", { name: /^model-c$/ }));
  fireEvent.click(
    screen.getByRole("checkbox", { name: "codexProvider.groupsOnly" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(routingUpstreamsApi.saveCodex).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        groupsOnly: true,
        models: expect.arrayContaining([
          expect.objectContaining({
            model: "model-c",
            reasoningLevels: ["high"],
          }),
        ]),
      }),
    ),
  );
});

it("reads only the requested account's quota and never treats missing data as zero usage", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    success: true,
    tiers: [{ name: "five_hour", utilization: 27, resetsAt: null }],
  } as never);
  await open();
  await screen.findByText("person@example.test");
  expect(screen.getByText("codexProvider.quotaUnknown")).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexProvider.refreshQuota person@example.test",
    }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith("native"),
  );
  expect(await screen.findByText("codexProvider.remaining")).toBeVisible();
});

it("cancelled model edits never save a policy or credentials", async () => {
  await open();
  await screen.findByText("person@example.test");
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.modeUsage" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
});

it("blocks saving and account changes while discovery is in flight", async () => {
  let finish!: (models: Array<Record<string, unknown>>) => void;
  vi.mocked(routingUpstreamsApi.codexModels).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await open();
  fireEvent.click(
    await screen.findByRole("button", { name: "codexProvider.fetchModels" }),
  );
  expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
  expect(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  ).toBeDisabled();
  finish([{ model: "model-a" }]);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled(),
  );
});

it("tests models only on explicit action using the selected account", async () => {
  vi.mocked(routingUpstreamsApi.testCodexModel).mockResolvedValue({
    ok: true,
    elapsedMs: 12,
  });
  await open();
  await screen.findByText("person@example.test");
  expect(routingUpstreamsApi.testCodexModel).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.testModels" }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.testCodexModel).toHaveBeenCalledWith(
      "fixture",
      2,
      "native",
      "model-a",
    ),
  );
  expect(await screen.findByText("codexProvider.testPassed")).toBeVisible();
});

it("never labels a different account's catalog as the newly selected account's list", async () => {
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: {
      ...codex,
      accounts: [
        ...codex.accounts,
        {
          id: "second",
          login: "second@example.test",
          native: false,
          ready: true,
        },
      ],
    },
  } as never);
  await open();
  fireEvent.click(
    await screen.findByRole("checkbox", { name: "second@example.test" }),
  );
  fireEvent.click(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.modeVendor" }),
  );
  expect(screen.queryByRole("button", { name: /^model-b$/ })).toBeNull();
});
