import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import { routingUpstreamsApi as routing } from "@/lib/api/routingUpstreams";
import {
  fetchModelsForConfig,
  fetchXaiOauthModels,
} from "@/lib/api/model-fetch";
import { copilotGetModelsForAccount } from "@/lib/api/copilot";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";
import { providerImportApi } from "@/lib/api/providerImport";
import { copyText } from "@/lib/clipboard";
import { getSettings, setSettings } from "../msw/state";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    save: vi.fn(),
    createCodex: vi.fn(),
    saveCodex: vi.fn(),
    editor: vi.fn(),
    codexModels: vi.fn(),
  },
}));
vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
  fetchXaiOauthModels: vi.fn(),
}));
vi.mock("@/lib/api/copilot", () => ({
  copilotGetModelsForAccount: vi.fn(),
}));
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: { models: vi.fn() },
}));
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
  vi.mocked(nativeSubscriptionApi.models).mockResolvedValue([
    { model: "fixture-model", inputModalities: ["text", "image"] },
    { model: "fixture-other" },
  ]);
  vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
    {
      id: "fixture-model",
      name: "Fixture model",
      vendor: "fixture",
      model_picker_enabled: true,
      supported_endpoints: ["/responses"],
    },
  ]);
  vi.mocked(fetchXaiOauthModels).mockResolvedValue([
    { id: "fixture-model", ownedBy: null, reasoning: true },
  ]);
  vi.mocked(routing.codexModels).mockResolvedValue([
    { model: "fixture-model", reasoningLevels: ["low", "high"] },
    { model: "fixture-other" },
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
it("resets copied feedback two seconds after the latest successful copy", async () => {
  vi.useFakeTimers();
  try {
    mount();
    const button = screen.getByRole("button", { name: "common.copy" });
    await act(async () => fireEvent.click(button));
    expect(button).toHaveAccessibleName("common.copied");
    expect(button).toBeEnabled();

    act(() => vi.advanceTimersByTime(1500));
    await act(async () => fireEvent.click(button));
    expect(copyText).toHaveBeenCalledTimes(2);
    act(() => vi.advanceTimersByTime(500));
    expect(button).toHaveAccessibleName("common.copied");
    act(() => vi.advanceTimersByTime(1499));
    expect(button).toHaveAccessibleName("common.copied");
    act(() => vi.advanceTimersByTime(1));
    expect(button).toHaveAccessibleName("common.copy");

    await act(async () => fireEvent.click(button));
    expect(copyText).toHaveBeenCalledTimes(3);
    expect(copyText).toHaveBeenLastCalledWith("onboarding.helpPrompt");
    expect(button).toHaveAccessibleName("common.copied");
  } finally {
    vi.useRealTimers();
  }
});

it("clears the copied feedback timer when onboarding unmounts", async () => {
  vi.useFakeTimers();
  const schedule = vi.spyOn(globalThis, "setTimeout");
  const cancel = vi.spyOn(globalThis, "clearTimeout");
  try {
    mount();
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "common.copy" })),
    );
    const timerIndex = schedule.mock.calls.findIndex(
      ([, delay]) => delay === 2000,
    );
    expect(timerIndex).toBeGreaterThanOrEqual(0);
    const timer = schedule.mock.results[timerIndex].value;
    cleanup();
    expect(cancel).toHaveBeenCalledWith(timer);
  } finally {
    schedule.mockRestore();
    cancel.mockRestore();
    vi.useRealTimers();
  }
});

it("allows retrying a failed help copy and clears the copy error on success", async () => {
  vi.mocked(copyText).mockRejectedValueOnce(
    new Error("Synthetic copy failure"),
  );
  mount();
  const button = screen.getByRole("button", { name: "common.copy" });
  await act(async () => fireEvent.click(button));
  expect(screen.getByText("onboarding.copyFailed")).toBeVisible();
  expect(button).toHaveAccessibleName("common.copy");
  expect(button).toBeEnabled();

  await act(async () => fireEvent.click(button));
  expect(copyText).toHaveBeenCalledTimes(2);
  expect(button).toHaveAccessibleName("common.copied");
  expect(screen.queryByText("onboarding.copyFailed")).not.toBeInTheDocument();
});

it("previews account and import sources as named brand icons", () => {
  mount();
  const accounts = screen.getByRole("list", {
    name: "onboarding.subscriptionTitle",
  });
  const imports = screen.getByRole("list", {
    name: "onboarding.importTitle",
  });
  const accountNames = [
    "Claude",
    "ChatGPT / Codex",
    "GitHub Copilot",
    "Gemini CLI",
    "Antigravity",
    "Grok",
  ];
  const importNames = [
    "Magpie",
    "CC Switch",
    "Claude Code",
    "Codex",
    "OpenCode",
  ];
  for (const [list, names] of [
    [accounts, accountNames],
    [imports, importNames],
  ] as const) {
    const icons = within(list).getAllByRole("img");
    expect(icons.map((icon) => icon.getAttribute("aria-label"))).toEqual(names);
    for (const [index, icon] of icons.entries()) {
      expect(icon).toHaveAttribute("title", names[index]);
      expect(icon.querySelector("svg, img")).not.toBeNull();
    }
  }
  expect(providerImportApi.preview).not.toHaveBeenCalled();
});
it("keeps both OpenAI login methods available behind the shared brand preview", () => {
  mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  expect(screen.getByRole("button", { name: "ChatGPT / Codex" })).toBeVisible();
  expect(screen.getByRole("button", { name: "ChatGPT API" })).toBeVisible();
});
it("toggles the onboarding key visibility without changing or submitting it", () => {
  mount();
  enterKey();
  const input = screen.getByLabelText("onboarding.key");
  const show = screen.getByRole("button", { name: "apiKeyInput.show" });
  expect(input).toHaveAttribute("type", "password");
  expect(show).toHaveAttribute("type", "button");
  expect(show).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(show);
  expect(input).toHaveAttribute("type", "text");
  expect(input).toHaveValue("sk-synthetic-fixture");
  const hide = screen.getByRole("button", { name: "apiKeyInput.hide" });
  expect(hide).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(hide);
  expect(input).toHaveAttribute("type", "password");
  expect(input).toHaveValue("sk-synthetic-fixture");
  expect(fetchModelsForConfig).not.toHaveBeenCalled();
  expect(routing.save).not.toHaveBeenCalled();
});

it("masks the key again when returning to the onboarding key form", () => {
  mount();
  enterKey();
  fireEvent.click(screen.getByRole("button", { name: "apiKeyInput.show" }));
  fireEvent.click(screen.getByRole("button", { name: "common.back" }));
  fireEvent.click(screen.getByRole("button", { name: "onboarding.keyAction" }));
  const input = screen.getByLabelText("onboarding.key");
  expect(input).toHaveAttribute("type", "password");
  expect(input).toHaveValue("sk-synthetic-fixture");
  expect(
    screen.getByRole("button", { name: "apiKeyInput.show" }),
  ).toBeEnabled();
});

it("checks credentials before saving and keeps the key masked", async () => {
  const finish = mount();
  enterKey();
  expect(screen.getByLabelText("onboarding.key")).toHaveAttribute(
    "type",
    "password",
  );
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
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
  expect(
    screen.queryByRole("button", { name: "onboarding.enterTools" }),
  ).toBeNull();
  expect(screen.queryByText("onboarding.nextHint")).toBeNull();
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
  expect(
    screen.getByRole("button", { name: "apiKeyInput.show" }),
  ).toBeDisabled();
  expect(screen.getByRole("button", { name: "common.back" })).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "onboarding.skip" }),
  ).toBeDisabled();
  await act(async () => resolve([]));
  expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
});
it("saves the account selected by a completed subscription login", async () => {
  const finish = mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
  fireEvent.click(screen.getByRole("button", { name: "Finish fixture login" }));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
  expect(routing.save).toHaveBeenCalledWith(
    expect.objectContaining({
      source: expect.objectContaining({
        kind: "subscription",
        provider: "gemini_cli",
        accountId: "fixture-account",
        mode: "picked",
        models: [
          { model: "fixture-model", inputModalities: ["text", "image"] },
          { model: "fixture-other" },
        ],
        catalog: [
          { model: "fixture-model", inputModalities: ["text", "image"] },
          { model: "fixture-other" },
        ],
        groupsOnly: false,
      }),
    }),
  );
});
it("adds the completed ChatGPT account to the Codex provider before saving", async () => {
  vi.mocked(routing.createCodex).mockResolvedValue({
    id: "fixture-codex",
    revision: 0,
    source: { kind: "codex" },
    codex: {
      settings: { accounts: [], models: [], mode: "vendor", groupsOnly: false },
    },
  } as never);
  const finish = mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "ChatGPT / Codex" }));
  fireEvent.click(screen.getByRole("button", { name: "Finish fixture login" }));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
  expect(routing.saveCodex).toHaveBeenCalledWith(
    expect.objectContaining({ id: "fixture-codex" }),
    expect.objectContaining({
      accounts: ["fixture-account"],
      mode: "picked",
      models: [
        { model: "fixture-model", reasoningLevels: ["low", "high"] },
        { model: "fixture-other" },
      ],
      groupsOnly: false,
    }),
  );
  expect(routing.codexModels).toHaveBeenCalledExactlyOnceWith(
    "fixture-codex",
    0,
    "fixture-account",
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
  const finish = mount();
  fireEvent.click(
    screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
  fireEvent.click(screen.getByRole("button", { name: "Finish fixture login" }));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
  expect(routing.save).toHaveBeenCalledWith(
    expect.objectContaining({
      id: "existing",
      revision: 4,
      source: {
        ...source,
        catalog: [
          { model: "fixture-model", inputModalities: ["text", "image"] },
          { model: "fixture-other" },
        ],
      },
    }),
  );
});
it.each(["same", "different"] as const)(
  "initializes all models when reconnecting an unconfigured subscription to the %s account",
  async (binding) => {
    vi.mocked(routing.list).mockResolvedValue([
      {
        id: "existing",
        revision: 4,
        source: {
          kind: "subscription",
          provider: "gemini_cli",
          accountId: binding === "same" ? "fixture-account" : "other-account",
          mode: "vendor",
          models: [],
          catalog: [],
          groupsOnly: true,
        },
      },
    ] as never);
    const finish = mount();
    await submitConnection("subscription");
    await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
    expect(routing.save).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "existing",
        revision: 4,
        source: expect.objectContaining({
          accountId: "fixture-account",
          mode: "picked",
          groupsOnly: false,
          models: [
            { model: "fixture-model", inputModalities: ["text", "image"] },
            { model: "fixture-other" },
          ],
        }),
      }),
    );
  },
);
it.each(["failed", "empty"] as const)(
  "does not save or finish Codex onboarding when its model catalog is %s",
  async (state) => {
    vi.mocked(routing.createCodex).mockResolvedValue({
      id: "fixture-codex",
      revision: 0,
      source: { kind: "codex" },
      codex: {
        settings: {
          accounts: [],
          models: [],
          mode: "vendor",
          groupsOnly: false,
        },
      },
    } as never);
    if (state === "failed")
      vi.mocked(routing.codexModels).mockRejectedValueOnce(
        new Error("synthetic-private-detail"),
      );
    else vi.mocked(routing.codexModels).mockResolvedValueOnce([]);
    const finish = mount();
    fireEvent.click(
      screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "ChatGPT / Codex" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Finish fixture login" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "onboarding.accountModelsFailed",
    );
    expect(routing.saveCodex).not.toHaveBeenCalled();
    expect(finish).not.toHaveBeenCalled();
    expect(document.body).not.toHaveTextContent("synthetic-private-detail");
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
    expect(routing.saveCodex).toHaveBeenCalledTimes(1);
  },
);
it("waits for account model discovery before saving or entering the application", async () => {
  let resolveModels!: (models: Array<Record<string, unknown>>) => void;
  vi.mocked(nativeSubscriptionApi.models).mockReturnValueOnce(
    new Promise((resolve) => {
      resolveModels = resolve;
    }),
  );
  const finish = mount();
  await submitConnection("subscription");
  await waitFor(() =>
    expect(nativeSubscriptionApi.models).toHaveBeenCalledExactlyOnceWith(
      "gemini_cli",
      "fixture-account",
    ),
  );
  expect(routing.save).not.toHaveBeenCalled();
  expect(finish).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "common.back" })).toBeDisabled();
  await act(async () => resolveModels([{ model: "fixture-model" }]));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
});
it.each([
  ["Claude", "claude"],
  ["ChatGPT API", "chatgpt_api"],
  ["Gemini CLI", "gemini_cli"],
  ["Antigravity", "antigravity"],
] as const)(
  "selects all discovered %s account models before saving",
  async (name, provider) => {
    const finish = mount();
    fireEvent.click(
      screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
    );
    fireEvent.click(screen.getByRole("button", { name }));
    fireEvent.click(
      screen.getByRole("button", { name: "Finish fixture login" }),
    );
    await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
    expect(nativeSubscriptionApi.models).toHaveBeenCalledExactlyOnceWith(
      provider,
      "fixture-account",
    );
    const source = vi.mocked(routing.save).mock.calls[0][0].source;
    expect(source).toMatchObject({
      provider,
      accountId: "fixture-account",
      mode: "picked",
      models: [
        { model: "fixture-model", inputModalities: ["text", "image"] },
        { model: "fixture-other" },
      ],
      groupsOnly: false,
    });
    expect(source.kind === "subscription" && source.catalog).toEqual(
      source.kind === "subscription" && source.models,
    );
  },
);
it.each(["GitHub Copilot", "Grok"] as const)(
  "selects all %s models with their metadata",
  async (name) => {
    const finish = mount();
    fireEvent.click(
      screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
    );
    fireEvent.click(screen.getByRole("button", { name }));
    fireEvent.click(
      screen.getByRole("button", { name: "Finish fixture login" }),
    );
    await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
    expect(
      name === "Grok" ? fetchXaiOauthModels : copilotGetModelsForAccount,
    ).toHaveBeenCalledExactlyOnceWith("fixture-account");
    expect(routing.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source: expect.objectContaining({
          mode: "picked",
          models: [
            expect.objectContaining(
              name === "Grok"
                ? { model: "fixture-model", reasoning: true }
                : {
                    model: "fixture-model",
                    displayName: "Fixture model",
                    supported_endpoints: ["/responses"],
                  },
            ),
          ],
        }),
      }),
    );
  },
);
it.each(["failed", "empty"] as const)(
  "keeps login retryable when the account model catalog is %s",
  async (state) => {
    const finish = mount();
    if (state === "failed")
      vi.mocked(nativeSubscriptionApi.models).mockRejectedValueOnce(
        new Error("synthetic-private-detail"),
      );
    else vi.mocked(nativeSubscriptionApi.models).mockResolvedValueOnce([]);
    await submitConnection("subscription");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "onboarding.accountModelsFailed",
    );
    expect(document.body).not.toHaveTextContent("synthetic-private-detail");
    expect(routing.save).not.toHaveBeenCalled();
    expect(finish).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
    expect(routing.save).toHaveBeenCalledTimes(1);
  },
);
it.each([
  ["anthropic", "x-api-key"],
  ["gemini_native", "x-goog-api-key"],
])("uses the correct credential header for %s", async (format, header) => {
  const finish = mount();
  enterKey();
  fireEvent.change(screen.getByLabelText("onboarding.protocol"), {
    target: { value: format },
  });
  fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
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
  const finish = mount();
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
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
  expect(providerImportApi.import).toHaveBeenCalledWith([
    {
      source: "cc-switch",
      reference: "new",
      fingerprint: "digest",
      mode: "add",
    },
  ]);
});
async function submitConnection(method: "key" | "subscription" | "import") {
  if (method === "key") {
    enterKey();
    fireEvent.click(screen.getByRole("button", { name: "onboarding.connect" }));
  } else if (method === "subscription") {
    fireEvent.click(
      screen.getByRole("button", { name: "onboarding.subscriptionAction" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Gemini CLI" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Finish fixture login" }),
    );
  } else {
    fireEvent.click(
      screen.getByRole("button", { name: "onboarding.importAction" }),
    );
    await screen.findByRole("checkbox", { name: "Fixture relay" });
    const submit = screen.getByRole("button", { name: "providerFlow.import" });
    fireEvent.click(submit);
    fireEvent.click(submit);
  }
}
it.each(["key", "subscription", "import"] as const)(
  "retries completion after %s without repeating the saved connection",
  async (method) => {
    const finish = mount();
    finish.mockRejectedValueOnce(new Error("synthetic-settings-failure"));
    await submitConnection(method);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "onboarding.finishFailed",
    );
    expect(document.body).not.toHaveTextContent("synthetic-settings-failure");
    expect(
      screen.queryByRole("button", { name: "onboarding.enterTools" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await waitFor(() => expect(finish).toHaveBeenCalledTimes(2));
    expect(finish).toHaveBeenLastCalledWith(true);
    expect(routing.save).toHaveBeenCalledTimes(method === "import" ? 0 : 1);
    expect(providerImportApi.import).toHaveBeenCalledTimes(
      method === "import" ? 1 : 0,
    );
  },
);
it.each(["subscription", "import"] as const)(
  "stays in onboarding if the %s connection cannot be saved",
  async (method) => {
    const finish = mount();
    const cause = new Error("synthetic-connection-failure");
    if (method === "subscription")
      vi.mocked(routing.save).mockRejectedValueOnce(cause);
    else vi.mocked(providerImportApi.import).mockRejectedValueOnce(cause);
    await submitConnection(method);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      method === "subscription"
        ? "onboarding.accountFailed"
        : "providerImport.failed",
    );
    expect(finish).not.toHaveBeenCalled();
    expect(document.body).not.toHaveTextContent("synthetic-connection-failure");
  },
);
it("waits for provider persistence before automatically finishing", async () => {
  let resolveSave!: (value: never) => void;
  vi.mocked(routing.save).mockReturnValueOnce(
    new Promise((resolve) => {
      resolveSave = resolve;
    }),
  );
  const finish = mount();
  await submitConnection("key");
  await waitFor(() => expect(routing.save).toHaveBeenCalledTimes(1));
  expect(finish).not.toHaveBeenCalled();
  await act(async () => resolveSave({ id: "saved", revision: 1 } as never));
  await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
});
it.each(["key", "subscription", "import"] as const)(
  "locks navigation while %s completion is persisted",
  async (method) => {
    let resolveFinish!: () => void;
    const finish = mount();
    finish.mockReturnValueOnce(
      new Promise<void>((resolve) => {
        resolveFinish = resolve;
      }),
    );
    await submitConnection(method);
    await waitFor(() => expect(finish).toHaveBeenCalledExactlyOnceWith(true));
    expect(
      screen.getByRole("button", { name: "onboarding.skip" }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "onboarding.enterTools" }),
    ).toBeNull();
    await act(async () => resolveFinish());
  },
);
it.each(["key", "subscription", "import"] as const)(
  "closes the real first-run guide and enters the application after %s succeeds",
  async (method) => {
    setSettings({ firstRunNoticeConfirmed: false });
    const onConnected = vi.fn();
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <FirstRunNoticeDialog onConnected={onConnected} />
      </QueryClientProvider>,
    );
    await screen.findByRole("button", { name: "onboarding.keyAction" });
    await submitConnection(method);
    await waitFor(() => {
      expect(getSettings().firstRunNoticeConfirmed).toBe(true);
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(onConnected).toHaveBeenCalledExactlyOnceWith();
    });
  },
);
