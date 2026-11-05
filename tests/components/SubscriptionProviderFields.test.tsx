import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { SubscriptionProviderFields } from "@/components/providers/SubscriptionProviderFields";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import {
  copilotGetUsageForAccount,
  copilotGetModelsForAccount,
} from "@/lib/api/copilot";

const auth = vi.hoisted(() => ({
  accounts: [
    { id: "fixture-account", login: "fixture-user", requires_reauth: false },
  ],
  isStatusSuccess: true,
  isStatusError: false,
  isLoadingStatus: false,
  isAddingAccount: false,
  deviceCode: null,
  error: null,
  loginCompletion: null as { accountId: string; generation: number } | null,
  addAccount: vi.fn(),
  cancelAuth: vi.fn(),
  refetchStatus: vi.fn(),
  removeAccount: vi.fn(),
}));
vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: () => auth,
}));
vi.mock("@/lib/api/copilot", () => ({
  copilotGetModelsForAccount: vi.fn(),
  copilotGetUsageForAccount: vi.fn(),
}));
const onChange = vi.fn();
function mount(provider: "github_copilot" | "xai_oauth" = "github_copilot") {
  function Editor() {
    const [draft, setDraft] = useState<RoutingUpstream>({
      schemaVersion: 1,
      id: "fixture",
      name: "Copilot",
      revision: 0,
      hasApiKey: false,
      source: {
        kind: "subscription",
        provider,
        accountId: "",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    });
    return (
      <SubscriptionProviderFields
        draft={draft}
        disabled={false}
        onChange={(next) => {
          setDraft(next);
          onChange(next);
        }}
        onError={vi.fn()}
        onBusyChange={vi.fn()}
      />
    );
  }
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <Editor />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(copilotGetUsageForAccount).mockResolvedValue({
    quota_snapshots: {},
    quota_reset_date: "2026-11-06",
  } as never);
  auth.accounts = [
    { id: "fixture-account", login: "fixture-user", requires_reauth: false },
  ];
  auth.loginCompletion = null;
  auth.isAddingAccount = false;
  auth.isStatusError = false;
  auth.isStatusSuccess = true;
});
it("requires an explicit account and fetches models for that binding", async () => {
  vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
    {
      id: "exact-model",
      name: "Exact model",
      vendor: "fixture",
      model_picker_enabled: true,
    },
  ]);
  mount();
  expect(
    screen.getByRole("button", { name: "codexProvider.fetchModels" }),
  ).toBeDisabled();
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("radio", { name: "fixture-user" }));
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.fetchModels" }),
  );
  expect(
    await screen.findByRole("button", { name: "Exact model" }),
  ).toBeVisible();
  expect(copilotGetModelsForAccount).toHaveBeenCalledWith("fixture-account");
  fireEvent.click(screen.getByRole("button", { name: "Exact model" }));
  await waitFor(() =>
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({
        source: expect.objectContaining({
          accountId: "fixture-account",
          mode: "picked",
          models: [
            expect.objectContaining({
              model: "exact-model",
              displayName: "Exact model",
            }),
          ],
        }),
      }),
    ),
  );
});
it("distinguishes unavailable account status and allows retry", () => {
  auth.isStatusError = true;
  auth.isStatusSuccess = false;
  mount();
  expect(screen.getByRole("alert")).toHaveTextContent(
    "subscriptionProvider.statusFailed",
  );
  fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
  expect(auth.refetchStatus).toHaveBeenCalledOnce();
});

it.each(["github_copilot", "xai_oauth"] as const)(
  "offers re-login for a rejected %s subscription without sending an unsupported target",
  (provider) => {
    auth.accounts[0].requires_reauth = true;
    mount(provider);
    expect(screen.getByText("codexOauth.reauthBadge")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "codexOauth.reauthLogin fixture-user",
      }),
    );
    expect(auth.addAccount).toHaveBeenCalledOnce();
  },
);

it("retains selected models when the bound managed account finishes re-login", async () => {
  auth.accounts[0].requires_reauth = true;
  const draft: RoutingUpstream = {
    schemaVersion: 1,
    id: "fixture",
    name: "Copilot",
    revision: 0,
    hasApiKey: false,
    source: {
      kind: "subscription",
      provider: "github_copilot",
      accountId: "fixture-account",
      models: [{ model: "chosen-model" }],
      catalog: [],
      mode: "picked",
      groupsOnly: false,
    },
  };
  vi.mocked(copilotGetModelsForAccount).mockResolvedValue([]);
  const client = new QueryClient();
  const props = {
    draft,
    disabled: false,
    onChange,
    onError: vi.fn(),
    onBusyChange: vi.fn(),
  };
  const view = render(
    <QueryClientProvider client={client}>
      <SubscriptionProviderFields {...props} />
    </QueryClientProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "codexOauth.reauthLogin fixture-user" }),
  );
  auth.accounts[0].requires_reauth = false;
  auth.loginCompletion = { accountId: "fixture-account", generation: 1 };
  view.rerender(
    <QueryClientProvider client={client}>
      <SubscriptionProviderFields {...props} />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        source: expect.objectContaining({
          accountId: "fixture-account",
          models: [{ model: "chosen-model" }],
          mode: "picked",
        }),
      }),
    ),
  );
  expect(screen.queryByText("codexOauth.reauthBadge")).not.toBeInTheDocument();
});

it("offers removal for a managed account alongside its login actions", () => {
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "subscriptionProvider.removeAccount" }),
  );
  expect(auth.removeAccount).toHaveBeenCalledWith("fixture-account");
});

it("supports selecting and clearing all discovered subscription models", async () => {
  vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
    {
      id: "model-one",
      name: "One",
      vendor: "fixture",
      model_picker_enabled: true,
    },
    {
      id: "model-two",
      name: "Two",
      vendor: "fixture",
      model_picker_enabled: true,
    },
  ]);
  mount();
  fireEvent.click(screen.getByRole("radio", { name: "fixture-user" }));
  fireEvent.click(
    screen.getByRole("button", { name: "codexProvider.fetchModels" }),
  );
  await screen.findByRole("button", { name: "One" });
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.selectAll" }),
  );
  expect(screen.getByRole("button", { name: "One" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(screen.getByRole("button", { name: "Two" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.selectNone" }),
  );
  expect(screen.getByRole("button", { name: "One" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
});

it("adds an exact manual subscription model ID without duplicating it", () => {
  mount();
  const input = screen.getByRole("textbox", {
    name: "providerEditor.addModelId",
  });
  fireEvent.change(input, { target: { value: "  exact-model  " } });
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.addModel" }),
  );
  expect(onChange).toHaveBeenLastCalledWith(
    expect.objectContaining({
      source: expect.objectContaining({
        models: [{ model: "exact-model" }],
        mode: "picked",
      }),
    }),
  );
  expect(input).toHaveValue("");
  fireEvent.change(input, { target: { value: "exact-model" } });
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.addModel" }),
  );
  expect(screen.getAllByRole("button", { name: "exact-model" })).toHaveLength(
    1,
  );
});
it("automatically starts re-login once when opened from a row re-login action", () => {
  auth.accounts[0].requires_reauth = true;
  const onReauthStarted = vi.fn();
  const draft: RoutingUpstream = {
    schemaVersion: 1,
    id: "fixture",
    name: "Copilot",
    revision: 1,
    hasApiKey: false,
    source: {
      kind: "subscription",
      provider: "github_copilot",
      accountId: "fixture-account",
      models: [],
      catalog: [],
      mode: "vendor",
      groupsOnly: false,
    },
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SubscriptionProviderFields
        draft={draft}
        disabled={false}
        startReauth
        onReauthStarted={onReauthStarted}
        onChange={onChange}
        onError={vi.fn()}
        onBusyChange={vi.fn()}
      />
    </QueryClientProvider>,
  );
  expect(onReauthStarted).toHaveBeenCalledOnce();
  expect(auth.addAccount).toHaveBeenCalledOnce();
});

it("keeps the subscription editor editable during managed device authorization", () => {
  auth.isAddingAccount = true;
  mount();
  expect(
    screen.getByRole("radio", { name: "providerParity.mode_picked" }),
  ).toBeEnabled();
  expect(
    screen.queryByRole("button", { name: "subscriptionProvider.addAccount" }),
  ).not.toBeInTheDocument();
  auth.isAddingAccount = false;
});

it("discovers the explicitly selected account automatically", async () => {
  vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
    {
      id: "automatic-model",
      name: "Automatic model",
      vendor: "fixture",
      model_picker_enabled: true,
    },
  ]);
  mount();
  fireEvent.click(screen.getByRole("radio", { name: "fixture-user" }));
  expect(
    await screen.findByRole("button", { name: "Automatic model" }),
  ).toBeVisible();
  expect(copilotGetModelsForAccount).toHaveBeenCalledWith("fixture-account");
});

it("ignores the previous account's discovery response after the binding changes", async () => {
  let finishOld!: (
    models: Awaited<ReturnType<typeof copilotGetModelsForAccount>>,
  ) => void;
  vi.mocked(copilotGetModelsForAccount).mockImplementation(async (id) =>
    id === "old-account"
      ? new Promise((resolve) => {
          finishOld = resolve;
        })
      : [
          {
            id: "new-model",
            name: "New model",
            vendor: "fixture",
            model_picker_enabled: true,
          },
        ],
  );
  const draft: RoutingUpstream = {
    schemaVersion: 1,
    id: "fixture",
    name: "Fixture",
    revision: 0,
    hasApiKey: false,
    source: {
      kind: "subscription",
      provider: "github_copilot",
      accountId: "old-account",
      models: [],
      catalog: [],
      mode: "vendor",
      groupsOnly: false,
    },
  };
  const client = new QueryClient();
  const props = {
    draft,
    disabled: false,
    onChange,
    onError: vi.fn(),
    onBusyChange: vi.fn(),
  };
  const { rerender } = render(
    <QueryClientProvider client={client}>
      <SubscriptionProviderFields {...props} />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(copilotGetModelsForAccount).toHaveBeenCalledWith("old-account"),
  );
  rerender(
    <QueryClientProvider client={client}>
      <SubscriptionProviderFields
        {...props}
        draft={{
          ...draft,
          source: {
            ...draft.source,
            kind: "subscription",
            provider: "github_copilot",
            accountId: "new-account",
            models: [],
            catalog: [],
            mode: "vendor",
            groupsOnly: false,
          },
        }}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        source: expect.objectContaining({
          accountId: "new-account",
          catalog: [expect.objectContaining({ model: "new-model" })],
        }),
      }),
    ),
  );
  const writes = onChange.mock.calls.length;
  finishOld([
    {
      id: "old-model",
      name: "Old model",
      vendor: "fixture",
      model_picker_enabled: true,
    },
  ]);
  await waitFor(() =>
    expect(
      client.getQueryData([
        "subscription-account-models",
        "github_copilot",
        "old-account",
      ]),
    ).toBeDefined(),
  );
  expect(onChange).toHaveBeenCalledTimes(writes);
});
