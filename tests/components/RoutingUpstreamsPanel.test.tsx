import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";
import {
  finishDialogAnimation,
  mockDialogAnimationStyles,
} from "../utils/dialogAnimation";
vi.mock("@/lib/api/model-fetch", () => ({ fetchModelsForConfig: vi.fn() }));
vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn(),
}));

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    fetchIcon: vi.fn(),
    list: vi.fn(),
    discover: vi.fn(),
    save: vi.fn(),
    remove: vi.fn(),
    probe: vi.fn(),
    editor: vi.fn(),
  },
}));
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
  vi.mocked(routingUpstreamsApi.save).mockImplementation(async (value) => ({
    ...value,
    revision: 1,
    hasApiKey: true,
  }));
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
describe("global upstream editor", () => {
  const primary = {
    schemaVersion: 1 as const,
    id: "primary",
    name: "Synthetic primary",
    revision: 4,
    hasApiKey: true,
    source: {
      kind: "api" as const,
      catalog: [],
      mode: "picked" as const,
      groupsOnly: false,
      apiFormat: "openai_responses" as const,
      baseUrl: "https://primary.example/v1",
      models: [
        { model: "large-model", contextWindow: 1000000, contextSource: "user" },
        {
          model: "small-model",
          contextWindow: 128000,
          contextSource: "vendor",
        },
      ],
    },
  };
  it.each([
    "ChatGPT Prolite",
    "ChatGPT Promax",
    "ChatGPT Go",
    "ChatGPT Team",
    "ChatGPT Edu",
  ])("shows the %s plan on the provider row", async (plan) => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        id: "codex",
        name: "Codex fixture",
        hasApiKey: false,
        source: { kind: "codex" },
        subscriptionPlans: [plan],
        reauthRequired: false,
      },
    ] as never);
    mount();
    const row = await screen.findByRole("article", { name: "Codex fixture" });
    expect(within(row).getByText(plan)).toBeVisible();
    expect(row.querySelector(".pv-key")).not.toHaveClass("imported");
  });
  it("shows ChatGPT for a valid sign-in with no known plan without claiming Plus", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        id: "codex",
        name: "Codex fixture",
        hasApiKey: false,
        source: { kind: "codex" },
        subscriptionPlans: [null],
        reauthRequired: false,
      },
    ] as never);
    mount();
    const row = await screen.findByRole("article", { name: "Codex fixture" });
    expect(within(row).getByText("ChatGPT")).toBeVisible();
    expect(row.querySelector(".pv-key")).not.toHaveClass("imported");
    expect(within(row).queryByText("ChatGPT Plus")).toBeNull();
  });
  it("supports batch deleting providers from the manage action next to search", async () => {
    const secondary = {
      ...primary,
      id: "secondary",
      name: "Synthetic secondary",
      revision: 7,
    };
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary, secondary]);
    vi.mocked(routingUpstreamsApi.remove).mockResolvedValue(undefined);
    mount();
    await screen.findByRole("article", { name: "Synthetic primary" });
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.manage" }),
    );
    expect(
      screen.getByRole("button", { name: "common.cancel" }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "modelProviders.selectAll" }),
    );
    expect(
      screen.getByRole("checkbox", { name: "Synthetic primary" }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Synthetic secondary" }),
    ).toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.removeSelected" }),
    );
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "modelProviders.removeSelected" }),
      ).getByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() => {
      expect(routingUpstreamsApi.remove).toHaveBeenCalledWith("primary", 4);
      expect(routingUpstreamsApi.remove).toHaveBeenCalledWith("secondary", 7);
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "common.cancel" }),
      ).not.toBeInTheDocument(),
    );
  });
  it("shows the selected subscription plan in the provider row without opening the editor", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        id: "codex-plus",
        name: "Synthetic subscription",
        source: { kind: "codex" },
        hasApiKey: false,
        subscriptionPlans: ["ChatGPT Plus"],
      },
    ] as never);
    mount();
    const row = await screen.findByRole("article", {
      name: "Synthetic subscription",
    });
    expect(within(row).getByText("ChatGPT Plus")).toBeVisible();
    expect(row.querySelector(".pv-key")).not.toHaveClass("imported");
    expect(within(row).queryByText("providerParity.subscription")).toBeNull();
    expect(routingUpstreamsApi.editor).not.toHaveBeenCalled();
    expect(routingUpstreamsApi.probe).not.toHaveBeenCalled();
  });
  it("shows distinct plans on disabled subscriptions and keeps unknown plans explicit", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        id: "codex-mixed",
        name: "Synthetic multiple plans",
        source: { kind: "codex" },
        hasApiKey: false,
        enabled: false,
        subscriptionPlans: ["ChatGPT Plus", "ChatGPT Plus", "ChatGPT Pro"],
      },
      {
        ...primary,
        id: "codex-unknown",
        name: "Synthetic unknown plan",
        source: { kind: "codex" },
        hasApiKey: false,
        subscriptionPlans: [null],
      },
    ] as never);
    mount();
    const unknown = await screen.findByRole("article", {
      name: "Synthetic unknown plan",
    });
    expect(
      within(unknown).getByText("providerParity.subscription"),
    ).toBeVisible();
    expect(unknown.querySelector(".pv-key")).toHaveClass("imported");
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.turnedOff" }),
    );
    const mixed = await screen.findByRole("article", {
      name: "Synthetic multiple plans",
    });
    expect(mixed.querySelector(".pv-key")).toHaveTextContent(
      "ChatGPT Plus · ChatGPT Pro",
    );
    expect(mixed.querySelector(".pv-key")).toHaveAttribute(
      "title",
      "ChatGPT Plus · ChatGPT Pro",
    );
  });
  it("lets a custom provider choose an icon without hiding its name field and saves the choice", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.builtinIcons" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      primary.name,
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ icon: "deepseek" }),
      ),
    );
  });
  it("shows an imported provider icon instead of the server fallback", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      { ...primary, icon: "deepseek-color" },
    ]);
    mount();
    const trigger = await screen.findByRole("button", {
      name: "common.edit Synthetic primary",
    });
    expect(trigger.querySelector(".pv-logo svg title")).toHaveTextContent(
      "DeepSeek",
    );
    expect(trigger.querySelector(".lucide-server")).toBeNull();
  });
  it("keeps the custom name visible when choosing an icon before the first save", async () => {
    mount();
    const add = await screen.findByRole("button", {
      name: "modelProviders.add",
    });
    await waitFor(() => expect(add).toBeEnabled());
    fireEvent.click(add);
    fireEvent.click(
      await screen.findByRole("button", { name: "providerFlow.custom" }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Synthetic custom" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.builtinIcons" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      "Synthetic custom",
    );
  });
  it("keeps the provider editor content mounted until its closing animation finishes", async () => {
    mockDialogAnimationStyles();
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    const trigger = await screen.findByRole("button", {
      name: "common.edit Synthetic primary",
    });
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Synthetic primary" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveAttribute("data-state", "closed");
    expect(
      within(dialog).getByRole("textbox", { name: "modelProviders.baseUrl" }),
    ).toHaveValue(primary.source.baseUrl);
    finishDialogAnimation(dialog);
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    await waitFor(() => expect(trigger).toHaveFocus());
  });
  it("opens the framed provider dialog while retaining the list behind it and restores focus on close", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    const dialog = screen.getByRole("dialog", { name: "Synthetic primary" });
    const editor = screen.getByRole("region", { name: "Synthetic primary" });
    expect(dialog).toContainElement(editor);
    expect(editor.closest(".pv-list")).toBeNull();
    expect(dialog.querySelector(".pv-editor-head")).toBeInTheDocument();
    expect(dialog.querySelector(".pv-editor-body")).toBeInTheDocument();
    expect(dialog.querySelector(".pv-editor-foot")).toBeInTheDocument();
    fireEvent.keyDown(editor, { key: "Escape" });
    await waitFor(() =>
      expect(
        screen.queryByRole("region", { name: "Synthetic primary" }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: "common.edit Synthetic primary" }),
    ).toHaveFocus();
  });
  it("focuses the provider form without moving its scroll containers", async () => {
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    const name = screen.getByRole("textbox", { name: "modelProviders.name" });
    await waitFor(() => expect(name).toHaveFocus());
    const call = focus.mock.contexts.findIndex((element) => element === name);
    expect(call).toBeGreaterThanOrEqual(0);
    expect(focus.mock.calls[call]).toEqual([{ preventScroll: true }]);
  });
  it("shows only a newly entered key and runs stored-key probes only after an explicit click", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    vi.mocked(routingUpstreamsApi.probe).mockResolvedValue([
      {
        format: "openai_responses",
        text: { status: "verified" },
        stream: { status: "verified" },
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    expect(routingUpstreamsApi.probe).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.detect" }),
    );
    await waitFor(() =>
      expect(routingUpstreamsApi.probe).toHaveBeenCalledWith(
        "primary",
        4,
        "large-model",
      ),
    );
    expect(
      await screen.findByText(/providerEditor.verified/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.editKey" }),
    );
    const input = screen.getByLabelText("modelProviders.apiKey");
    fireEvent.change(input, { target: { value: "synthetic-replacement-key" } });
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.showKey" }),
    );
    expect(input).toHaveAttribute("type", "text");
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.hideKey" }),
    );
    expect(input).toHaveAttribute("type", "password");
  });
  it("shows the saved primary key and imported key pool without requesting credentials", async () => {
    const transport = {
      endpoints: { openai_responses: primary.source.baseUrl },
      fullUrl: false,
      keys: [
        { id: "backup", name: "Synthetic backup", enabled: true },
        { id: "paused", name: "", enabled: false },
      ],
    };
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      { ...primary, transport },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    const keys = screen.getByRole("group", {
      name: "providerEditor.savedKeys",
    });
    const rows = within(keys).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(
      within(rows[0]).getByText("providerEditor.primaryKey"),
    ).toBeVisible();
    expect(within(rows[1]).getByText("Synthetic backup")).toBeVisible();
    expect(within(rows[1]).getByRole("checkbox")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(
      within(rows[2]).getByText("providerEditor.additionalKey"),
    ).toBeVisible();
    expect(within(rows[2]).getByRole("checkbox")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(
      screen.queryByLabelText("modelProviders.apiKey"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "providerEditor.showKey" }),
    ).not.toBeInTheDocument();
    expect(routingUpstreamsApi.discover).not.toHaveBeenCalled();
    expect(routingUpstreamsApi.probe).not.toHaveBeenCalled();
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ source: primary.source, transport }),
      ),
    );
  });
  it("distinguishes a saved single key from a provider that still needs a key", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      primary,
      {
        ...primary,
        id: "missing",
        name: "Synthetic missing",
        hasApiKey: false,
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    expect(
      within(
        screen.getByRole("group", { name: "providerEditor.savedKeys" }),
      ).getAllByRole("listitem"),
    ).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "common.edit Synthetic missing" }),
    );
    expect(screen.getByLabelText("modelProviders.apiKey")).toBeRequired();
  });
  it("stages model names, reasoning levels and image overrides until the provider is saved", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        source: {
          ...primary.source,
          models: [
            {
              model: "large-model",
              displayName: "Vendor name",
              reasoningLevels: ["low", "high"],
              inputModalities: ["text", "image"],
              contextWindow: 1000000,
            },
          ],
        },
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.namesAndLevels" }),
    );
    const preferences = screen.getByRole("region", {
      name: "modelProviders.namesAndLevels",
    });
    fireEvent.change(
      within(preferences).getByRole("textbox", {
        name: "modelProviders.modelName large-model",
      }),
      { target: { value: "My model" } },
    );
    fireEvent.click(within(preferences).getByRole("checkbox", { name: "low" }));
    fireEvent.change(
      within(preferences).getByRole("textbox", {
        name: "modelProviders.sameAs large-model",
      }),
      { target: { value: "vendor/canonical-model" } },
    );
    fireEvent.click(
      within(preferences).getByRole("checkbox", {
        name: "modelProviders.acceptsImages",
      }),
    );
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({
            models: [
              {
                model: "large-model",
                displayName: "Vendor name",
                reasoningLevels: ["low", "high"],
                inputModalities: ["text", "image"],
                contextWindow: 1000000,
                modelPreferences: {
                  sameAs: "vendor/canonical-model",
                  name: "My model",
                  reasoningLevels: ["high"],
                  images: false,
                },
              },
            ],
          }),
        }),
      ),
    );
  });
  it("restores a model's vendor defaults without losing its exact ID or context metadata", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        source: {
          ...primary.source,
          models: [
            {
              model: "large-model",
              displayName: "Vendor name",
              reasoningLevels: ["low", "high"],
              inputModalities: ["text", "image"],
              contextWindow: 1000000,
              modelPreferences: {
                name: "Custom",
                reasoningLevels: [],
                images: false,
              },
            },
          ],
        },
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.namesAndLevels" }),
    );
    expect(screen.getByText("modelProviders.noReasoning")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "modelProviders.restoreModelDefaults large-model",
      }),
    );
    expect(
      screen.getByRole("textbox", {
        name: "modelProviders.modelName large-model",
      }),
    ).toHaveValue("");
    expect(screen.getByRole("checkbox", { name: "low" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "modelProviders.acceptsImages" }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({
            models: [
              {
                model: "large-model",
                displayName: "Vendor name",
                reasoningLevels: ["low", "high"],
                inputModalities: ["text", "image"],
                contextWindow: 1000000,
              },
            ],
          }),
        }),
      ),
    );
  });
  it("returns focus when folding model preferences and drops them on confirmed cancellation", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    const toggle = screen.getByRole("button", {
      name: "modelProviders.namesAndLevels",
    });
    fireEvent.click(toggle);
    fireEvent.change(
      screen.getByRole("textbox", {
        name: "modelProviders.modelName large-model",
      }),
      { target: { value: "Unsaved" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.foldNames" }),
    );
    expect(toggle).toHaveFocus();
    expect(
      screen.queryByRole("region", { name: "modelProviders.namesAndLevels" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    fireEvent.click(
      screen.getByRole("button", { name: "providerFlow.discard" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "modelProviders.namesAndLevels" }),
    );
    expect(
      screen.getByRole("textbox", {
        name: "modelProviders.modelName large-model",
      }),
    ).toHaveValue("");
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  });
  it("keeps the draft in its modal while background filtering and folding are locked", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      primary,
      { ...primary, id: "off", name: "Synthetic off", enabled: false },
    ]);
    mount();
    const folding = await screen.findByRole("button", {
      name: "modelProviders.turnedOff",
    });
    fireEvent.click(folding);
    fireEvent.click(
      screen.getByRole("button", { name: "common.edit Synthetic off" }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved name" },
    });
    expect(
      screen.getByRole("searchbox", {
        name: "modelProviders.find",
        hidden: true,
      }),
    ).toBeDisabled();
    expect(folding).toBeDisabled();
    expect(document.querySelectorAll(".pv-drag-handle:disabled")).toHaveLength(
      2,
    );
    expect(screen.getByLabelText("modelProviders.name")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      "Unsaved name",
    );
  });
  it("duplicates into a new draft and copies the saved credential privately only on save", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.duplicate" }),
    );
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Synthetic copy" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Synthetic copy",
          revision: 0,
          source: primary.source,
        }),
        { id: primary.id, revision: primary.revision },
      ),
    );
    expect(vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].id).not.toBe(
      primary.id,
    );
  });
  it("protects unsaved changes before duplicating and keeps them when the confirmation is cancelled", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.duplicate" }),
    );
    const confirmation = screen.getByRole("dialog", {
      name: "providerFlow.discardTitle",
    });
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue("Unsaved");
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  });
  it("does not probe a changed endpoint with a stored key and never displays raw failures", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.baseUrl"), {
      target: { value: "https://changed.example/v1" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.detect" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "modelProviders.failed",
    );
    expect(routingUpstreamsApi.probe).not.toHaveBeenCalled();
    expect(detectProviderProtocols).not.toHaveBeenCalled();
  });
  it("can choose a probe model from discovery without changing the user's model selection", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      { ...primary, source: { ...primary.source, models: [] } },
    ]);
    vi.mocked(routingUpstreamsApi.discover).mockResolvedValue([
      { id: "probe-only-model", ownedBy: null },
    ]);
    vi.mocked(routingUpstreamsApi.probe).mockResolvedValue([]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.detect" }),
    );
    await waitFor(() =>
      expect(routingUpstreamsApi.probe).toHaveBeenCalledWith(
        "primary",
        4,
        "probe-only-model",
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({ models: [] }),
        }),
      ),
    );
  });
  it("retains discovered reasoning and image capabilities when selecting a model", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      { ...primary, source: { ...primary.source, models: [] } },
    ]);
    vi.mocked(routingUpstreamsApi.discover).mockResolvedValue([
      {
        id: "reasoning-model",
        ownedBy: null,
        reasoningLevels: ["low", "high"],
        reasoning: true,
        inputModalities: ["text", "image"],
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "reasoning-model" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({
            models: [
              expect.objectContaining({
                model: "reasoning-model",
                reasoningLevels: ["low", "high"],
                reasoning: true,
                inputModalities: ["text", "image"],
              }),
            ],
          }),
        }),
      ),
    );
  });
  it("uses an explicitly entered key to test a draft and keeps the form locked during the test", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    let finish!: (
      result: Awaited<ReturnType<typeof detectProviderProtocols>>,
    ) => void;
    vi.mocked(detectProviderProtocols).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.editKey" }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
      target: { value: "synthetic-probe-key" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.detect" }),
    );
    expect(detectProviderProtocols).toHaveBeenCalledWith(
      expect.objectContaining({
        apiKey: (
          screen.getByLabelText("modelProviders.apiKey") as HTMLInputElement
        ).value,
        model: "large-model",
      }),
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    finish([]);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled(),
    );
  });
  it("recovers a saved preset's regional controls when editing it again", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        presetId: "moonshot",
        region: "china",
        name: "My Kimi",
        source: {
          ...primary.source,
          apiFormat: "openai_chat",
          baseUrl: "https://api.moonshot.cn/v1",
        },
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.edit My Kimi" }),
    );
    expect(
      screen.getByRole("radio", { name: "providerFlow.china" }),
    ).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Anthropic" }));
    expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
      "https://api.moonshot.cn/anthropic",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          presetId: "moonshot",
          region: "china",
          name: "My Kimi",
        }),
      ),
    );
  });
  it("recovers a saved Coding Plan without falling back to its pay-as-you-go endpoint", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        presetId: "zai",
        region: "coding",
        name: "My plan",
        source: {
          ...primary.source,
          apiFormat: "openai_responses",
          baseUrl: "https://api.z.ai/api/v1",
        },
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.edit My plan" }),
    );
    expect(screen.getByRole("radio", { name: "Coding Plan" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Chat Completions" }));
    expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
      "https://api.z.ai/api/coding/paas/v4",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ region: "coding", name: "My plan" }),
      ),
    );
  });
  it("searches provider hosts and models, revealing matching switched-off providers without changing the fold preference", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      primary,
      {
        ...primary,
        id: "secondary",
        name: "Synthetic secondary",
        enabled: false,
        source: {
          ...primary.source,
          baseUrl: "https://secondary.example/v1",
          models: [{ model: "research-model" }],
        },
      },
    ]);
    mount();
    const search = await screen.findByRole("searchbox", {
      name: "modelProviders.find",
    });
    expect(
      screen.getByRole("button", { name: "modelProviders.turnedOff" }),
    ).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.queryByRole("button", { name: "common.edit Synthetic secondary" }),
    ).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "research-model" } });
    expect(
      screen.getByRole("button", { name: "common.edit Synthetic secondary" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "common.edit Synthetic primary" }),
    ).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "" } });
    expect(
      screen.queryByRole("button", { name: "common.edit Synthetic secondary" }),
    ).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "primary.example" } });
    expect(
      screen.getByRole("button", { name: "common.edit Synthetic primary" }),
    ).toBeInTheDocument();
    fireEvent.change(search, { target: { value: "missing-model" } });
    expect(screen.getByRole("status")).toHaveTextContent(
      "modelProviders.noMatches",
    );
  });
  it("switches a provider off while retaining its revision, models and saved credential", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("switch", {
        name: "modelPicker.enableProvider Synthetic primary",
      }),
    );
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith({
        ...primary,
        enabled: false,
      }),
    );
    expect(
      vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
    ).not.toHaveProperty("apiKey");
  });
  it("applies the existing provider's header switch immediately without saving other draft fields", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    vi.mocked(routingUpstreamsApi.save).mockImplementationOnce(
      async (value) => ({ ...value, revision: 5 }),
    );
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved" },
    });
    fireEvent.click(
      screen.getByRole("switch", { name: "providerEditor.enabled" }),
    );
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledExactlyOnceWith({
        ...primary,
        enabled: false,
      }),
    );
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue("Unsaved");
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenLastCalledWith(
        expect.objectContaining({
          name: "Unsaved",
          revision: 5,
          enabled: false,
        }),
      ),
    );
  });
  it("removes a selected model chip without losing the other model's metadata", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(
      screen.getAllByRole("button", {
        name: "large-model",
      })[0],
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({
            models: [primary.source.models[1]],
          }),
        }),
      ),
    );
  });
  it("keeps the editor and its draft on delete cancellation or failure, closing it only after a successful removal", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    vi.mocked(routingUpstreamsApi.remove)
      .mockRejectedValueOnce("routingUpstreams.referenced")
      .mockResolvedValueOnce();
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved name" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    const confirmation = await screen.findByRole("dialog", {
      name: "modelProviders.delete",
    });
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      "Unsaved name",
    );
    expect(routingUpstreamsApi.remove).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "modelProviders.delete" }),
      ).getByRole("button", { name: "common.confirm" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "modelProviders.referenced",
    );
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      "Unsaved name",
    );
    expect(routingUpstreamsApi.remove).toHaveBeenCalledWith("primary", 4);
    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "modelProviders.delete" }),
      ).getByRole("button", { name: "common.confirm" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });
  it("shows only the endpoint host in the list", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        source: {
          ...primary.source,
          baseUrl:
            "https://synthetic-user:synthetic-pass@primary.example/private-path?token=synthetic-token",
        },
      },
    ]);
    mount();
    expect(await screen.findByText(/primary\.example/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "codexOauth.reauthLogin Synthetic primary",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        /synthetic-user|synthetic-pass|private-path|synthetic-token/,
      ),
    ).not.toBeInTheDocument();
  });
  it("never displays raw backend errors", async () => {
    vi.mocked(routingUpstreamsApi.list).mockRejectedValue(
      new Error("private-config-synthetic"),
    );
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "modelProviders.failed",
    );
    expect(
      screen.queryByText(/private-config-synthetic/),
    ).not.toBeInTheDocument();
  });
});

it("shows logged out instead of a cached subscription plan when selected credentials need re-login", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "codex",
      revision: 1,
      name: "Expired fixture",
      hasApiKey: false,
      source: { kind: "codex" },
      subscriptionPlans: ["ChatGPT Plus"],
      reauthRequired: true,
    },
  ] as never);
  mount();
  const row = await screen.findByRole("article", { name: "Expired fixture" });
  expect(within(row).getByText("codexProvider.loggedOut")).toBeVisible();
  expect(
    within(row).getByRole("button", {
      name: "codexOauth.reauthLogin Expired fixture",
    }),
  ).toBeVisible();
  expect(row.querySelector(".pv-key")).toHaveClass("missing");
  expect(routingUpstreamsApi.editor).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.probe).not.toHaveBeenCalled();
});

it.each([
  "claude",
  "chatgpt_api",
  "github_copilot",
  "gemini_cli",
  "antigravity",
  "xai_oauth",
] as const)(
  "prioritizes known signed-out status for %s subscriptions",
  async (provider) => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        schemaVersion: 1,
        id: "subscription",
        revision: 1,
        name: "Subscription fixture",
        hasApiKey: false,
        source: {
          kind: "subscription",
          provider,
          accountId: "fixture",
          models: [],
          catalog: [],
          mode: "vendor",
          groupsOnly: false,
        },
        subscriptionPlans: ["Synthetic plan"],
        reauthRequired: true,
      },
    ] as never);
    mount();
    const row = await screen.findByRole("article", {
      name: "Subscription fixture",
    });
    expect(within(row).getByText("codexProvider.loggedOut")).toBeVisible();
    expect(
      within(row).getByRole("button", {
        name: "codexOauth.reauthLogin Subscription fixture",
      }),
    ).toBeVisible();
    expect(row.querySelector(".pv-key")).toHaveClass("missing");
  },
);
