import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
import { IntentRoutingPanel } from "@/components/routing/IntentRoutingPanel";
import { intentRoutingApi, type RoutingGroup } from "@/lib/api/intentRouting";
vi.mock("@/components/routing/RoutingClient", () => ({
  RoutingClient: () => null,
}));
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    list: vi.fn(),
    providers: vi.fn(),
    discoverCatalogs: vi.fn(),
    remove: vi.fn(),
    save: vi.fn(),
    autoGroupSkip: vi.fn(),
  },
}));
const group: RoutingGroup = {
  id: "fixture",
  name: "Synthetic group",
  revision: 1,
  client: "global",
  enabled: true,
  intentRecognitionEnabled: false,
  classifierTarget: null,
  classificationBudgetMs: 2000,
  members: [
    { id: "first", providerId: "provider", modelId: "first" },
    { id: "removed", providerId: "provider", modelId: "removed" },
  ],
  defaultMemberId: "first",
  fallbackMemberIds: ["removed"],
  rules: [],
  routing: "ordered",
};
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <IntentRoutingPanel />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(intentRoutingApi.list).mockResolvedValue({
    groups: [group],
    autoCreate: false,
    omittedGroups: 0,
  });
  vi.mocked(intentRoutingApi.providers).mockResolvedValue([
    {
      id: "provider",
      name: "Synthetic provider",
      taskEligible: true,
      classifierEligible: false,
      models: [{ id: "first", name: "first" }],
    },
  ]);
  vi.mocked(intentRoutingApi.autoGroupSkip).mockResolvedValue([]);
});
afterEach(() => vi.restoreAllMocks());
it("reopens a legacy capability group with its saved, editable priority order", async () => {
  const legacy: RoutingGroup = {
    ...group,
    affinity: "turn",
    contextWindow: -1,
    firstToken: 0,
    patience: 0,
  };
  const catalog = {
    groups: [legacy],
    autoCreate: false,
    omittedGroups: 0,
  };
  vi.mocked(intentRoutingApi.list).mockResolvedValue(catalog);
  vi.mocked(intentRoutingApi.save).mockImplementationOnce(async (draft) => {
    const saved = { ...draft, revision: legacy.revision + 1 };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      ...catalog,
      groups: [saved],
    });
    return saved;
  });
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
  const rows = () =>
    within(
      screen.getByRole("list", { name: "routingPage.modelOrder" }),
    ).getAllByRole("listitem");
  expect(rows()[0]).toHaveTextContent("first");
  expect(rows()[1]).toHaveTextContent("removed");
  fireEvent.click(
    within(rows()[1]).getByRole("button", {
      name: "intentRouting.moveUp removed",
    }),
  );
  expect(rows()[0]).toHaveTextContent("removed");
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(intentRoutingApi.save).toHaveBeenCalledWith(
      expect.objectContaining({
        members: [legacy.members[1], legacy.members[0]],
        routing: "ordered",
        revision: legacy.revision,
        defaultMemberId: "removed",
        fallbackMemberIds: ["first"],
      }),
    ),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
  expect(rows()[0]).toHaveTextContent("removed");
  expect(rows()[1]).toHaveTextContent("first");
});

it("restores the same recommended order for models with tied ranking metadata", async () => {
  const members = [
    { id: "z", providerId: "provider", modelId: "z-model" },
    { id: "a", providerId: "provider", modelId: "a-model" },
  ];
  vi.mocked(intentRoutingApi.list).mockResolvedValue({
    groups: [
      {
        ...group,
        template: "templateCapability",
        members,
        defaultMemberId: "z",
        fallbackMemberIds: ["a"],
      },
    ],
    autoCreate: false,
    omittedGroups: 0,
  });
  vi.mocked(intentRoutingApi.providers).mockResolvedValue([
    {
      id: "provider",
      name: "Synthetic provider",
      taskEligible: true,
      classifierEligible: false,
      models: [
        { id: "z-model", name: "Z model" },
        { id: "a-model", name: "A model" },
      ],
    },
  ]);
  mount();
  fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
  const rows = () =>
    within(
      screen.getByRole("list", { name: "routingPage.modelOrder" }),
    ).getAllByRole("listitem");
  fireEvent.click(
    within(rows()[1]).getByRole("button", {
      name: "intentRouting.moveUp A model",
    }),
  );
  expect(rows()[0]).toHaveTextContent("A model");
  fireEvent.click(
    screen.getByRole("button", {
      name: "routingPage.applyRecommendedModelOrder",
    }),
  );
  expect(rows()[0]).toHaveTextContent("Z model");
  expect(rows()[1]).toHaveTextContent("A model");
  expect(intentRoutingApi.save).not.toHaveBeenCalled();
});

it.each([false, true])(
  "removes groups without exposing archived controls with automatic discovery=%s",
  async (automatic) => {
    const auto = automatic
      ? { ...group, id: "auto-fixture", revision: 0, autoGenerated: true }
      : group;
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [auto],
      autoCreate: automatic,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.remove).mockImplementation(async () => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [],
        autoCreate: automatic,
        omittedGroups: 0,
        removedGroups: [{ id: auto.id, name: auto.name }],
      });
    });
    mount();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.delete" }),
    );
    const confirm = screen.getByRole("dialog", { name: "common.delete" });
    fireEvent.click(
      within(confirm).getByRole("button", {
        name: "intentRouting.confirmDelete",
      }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.remove).toHaveBeenCalledWith(
        auto.id,
        auto.revision,
      ),
    );
    await waitFor(() =>
      expect(screen.queryByText("Synthetic group")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: "intentRouting.removedGroups" }),
    ).not.toBeInTheDocument();
  },
);
it.each([
  "templateSmart",
  "templateCapability",
  "templateSpeed",
  "templateExpiry",
])("shows a warning instead of opening %s without models", async (template) => {
  vi.mocked(intentRoutingApi.list).mockResolvedValue({
    groups: [],
    autoCreate: false,
    omittedGroups: 0,
  });
  vi.mocked(intentRoutingApi.providers).mockResolvedValue([]);
  mount();
  const create = screen.getByRole("button", { name: "intentRouting.create" });
  await waitFor(() => expect(create).toBeEnabled());
  fireEvent.click(create);
  const add = screen.getByRole("button", { name: `routingPage.${template}` });
  expect(add).toBeEnabled();
  expect(add).toHaveAccessibleDescription("routingPage.templateNeed_provider");
  fireEvent.click(add);
  const dialog = await screen.findByRole("dialog", {
    name: "routingPage.cannotCreateGroup",
  });
  expect(dialog).toHaveAccessibleDescription("routingPage.noAvailableModels");
  expect(screen.queryByRole("form")).toBeNull();
  expect(intentRoutingApi.save).not.toHaveBeenCalled();
});

it.each(["loading", "failed"])(
  "does not show a false zero-model warning when the model catalog is %s",
  async (state) => {
    if (state === "loading")
      vi.mocked(intentRoutingApi.providers).mockImplementation(
        () => new Promise(() => {}),
      );
    else
      vi.mocked(intentRoutingApi.providers).mockRejectedValue(
        new Error("Synthetic catalog failure"),
      );
    mount();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    fireEvent.click(create);
    const add = screen.getByRole("button", {
      name: "routingPage.templateSmart",
    });
    if (state === "loading") {
      expect(add).toBeDisabled();
    } else {
      await waitFor(() => expect(add).toBeEnabled());
      fireEvent.click(add);
      expect(await screen.findByRole("form")).toBeVisible();
    }
    expect(
      screen.queryByRole("dialog", { name: "routingPage.cannotCreateGroup" }),
    ).toBeNull();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  },
);

it("explains the shared task target when no cheaper model is available", async () => {
  vi.mocked(intentRoutingApi.providers).mockResolvedValue([
    {
      id: "synthetic",
      name: "Synthetic",
      taskEligible: true,
      classifierEligible: true,
      models: [
        { id: "synthetic-strong", name: "Strong", outputPrice: 12 },
        { id: "synthetic-alternate", name: "Alternate", outputPrice: 12 },
      ],
    },
  ]);
  mount();
  const create = screen.getByRole("button", { name: "intentRouting.create" });
  await waitFor(() => expect(create).toBeEnabled());
  fireEvent.click(create);
  const title = await screen.findByText("routingPage.templateSmart");
  const card = title.parentElement!;
  expect(
    within(card).getByText("routingPage.templateSmartSameModelHint"),
  ).toBeVisible();
  const add = within(card).getByRole("button", {
    name: "routingPage.templateSmart",
  });
  expect(add).toBeEnabled();
  fireEvent.click(add);
  expect(await screen.findByRole("dialog")).toBeVisible();
  expect(intentRoutingApi.save).not.toHaveBeenCalled();
});
