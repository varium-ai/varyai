import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { NativeSubscriptionAccounts } from "@/components/providers/NativeSubscriptionAccounts";
import {
  nativeSubscriptionApi as api,
  type NativeSubscriptionProvider,
} from "@/lib/api/nativeSubscription";
import { copyText } from "@/lib/clipboard";
import { settingsApi } from "@/lib/api/settings";

vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: {
    accounts: vi.fn(),
    quota: vi.fn(),
    start: vi.fn(),
    poll: vi.fn(),
    cancel: vi.fn(),
    import: vi.fn(),
    remove: vi.fn(),
    project: vi.fn(),
  },
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));
vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(settingsApi.openExternal).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.quota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [],
  } as never);
  vi.mocked(api.accounts).mockResolvedValue([
    {
      id: "fixture-account",
      login: "Fixture account",
      project: "fixture-project",
    },
  ]);
  vi.mocked(api.cancel).mockResolvedValue();
});
function mount(provider: NativeSubscriptionProvider, accountId = "") {
  const onSelect = vi.fn();
  const onError = vi.fn();
  const view = render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <NativeSubscriptionAccounts
        provider={provider}
        accountId={accountId}
        disabled={false}
        onSelect={onSelect}
        onBusyChange={vi.fn()}
        onError={onError}
      />
    </QueryClientProvider>,
  );
  return { ...view, onSelect, onError };
}
it("lists each provider separately and requires explicit account selection", async () => {
  const { onSelect } = mount("antigravity");
  const account = await screen.findByRole("radio", { name: /Fixture account/ });
  expect(api.accounts).toHaveBeenCalledWith("antigravity");
  expect(account).not.toBeChecked();
  expect(onSelect).not.toHaveBeenCalled();
  fireEvent.click(account);
  expect(onSelect).toHaveBeenCalledWith("fixture-account");
});
it.each([
  "claude",
  "chatgpt_api",
  "gemini_cli",
  "antigravity",
  "cursor",
] as const)(
  "keeps %s account actions inside the same account frame",
  async (provider) => {
    mount(provider);
    const radio = await screen.findByRole("radio", { name: /Fixture account/ });
    const add = screen.getByRole("button", {
      name:
        provider === "chatgpt_api"
          ? "nativeSubscription.continueChatgpt"
          : "subscriptionProvider.addAccount",
    });
    const frame = radio.closest(".cx-accounts");
    expect(frame).toContainElement(add);
    if (provider !== "chatgpt_api") {
      expect(frame).toContainElement(
        screen.getByRole("button", {
          name: "nativeSubscription.importFile",
        }),
      );
    }
    if (
      provider === "claude" ||
      provider === "gemini_cli" ||
      provider === "cursor"
    ) {
      expect(frame).toContainElement(
        screen.getByRole("button", {
          name: "nativeSubscription.importLocal",
        }),
      );
    }
  },
);
it("opens independent ChatGPT API authorization and cancels on unmount", async () => {
  vi.mocked(api.start).mockResolvedValue({
    id: "fixture-flow",
    state: "waiting",
    url: "https://auth.openai.com/fixture",
  });
  const { unmount } = mount("chatgpt_api");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "nativeSubscription.continueChatgpt",
    }),
  );
  await waitFor(() =>
    expect(settingsApi.openExternal).toHaveBeenCalledWith(
      "https://auth.openai.com/fixture",
    ),
  );
  expect(api.start).toHaveBeenCalledWith("chatgpt_api");
  expect(
    screen.queryByRole("button", { name: "nativeSubscription.importLocal" }),
  ).not.toBeInTheDocument();
  unmount();
  expect(api.cancel).toHaveBeenCalledWith("fixture-flow");
});
it("saves a Google project only for the bound account", async () => {
  mount("gemini_cli", "fixture-account");
  const input = await screen.findByPlaceholderText(
    "nativeSubscription.projectPlaceholder",
  );
  await waitFor(() => expect(input).toHaveValue("fixture-project"));
  fireEvent.change(input, { target: { value: "new-fixture-project" } });
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(api.project).toHaveBeenCalledWith(
      "gemini_cli",
      "fixture-account",
      "new-fixture-project",
    ),
  );
});
it("imports local Claude credentials and selects the returned account", async () => {
  vi.mocked(api.import).mockResolvedValue({
    id: "imported-fixture",
    login: "Imported",
    project: "",
  });
  const { onSelect } = mount("claude");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "nativeSubscription.importLocal",
    }),
  );
  await waitFor(() =>
    expect(onSelect).toHaveBeenCalledWith("imported-fixture"),
  );
  expect(api.import).toHaveBeenCalledWith("claude", undefined);
});

it.each([
  "claude",
  "chatgpt_api",
  "gemini_cli",
  "antigravity",
  "cursor",
] as const)(
  "offers re-login and a cached plan for rejected %s credentials",
  async (provider) => {
    vi.mocked(api.accounts).mockResolvedValue([
      {
        id: "fixture-account",
        login: "Fixture account",
        project: "",
        reauthRequired: true,
        plan: "Synthetic plan",
      },
    ] as never);
    vi.mocked(api.start).mockResolvedValue({
      id: "relogin-flow",
      state: "waiting",
    });
    mount(provider, "fixture-account");
    expect(await screen.findByText("codexOauth.reauthBadge")).toBeVisible();
    expect(screen.getByText("Synthetic plan")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "codexOauth.reauthLogin Fixture account",
      }),
    );
    await waitFor(() =>
      expect(api.start).toHaveBeenCalledWith(provider, "fixture-account"),
    );
  },
);

it("opens Antigravity in the browser and keeps its authorization URL available to copy and reopen", async () => {
  const url =
    "https://accounts.google.com/o/oauth2/v2/auth?state=synthetic-state";
  vi.mocked(api.start).mockResolvedValue({
    id: "synthetic-flow",
    state: "waiting",
    url,
  });
  mount("antigravity");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "subscriptionProvider.addAccount",
    }),
  );
  await waitFor(() =>
    expect(settingsApi.openExternal).toHaveBeenCalledWith(url),
  );
  expect(
    screen.getByRole("textbox", {
      name: "nativeSubscription.authorizationUrl",
    }),
  ).toHaveValue(url);
  fireEvent.click(screen.getByRole("button", { name: "common.copy" }));
  await waitFor(() => expect(copyText).toHaveBeenCalledWith(url));
  fireEvent.click(
    screen.getByRole("button", { name: "subscriptionProvider.openLogin" }),
  );
  await waitFor(() =>
    expect(settingsApi.openExternal).toHaveBeenCalledTimes(2),
  );
});
it("retains the pending login and offers browser recovery when automatic opening fails", async () => {
  vi.mocked(api.start).mockResolvedValue({
    id: "synthetic-flow",
    state: "waiting",
    url: "https://accounts.google.com/fixture",
  });
  vi.mocked(settingsApi.openExternal).mockRejectedValueOnce(
    new Error("synthetic-private-error"),
  );
  mount("antigravity");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "subscriptionProvider.addAccount",
    }),
  );
  expect(
    await screen.findByText("nativeSubscription.browserOpenFailed"),
  ).toBeVisible();
  expect(
    screen.getByRole("textbox", {
      name: "nativeSubscription.authorizationUrl",
    }),
  ).toBeVisible();
  expect(screen.queryByText("synthetic-private-error")).not.toBeInTheDocument();
});

it("reports login preparation errors in the account panel without reporting a catalog failure", async () => {
  vi.mocked(api.start).mockRejectedValue("nativeSubscription.failed");
  const { onError } = mount("antigravity");
  fireEvent.click(
    await screen.findByRole("button", {
      name: "subscriptionProvider.addAccount",
    }),
  );
  expect(await screen.findByText("nativeSubscription.failed")).toBeVisible();
  expect(onError).not.toHaveBeenCalled();
  expect(settingsApi.openExternal).not.toHaveBeenCalled();
});
it("automatically starts target re-login when opened from a row re-login action", async () => {
  vi.mocked(api.accounts).mockResolvedValue([
    {
      id: "fixture-account",
      login: "Fixture account",
      project: "",
      reauthRequired: true,
    },
  ] as never);
  vi.mocked(api.start).mockResolvedValue({
    id: "relogin-flow",
    state: "waiting",
  });
  const onReauthStarted = vi.fn();
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <NativeSubscriptionAccounts
        provider="chatgpt_api"
        accountId="fixture-account"
        disabled={false}
        startReauth
        onReauthStarted={onReauthStarted}
        onSelect={vi.fn()}
        onBusyChange={vi.fn()}
        onError={vi.fn()}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(api.start).toHaveBeenCalledWith("chatgpt_api", "fixture-account"),
  );
  expect(onReauthStarted).toHaveBeenCalledOnce();
});

it("reports only login preparation as busy and leaves the editor unlocked while waiting for the browser", async () => {
  const onBusyChange = vi.fn();
  vi.mocked(api.start).mockResolvedValue({
    id: "synthetic-waiting",
    state: "waiting",
    url: "https://accounts.google.com/fixture",
  });
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <NativeSubscriptionAccounts
        provider="antigravity"
        accountId="fixture-account"
        disabled={false}
        onSelect={vi.fn()}
        onBusyChange={onBusyChange}
        onError={vi.fn()}
      />
    </QueryClientProvider>,
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "subscriptionProvider.addAccount",
    }),
  );
  await screen.findByText("nativeSubscription.browserWaiting");
  await waitFor(() => expect(onBusyChange).toHaveBeenLastCalledWith(false));
  expect(screen.getByRole("button", { name: "common.cancel" })).toBeEnabled();
});

it("does not restart browser authorization in a closing editor", async () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <NativeSubscriptionAccounts
        provider="antigravity"
        accountId="fixture-account"
        disabled={false}
        editorOpen={false}
        startReauth
        onSelect={vi.fn()}
        onBusyChange={vi.fn()}
        onError={vi.fn()}
      />
    </QueryClientProvider>,
  );
  await screen.findByRole("radio", { name: /Fixture account/ });
  expect(api.start).not.toHaveBeenCalled();
});
