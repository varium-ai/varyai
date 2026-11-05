import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { SubscriptionProviderFields } from "@/components/providers/SubscriptionProviderFields";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import { copilotGetModelsForAccount } from "@/lib/api/copilot";

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
  addAccount: vi.fn(),
  cancelAuth: vi.fn(),
  refetchStatus: vi.fn(),
}));
vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: () => auth,
}));
vi.mock("@/lib/api/copilot", () => ({ copilotGetModelsForAccount: vi.fn() }));
const onChange = vi.fn();
function mount() {
  function Editor() {
    const [draft, setDraft] = useState<RoutingUpstream>({
      schemaVersion: 1,
      id: "fixture",
      name: "Copilot",
      revision: 0,
      hasApiKey: false,
      source: {
        kind: "subscription",
        provider: "github_copilot",
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
  render(
    <QueryClientProvider client={new QueryClient()}>
      <Editor />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
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
          models: [{ model: "exact-model", displayName: "Exact model" }],
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
