import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

const managedAccounts = vi.hoisted(() => ({
  rows: [] as Array<{ id: string; login: string; reauth_required: boolean }>,
}));

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
    accounts: managedAccounts.rows,
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
  managedAccounts.rows = [];
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([upstream]);
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex,
  } as never);
  vi.mocked(routingUpstreamsApi.codexModels).mockResolvedValue([]);
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    tool: "codex",
    credentialStatus: "valid",
    credentialMessage: null,
    success: true,
    tiers: [],
    extraUsage: null,
    error: null,
    queriedAt: null,
  });
});

it("does not add the native alias back from the managed account status", async () => {
  managedAccounts.rows = [
    { id: "managed", login: "person@example.test", reauth_required: false },
  ];
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: {
      ...codex,
      accounts: [{ ...codex.accounts[0], aliases: ["managed"] }],
    },
  } as never);
  await open();
  expect(
    screen.getAllByRole("checkbox", { name: "person@example.test" }),
  ).toHaveLength(1);
  expect(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  ).toBeChecked();
});

it("keeps same-email accounts separate unless the backend proves an alias", async () => {
  managedAccounts.rows = [
    {
      id: "other-workspace",
      login: "person@example.test",
      reauth_required: false,
    },
  ];
  await open();
  expect(
    screen.getAllByRole("checkbox", { name: "person@example.test" }),
  ).toHaveLength(2);
});
function mount(
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
}
async function open(client?: QueryClient) {
  mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  return screen.findByRole("dialog", { name: "Synthetic Codex" });
}
it("loads the complete subscription form before mounting the dialog so its opening size cannot jump", async () => {
  let finish!: (value: typeof upstream & { codex: typeof codex }) => void;
  vi.mocked(routingUpstreamsApi.editor).mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve as typeof finish;
      }),
  );
  mount();
  const trigger = await screen.findByRole("button", {
    name: "common.edit Synthetic Codex",
  });
  fireEvent.click(trigger);
  await waitFor(() => expect(routingUpstreamsApi.editor).toHaveBeenCalled());
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toBeDisabled();
  finish({ ...upstream, codex });
  expect(await screen.findByRole("dialog")).toBeVisible();
  expect(screen.getByText("person@example.test")).toBeVisible();
  expect(screen.queryByText("common.loading")).not.toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("dialog")).toHaveFocus());
});

it("reports a failed editor load on the provider list without opening a partial form", async () => {
  vi.mocked(routingUpstreamsApi.editor).mockRejectedValue(
    new Error("synthetic"),
  );
  mount();
  fireEvent.click(
    await screen.findByRole("button", {
      name: "common.edit Synthetic Codex",
    }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "modelProviders.failed",
  );
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

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

it("opens the account/model editor and reads allowance without model discovery or probes", async () => {
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
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith("native");
  expect(routingUpstreamsApi.testCodexModel).not.toHaveBeenCalled();
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

it("automatically reads each ready account independently, without reading unavailable accounts", async () => {
  let finish!: (
    value: Awaited<ReturnType<typeof routingUpstreamsApi.codexQuota>>,
  ) => void;
  managedAccounts.rows = [
    { id: "second", login: "second@example.test", reauth_required: false },
    { id: "expired", login: "expired@example.test", reauth_required: true },
  ];
  vi.mocked(routingUpstreamsApi.codexQuota).mockImplementation((account) =>
    account === "second"
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : Promise.resolve({
          credentialStatus: "valid",
          success: true,
          tiers: [
            { name: "five_hour", utilization: 27, resetsAt: null },
            { name: "seven_day", utilization: 62, resetsAt: null },
          ],
        } as never),
  );
  await open();
  const native = within(
    screen.getByRole("group", { name: "person@example.test" }),
  );
  const second = within(
    screen.getByRole("group", { name: "second@example.test" }),
  );
  expect(await native.findAllByText("codexProvider.remaining")).toHaveLength(2);
  expect(second.getByText("codexProvider.quotaLoading")).toBeVisible();
  expect(second.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
  expect(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  ).toBeEnabled();
  expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
  expect(vi.mocked(routingUpstreamsApi.codexQuota).mock.calls).toEqual([
    ["native"],
    ["second"],
  ]);
  expect(routingUpstreamsApi.codexModels).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.testCodexModel).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
  await act(async () =>
    finish({ success: true, credentialStatus: "valid", tiers: [] } as never),
  );
  expect(await second.findByText("codexProvider.quotaEmpty")).toBeVisible();
});

it("reuses fresh allowance when reopening and lets manual refresh bypass the 60-second cache", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [{ name: "five_hour", utilization: 27, resetsAt: null }],
  } as never);
  await open();
  expect(await screen.findByText("codexProvider.remaining")).toBeVisible();
  await reopen();
  expect(screen.getByText("codexProvider.remaining")).toBeVisible();
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(1);
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexProvider.refreshQuota person@example.test",
    }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(2),
  );
});

async function reopen() {
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  return screen.findByRole("dialog", { name: "Synthetic Codex" });
}

it("revalidates allowance older than 60 seconds on reopening", async () => {
  const client = new QueryClient();
  await open(client);
  await screen.findByText("codexProvider.quotaEmpty");
  const [cached] = client.getQueriesData({
    queryKey: ["codex-provider-quota"],
  });
  expect(cached).toBeDefined();
  act(() =>
    client.setQueryData(cached[0], cached[1], {
      updatedAt: Date.now() - 60_001,
    }),
  );
  await reopen();
  await waitFor(() =>
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(2),
  );
});

it("deduplicates an in-flight allowance request across reopening without locking the editor", async () => {
  let finish!: (
    value: Awaited<ReturnType<typeof routingUpstreamsApi.codexQuota>>,
  ) => void;
  vi.mocked(routingUpstreamsApi.codexQuota).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await open();
  expect(screen.getByText("codexProvider.quotaLoading")).toBeVisible();
  expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
  expect(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  ).toBeEnabled();
  await reopen();
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(1);
  await act(async () =>
    finish({ success: true, credentialStatus: "valid", tiers: [] } as never),
  );
  expect(await screen.findByText("codexProvider.quotaEmpty")).toBeVisible();
});

it("shows safe allowance failures, caches the failed attempt and offers a manual retry", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockRejectedValue(
    new Error("synthetic-private-error"),
  );
  await open();
  expect(await screen.findByText("codexProvider.quotaFailed")).toBeVisible();
  expect(screen.queryByText("synthetic-private-error")).not.toBeInTheDocument();
  expect(
    screen.queryByText("codexProvider.quotaUnknown"),
  ).not.toBeInTheDocument();
  expect(screen.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
  await reopen();
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(1);
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    success: true,
    credentialStatus: "valid",
    tiers: [{ name: "five_hour", utilization: 27, resetsAt: null }],
  } as never);
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexProvider.refreshQuota person@example.test",
    }),
  );
  expect(await screen.findByText("codexProvider.remaining")).toBeVisible();
  expect(
    screen.queryByText("codexProvider.quotaFailed"),
  ).not.toBeInTheDocument();
});

it("keeps previous meters visible during refresh and a transient failure but not after sign-in expires", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [{ name: "five_hour", utilization: 27, resetsAt: null }],
  } as never);
  await open();
  await screen.findByText("codexProvider.remaining");
  let finish!: (
    value: Awaited<ReturnType<typeof routingUpstreamsApi.codexQuota>>,
  ) => void;
  vi.mocked(routingUpstreamsApi.codexQuota).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const refresh = screen.getByRole("button", {
    name: "codexProvider.refreshQuota person@example.test",
  });
  fireEvent.click(refresh);
  expect(screen.getByText("codexProvider.remaining")).toBeVisible();
  await waitFor(() => expect(refresh).toBeDisabled());
  await act(async () =>
    finish({
      credentialStatus: "valid",
      success: false,
      tiers: [],
      error: "synthetic-private-error",
    } as never),
  );
  expect(await screen.findByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.getByText("codexProvider.remaining")).toBeVisible();
  expect(screen.queryByText("synthetic-private-error")).not.toBeInTheDocument();
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "expired",
    success: false,
    tiers: [],
  } as never);
  await waitFor(() => expect(refresh).toBeEnabled());
  fireEvent.click(refresh);
  expect(
    await screen.findByText("codexProvider.quotaLoginRequired"),
  ).toBeVisible();
  expect(screen.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
});

it("never turns a missing or invalid allowance into 100 percent remaining", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "valid",
    success: false,
    tiers: [],
  } as never);
  await open();
  expect(await screen.findByText("codexProvider.quotaFailed")).toBeVisible();
  expect(screen.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
});

it("does not mistake a generic source or refresh failure for an expired authorization", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [{ name: "five_hour", utilization: 27, resetsAt: null }],
  } as never);
  await open();
  await screen.findByText("codexProvider.remaining");
  vi.mocked(routingUpstreamsApi.codexQuota).mockRejectedValue(
    "routingUpstreams.sourceUnavailable",
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexProvider.refreshQuota person@example.test",
    }),
  );
  expect(await screen.findByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.getByText("codexProvider.remaining")).toBeVisible();
  expect(
    screen.queryByText("codexProvider.quotaLoginRequired"),
  ).not.toBeInTheDocument();
});

it("distinguishes missing or unreadable credentials from a rejected sign-in", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "parse_error",
    success: false,
    tiers: [],
  } as never);
  await open();
  expect(
    await screen.findByText("codexProvider.quotaCredentialsInvalid"),
  ).toBeVisible();
  expect(
    screen.queryByText("codexProvider.quotaLoginRequired"),
  ).not.toBeInTheDocument();
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "not_found",
    success: false,
    tiers: [],
  } as never);
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexProvider.refreshQuota person@example.test",
    }),
  );
  expect(
    await screen.findByText("codexProvider.quotaCredentialsMissing"),
  ).toBeVisible();
});

it("keeps allowance cached by account ID, even for accounts sharing an email", async () => {
  managedAccounts.rows = [
    {
      id: "other-workspace",
      login: "person@example.test",
      reauth_required: false,
    },
  ];
  vi.mocked(routingUpstreamsApi.codexQuota).mockImplementation(
    async (id) =>
      ({
        credentialStatus: "valid",
        success: true,
        tiers: [
          {
            name: id === "native" ? "five_hour" : "seven_day",
            utilization: 20,
            resetsAt: null,
          },
        ],
      }) as never,
  );
  await open();
  const groups = screen.getAllByRole("group", { name: "person@example.test" });
  expect(await within(groups[0]).findByText("five_hour")).toBeVisible();
  expect(await within(groups[1]).findByText("seven_day")).toBeVisible();
  fireEvent.click(within(groups[1]).getByRole("checkbox"));
  fireEvent.click(within(groups[0]).getByRole("checkbox"));
  const reordered = screen.getAllByRole("group", {
    name: "person@example.test",
  });
  expect(within(reordered[0]).getByText("seven_day")).toBeVisible();
  expect(within(reordered[0]).queryByText("five_hour")).not.toBeInTheDocument();
  expect(within(reordered[1]).getByText("five_hour")).toBeVisible();
  expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledTimes(2);
  await screen.findByText("codexProvider.catalogEmpty");
});

it("does not render a meter for non-finite or absent utilization", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [
      { name: "five_hour", utilization: Number.NaN, resetsAt: null },
      { name: "seven_day", utilization: null, resetsAt: null },
    ],
  } as never);
  await open();
  expect(await screen.findByText("codexProvider.quotaEmpty")).toBeVisible();
  expect(screen.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
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
  await waitFor(() =>
    expect(screen.getAllByText("codexProvider.quotaEmpty")).toHaveLength(2),
  );
  fireEvent.click(
    await screen.findByRole("checkbox", { name: "second@example.test" }),
  );
  fireEvent.click(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.modeVendor" }),
  );
  await screen.findByText("codexProvider.catalogEmpty");
  expect(screen.queryByRole("button", { name: /^model-b$/ })).toBeNull();
});
