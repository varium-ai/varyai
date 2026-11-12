import {
  act,
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
import i18n from "i18next";
import { copyText } from "@/lib/clipboard";
import { toast } from "sonner";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), dismiss: vi.fn() } }));

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));
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
    codexQuota: vi.fn(),
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
  vi.mocked(copyText).mockResolvedValue(undefined);
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
describe("provider setup-help shortcut", () => {
  it("copies the onboarding prompt from the header before the import action", async () => {
    mount();
    const button = screen.getByRole("button", {
      name: "onboarding.copyForAgent",
    });
    const actions = within(button.parentElement!);
    expect(actions.getAllByRole("button")).toEqual([
      button,
      actions.getByRole("button", { name: "routingUpstreams.import" }),
      actions.getByRole("button", { name: "modelProviders.add" }),
    ]);
    await act(async () => fireEvent.click(button));
    expect(copyText).toHaveBeenCalledWith("onboarding.helpPrompt");
    expect(button).toHaveAccessibleName("common.copied");
    expect(button).toBeEnabled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  });

  it("shows a safe copy error and clears it after a successful retry", async () => {
    vi.mocked(copyText).mockRejectedValueOnce(
      new Error("Synthetic private detail"),
    );
    mount();
    const button = screen.getByRole("button", {
      name: "onboarding.copyForAgent",
    });
    await act(async () => fireEvent.click(button));
    expect(toast.error).toHaveBeenCalledWith(
      "onboarding.copyFailed",
      expect.any(Object),
    );
    expect(
      screen.queryByText("Synthetic private detail"),
    ).not.toBeInTheDocument();
    expect(button).toBeEnabled();
    await act(async () => fireEvent.click(button));
    expect(button).toHaveAccessibleName("common.copied");
    expect(screen.queryByText("onboarding.copyFailed")).not.toBeInTheDocument();
  });

  it("returns to the Agent label two seconds after the latest copy", async () => {
    vi.useFakeTimers();
    try {
      const view = mount();
      const button = screen.getByRole("button", {
        name: "onboarding.copyForAgent",
      });
      await act(async () => fireEvent.click(button));
      act(() => vi.advanceTimersByTime(1500));
      await act(async () => fireEvent.click(button));
      act(() => vi.advanceTimersByTime(1999));
      expect(button).toHaveAccessibleName("common.copied");
      act(() => vi.advanceTimersByTime(1));
      expect(button).toHaveAccessibleName("onboarding.copyForAgent");
      expect(copyText).toHaveBeenCalledTimes(2);
      view.unmount();
    } finally {
      vi.useRealTimers();
    }
  });
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
  it("edits the provider ID like Magpie while preserving the saved credential identity", async () => {
    const saved = { ...primary, notes: "Synthetic original note" };
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([saved]);
    vi.mocked(routingUpstreamsApi.editor).mockResolvedValue(saved);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    const id = await screen.findByLabelText("ID");
    expect(id).toHaveValue(saved.id);
    expect(id).not.toHaveAttribute("readonly");
    expect(screen.queryByLabelText("provider.notes")).not.toBeInTheDocument();
    fireEvent.change(id, { target: { value: "My New ID!" } });
    fireEvent.blur(id);
    expect(screen.getByLabelText("ID")).toHaveValue("my-new-id");
    expect(screen.getByLabelText("ID")).toBe(id);
    vi.mocked(routingUpstreamsApi.probe).mockResolvedValue([]);
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.detect" }),
    );
    await waitFor(() =>
      expect(routingUpstreamsApi.probe).toHaveBeenCalledWith(
        saved.id,
        saved.revision,
        "large-model",
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "my-new-id",
          notes: "Synthetic original note",
        }),
        undefined,
        saved.id,
      ),
    );
  });
  it("saves selected catalog models without exposing removed manual-entry controls", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    expect(screen.queryByLabelText("providerEditor.addModelId")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "providerEditor.addModel" }),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.selectNone" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "small-model" }));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({
            mode: "picked",
            models: [primary.source.models[1]],
          }),
        }),
      ),
    );
  });
  it("shows subscription allowance beneath the provider name without opening the form", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      {
        ...primary,
        id: "codex",
        name: "Codex fixture",
        hasApiKey: false,
        source: { kind: "codex" },
        subscriptionPlans: ["ChatGPT Prolite"],
        reauthRequired: false,
        codexSettings: {
          accounts: ["selected"],
          mode: "vendor",
          models: [],
          groupsOnly: false,
        },
      },
    ]);
    vi.mocked(routingUpstreamsApi.editor).mockResolvedValue({
      ...primary,
      source: { kind: "codex" },
      codex: {
        settings: {
          accounts: ["selected"],
          mode: "vendor",
          models: [],
          groupsOnly: false,
        },
        accounts: [
          { id: "selected", login: "Fixture", native: false, ready: true },
        ],
        models: [],
      },
    });
    vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValue({
      credentialStatus: "valid",
      success: true,
      tiers: [{ name: "five_hour", utilization: 27, resetsAt: null }],
    } as never);
    mount();
    const row = await screen.findByRole("article", { name: "Codex fixture" });
    expect(
      await within(row).findByText("codexProvider.remaining"),
    ).toBeVisible();
    expect(within(row).getByText("ChatGPT Prolite")).toBeVisible();
    expect(within(row).queryByText("ChatGPT / Codex")).not.toBeInTheDocument();
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith("selected");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
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
  it.each([false, true])(
    "keeps batch deletion local and the count stable until completion (failure: %s)",
    async (failSecond) => {
      const translations = structuredClone(
        i18n.getResourceBundle("zh", "translation"),
      );
      i18n.addResourceBundle(
        "zh",
        "translation",
        {
          modelProviders: {
            removeSelected: "Delete selected ({{count}})",
            removeSelectedHint: "Delete {{count}} providers?",
          },
        },
        true,
        true,
      );
      try {
        const secondary = {
          ...primary,
          id: "secondary",
          name: "Synthetic secondary",
          revision: 7,
        };
        let remaining = [primary, secondary];
        vi.mocked(routingUpstreamsApi.list).mockImplementation(
          async () => remaining,
        );
        let finishFirst!: () => void;
        let finishSecond!: () => void;
        vi.mocked(routingUpstreamsApi.remove)
          .mockImplementationOnce(
            () =>
              new Promise<void>((resolve) => {
                finishFirst = () => {
                  remaining = [secondary];
                  resolve();
                };
              }),
          )
          .mockImplementationOnce(
            () =>
              new Promise<void>((resolve, reject) => {
                finishSecond = () => {
                  if (failSecond) reject("routingUpstreams.referenced");
                  else {
                    remaining = [];
                    resolve();
                  }
                };
              }),
          );
        const { container } = mount();
        fireEvent.click(
          await screen.findByRole("button", { name: "modelProviders.manage" }),
        );
        fireEvent.click(
          screen.getByRole("checkbox", { name: "modelProviders.selectAll" }),
        );
        fireEvent.click(
          screen.getByRole("button", { name: "Delete selected (2)" }),
        );
        const dialog = screen.getByRole("dialog", {
          name: "Delete selected (2)",
        });
        const confirm = within(dialog).getByRole("button", {
          name: "common.confirm",
        });
        fireEvent.click(confirm);
        expect(confirm).toHaveAttribute("aria-busy", "true");
        expect(confirm).toBeDisabled();
        expect(container.querySelector(".pv-query")).toBeEnabled();
        expect(container.querySelector(".pv-add-trigger")).toBeEnabled();
        fireEvent.click(confirm);
        expect(routingUpstreamsApi.remove).toHaveBeenCalledTimes(1);
        finishFirst();
        await waitFor(() =>
          expect(routingUpstreamsApi.remove).toHaveBeenCalledTimes(2),
        );
        expect(dialog).toHaveAccessibleName("Delete selected (2)");
        expect(dialog).toHaveAccessibleDescription("Delete 2 providers?");
        expect(dialog).toBeVisible();
        expect(
          container.querySelector('.pv-row input[type="checkbox"]'),
        ).toBeChecked();
        if (!failSecond) {
          vi.mocked(routingUpstreamsApi.list).mockImplementation(
            () => new Promise(() => {}),
          );
        }
        finishSecond();
        await waitFor(() =>
          expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
        );
        if (failSecond) {
          expect(toast.error).toHaveBeenCalledWith(
            "modelProviders.errors.remove",
            expect.objectContaining({
              description: "modelProviders.referenced",
            }),
          );
          expect(
            screen.getByRole("checkbox", { name: "Synthetic secondary" }),
          ).toBeChecked();
          expect(
            screen.getByRole("button", { name: "Delete selected (1)" }),
          ).toBeEnabled();
          expect(
            screen.queryByRole("article", { name: "Synthetic primary" }),
          ).not.toBeInTheDocument();
        } else {
          expect(screen.queryByRole("article")).not.toBeInTheDocument();
          const add = screen.getByRole("button", {
            name: "modelProviders.add",
          });
          expect(add).toBeEnabled();
          fireEvent.click(add);
          expect(
            await screen.findByRole("button", { name: "providerFlow.custom" }),
          ).toBeVisible();
        }
      } finally {
        i18n.removeResourceBundle("zh", "translation");
        i18n.addResourceBundle("zh", "translation", translations);
      }
    },
  );
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
  it.each(["", "synthetic\u0001invalid", "x".repeat(16385)])(
    "rejects invalid staged keys with safe feedback",
    async (key) => {
      const saved = {
        ...primary,
        transport: {
          endpoints: {},
          fullUrl: false,
          keys: [{ id: "new-key", name: "", enabled: true, key }],
        },
      };
      vi.mocked(routingUpstreamsApi.list).mockResolvedValue([saved]);
      vi.mocked(routingUpstreamsApi.editor).mockResolvedValue(saved);
      mount();
      fireEvent.click(
        await screen.findByRole("button", {
          name: "common.edit Synthetic primary",
        }),
      );
      await screen.findByRole("button", { name: "common.save" });
      fireEvent.submit(screen.getByRole("dialog").querySelector("form")!);
      await waitFor(() =>
        expect(toast.error).toHaveBeenCalledWith(
          "modelProviders.errors.save",
          expect.objectContaining({
            description: "modelProviders.errors.invalid",
          }),
        ),
      );
      expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    },
  );
  it.each(["api", "subscription"] as const)(
    "explains model matches in %s vendor catalogs and display names",
    async (kind) => {
      const vendor = {
        ...primary,
        source:
          kind === "api"
            ? {
                ...primary.source,
                mode: "vendor" as const,
                models: [],
                catalog: [
                  { model: "vision-3", displayName: "Friendly vision" },
                ],
              }
            : {
                kind: "subscription" as const,
                provider: "gemini_cli" as const,
                accountId: "synthetic-account",
                mode: "vendor" as const,
                models: [],
                catalog: [
                  { model: "vision-3", displayName: "Friendly vision" },
                ],
                groupsOnly: false,
              },
      };
      const other = {
        ...primary,
        id: "other",
        name: "Another supplier",
        source: { ...primary.source, models: [{ model: "fast-model" }] },
      };
      vi.mocked(routingUpstreamsApi.list).mockResolvedValue([vendor, other]);
      mount();
      const search = await screen.findByRole("searchbox", {
        name: "modelProviders.find",
      });
      expect(search).toHaveAccessibleDescription("modelProviders.searchScope");
      fireEvent.change(search, { target: { value: "3" } });
      expect(
        screen.getByRole("article", { name: primary.name }),
      ).toHaveTextContent("vision-3");
      expect(
        screen.queryByRole("article", { name: other.name }),
      ).not.toBeInTheDocument();
      fireEvent.change(search, { target: { value: "Friendly vision" } });
      expect(
        screen.getByRole("article", { name: primary.name }),
      ).toHaveTextContent("modelProviders.matchedModels");
    },
  );
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
  it("preserves stored model names, reasoning levels and image overrides in the simplified form", async () => {
    const model = {
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
    };
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      { ...primary, source: { ...primary.source, models: [model] } },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    expect(
      screen.queryByRole("button", { name: "modelProviders.namesAndLevels" }),
    ).toBeNull();
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({ models: [model] }),
        }),
      ),
    );
  });
  it("keeps existing empty reasoning overrides and context metadata when saving", async () => {
    const model = {
      model: "large-model",
      displayName: "Vendor name",
      reasoningLevels: ["low", "high"],
      inputModalities: ["text", "image"],
      contextWindow: 1000000,
      modelPreferences: { name: "Custom", reasoningLevels: [], images: false },
    };
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
      { ...primary, source: { ...primary.source, models: [model] } },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    expect(
      screen.queryByRole("button", {
        name: "modelProviders.restoreModelDefaults large-model",
      }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          source: expect.objectContaining({ models: [model] }),
        }),
      ),
    );
  });
  it("discards cancelled model selection edits without changing the saved models", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "large-model" }));
    expect(screen.getByRole("button", { name: "large-model" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "providerFlow.discardTitle" }),
      ).getByRole("button", { name: "providerFlow.discard" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    expect(screen.getByRole("button", { name: "large-model" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "small-model" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.queryByRole("button", { name: "modelProviders.namesAndLevels" }),
    ).toBeNull();
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
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(
      screen.getByRole("heading", { name: "providerEditor.copyTitle" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "common.add" })).toBeVisible();
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Synthetic copy" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.add" }));
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
  it("duplicates the saved configuration directly like Magpie and allows cancelling the copy", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved changes" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.duplicate" }),
    );
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(
      screen.queryByRole("dialog", { name: "providerFlow.discardTitle" }),
    ).toBeNull();
    expect(
      screen.getByRole("heading", { name: "providerEditor.copyTitle" }),
    ).toBeVisible();
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      i18n.t("providerEditor.copyName", { name: primary.name }),
    );
    expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
      primary.source.baseUrl,
    );
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  });
  it("retains edits to the copy when its cancellation is declined", async () => {
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
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved copy" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    const confirmation = screen.getByRole("dialog", {
      name: "providerFlow.discardTitle",
    });
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      "Unsaved copy",
    );
    expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  });
  it("reports a failed save in a toast and preserves the editable draft", async () => {
    vi.mocked(routingUpstreamsApi.list).mockResolvedValue([primary]);
    vi.mocked(routingUpstreamsApi.save).mockRejectedValueOnce(
      new Error("synthetic-private-credential"),
    );
    mount();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "common.edit Synthetic primary",
      }),
    );
    fireEvent.change(screen.getByLabelText("modelProviders.name"), {
      target: { value: "Unsaved draft" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "modelProviders.errors.save",
        expect.any(Object),
      ),
    );
    expect(screen.getByLabelText("modelProviders.name")).toHaveValue(
      "Unsaved draft",
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    expect(within(screen.getByRole("dialog")).queryByRole("alert")).toBeNull();
    expect(JSON.stringify(vi.mocked(toast.error).mock.calls)).not.toContain(
      "synthetic-private-credential",
    );
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
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "modelProviders.errors.testModels",
        expect.any(Object),
      ),
    );
    expect(screen.queryByRole("alert")).toBeNull();
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
    expect(
      screen.queryByLabelText("modelProviders.baseUrl"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("radio", { name: "Anthropic" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          presetId: "moonshot",
          region: "china",
          name: "My Kimi",
          source: expect.objectContaining({
            apiFormat: "openai_chat",
            baseUrl: "https://api.moonshot.cn/v1",
          }),
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
    expect(
      screen.queryByLabelText("modelProviders.baseUrl"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("radio", { name: "Chat Completions" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          region: "coding",
          name: "My plan",
          source: expect.objectContaining({
            apiFormat: "openai_responses",
            baseUrl: "https://api.z.ai/api/v1",
          }),
        }),
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
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "modelProviders.errors.remove",
        expect.objectContaining({ description: "modelProviders.referenced" }),
      ),
    );
    expect(within(screen.getByRole("dialog")).queryByRole("alert")).toBeNull();
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
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "modelProviders.errors.loadProviders",
        expect.any(Object),
      ),
    );
    expect(screen.queryByRole("alert")).toBeNull();
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
