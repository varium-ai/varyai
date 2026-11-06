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
  browser: null as Record<string, unknown> | null,
  reauthAccount: vi.fn(),
  addAccount: vi.fn(),
  cancelAuth: vi.fn(),
  deviceCode: null as { user_code: string; verification_uri: string } | null,
  loginCompletion: null as { accountId: string; generation: number } | null,
  pollingState: "idle",
  isAddingAccount: false,
  rows: [] as Array<{ id: string; login: string; reauth_required: boolean }>,
}));

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    createCodex: vi.fn(),
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
    browser: managedAccounts.browser ?? undefined,
    addAccountWithProxy: managedAccounts.addAccount,
    loginCompletion: managedAccounts.loginCompletion,
    pollingState: managedAccounts.pollingState,
    isStatusSuccess: true,
    reauthAccount: managedAccounts.reauthAccount,
    addAccount: managedAccounts.addAccount,
    cancelAuth: managedAccounts.cancelAuth,
    deviceCode: managedAccounts.deviceCode,
    refetchStatus: vi.fn(),
    isAddingAccount: managedAccounts.isAddingAccount,
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
  managedAccounts.browser = null;
  managedAccounts.deviceCode = null;
  managedAccounts.loginCompletion = null;
  managedAccounts.pollingState = "idle";
  managedAccounts.isAddingAccount = false;
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
    screen.queryByRole("button", {
      name: "codexOauth.reauthLogin person@example.test",
    }),
  ).not.toBeInTheDocument();
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

it("offers targeted re-login when an allowance confirms rejected credentials", async () => {
  managedAccounts.rows = [
    { id: "managed", login: "person@example.test", reauth_required: false },
  ];
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: {
      ...codex,
      settings: { ...codex.settings, accounts: ["managed"] },
      accounts: [{ ...codex.accounts[0], id: "managed", native: false }],
    },
  } as never);
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "expired",
    success: false,
    tiers: [],
  } as never);
  await open();
  const account = within(
    screen.getByRole("group", { name: "person@example.test" }),
  );
  expect(await account.findByText("codexOauth.reauthBadge")).toBeVisible();
  fireEvent.click(
    account.getByRole("button", {
      name: "codexOauth.reauthLogin person@example.test",
    }),
  );
  expect(managedAccounts.reauthAccount).toHaveBeenCalledWith("managed");
  expect(managedAccounts.addAccount).not.toHaveBeenCalled();
});

it("offers re-login for an incomplete managed identity without querying its quota", async () => {
  managedAccounts.rows = [
    {
      id: "incomplete",
      login: "incomplete@example.test",
      reauth_required: true,
    },
  ];
  await open();
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexOauth.reauthLogin incomplete@example.test",
    }),
  );
  expect(managedAccounts.reauthAccount).toHaveBeenCalledWith("incomplete");
  expect(routingUpstreamsApi.codexQuota).not.toHaveBeenCalledWith("incomplete");
});

it("uses the proven managed alias when reauthenticating a native row", async () => {
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
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    credentialStatus: "expired",
    success: false,
    tiers: [],
  } as never);
  await open();
  fireEvent.click(
    await screen.findByRole("button", {
      name: "codexOauth.reauthLogin person@example.test",
    }),
  );
  expect(managedAccounts.reauthAccount).toHaveBeenCalledWith("managed");
});

it("refreshes repaired credentials and model observations without discarding unsaved choices", async () => {
  managedAccounts.rows = [
    { id: "managed", login: "person@example.test", reauth_required: true },
  ];
  const draftEditor = {
    ...upstream,
    codex: {
      ...codex,
      settings: { ...codex.settings, accounts: ["managed"] },
      accounts: [
        { ...codex.accounts[0], id: "managed", native: false, ready: false },
      ],
    },
  };
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue(draftEditor as never);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const view = mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  await screen.findByRole("dialog");
  fireEvent.click(
    screen.getByRole("button", {
      name: "codexOauth.reauthLogin person@example.test",
    }),
  );
  managedAccounts.rows = [
    { id: "managed", login: "person@example.test", reauth_required: false },
  ];
  managedAccounts.loginCompletion = { accountId: "managed", generation: 1 };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith("managed"),
  );
  expect(invalidate).toHaveBeenCalledWith({ queryKey: ["routing-upstreams"] });
  expect(screen.queryByText("codexOauth.reauthBadge")).not.toBeInTheDocument();
  expect(
    screen.getByRole("checkbox", { name: "person@example.test" }),
  ).toBeChecked();
  expect(screen.getByRole("button", { name: "model-a" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(routingUpstreamsApi.editor).toHaveBeenCalledTimes(1);
});

it("selects the repaired native alias in the draft while preserving model choices", async () => {
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
  vi.mocked(routingUpstreamsApi.codexQuota).mockImplementation(
    async (account) =>
      ({
        credentialStatus: account === "native" ? "expired" : "valid",
        success: account !== "native",
        tiers: [],
      }) as never,
  );
  vi.mocked(routingUpstreamsApi.saveCodex).mockResolvedValue(upstream);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "codexOauth.reauthLogin person@example.test",
    }),
  );
  managedAccounts.loginCompletion = { accountId: "managed", generation: 1 };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith("managed"),
  );
  expect(
    screen.getAllByRole("checkbox", { name: "person@example.test" }),
  ).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(routingUpstreamsApi.saveCodex).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        accounts: ["managed"],
        models: codex.settings.models,
      }),
    ),
  );
});
it("starts re-login directly from the provider row when credentials are logged out", async () => {
  managedAccounts.rows = [
    { id: "managed", login: "person@example.test", reauth_required: true },
  ];
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    { ...upstream, reauthRequired: true },
  ] as never);
  vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
    ...upstream,
    codex: {
      ...codex,
      settings: { ...codex.settings, accounts: ["managed"] },
      accounts: [
        { ...codex.accounts[0], id: "managed", native: false, ready: false },
      ],
    },
  } as never);
  mount();
  fireEvent.click(
    await screen.findByRole("button", {
      name: "codexOauth.reauthLogin Synthetic Codex",
    }),
  );
  await screen.findByRole("dialog");
  await waitFor(() =>
    expect(managedAccounts.reauthAccount).toHaveBeenCalledWith("managed"),
  );
  expect(managedAccounts.addAccount).not.toHaveBeenCalled();
});

it("keeps model editing and closing available while a browser login waits", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "codexProvider.addAccount" }),
  );
  managedAccounts.isAddingAccount = true;
  managedAccounts.pollingState = "polling";
  managedAccounts.deviceCode = {
    user_code: "SYNTHETIC-CODE",
    verification_uri: "https://example.test/device",
  };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  const dialog = within(screen.getByRole("dialog"));
  expect(dialog.getByRole("button", { name: "model-a" })).toBeEnabled();
  expect(dialog.getByRole("button", { name: "common.save" })).toBeEnabled();
  const cancels = dialog.getAllByRole("button", { name: "common.cancel" });
  expect(cancels.at(-1)).toBeEnabled();
  expect(
    dialog.queryByRole("button", { name: "codexProvider.addAccount" }),
  ).not.toBeInTheDocument();
  fireEvent.click(cancels[0]);
  expect(managedAccounts.cancelAuth).toHaveBeenCalledOnce();
});

it("keeps a new Codex provider as a draft until save and discards it on cancellation", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
  vi.mocked(routingUpstreamsApi.createCodex).mockResolvedValue({
    ...upstream,
    id: "new-fixture",
    revision: 0,
    name: "ChatGPT / Codex",
    codex,
  } as never);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(
    await screen.findByRole("button", { name: "ChatGPT / Codex" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(routingUpstreamsApi.createCodex).not.toHaveBeenCalled();
  managedAccounts.loginCompletion = { accountId: "native", generation: 1 };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  const dialog = await screen.findByRole("dialog", { name: "ChatGPT / Codex" });
  expect(routingUpstreamsApi.editor).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "common.cancel" }),
  );
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
});

it("adds an account from the subscription picker before opening the existing Codex provider", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(
    await screen.findByRole("button", { name: "ChatGPT / Codex" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(routingUpstreamsApi.editor).not.toHaveBeenCalled();
  managedAccounts.loginCompletion = { accountId: "native", generation: 1 };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  await screen.findByRole("dialog", { name: "Synthetic Codex" });
  expect(routingUpstreamsApi.createCodex).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.editor).toHaveBeenCalledWith("fixture", 2);
});

it("selects a newly authenticated account inside the existing draft without creating a provider", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "codexProvider.addAccount" }),
  );
  managedAccounts.rows = [
    { id: "new-account", login: "new@example.test", reauth_required: false },
  ];
  managedAccounts.loginCompletion = { accountId: "new-account", generation: 1 };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(
      screen.getByRole("checkbox", { name: "new@example.test" }),
    ).toBeChecked(),
  );
  expect(screen.getByRole("button", { name: "model-a" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(routingUpstreamsApi.createCodex).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
});

it("cancels a browser login immediately when the editor closes", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "codexProvider.addAccount" }),
  );
  managedAccounts.isAddingAccount = true;
  managedAccounts.deviceCode = {
    user_code: "SYNTHETIC",
    verification_uri: "https://example.test/device",
  };
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  fireEvent.click(
    within(screen.getByRole("dialog"))
      .getAllByRole("button", { name: "common.cancel" })
      .at(-1)!,
  );
  await waitFor(() => expect(managedAccounts.cancelAuth).toHaveBeenCalled());
  expect(routingUpstreamsApi.saveCodex).not.toHaveBeenCalled();
});

it("starts browser login by default and renders its complete panel inside the account list", async () => {
  const start = vi.fn().mockResolvedValue(undefined);
  const submit = vi.fn();
  managedAccounts.browser = {
    start,
    cancel: vi.fn().mockResolvedValue(undefined),
    reopen: vi.fn(),
    submit,
    isPending: false,
    starting: false,
  };
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "common.edit Synthetic Codex" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "codexProvider.addAccount" }),
  );
  await waitFor(() => expect(start).toHaveBeenCalledWith(undefined, ""));
  expect(managedAccounts.addAccount).not.toHaveBeenCalled();
  managedAccounts.browser.flow = {
    id: "synthetic-browser-flow",
    url: "https://auth.openai.com/oauth/authorize?state=synthetic",
    status: "waiting",
    callbackBlocked: false,
  };
  managedAccounts.browser.isPending = true;
  view.rerender(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  const panel = screen.getByRole("region", { name: "codexBrowser.title" });
  expect(panel.closest(".cx-accounts")).not.toBeNull();
  expect(
    within(panel).getByRole("textbox", {
      name: "codexBrowser.authorizationUrl",
    }),
  ).toHaveValue("https://auth.openai.com/oauth/authorize?state=synthetic");
  fireEvent.change(
    within(panel).getByRole("textbox", { name: "codexBrowser.callbackUrl" }),
    {
      target: {
        value:
          "http://localhost:1455/auth/callback?state=synthetic&code=synthetic",
      },
    },
  );
  fireEvent.click(
    within(panel).getByRole("button", { name: "codexBrowser.finish" }),
  );
  expect(submit).toHaveBeenCalledWith(
    "http://localhost:1455/auth/callback?state=synthetic&code=synthetic",
  );
  expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
});

it.each(["cancel", "device"] as const)(
  "does not open a delayed browser login after %s",
  async (action) => {
    let release!: () => void;
    managedAccounts.cancelAuth.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        release = resolve;
      }),
    );
    const start = vi.fn().mockResolvedValue(undefined);
    managedAccounts.browser = {
      start,
      cancel: vi.fn().mockResolvedValue(undefined),
      reopen: vi.fn(),
      submit: vi.fn(),
      isPending: false,
      starting: false,
    };
    await open();
    fireEvent.click(
      screen.getByRole("button", { name: "codexProvider.addAccount" }),
    );
    const panel = within(
      screen.getByRole("region", { name: "codexBrowser.title" }),
    );
    fireEvent.click(
      panel.getByRole("button", {
        name: action === "cancel" ? "common.cancel" : "codexBrowser.deviceWay",
      }),
    );
    await act(async () => release());
    expect(start).not.toHaveBeenCalled();
    if (action === "device") {
      expect(managedAccounts.addAccount).toHaveBeenCalledWith("");
    } else {
      expect(
        screen.queryByRole("region", { name: "codexBrowser.title" }),
      ).toBeNull();
    }
  },
);

it("shows account restrictions and credits without inventing percentage windows", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    tool: "codex",
    credentialStatus: "valid",
    success: true,
    tiers: [],
    codex: {
      allowed: false,
      limitReached: true,
      modelUsage: {},
      credits: { balance: "25.5" },
    },
  } as never);
  await open();
  expect(await screen.findByText("codexProvider.quotaHeld")).toBeVisible();
  expect(screen.getByText("codexProvider.creditBalance")).toBeVisible();
  expect(screen.queryByText("codexProvider.remaining")).not.toBeInTheDocument();
  expect(
    screen.queryByText("codexProvider.quotaEmpty"),
  ).not.toBeInTheDocument();
});

it("keeps the empty-window status when account metadata contains no displayed quota", async () => {
  vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
    tool: "codex",
    credentialStatus: "valid",
    success: true,
    tiers: [],
    codex: { allowed: true, modelUsage: {} },
  } as never);
  await open();
  expect(await screen.findByText("codexProvider.quotaEmpty")).toBeVisible();
  expect(screen.queryByText("codexProvider.quotaHeld")).not.toBeInTheDocument();
});
