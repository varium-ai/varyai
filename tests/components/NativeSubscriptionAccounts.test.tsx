import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { NativeSubscriptionAccounts } from "@/components/providers/NativeSubscriptionAccounts";
import {
  nativeSubscriptionApi as api,
  type NativeSubscriptionProvider,
} from "@/lib/api/nativeSubscription";
import { settingsApi } from "@/lib/api/settings";

vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: {
    accounts: vi.fn(),
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
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
beforeEach(() => {
  vi.clearAllMocks();
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
        onError={vi.fn()}
      />
    </QueryClientProvider>,
  );
  return { ...view, onSelect };
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
