import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";
import { routingUpstreamsApi as routing } from "@/lib/api/routingUpstreams";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { providerImportApi } from "@/lib/api/providerImport";
import { copyText } from "@/lib/clipboard";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    save: vi.fn(),
    createCodex: vi.fn(),
    saveCodex: vi.fn(),
    editor: vi.fn(),
  },
}));
vi.mock("@/lib/api/model-fetch", () => ({ fetchModelsForConfig: vi.fn() }));
vi.mock("@/lib/api/providerImport", () => ({
  providerImportApi: { preview: vi.fn(), import: vi.fn() },
}));
vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));
vi.mock("@/components/providers/SubscriptionSignIn", () => ({
  SubscriptionSignIn: ({
    onComplete,
  }: {
    onComplete: (id: string) => void;
  }) => (
    <button onClick={() => onComplete("fixture-account")}>
      Finish fixture login
    </button>
  ),
}));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(routing.list).mockResolvedValue([]);
  vi.mocked(routing.save).mockResolvedValue({
    id: "saved",
    revision: 1,
  } as never);
  vi.mocked(fetchModelsForConfig).mockResolvedValue([
    { id: "fixture-model", ownedBy: null },
  ]);
  vi.mocked(copyText).mockResolvedValue(undefined);
  vi.mocked(providerImportApi.preview).mockResolvedValue([
    {
      id: "cc-switch",
      name: "CC Switch",
      path: "~/.fixture/config",
      found: true,
      items: [
        {
          reference: "new",
          fingerprint: "digest",
          name: "Fixture relay",
          host: "relay.example.test",
          modelCount: 1,
          status: "new",
          enabled: true,
        },
        {
          reference: "old",
          fingerprint: "old-digest",
          name: "Existing relay",
          host: "old.example.test",
          modelCount: 1,
          status: "same",
          enabled: true,
        },
      ],
    },
  ]);
  vi.mocked(providerImportApi.import).mockResolvedValue({
    added: 1,
    skipped: 0,
  });
});
function mount() {
  const onFinish = vi.fn().mockResolvedValue(undefined);
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <OnboardingWizard onFinish={onFinish} />
    </QueryClientProvider>,
  );
  return onFinish;
}
function enterKey() {
  fireEvent.click(screen.getByRole("button", { name: "onboarding.keyAction" }));
  fireEvent.change(screen.getByLabelText("onboarding.url"), {
    target: { value: "https://relay.example.test/v1" },
  });
  fireEvent.change(screen.getByLabelText("onboarding.key"), {
    target: { value: "sk-synthetic-fixture" },
  });
}
it("starts with three paths, copies setup help and allows skipping", async () => {
  const finish = mount();
  expect(
    screen.getByRole("button", { name: "onboarding.keyAction" }),
  ).toBeVisible();
  expect(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  ).toBeVisible();
  expect(
    screen.getByRole("button", { name: "onboarding.importAction" }),
  ).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "common.copy" }));
  await waitFor(() =>
    expect(copyText).toHaveBeenCalledWith("onboarding.helpPrompt"),
  );
  fireEvent.click(screen.getByRole("button", { name: "onboarding.skip" }));
  await waitFor(() => expect(finish).toHaveBeenCalledWith(false));
});
it("checks credentials before saving and keeps the key masked", async () => {
  const finish = mount();
  enterKey();
  expect(screen.getByLabelText("onboarding.key")).toHaveAttribute(
    "type",
    "password",
  );
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  await screen.findByRole("button", { name: "onboarding.enterTools" });
  expect(fetchModelsForConfig).toHaveBeenCalledWith(
    "https://relay.example.test/v1",
    "",
    false,
    undefined,
    undefined,
    {
      apiFormat: "openai_chat",
      requestHeaders: { Authorization: "Bearer sk-synthetic-fixture" },
    },
  );
  expect(routing.save).toHaveBeenCalledWith(
    expect.objectContaining({
      source: expect.objectContaining({
        apiKey: "sk-synthetic-fixture",
        apiFormat: "openai_chat",
        catalog: [expect.objectContaining({ model: "fixture-model" })],
        mode: "vendor",
      }),
    }),
  );
  expect(finish).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.enterTools" }),
  );
  await waitFor(() => expect(finish).toHaveBeenCalledWith(true));
});
it("keeps failed connections editable, sanitizes errors and never saves an empty catalog", async () => {
  mount();
  enterKey();
  vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
    new Error("synthetic-private-upstream-detail"),
  );
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "onboarding.connectionFailed",
  );
  expect(document.body).not.toHaveTextContent(
    "synthetic-private-upstream-detail",
  );
  expect(routing.save).not.toHaveBeenCalled();
  vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([]);
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("onboarding.noModels"),
  );
  expect(routing.save).not.toHaveBeenCalled();
});
it("locks navigation and prevents duplicate saves while connecting", async () => {
  let resolve!: (value: never[]) => void;
  vi.mocked(fetchModelsForConfig).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  mount();
  enterKey();
  const connect = screen.getByRole("button", { name: "onboarding.connect" });
  fireEvent.click(connect);
  fireEvent.click(connect);
  expect(screen.getByRole("button", { name: "common.back" })).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "onboarding.skip" }),
  ).toBeDisabled();
  await act(async () => resolve([]));
  expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
});
it("saves the account selected by a completed subscription login", async () => {
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
  fireEvent.click(screen.getByRole("button", { name: "Finish fixture login" }));
  await screen.findByRole("button", { name: "onboarding.enterTools" });
  expect(routing.save).toHaveBeenCalledWith(
    expect.objectContaining({
      source: expect.objectContaining({
        kind: "subscription",
        provider: "gemini_cli",
        accountId: "fixture-account",
      }),
    }),
  );
});
it("adds the completed ChatGPT account to the Codex provider before saving", async () => {
  vi.mocked(routing.createCodex).mockResolvedValue({
    id: "fixture-codex",
    source: { kind: "codex" },
    codex: {
      settings: { accounts: [], models: [], mode: "vendor", groupsOnly: false },
    },
  } as never);
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "ChatGPT / Codex" }));
  fireEvent.click(screen.getByRole("button", { name: "Finish fixture login" }));
  await screen.findByRole("button", { name: "onboarding.enterTools" });
  expect(routing.saveCodex).toHaveBeenCalledWith(
    expect.objectContaining({ id: "fixture-codex" }),
    expect.objectContaining({ accounts: ["fixture-account"] }),
  );
});
it("preserves an existing subscription's model policy when reconnecting the same account", async () => {
  const source = {
    kind: "subscription" as const,
    provider: "gemini_cli" as const,
    accountId: "fixture-account",
    models: ["fixture-model"],
    catalog: ["fixture-model"],
    mode: "picked" as const,
    groupsOnly: true,
  };
  vi.mocked(routing.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "existing",
      revision: 4,
      name: "Existing subscription",
      hasApiKey: false,
      source,
    },
  ]);
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
  fireEvent.click(screen.getByRole("button", { name: "Finish fixture login" }));
  await screen.findByRole("button", { name: "onboarding.enterTools" });
  expect(routing.save).toHaveBeenCalledWith(
    expect.objectContaining({ id: "existing", revision: 4, source }),
  );
});
it.each([
  ["anthropic", "x-api-key"],
  ["gemini_native", "x-goog-api-key"],
])("uses the correct credential header for %s", async (format, header) => {
  mount();
  enterKey();
  fireEvent.change(screen.getByLabelText("onboarding.protocol"), {
    target: { value: format },
  });
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  await screen.findByRole("button", { name: "onboarding.enterTools" });
  expect(fetchModelsForConfig).toHaveBeenCalledWith(
    expect.anything(),
    "",
    false,
    undefined,
    undefined,
    { apiFormat: format, requestHeaders: { [header]: "sk-synthetic-fixture" } },
  );
});
it("embeds import selection, prevents duplicate imports and only imports selected references", async () => {
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.importAction" }),
  );
  expect(
    await screen.findByRole("checkbox", { name: "Fixture relay" }),
  ).toBeChecked();
  expect(
    screen.getByRole("checkbox", { name: "Existing relay" }),
  ).toBeDisabled();
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.import" }));
  await screen.findByRole("button", { name: "onboarding.enterTools" });
  expect(providerImportApi.import).toHaveBeenCalledWith([
    {
      source: "cc-switch",
      reference: "new",
      fingerprint: "digest",
      mode: "add",
    },
  ]);
});
it("retries finishing without saving the provider twice", async () => {
  const finish = mount();
  finish.mockRejectedValueOnce(new Error("synthetic-settings-failure"));
  enterKey();
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "onboarding.enterTools" }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "onboarding.finishFailed",
  );
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.enterTools" }),
  );
  await waitFor(() => expect(finish).toHaveBeenCalledTimes(2));
  expect(routing.save).toHaveBeenCalledTimes(1);
});
