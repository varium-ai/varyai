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
import { subscriptionProviders } from "@/config/subscriptionProviders";
import { routingUpstreamsApi as routing } from "@/lib/api/routingUpstreams";
import { nativeSubscriptionApi as native } from "@/lib/api/nativeSubscription";
import { codexBrowserLoginApi as browser } from "@/lib/api/codexBrowserLogin";
import { authApi } from "@/lib/api";
import { settingsApi } from "@/lib/api/settings";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    createCodex: vi.fn(),
    editor: vi.fn(),
    save: vi.fn(),
    saveCodex: vi.fn(),
    codexModels: vi.fn(),
    codexQuota: vi.fn(),
    importCodexAccount: vi.fn(),
  },
}));
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: {
    start: vi.fn(),
    poll: vi.fn(),
    cancel: vi.fn(),
    accounts: vi.fn(),
    models: vi.fn(),
    quota: vi.fn(),
    import: vi.fn(),
    callback: vi.fn(),
  },
}));
vi.mock("@/lib/api/codexBrowserLogin", () => ({
  codexBrowserLoginApi: {
    start: vi.fn(),
    poll: vi.fn(),
    cancel: vi.fn(),
    callback: vi.fn(),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));
vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: vi.fn(),
    authStartLogin: vi.fn(),
    authPollForAccount: vi.fn(),
    authCancelLogin: vi.fn(),
  },
  settingsApi: { openExternal: vi.fn().mockResolvedValue(undefined) },
}));
vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));
const url = "https://login.example.test/authorize?state=synthetic";
const account = {
  id: "fixture-account",
  login: "fixture@example.test",
  project: "",
  reauth_required: false,
};
const pending = { id: "fixture-flow", url, state: "waiting" as const };
let finishNative: (value: unknown) => void;
let finishBrowser: (value: unknown) => void;
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(routing.list).mockResolvedValue([]);
  vi.mocked(native.start).mockResolvedValue(pending);
  vi.mocked(native.poll).mockImplementation(
    () =>
      new Promise((resolve) => {
        finishNative = resolve as never;
      }),
  );
  vi.mocked(native.cancel).mockResolvedValue(undefined);
  vi.mocked(native.accounts).mockResolvedValue([account]);
  vi.mocked(native.models).mockResolvedValue([]);
  vi.mocked(native.quota).mockResolvedValue({
    success: true,
    tiers: [],
  } as never);
  vi.mocked(browser.start).mockResolvedValue({
    ...pending,
    status: "waiting",
    callbackBlocked: false,
  });
  vi.mocked(browser.poll).mockImplementation(
    () =>
      new Promise((resolve) => {
        finishBrowser = resolve as never;
      }),
  );
  vi.mocked(browser.cancel).mockResolvedValue(true);
  vi.mocked(authApi.authGetStatus).mockResolvedValue({
    accounts: [],
    authenticated: false,
  } as never);
  vi.mocked(authApi.authStartLogin).mockResolvedValue({
    device_code: "fixture-device",
    user_code: "SYNTHETIC",
    verification_uri: url,
    expires_in: 300,
    interval: 5,
  } as never);
  vi.mocked(authApi.authPollForAccount).mockResolvedValue(null);
  vi.mocked(settingsApi.openExternal).mockResolvedValue(undefined);
});
function mount(
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
      <RoutingUpstreamsPanel startAdding />
    </QueryClientProvider>,
  );
}
it.each(subscriptionProviders)(
  "starts $name inside the subscription dialog without opening an editor",
  async (provider) => {
    mount();
    fireEvent.click(
      screen.getByRole("button", { name: "providerParity.subscription" }),
    );
    fireEvent.click(screen.getByRole("button", { name: provider.name }));
    await waitFor(() =>
      expect(
        screen.getByRole("region", {
          name:
            provider.id === "codex"
              ? "codexBrowser.title"
              : "subscriptionLogin.title",
        }),
      ).toBeVisible(),
    );
    const picker = screen.getByRole("dialog", {
      name: "providerParity.subscription",
    });
    expect(picker.querySelector(".cx-signing")).not.toBeNull();
    expect(screen.getAllByRole("dialog")).toEqual([picker]);
    expect(routing.createCodex).not.toHaveBeenCalled();
    expect(routing.save).not.toHaveBeenCalled();
    expect(
      screen.getByRole("searchbox", { name: "providerFlow.findVendor" }),
    ).toBeEnabled();
    if (provider.id === "codex")
      await waitFor(() => expect(browser.start).toHaveBeenCalled());
    else if (["github_copilot", "xai_oauth"].includes(provider.id))
      await waitFor(() => expect(authApi.authStartLogin).toHaveBeenCalled());
    else await waitFor(() => expect(native.start).toHaveBeenCalled());
  },
);
it.each(["shortcut", "picker"])(
  "opens a selected native account's unsaved editor after sign-in from the %s",
  async (entry) => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const reset = vi.spyOn(client, "resetQueries");
    const invalidate = vi.spyOn(client, "invalidateQueries");
    mount(client);
    if (entry === "picker")
      fireEvent.click(
        screen.getByRole("button", { name: "providerParity.subscription" }),
      );
    fireEvent.click(screen.getByRole("button", { name: "Antigravity" }));
    expect(screen.queryByRole("dialog", { name: "Antigravity" })).toBeNull();
    await waitFor(() => expect(native.poll).toHaveBeenCalled(), {
      timeout: 2500,
    });
    await act(async () => finishNative({ ...pending, state: "done", account }));
    const dialog = await screen.findByRole("dialog", { name: "Antigravity" });
    expect(
      screen.queryByRole("dialog", { name: "providerParity.subscription" }),
    ).toBeNull();
    expect(
      within(dialog).getByRole("radio", { name: account.login }),
    ).toBeChecked();
    expect(routing.save).not.toHaveBeenCalled();
    expect(reset).toHaveBeenCalledWith({
      queryKey: ["subscription-account-quota", "antigravity", account.id],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["routing-upstreams"],
    });
  },
);
it("cancels a late native start after switching subscriptions and never opens the stale URL", async () => {
  let finish!: (value: typeof pending) => void;
  vi.mocked(native.start).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  mount();
  fireEvent.click(screen.getByRole("button", { name: "Antigravity" }));
  await waitFor(() => expect(native.start).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
  await waitFor(() => expect(native.start).toHaveBeenCalledTimes(2));
  await act(async () =>
    finish({ ...pending, id: "stale-flow", url: "https://stale.example.test" }),
  );
  expect(native.cancel).toHaveBeenCalledWith("stale-flow");
  expect(settingsApi.openExternal).not.toHaveBeenCalledWith(
    "https://stale.example.test",
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("discards a completed poll when the add page has been closed", async () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
  await waitFor(() => expect(native.poll).toHaveBeenCalled(), {
    timeout: 2500,
  });
  fireEvent.click(screen.getByRole("button", { name: "common.back" }));
  await act(async () => finishNative({ ...pending, state: "done", account }));
  expect(native.cancel).toHaveBeenCalledWith(pending.id);
  expect(
    screen.queryByRole("region", { name: "providerFlow.sheet" }),
  ).toBeNull();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(routing.save).not.toHaveBeenCalled();
});
it("cancels subscription authorization when an API vendor is chosen", async () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: "Antigravity" }));
  await waitFor(() => expect(native.start).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
  await waitFor(() => expect(native.cancel).toHaveBeenCalledWith(pending.id));
  expect(
    screen.queryByRole("region", { name: "subscriptionLogin.title" }),
  ).toBeNull();
  expect(screen.getByRole("dialog", { name: "DeepSeek" })).toBeVisible();
});
it("wires the native browser link, proxy selection and manual callback in the inline panel", async () => {
  mount();
  fireEvent.click(screen.getByRole("button", { name: "Antigravity" }));
  const link = await screen.findByRole("textbox", {
    name: "codexBrowser.authorizationUrl",
  });
  expect(link).toHaveValue(url);
  fireEvent.change(
    screen.getByRole("combobox", { name: "codexBrowser.proxy" }),
    { target: { value: "direct" } },
  );
  await waitFor(() =>
    expect(native.start).toHaveBeenLastCalledWith(
      "antigravity",
      undefined,
      "direct",
    ),
  );
  expect(native.cancel).toHaveBeenCalledWith(pending.id);
  const value =
    "http://127.0.0.1:1456/oauth-callback?state=synthetic&code=synthetic";
  fireEvent.change(
    screen.getByRole("textbox", { name: "codexBrowser.callbackUrl" }),
    { target: { value } },
  );
  fireEvent.click(screen.getByRole("button", { name: "codexBrowser.finish" }));
  await waitFor(() =>
    expect(native.callback).toHaveBeenCalledWith(pending.id, value),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("adds another account inline when the subscription already has a provider", async () => {
  vi.mocked(routing.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "existing",
      revision: 2,
      name: "My Antigravity",
      hasApiKey: false,
      source: {
        kind: "subscription",
        provider: "antigravity",
        accountId: "old-account",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    },
  ] as never);
  mount();
  fireEvent.click(screen.getByRole("button", { name: "Antigravity" }));
  await waitFor(() => expect(native.start).toHaveBeenCalled());
  expect(screen.queryByRole("dialog")).toBeNull();
  await waitFor(() => expect(native.poll).toHaveBeenCalled(), {
    timeout: 2500,
  });
  await act(async () => finishNative({ ...pending, state: "done", account }));
  expect(
    await screen.findByRole("dialog", { name: "My Antigravity" }),
  ).toBeVisible();
  expect(routing.save).not.toHaveBeenCalled();
});
it("prepares the Codex draft only after browser sign-in and selects the completed account", async () => {
  vi.mocked(routing.createCodex).mockResolvedValue({
    schemaVersion: 1,
    id: "draft-codex",
    revision: 0,
    name: "ChatGPT / Codex",
    hasApiKey: false,
    source: { kind: "codex" },
    codex: {
      settings: { accounts: [], mode: "vendor", models: [], groupsOnly: false },
      accounts: [{ ...account, native: false, ready: true }],
      models: [],
    },
  } as never);
  vi.mocked(routing.codexModels).mockResolvedValue([]);
  vi.mocked(routing.codexQuota).mockResolvedValue({
    success: true,
    tiers: [],
    credentialStatus: "valid",
  } as never);
  mount();
  fireEvent.click(screen.getByRole("button", { name: "ChatGPT / Codex" }));
  await waitFor(() => expect(browser.poll).toHaveBeenCalled(), {
    timeout: 2500,
  });
  expect(routing.createCodex).not.toHaveBeenCalled();
  await act(async () =>
    finishBrowser({ id: pending.id, url, status: "done", account }),
  );
  const dialog = await screen.findByRole("dialog", { name: "ChatGPT / Codex" });
  expect(
    within(dialog).getByRole("checkbox", { name: account.login }),
  ).toBeChecked();
  expect(routing.saveCodex).not.toHaveBeenCalled();
});
it("keeps existing model preferences when sign-in renews the already bound account", async () => {
  vi.mocked(routing.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "existing",
      revision: 2,
      name: "My Antigravity",
      hasApiKey: false,
      source: {
        kind: "subscription",
        provider: "antigravity",
        accountId: account.id,
        models: [{ model: "manual-fixture" }],
        catalog: [],
        mode: "picked",
        groupsOnly: true,
      },
    },
  ] as never);
  mount();
  fireEvent.click(screen.getByRole("button", { name: "Antigravity" }));
  await waitFor(() => expect(native.poll).toHaveBeenCalled(), {
    timeout: 2500,
  });
  await act(async () => finishNative({ ...pending, state: "done", account }));
  await screen.findByRole("dialog", { name: "My Antigravity" });
  expect(
    screen.getByRole("button", { name: "manual-fixture" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(
    screen.queryByRole("checkbox", { name: "codexProvider.groupsOnly" }),
  ).toBeNull();
  expect(routing.save).not.toHaveBeenCalled();
  fireEvent.click(
    within(
      screen
        .getByRole("dialog", { name: "My Antigravity" })
        .querySelector(".pv-editor-foot") as HTMLElement,
    ).getByRole("button", { name: "common.save" }),
  );
  await waitFor(() =>
    expect(routing.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source: expect.objectContaining({
          accountId: account.id,
          groupsOnly: true,
          models: [{ model: "manual-fixture" }],
        }),
      }),
    ),
  );
});

it("cancels subscription authorization and ignores late completion after closing its picker", async () => {
  mount();
  const trigger = screen.getByRole("button", {
    name: "providerParity.subscription",
  });
  fireEvent.click(trigger);
  const picker = screen.getByRole("dialog", {
    name: "providerParity.subscription",
  });
  fireEvent.click(within(picker).getByRole("button", { name: "Antigravity" }));
  await waitFor(() => expect(native.poll).toHaveBeenCalled(), {
    timeout: 2500,
  });
  fireEvent.click(within(picker).getByRole("button", { name: "common.close" }));
  await waitFor(() => expect(native.cancel).toHaveBeenCalledWith(pending.id));
  await act(async () => finishNative({ ...pending, state: "done", account }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(trigger).toHaveFocus();
  expect(routing.save).not.toHaveBeenCalled();
});

it.each([false, true])(
  "loads models for the confirmed Codex alias when an existing provider is %s",
  async (existing) => {
    const draft = {
      schemaVersion: 1,
      id: existing ? "existing-codex" : "draft-codex",
      revision: existing ? 2 : 0,
      name: "ChatGPT / Codex",
      hasApiKey: false,
      source: { kind: "codex" },
      codex: {
        settings: {
          accounts: existing ? ["native", "other"] : ["native"],
          mode: existing ? "picked" : "vendor",
          models: existing
            ? [
                {
                  model: "manual-fixture",
                  modelPreferences: { name: "My fixture" },
                },
              ]
            : [],
          groupsOnly: false,
        },
        accounts: [
          {
            id: "native",
            login: account.login,
            native: true,
            ready: false,
            aliases: [account.id],
          },
          ...(existing
            ? [
                {
                  id: "other",
                  login: "other@example.test",
                  native: false,
                  ready: true,
                },
              ]
            : []),
        ],
        models: [],
      },
    };
    vi.mocked(routing.list).mockResolvedValue(
      existing ? ([draft] as never) : [],
    );
    vi.mocked(routing.createCodex).mockResolvedValue(draft as never);
    vi.mocked(routing.editor).mockResolvedValue(draft as never);
    vi.mocked(routing.saveCodex).mockResolvedValue(draft as never);
    vi.mocked(routing.codexModels).mockResolvedValue([
      { model: "confirmed-model" },
    ]);
    vi.mocked(routing.codexQuota).mockResolvedValue({
      success: true,
      credentialStatus: "valid",
      tiers: [],
    } as never);
    vi.mocked(authApi.authGetStatus).mockResolvedValue({
      accounts: [{ ...account, reauth_required: false }],
      authenticated: true,
    } as never);
    mount();
    await waitFor(() => expect(routing.list).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "ChatGPT / Codex" }));
    await waitFor(() => expect(browser.poll).toHaveBeenCalled(), {
      timeout: 2500,
    });
    await act(async () =>
      finishBrowser({ id: pending.id, url, status: "done", account }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "ChatGPT / Codex",
    });
    expect(
      within(dialog).getAllByRole("checkbox", { name: account.login }),
    ).toHaveLength(1);
    expect(
      within(dialog).getByRole("checkbox", { name: account.login }),
    ).toBeChecked();
    await waitFor(() =>
      expect(routing.codexModels).toHaveBeenCalledWith(
        draft.id,
        draft.revision,
        account.id,
      ),
    );
    expect(
      await within(dialog).findByRole("button", { name: "confirmed-model" }),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", {
          name: existing ? "common.save" : "common.add",
        }),
      ).toBeEnabled(),
    );
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: existing ? "common.save" : "common.add",
      }),
    );
    await waitFor(() =>
      expect(routing.saveCodex).toHaveBeenCalledWith(
        expect.anything(),
        expect.objectContaining({
          accounts: existing ? [account.id, "other"] : [account.id],
          models: draft.codex.settings.models,
        }),
      ),
    );
  },
);
